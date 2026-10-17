//! Worker-local discovery. Internal search calls never reach the host, and
//! discovered functions are exact members of the client-supplied catalog.
use serde_json::{json, Value};
use std::collections::BTreeSet;

const SEARCH: &str = "__mundusx_discover_tools";
const PREFIX: &str = "__MUNDUSX_OPENAI_TOOL_RESULT_V1__";
const MAX_SEARCHES: usize = 2;
const MAX_RESULTS: usize = 5;

pub struct Discovery {
    catalog: Vec<Value>,
    loaded: BTreeSet<String>,
}

impl Discovery {
    pub fn take(payload: &mut Value) -> Result<Option<Self>, String> {
        let Some(hint) = payload
            .as_object_mut()
            .and_then(|p| p.remove("mundusx_tool_discovery"))
        else {
            return Ok(None);
        };
        if payload
            .get("tool_choice")
            .is_some_and(|c| c.as_str().is_none_or(|s| !matches!(s, "auto" | "required")))
        {
            return Ok(None);
        }
        let catalog = payload["tools"]
            .as_array()
            .ok_or("native discovery requires a registered catalog")?
            .clone();
        if catalog.len() > 256 {
            return Err("native discovery catalog exceeds 256 tools".into());
        }
        let mut names = BTreeSet::new();
        for tool in &catalog {
            let name = tool
                .pointer("/function/name")
                .and_then(Value::as_str)
                .ok_or("native discovery supports registered function tools only")?;
            if name == SEARCH || !names.insert(name.to_string()) {
                return Err(
                    "native discovery catalog has a reserved or duplicate tool name".into(),
                );
            }
        }
        let active = hint["active_tools"]
            .as_array()
            .ok_or("native discovery active_tools must be an array")?;
        if active.len() > 32 {
            return Err("native discovery core exceeds 32 tools".into());
        }
        let mut loaded = BTreeSet::new();
        for entry in active {
            let name = entry
                .as_str()
                .filter(|n| names.contains(*n))
                .ok_or("native discovery active tool is not registered")?;
            loaded.insert(name.to_string());
        }
        // Host-visible calls already made in this task remain available on
        // subsequent client turns, without replaying the private search loop.
        let messages = payload["messages"]
            .as_array()
            .ok_or("native discovery requires messages")?;
        let start = messages
            .iter()
            .rposition(|m| m["role"] == "user")
            .unwrap_or(0);
        for message in &messages[start..] {
            if message["role"] != "assistant" {
                continue;
            }
            for call in message["tool_calls"].as_array().into_iter().flatten() {
                if let Some(name) = call["function"]["name"]
                    .as_str()
                    .filter(|n| names.contains(*n))
                {
                    loaded.insert(name.into());
                }
            }
        }
        if loaded.len() > 48 {
            return Err("native discovery active history exceeds its tool budget".into());
        }
        Ok(Some(Self { catalog, loaded }))
    }

    pub fn run(
        mut self,
        mut payload: Value,
        mut attempt: impl FnMut(Value) -> Result<String, String>,
    ) -> Result<String, String> {
        payload["messages"].as_array_mut().ok_or("native discovery requires messages")?.insert(0,json!({"role":"system","content":
            "Use the currently available registered tools. If a necessary specialized capability is missing, call __mundusx_discover_tools with a short query describing that capability. Discovery only loads definitions; it never executes a host action or grants permissions. After discovery, call the loaded registered function with its actual schema. Do not say a tool ran until a host result confirms it. Do not search for a capability already available."}));
        for round in 0..=MAX_SEARCHES {
            let mut tools: Vec<Value> = self
                .catalog
                .iter()
                .filter(|t| {
                    self.loaded
                        .contains(t["function"]["name"].as_str().unwrap())
                })
                .cloned()
                .collect();
            if round < MAX_SEARCHES && tools.len() < self.catalog.len() {
                tools.push(search_schema());
            }
            payload["tools"] = json!(tools);
            let result = attempt(payload.clone())?;
            let parsed: Value = serde_json::from_str(
                result
                    .strip_prefix(PREFIX)
                    .ok_or("native discovery received an invalid result envelope")?,
            )
            .map_err(|e| format!("native discovery received invalid result JSON: {e}"))?;
            let message = &parsed["message"];
            let calls = message["tool_calls"].as_array();
            let internal =
                calls.is_some_and(|calls| calls.iter().any(|c| c["function"]["name"] == SEARCH));
            if !internal {
                // Defense in depth: a runtime cannot invent a callable tool
                // merely because it exists elsewhere in the full catalog.
                for call in calls.into_iter().flatten() {
                    let name = call["function"]["name"]
                        .as_str()
                        .ok_or("native discovery result has no tool name")?;
                    if !self.loaded.contains(name) {
                        return Err(format!("native discovery rejected unloaded tool {name}"));
                    }
                }
                return Ok(result);
            }
            if round == MAX_SEARCHES {
                return Err("native discovery exhausted its two-search budget; no host action was executed by discovery".into());
            }
            let calls = calls.unwrap();
            if calls.len() != 1 {
                return Err(
                    "native discovery cannot mix an internal search with host actions".into(),
                );
            }
            let call = &calls[0];
            let id = call["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("native discovery search requires a call id")?;
            let args: Value = serde_json::from_str(
                call["function"]["arguments"]
                    .as_str()
                    .ok_or("native discovery query arguments must be JSON")?,
            )
            .map_err(|_| "native discovery query arguments must be valid JSON")?;
            if args.as_object().is_none_or(|a| a.len() != 1) {
                return Err("native discovery accepts only a query".into());
            }
            let query = args["query"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 512)
                .ok_or("native discovery query must contain 1 to 512 bytes")?;
            let found = self.search(query);
            for name in &found {
                self.loaded.insert(name.clone());
            }
            let messages = payload["messages"].as_array_mut().unwrap();
            messages.push(message.clone());
            messages.push(json!({"role":"tool","tool_call_id":id,"content":json!({"loaded_tools":found,"note":"Only definitions were loaded. No host tools were executed. Use the exact schemas now available, or refine the search if none matched."}).to_string()}));
        }
        unreachable!()
    }

    fn search(&self, query: &str) -> Vec<String> {
        let words = terms(query);
        let mut matches: Vec<(usize, usize, String)> = self
            .catalog
            .iter()
            .enumerate()
            .filter_map(|(index, tool)| {
                let f = &tool["function"];
                let name = f["name"].as_str()?;
                if self.loaded.contains(name) {
                    return None;
                }
                let name_words = terms(name);
                let description = terms(f["description"].as_str().unwrap_or(""));
                let score = words
                    .iter()
                    .map(|w| {
                        if name_words.contains(w) {
                            10
                        } else if description.contains(w) {
                            1
                        } else {
                            0
                        }
                    })
                    .sum::<usize>()
                    + if query
                        .to_ascii_lowercase()
                        .contains(&name.to_ascii_lowercase())
                    {
                        100
                    } else {
                        0
                    };
                (score > 0).then(|| (score, index, name.to_string()))
            })
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        matches
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, _, name)| name)
            .collect()
    }
}

fn terms(s: &str) -> BTreeSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            w.len() > 1
                && !matches!(
                    *w,
                    "tool"
                        | "tools"
                        | "the"
                        | "for"
                        | "and"
                        | "with"
                        | "use"
                        | "using"
                        | "find"
                        | "search"
                        | "get"
                        | "to"
                        | "of"
                        | "in"
                        | "on"
                        | "is"
                        | "this"
                        | "that"
                        | "a"
                )
        })
        .map(str::to_string)
        .collect()
}

fn search_schema() -> Value {
    json!({"type":"function","function":{"name":SEARCH,"description":"Discover additional registered client tools for specialized work, such as browser testing, GitHub, notebooks, migrations, language environments or static analysis. Use a short specific query. Does not execute tools or grant permissions.","parameters":{"type":"object","properties":{"query":{"type":"string","maxLength":512}},"required":["query"],"additionalProperties":false}}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"tools":[
            {"type":"function","function":{"name":"read_file","parameters":{"type":"object"}}},
            {"type":"function","function":{"name":"run_in_terminal","parameters":{"type":"object"}}},
            {"type":"function","function":{"name":"open_browser_page","description":"Open a browser page","parameters":{"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}}},
            {"type":"function","function":{"name":"sonarqube_analyze_file","description":"SonarQube static analysis"}},
            {"type":"function","function":{"name":"mcp_github_create_issue","description":"Create a GitHub issue"}}],
            "messages":[{"role":"user","content":"Run this project and verify it in the browser"}],
            "mundusx_tool_discovery":{"active_tools":["read_file","run_in_terminal"]}})
    }
    fn result(name: &str, args: Value) -> String {
        format!(
            "{PREFIX}{}",
            json!({"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call1","type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"})
        )
    }
    #[test]
    fn core_work_uses_one_model_call_and_does_not_expose_catalog() {
        let mut p = fixture();
        let d = Discovery::take(&mut p).unwrap().unwrap();
        let mut count = 0;
        let out = d
            .run(p, |body| {
                count += 1;
                assert!(body.get("mundusx_tool_discovery").is_none());
                assert_eq!(body["tools"].as_array().unwrap().len(), 3);
                Ok(result("read_file", json!({})))
            })
            .unwrap();
        assert_eq!(count, 1);
        assert!(!out.contains(SEARCH));
    }
    #[test]
    fn discovery_loads_exact_original_schema_and_only_releases_host_call() {
        let mut p = fixture();
        let original = p["tools"][2].clone();
        let d = Discovery::take(&mut p).unwrap().unwrap();
        let mut count = 0;
        let out = d
            .run(p, |body| {
                count += 1;
                if count == 1 {
                    Ok(result(SEARCH, json!({"query":"browser page"})))
                } else {
                    assert!(body["tools"].as_array().unwrap().contains(&original));
                    assert!(!body["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|t| t["function"]["name"] == "mcp_github_create_issue"));
                    assert!(body["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|m| m["role"] == "tool"));
                    Ok(result(
                        "open_browser_page",
                        json!({"url":"http://localhost:3000"}),
                    ))
                }
            })
            .unwrap();
        assert_eq!(count, 2);
        assert!(out.contains("open_browser_page"));
        assert!(!out.contains(SEARCH));
    }
    #[test]
    fn repeated_search_is_bounded_and_unknown_tools_cannot_be_released() {
        let mut p = fixture();
        let d = Discovery::take(&mut p).unwrap().unwrap();
        let mut count = 0;
        assert!(d
            .run(p, |_| {
                count += 1;
                Ok(result(SEARCH, json!({"query":"no_such_capability_987"})))
            })
            .unwrap_err()
            .contains("budget"));
        assert_eq!(count, 3);
        for name in ["unregistered_command", "open_browser_page"] {
            let mut p = fixture();
            let d = Discovery::take(&mut p).unwrap().unwrap();
            assert!(d
                .run(p, |_| Ok(result(name, json!({}))))
                .unwrap_err()
                .contains("unloaded"));
        }
    }
    #[test]
    fn host_visible_specialized_calls_stay_loaded_in_current_task() {
        let mut p = fixture();
        p["messages"].as_array_mut().unwrap().push(
            json!({"role":"assistant","tool_calls":[{"function":{"name":"open_browser_page"}}]}),
        );
        let d = Discovery::take(&mut p).unwrap().unwrap();
        assert!(d.loaded.contains("open_browser_page"));
        p["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":"New task"}));
        let mut hinted = p.clone();
        hinted["mundusx_tool_discovery"] = json!({"active_tools":["read_file"]});
        let d = Discovery::take(&mut hinted).unwrap().unwrap();
        assert!(!d.loaded.contains("open_browser_page"));
    }
    #[test]
    fn malformed_catalog_hints_and_search_arguments_are_rejected() {
        let mut p = fixture();
        p["mundusx_tool_discovery"]["active_tools"] = json!(["invented"]);
        assert!(Discovery::take(&mut p).is_err());
        let mut p = fixture();
        p["tools"].as_array_mut().unwrap().push(search_schema());
        assert!(Discovery::take(&mut p).is_err());
        for args in [
            json!({"query":""}),
            json!({"query":"browser","execute":true}),
            json!({"query":7}),
        ] {
            let mut p = fixture();
            let d = Discovery::take(&mut p).unwrap().unwrap();
            assert!(d.run(p, |_| Ok(result(SEARCH, args.clone()))).is_err());
        }
        let mut p = fixture();
        p["tool_choice"] = json!("none");
        assert!(Discovery::take(&mut p).unwrap().is_none());
        assert!(p.get("mundusx_tool_discovery").is_none());
    }
}
