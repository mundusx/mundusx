//! Connector outbox: durable execution boundaries, never token streaming history.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;

pub struct TaskJournal(Connection);

impl TaskJournal {
    pub fn open(home: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(home).map_err(|e| e.to_string())?;
        let db = Connection::open(home.join("harness-tasks.db")).map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS task_journal (
              scope TEXT NOT NULL, task_id TEXT NOT NULL, workspace TEXT NOT NULL,
              session_id TEXT NOT NULL, state TEXT NOT NULL, completion TEXT,
              boundary TEXT, updated INTEGER NOT NULL DEFAULT (unixepoch()),
              PRIMARY KEY(scope, task_id));
            CREATE INDEX IF NOT EXISTS task_journal_state ON task_journal(state, updated);
            DELETE FROM task_journal WHERE state='acknowledged' AND updated < unixepoch()-2592000;",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self(db))
    }

    // Call only after the server grants the execution claim. Scope binds the
    // record to the service and connector identity, including account rebinds.
    pub fn claim(
        &self,
        scope: &str,
        task: &Value,
        workspace: &Path,
    ) -> Result<Option<Value>, String> {
        let id = task["task_id"].as_str().ok_or("task identity missing")?;
        let session = task["session_id"]
            .as_str()
            .ok_or("session identity missing")?;
        let workspace = workspace.to_string_lossy();
        let prior: Option<(String, String, Option<String>)> = self.0.query_row(
            "SELECT workspace, session_id, completion FROM task_journal WHERE scope=?1 AND task_id=?2",
            params![scope, id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional().map_err(|e| e.to_string())?;
        if let Some((previous_workspace, previous_session, completion)) = prior {
            if previous_workspace != workspace || previous_session != session {
                return Err("Recovery workspace or session differs from the saved task".into());
            }
            return completion
                .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
                .transpose();
        }
        self.0.execute("INSERT INTO task_journal(scope,task_id,workspace,session_id,state) VALUES(?1,?2,?3,?4,'active')",
            params![scope,id,workspace,session]).map_err(|e| e.to_string())?;
        Ok(None)
    }

    pub fn boundary(&self, scope: &str, id: &str, event: &Value) -> Result<(), String> {
        if !matches!(
            event["type"].as_str(),
            Some("tool_started" | "tool_completed")
        ) {
            return Ok(());
        }
        self.0.execute("UPDATE task_journal SET boundary=?3, updated=unixepoch() WHERE scope=?1 AND task_id=?2 AND completion IS NULL",
            params![scope,id,event.to_string()]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn prepare(&self, scope: &str, id: &str, completion: &Value) -> Result<(), String> {
        let n = self.0.execute("UPDATE task_journal SET completion=?3,state='report_pending',updated=unixepoch() WHERE scope=?1 AND task_id=?2",
            params![scope,id,completion.to_string()]).map_err(|e| e.to_string())?;
        if n != 1 {
            return Err("Task journal record missing".into());
        }
        Ok(())
    }

    pub fn acknowledge(&self, scope: &str, id: &str) -> Result<(), String> {
        self.0.execute("UPDATE task_journal SET state='acknowledged',updated=unixepoch() WHERE scope=?1 AND task_id=?2",
            params![scope,id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn pending_reports(&self, scope: &str) -> Result<Vec<(String, Value)>, String> {
        let mut query = self.0.prepare("SELECT task_id,completion FROM task_journal WHERE scope=?1 AND state='report_pending' ORDER BY updated LIMIT 100").map_err(|e| e.to_string())?;
        let rows = query
            .query_map([scope], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            let (id, raw) = r.map_err(|e| e.to_string())?;
            Ok((id, serde_json::from_str(&raw).map_err(|e| e.to_string())?))
        })
        .collect()
    }

    pub fn list(&self) -> Result<Vec<Value>, String> {
        let mut query = self.0.prepare("SELECT task_id,workspace,state,updated,boundary FROM task_journal ORDER BY updated DESC LIMIT 100").map_err(|e| e.to_string())?;
        let rows = query
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "task_id":r.get::<_, String>(0)?, "workspace":r.get::<_, String>(1)?,
                    "state":r.get::<_, String>(2)?, "updated_unix":r.get::<_, i64>(3)?,
                    "last_tool_boundary":r.get::<_, Option<String>>(4)?
                }))
            })
            .map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restart_replays_completion_without_reexecuting_and_isolates_identity() {
        let home = std::env::temp_dir().join(format!("journal-{}", uuid::Uuid::new_v4()));
        let task = json!({"task_id":"task", "session_id":"session"});
        let result = json!({"status":"completed","result":{"changed_files":["server.js"]}});
        {
            let journal = TaskJournal::open(&home).unwrap();
            assert!(journal
                .claim("service/device", &task, Path::new("project"))
                .unwrap()
                .is_none());
            journal.prepare("service/device", "task", &result).unwrap();
        }
        {
            let journal = TaskJournal::open(&home).unwrap();
            assert_eq!(
                journal
                    .claim("service/device", &task, Path::new("project"))
                    .unwrap(),
                Some(result)
            );
            assert!(journal
                .claim("service/device", &task, Path::new("other"))
                .is_err());
            assert!(journal
                .claim("other/device", &task, Path::new("project"))
                .unwrap()
                .is_none());
            journal.acknowledge("service/device", "task").unwrap();
        }
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn unfinished_work_survives_retention_and_tokens_are_not_journaled() {
        let home = std::env::temp_dir().join(format!("journal-{}", uuid::Uuid::new_v4()));
        {
            let journal = TaskJournal::open(&home).unwrap();
            let task = json!({"task_id":"task", "session_id":"session"});
            journal.claim("scope", &task, Path::new("project")).unwrap();
            journal
                .boundary(
                    "scope",
                    "task",
                    &json!({"type":"tool_started","data":{"call_id":"a"}}),
                )
                .unwrap();
            journal
                .boundary(
                    "scope",
                    "task",
                    &json!({"type":"assistant_snapshot","data":{"text":"token"}}),
                )
                .unwrap();
            journal
                .0
                .execute("UPDATE task_journal SET updated=0", [])
                .unwrap();
        }
        {
            let journal = TaskJournal::open(&home).unwrap();
            let value: String = journal
                .0
                .query_row("SELECT boundary FROM task_journal", [], |r| r.get(0))
                .unwrap();
            assert!(value.contains("tool_started"));
            assert!(!value.contains("token"));
        }
        std::fs::remove_dir_all(home).unwrap();
    }
}
