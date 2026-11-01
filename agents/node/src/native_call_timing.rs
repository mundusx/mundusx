//! Local metadata only: never record prompts, commands, outputs, or credentials.
use fs2::FileExt;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    fs::OpenOptions,
    io::Write,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
thread_local! {
    static CURRENT: RefCell<Option<(Instant, Option<u64>)>> = const { RefCell::new(None) };
    static COUNTS: RefCell<Value> = const { RefCell::new(Value::Null) };
}
pub struct CallTiming {
    job: String,
    retry: bool,
    started: Instant,
}
impl CallTiming {
    pub fn start(job: &str, retry: bool) -> Self {
        let started = Instant::now();
        CURRENT.with(|slot| *slot.borrow_mut() = Some((started, None)));
        COUNTS.with(|slot| *slot.borrow_mut() = Value::Null);
        Self {
            job: job.into(),
            retry,
            started,
        }
    }
    pub fn finish(self, success: bool) {
        let first = CURRENT.with(|slot| slot.borrow_mut().take().and_then(|(_, first)| first));
        let counts = COUNTS.with(|slot| slot.replace(Value::Null));
        let row = json!({"event":"native_model_call","job_id":self.job,"retry":self.retry,
            "timestamp_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            "model_call_ms":self.started.elapsed().as_millis() as u64,"first_model_token_ms":first,
            "success":success,"prompt_tokens":counts["prompt_tokens"],"cached_prompt_tokens":counts["cached_prompt_tokens"],
            "completion_tokens":counts["completion_tokens"],"queue_wait_ms":null,"host_tool_ms":null,"host_verification_ms":null});
        if !cfg!(test) {
            let _ = persist(&crate::storage::config_dir(), &row);
        }
    }
}
pub fn usage(value: &Value) {
    COUNTS.with(|slot| *slot.borrow_mut() = json!({
        "prompt_tokens":value["prompt_tokens"].as_u64(),
        "cached_prompt_tokens":value.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64),
        "completion_tokens":value["completion_tokens"].as_u64(),
    }));
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
    fn usage_keeps_counts_only_and_missing_cache_usage_stays_unknown() {
        usage(&json!({"prompt_tokens":100,"completion_tokens":2,
            "prompt_tokens_details":{"cached_tokens":90},"private_prompt":"CANARY"}));
        COUNTS.with(|slot| {
            assert_eq!(slot.borrow()["cached_prompt_tokens"],90);
            assert!(!slot.borrow().to_string().contains("CANARY"));
        });
        usage(&json!({"prompt_tokens":100}));
        COUNTS.with(|slot| assert!(slot.borrow()["cached_prompt_tokens"].is_null()));
        let timing=CallTiming::start("test",false);
        COUNTS.with(|slot| assert!(slot.borrow().is_null()));
        timing.finish(true);
    }
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
