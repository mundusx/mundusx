//! Bounded desktop metadata. Never persist request content, outputs or raw errors.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
static LOCK: Mutex<()> = Mutex::new(());
const LIMIT: usize = 200;
#[derive(Serialize, Deserialize)]
struct Entry {
    job_id: String,
    status: String,
    started_at: u64,
    updated_at: u64,
    duration_ms: Option<u64>,
    process_id: u32,
    #[serde(default)]
    model: Option<String>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn path() -> PathBuf {
    crate::storage::config_dir().join("desktop-jobs.json")
}
fn read(path: &Path) -> Vec<Entry> {
    if fs::metadata(path).is_ok_and(|m| m.len() <= 256000) {
        fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}
fn save(path: &Path, rows: &[Entry]) -> std::io::Result<()> {
    let parent = path.parent().expect("activity parent");
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!("desktop-jobs-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut f = options.open(&stage)?;
        f.write_all(&serde_json::to_vec(rows)?)?;
        drop(f);
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&stage, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&stage);
    }
    result
}
fn update(
    path: &Path,
    id: &str,
    model: Option<&str>,
    status: &str,
    duration: Option<u64>,
) -> std::io::Result<()> {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows = read(path);
    let timestamp = now();
    let id: String = id.chars().take(256).collect();
    if let Some(row) = rows.iter_mut().find(|r| r.job_id == id) {
        // A late relay callback cannot revive a terminal outcome.
        if status == "streaming" && !matches!(row.status.as_str(), "running" | "streaming") {
            return Ok(());
        }
        if status == "running" {
            row.started_at = timestamp;
            row.process_id = std::process::id();
        }
        row.status = status.into();
        row.updated_at = timestamp;
        row.duration_ms = duration;
        if let Some(model) = model {
            row.model = Some(model.chars().take(256).collect());
        }
    } else {
        rows.push(Entry {
            job_id: id,
            status: status.into(),
            started_at: timestamp,
            updated_at: timestamp,
            duration_ms: duration,
            process_id: std::process::id(),
            model: model.map(|s| s.chars().take(256).collect()),
        });
    }
    while rows.len() > LIMIT {
        let index = rows
            .iter()
            .position(|r| !matches!(r.status.as_str(), "running" | "streaming"))
            .unwrap_or(0);
        rows.remove(index);
    }
    save(path, &rows)
}
fn report(result: std::io::Result<()>) {
    if result.is_err() {
        eprintln!("desktopActivity: local metadata write failed");
    }
}
pub fn initialize() {
    report(initialize_at(&path()));
}
fn initialize_at(path: &Path) -> std::io::Result<()> {
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows = read(path);
    for row in &mut rows {
        if matches!(row.status.as_str(), "running" | "streaming") {
            row.status = "interrupted".into();
            row.updated_at = now();
        }
    }
    save(path, &rows)
}
pub fn streaming(id: &str) {
    report(update(&path(), id, None, "streaming", None));
}
pub struct Job {
    path: PathBuf,
    id: String,
    started: Instant,
    finished: bool,
}
impl Job {
    pub fn start(id: &str, model: Option<&str>) -> Self {
        Self::start_at(path(), id, model)
    }
    fn start_at(path: PathBuf, id: &str, model: Option<&str>) -> Self {
        report(update(&path, id, model, "running", None));
        Self {
            path,
            id: id.into(),
            started: Instant::now(),
            finished: false,
        }
    }
    pub fn finish(&mut self, status: &str) {
        report(update(
            &self.path,
            &self.id,
            None,
            status,
            Some(self.started.elapsed().as_millis().min(u64::MAX as u128) as u64),
        ));
        self.finished = true;
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("interrupted");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PathBuf {
        std::env::temp_dir()
            .join(format!("activity-{}", uuid::Uuid::new_v4()))
            .join("desktop-jobs.json")
    }
    #[test]
    fn lifecycle_is_visible_before_completion_and_retains_model_without_content() {
        let p = fixture();
        let mut job = Job::start_at(p.clone(), "job-1", Some("test/model"));
        assert_eq!(read(&p)[0].status, "running");
        update(&p, "job-1", None, "streaming", None).unwrap();
        assert_eq!(read(&p)[0].status, "streaming");
        job.finish("completed");
        drop(job);
        update(&p, "job-1", None, "streaming", None).unwrap();
        let rows = read(&p);
        assert_eq!(rows[0].status, "completed");
        assert_eq!(rows[0].model.as_deref(), Some("test/model"));
        let v: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert!(v[0].get("prompt").is_none());
        assert!(v[0].get("output").is_none());
        assert!(v[0]["duration_ms"].is_number());
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn failure_cancellation_and_restart_never_become_success() {
        let p = fixture();
        for (id, status) in [("failed", "failed"), ("cancelled", "cancelled")] {
            let mut job = Job::start_at(p.clone(), id, None);
            job.finish(status);
        }
        {
            let _job = Job::start_at(p.clone(), "unwound", None);
        }
        update(&p, "old-process", None, "running", None).unwrap();
        initialize_at(&p).unwrap();
        let rows = read(&p);
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[1].status, "cancelled");
        assert_eq!(rows[2].status, "interrupted");
        assert_eq!(rows[3].status, "interrupted");
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn retention_is_bounded_and_prefers_active_jobs() {
        let p = fixture();
        let active = Job::start_at(p.clone(), "active", None);
        for i in 0..205 {
            let mut j = Job::start_at(p.clone(), &format!("job-{i}"), None);
            j.finish("completed");
        }
        assert_eq!(read(&p).len(), LIMIT);
        assert!(read(&p).iter().any(|r| r.job_id == "active"));
        drop(active);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
