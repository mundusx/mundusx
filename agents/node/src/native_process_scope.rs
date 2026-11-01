//! Reject recognizable shared-runtime shutdowns before the IDE receives a call.
//! This is a narrow guard, not a shell sandbox or a complete command parser.
use serde_json::Value;

pub fn validate(message: &Value) -> Result<(), String> {
    for call in message["tool_calls"].as_array().into_iter().flatten() {
        let function = &call["function"];
        let name = function["name"].as_str().unwrap_or("");
        if !matches!(name, "run_in_terminal" | "terminal" | "execute_command") {
            continue;
        }
        let arguments = match function["arguments"].as_str() {
            Some(raw) => serde_json::from_str::<Value>(raw).unwrap_or(Value::Null),
            None => function["arguments"].clone(),
        };
        let command = arguments["command"]
            .as_str()
            .or_else(|| arguments["cmd"].as_str())
            .unwrap_or("");
        if broad_shutdown(command) {
            return Err("native tool schema precondition failed: run_in_terminal.command would terminate shared runtime/browser processes by name. No command was executed. Discover the requested app's exact PID from its port, executable path, command line or owned terminal handle; stop only that app and verify its exit. Do not claim success or repeat a broad process-name shutdown.".into());
        }
    }
    Ok(())
}

fn broad_shutdown(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    let runtime = [
        "node", "chrome", "firefox", "python", "electron", "msedge", "java",
    ];
    let termination = c.contains("stop-process")
        || c.contains("taskkill")
        || c.contains("killall")
        || c.contains("pkill");
    if !termination {
        return false;
    }
    let broad_name = runtime.iter().any(|name| {
        c.contains(&format!("*{name}*"))
            || c.contains(&format!("-name {name}"))
            || c.contains(&format!("-name '{name}'"))
            || c.contains(&format!("-name \"{name}\""))
            || c.contains(&format!("/im {name}.exe"))
            || c.contains(&format!("/im \"{name}.exe\""))
            || c.contains(&format!("killall {name}"))
            || c.contains(&format!("pkill {name}"))
            || c.contains(&format!("pkill -f {name}"))
    });
    broad_name
        || (c.contains("get-process")
            && c.contains("stop-process")
            && !c.contains("-id ")
            && !c.contains("-processid ")
            && !c.contains("executablepath")
            && !c.contains("commandline"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn call(command: &str) -> Value {
        json!({"tool_calls":[{"function":{"name":"run_in_terminal","arguments":json!({"command":command}).to_string()}}]})
    }
    #[test]
    fn rejects_incident_even_when_errors_are_suppressed() {
        let command = r#"Get-Process | Where-Object {$_.ProcessName -like "*node*" -or $_.ProcessName -like "*chrome*" -or $_.ProcessName -like "*firefox*"} | Stop-Process -Force -ErrorAction SilentlyContinue"#;
        assert!(validate(&call(command)).is_err());
    }
    #[test]
    fn rejects_name_wide_termination_variants() {
        for c in [
            "Stop-Process -Name node -Force",
            "taskkill /F /IM chrome.exe",
            "killall node",
            "pkill -f python",
            "Get-Process | Stop-Process",
        ] {
            assert!(validate(&call(c)).is_err(), "{c}");
        }
    }
    #[test]
    fn preserves_discovery_and_targeted_actions() {
        for c in [
            "Get-NetTCPConnection -LocalPort 3000 | Select-Object OwningProcess",
            "Get-Process -Id 1234",
            "Stop-Process -Id 1234",
            "taskkill /PID 1234",
            "kill -TERM 1234",
            "Get-Process -Id 1234 | Stop-Process",
            "npm run build",
        ] {
            assert!(validate(&call(c)).is_ok(), "{c}");
        }
    }
    #[test]
    fn does_not_treat_file_content_as_a_command() {
        let m = json!({"tool_calls":[{"function":{"name":"write_file","arguments":json!({"content":"killall node"}).to_string()}}]});
        assert!(validate(&m).is_ok());
    }
}
