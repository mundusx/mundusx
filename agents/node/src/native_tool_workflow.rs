//! Read-only handoffs to the host. Never execute code or synthesize a patch.
use serde_json::{json, Value};

fn key(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    if path.split('/').any(|p| p == "..") { return None; }
    let absolute = path.starts_with('/') || path.as_bytes().get(1) == Some(&b':');
    if !absolute || path.chars().any(char::is_control) { return None; }
    let path = path.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("/");
    Some(if path.as_bytes().get(1) == Some(&b':') { path.to_ascii_lowercase() } else { format!("/{path}") })
}

fn catalog(payload: &Value) -> Option<&Vec<Value>> {
    if payload.get("tool_choice").is_some_and(|choice| !choice.is_null() && !matches!(choice.as_str(),Some("auto" | "required"))) { return None; }
    let tools = payload["tools"].as_array()?;
    ["read_file", "list_dir", "replace_string_in_file"].iter().all(|name|
        tools.iter().any(|t| t["function"]["name"].as_str() == Some(name))).then_some(tools)
}

pub fn is_file_turn(payload: &Value) -> bool { catalog(payload).is_some() }

/// Repeated inspection without a write or diagnostics is not progress. Return
/// actual host errors once, then stop rather than rereading indefinitely.
pub fn inspection_progress(payload:&Value,message:&Value)->Option<Value> {
    let tools=catalog(payload)?;
    tools.iter().find(|t|t["function"]["name"]=="get_errors")?;
    let calls=message["tool_calls"].as_array()?;
    if calls.len()!=1 || !matches!(calls[0]["function"]["name"].as_str(),Some("read_file"|"list_dir"|"file_search"|"grep_search")) {return None;}
    let h=history(payload)?;let mut reads=0;let mut rounds=0;let mut paths=std::collections::BTreeMap::new();
    for m in h {
        for c in m["tool_calls"].as_array().into_iter().flatten() {
            if !h.iter().any(|r|r["role"]=="tool"&&r["tool_call_id"]==c["id"]) {continue;}
            match c["function"]["name"].as_str() {
                Some("create_file"|"replace_string_in_file"|"get_errors")=>{reads=0;paths.clear();},
                Some("read_file"|"list_dir"|"file_search"|"grep_search")=>{
                    reads+=1;
                    if let Ok(a)=serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) {
                        if let Some(path)=a["filePath"].as_str() {if let Some(k)=key(path) {paths.insert(k,path.to_string());}}
                    }
                },_=>{}
            }
            if c["id"].as_str().is_some_and(|id|id.starts_with("call_verify_progress_")) {rounds+=1;}
        }
    }
    if reads<8 || paths.is_empty() {return None;}
    if rounds>=1 {return Some(incomplete("inspection repeated without repairs after actionable host diagnostics."));}
    Some(call("get_errors",json!({"filePaths":paths.into_values().take(32).collect::<Vec<_>>()}),"call_verify_progress_"))
}

fn history(payload: &Value) -> Option<&[Value]> {
    let messages = payload["messages"].as_array()?;
    let start = messages.iter().rposition(|m| m["role"] == "user")? + 1;
    Some(&messages[start..])
}

fn call(name: &str, args: Value, prefix: &str) -> Value {
    json!({"role":"assistant","content":null,"tool_calls":[{
        "id":format!("{prefix}{}",uuid::Uuid::new_v4().simple()),"type":"function",
        "function":{"name":name,"arguments":args.to_string()}}]})
}

fn incomplete(reason: &str) -> Value {
    json!({"role":"assistant","content":format!("Verification incomplete: {reason} No further automatic edits were applied. The generated project must not be treated as verified.")})
}

/// A rejected unexecuted edit becomes a fresh read of that same target. Scope,
/// schema and traversal failures are never converted into another operation.
pub fn recover_read(payload: &Value, message: &Value, error: &str) -> Option<Value> {
    catalog(payload)?;
    if !["native edit precondition failed: read the exact target", "native edit precondition failed: oldString is ambiguous", "native edit precondition failed: target was already read"].iter().any(|p| error.starts_with(p)) { return None; }
    let calls = message["tool_calls"].as_array()?;
    if calls.len() != 1 { return None; }
    let args: Value = serde_json::from_str(calls[0]["function"]["arguments"].as_str()?).ok()?;
    let path = args["filePath"].as_str()?;
    let target = key(path)?;
    let h = history(payload)?;
    let mut reads = 0;
    let mut end = 0;
    for m in h {
        if let Some(calls) = m["tool_calls"].as_array() {
            for c in calls {
                let Ok(a) = serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) else { continue; };
                if a["filePath"].as_str().and_then(key).as_ref() != Some(&target) { continue; }
                let observed = h.iter().any(|r| r["role"] == "tool" && r["tool_call_id"] == c["id"] && r["content"].as_str().is_some_and(|s| !s.trim_start().to_ascii_lowercase().starts_with("error")));
                if observed && matches!(c["function"]["name"].as_str(), Some("create_file" | "replace_string_in_file")) { reads = 0; end = 0; }
                if c["id"].as_str().is_some_and(|id| id.starts_with("call_grounding_")) { reads += 1; end = a["endLine"].as_u64().unwrap_or(0); }
            }
        }
    }
    if reads >= 2 { return Some(incomplete("the edit still did not match observed source after two recovery reads.")); }
    // First recovery expands a partial read; the second moves forward rather
    // than repeatedly reading the same initial ten lines.
    Some(call("read_file", json!({"filePath":path,"startLine":end+1,"endLine":end+400}), "call_grounding_"))
}

/// Before a final answer, ask host diagnostics for files changed since their
/// last check. This uses language services, not file extensions or remote exec.
pub fn verification(payload: &Value, message: &Value) -> Option<Value> {
    let tools = catalog(payload)?;
    if message["tool_calls"].as_array().is_some_and(|c| !c.is_empty()) { return None; }
    let schema = &tools.iter().find(|t| t["function"]["name"] == "get_errors")?["function"]["parameters"];
    if schema.pointer("/properties/filePaths/type")?.as_str()? != "array" { return None; }
    let h = history(payload)?;
    let mut pending = std::collections::BTreeMap::new();
    let mut rounds = 0;
    for m in h {
        let Some(calls) = m["tool_calls"].as_array() else { continue; };
        for c in calls {
            let Ok(args) = serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) else { continue; };
            let result = h.iter().find(|r| r["role"] == "tool" && r["tool_call_id"] == c["id"]);
            let Some(text) = result.and_then(|r| r["content"].as_str()).filter(|s| !s.trim_start().to_ascii_lowercase().starts_with("error")) else { continue; };
            match c["function"]["name"].as_str() {
                Some("create_file" | "replace_string_in_file") => {
                    if let Some(path) = args["filePath"].as_str() { if let Some(k) = key(path) { pending.insert(k, path.to_string()); } }
                }
                Some("get_errors") => {
                    if c["id"].as_str().is_some_and(|id| id.starts_with("call_verify_final_")) { rounds += 1; }
                    // Diagnostics containing errors must reach the model, not
                    // be silently counted as a successful verification.
                    if !text.contains("<compileError>") || advisory_only(text) {
                        if let Some(paths) = args["filePaths"].as_array() { for p in paths { if let Some(k) = p.as_str().and_then(key) { pending.remove(&k); } } }
                    }
                }
                _ => {}
            }
        }
    }
    if pending.is_empty() { return None; }
    if rounds >= 3 { return Some(incomplete("diagnostics did not clear within three verification rounds.")); }
    Some(call("get_errors", json!({"filePaths":pending.into_values().take(32).collect::<Vec<_>>()}), "call_verify_final_"))
}

fn advisory_only(text:&str)->bool {
    let mut messages=Vec::new();
    let mut rest=text;
    while let Some((_,tail))=rest.split_once("<compileError>") {
        let Some((message,tail))=tail.split_once("</compileError>") else {return false;};
        messages.push(message.trim());rest=tail;
    }
    !messages.is_empty() && messages.iter().all(|m|m.starts_with("Code Duplication (")
        || (m.starts_with("Prefer `node:")&&m.contains(" over "))
        || *m=="This framework implicitly discloses version information by default. Make sure it is safe here.")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload()->Value {json!({"tools":[
        {"function":{"name":"read_file"}},{"function":{"name":"list_dir"}},{"function":{"name":"replace_string_in_file"}},
        {"function":{"name":"get_errors","parameters":{"properties":{"filePaths":{"type":"array","items":{"type":"string"}}}}}}],
        "messages":[{"role":"user","content":"Create project"}]})}
    fn observe(p:&mut Value,m:Value,text:&str) {
        let id=m["tool_calls"][0]["id"].clone();
        p["messages"].as_array_mut().unwrap().extend([m,json!({"role":"tool","tool_call_id":id,"content":text})]);
    }
    fn args(m:&Value)->Value {serde_json::from_str(m["tool_calls"][0]["function"]["arguments"].as_str().unwrap()).unwrap()}
    #[test]
    fn rejected_edit_reads_exact_target_instead_of_editing_again() {
        let mut p=payload();let edit=call("replace_string_in_file",json!({"filePath":"D:/sample/src/main.rs","oldString":"unseen","newString":"bad"}),"edit_");
        let error="native edit precondition failed: read the exact target and use its observed oldString before replacing it";
        let first=recover_read(&p,&edit,error).unwrap();
        assert_eq!(first["tool_calls"][0]["function"]["name"],"read_file");
        assert_eq!(args(&first),json!({"filePath":"D:/sample/src/main.rs","startLine":1,"endLine":400}));
        observe(&mut p,first,"partial observed source");
        let second=recover_read(&p,&edit,error).unwrap();assert_eq!(args(&second)["startLine"],401);
        observe(&mut p,second,"later source");
        let stopped=recover_read(&p,&edit,error).unwrap();assert!(stopped.get("tool_calls").is_none());
        assert!(stopped["content"].as_str().unwrap().contains("Verification incomplete"));
        for denied in ["native edit precondition failed: mutation target is outside the explicit files", "native tool schema precondition failed: invalid arguments"] {assert!(recover_read(&p,&edit,denied).is_none());}
    }
    #[test]
    fn verification_is_language_independent_and_does_not_execute_commands() {
        for file in ["main.rs","main.py","main.cpp","Student.java","server.js","app.ts","Program.cs","main.go"] {
            let mut p=payload();let path=format!("D:/sample/{file}");
            observe(&mut p,call("create_file",json!({"filePath":path,"content":"source"}),"create_"),"Created file");
            let final_message=json!({"role":"assistant","content":"Complete"});
            let check=verification(&p,&final_message).unwrap();
            assert_eq!(check["tool_calls"][0]["function"]["name"],"get_errors");assert_eq!(args(&check)["filePaths"],json!([path]));
            observe(&mut p,check,"No errors found");assert!(verification(&p,&final_message).is_none());
            observe(&mut p,call("replace_string_in_file",json!({"filePath":path,"oldString":"source","newString":"edited"}),"edit_"),"Applied edit");
            assert!(verification(&p,&final_message).is_some());
        }
    }
    #[test]
    fn errors_remain_actionable_but_repeated_validation_is_bounded() {
        let mut p=payload();observe(&mut p,call("create_file",json!({"filePath":"/src/main.py"}),"write_"),"Created");
        let answer=json!({"content":"Done"});
        for _ in 0..3 {let check=verification(&p,&answer).unwrap();observe(&mut p,check,"<compileError>SyntaxError</compileError>");}
        assert!(verification(&p,&answer).unwrap()["content"].as_str().unwrap().contains("Verification incomplete"));
        p["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":"hello"}));
        assert!(verification(&p,&answer).is_none());
    }
    #[test]
    fn style_advisories_do_not_loop_and_unknown_errors_do() {
        assert!(advisory_only("<compileError>Code Duplication (3%)</compileError>"));
        assert!(!advisory_only("<compileError>Code Duplication (3%)</compileError><compileError>Unexpected token</compileError>"));
        assert!(!advisory_only("<compileError>Unknown diagnostic</compileError>"));
    }
    #[test]
    fn explicit_client_tool_choice_is_not_overridden() {
        let mut p=payload();observe(&mut p,call("create_file",json!({"filePath":"/src/main.rs"}),"write_"),"Created");
        for choice in [json!("none"),json!({"type":"function","function":{"name":"custom"}})] {
            p["tool_choice"]=choice;
            assert!(verification(&p,&json!({"content":"answer"})).is_none());
            assert!(!is_file_turn(&p));
        }
    }
    #[test]
    fn inspection_loop_requests_diagnostics_then_stops_without_inventing_repairs() {
        let mut p=payload();
        let read=call("read_file",json!({"filePath":"/src/main.rs","startLine":1,"endLine":50}),"read_");
        for _ in 0..8 {observe(&mut p,read.clone(),"source");}
        let check=inspection_progress(&p,&read).unwrap();assert_eq!(check["tool_calls"][0]["function"]["name"],"get_errors");
        observe(&mut p,check,"<compileError>Syntax error</compileError>");
        for _ in 0..8 {observe(&mut p,read.clone(),"source");}
        assert!(inspection_progress(&p,&read).unwrap()["content"].as_str().unwrap().contains("Verification incomplete"));
    }
}
