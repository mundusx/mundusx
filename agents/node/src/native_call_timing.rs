//! Local metadata only: never record prompts, commands, outputs, or credentials.
use fs2::FileExt;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    fs::OpenOptions,
    io::Write,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
thread_local! { static CURRENT: RefCell<Option<(Instant, Option<u64>)>> = const { RefCell::new(None) }; }
pub struct CallTiming {
    job: String,
    retry: bool,
    started: Instant,
}
impl CallTiming {
    pub fn start(job: &str, retry: bool) -> Self {
        let started = Instant::now();
        CURRENT.with(|slot| *slot.borrow_mut() = Some((started, None)));
        Self {
            job: job.into(),
            retry,
            started,
        }
    }
    pub fn finish(self, success: bool) {
        let first = CURRENT.with(|slot| slot.borrow_mut().take().and_then(|(_, first)| first));
        let row = json!({"event":"native_model_call","job_id":self.job,"retry":self.retry,
            "timestamp_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            "model_call_ms":self.started.elapsed().as_millis() as u64,"first_model_token_ms":first,
            "success":success,"queue_wait_ms":null,"host_tool_ms":null,"host_verification_ms":null});
        if !cfg!(test) { let _ = persist(&crate::storage::config_dir(), &row); }
    }
}
pub fn token_progress() {
    CURRENT.with(|slot| {
        if let Some((start, first)) = slot.borrow_mut().as_mut() {
            if first.is_none() {
                *first = Some(start.elapsed().as_millis() as u64);
            }
        }
    });
}
fn persist(home: &std::path::Path, row: &Value) -> std::io::Result<()> {
    std::fs::create_dir_all(home)?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(home.join("native-call-timings.jsonl"))?;
    file.lock_exclusive()?;
    // Bound local history; missing measurements remain null, never fabricated zero.
    if file.metadata()?.len() >= 2_000_000 {
        file.set_len(0)?;
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::End(0))?;
    writeln!(file, "{row}")?;
    FileExt::unlock(&file)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_progress_is_recorded_once_and_missing_remains_unknown() {
        CURRENT.with(|s| *s.borrow_mut() = Some((Instant::now(), None)));
        assert!(CURRENT.with(|s| s.borrow().as_ref().unwrap().1.is_none()));
        token_progress();
        let first = CURRENT.with(|s| s.borrow().as_ref().unwrap().1);
        token_progress();
        assert_eq!(first, CURRENT.with(|s| s.borrow().as_ref().unwrap().1));
        CURRENT.with(|s| s.borrow_mut().take());
    }
    #[test]
    fn journal_contains_only_supplied_metadata_and_is_bounded() {
        let dir = std::env::temp_dir().join(format!("opengpu-timing-{}", uuid::Uuid::new_v4()));
        persist(&dir, &json!({"job_id":"test","host_tool_ms":null})).unwrap();
        let p = dir.join("native-call-timings.jsonl");
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("\"host_tool_ms\":null"));
        OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_len(2_000_001)
            .unwrap();
        persist(&dir, &json!({"job_id":"next"})).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() < 1000);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
