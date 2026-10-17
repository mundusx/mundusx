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

/// VS Code requires display labels in addition to executable terminal args.
/// Fill only absent display strings declared by the registered contract. Never
/// synthesize or rewrite the command, execution mode, timeout or permissions.
pub fn terminal_metadata(payload:&Value,message:&Value)->Value {
    let Some(tool)=payload["tools"].as_array().and_then(|tools|tools.iter().find(|t|t["function"]["name"]=="run_in_terminal")) else {return message.clone();};
    let schema=&tool["function"]["parameters"];
    let mut result=message.clone();
    for call in result["tool_calls"].as_array_mut().into_iter().flatten() {
        if call["function"]["name"]!="run_in_terminal" {continue;}
        let Ok(mut args)=serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap_or("")) else {continue;};
        if !args.is_object() || args["command"].as_str().is_none_or(|s|s.trim().is_empty()) {continue;}
        let mut changed=false;
        for (field,text) in [("goal","Execute the requested project command"),("explanation","Run the command requested for this task.")] {
            let required=schema["required"].as_array().is_some_and(|keys|keys.iter().any(|k|k.as_str()==Some(field)));
            if required && schema["properties"][field]["type"]=="string" && args.get(field).is_none() {
                args[field]=json!(text);changed=true;
            }
        }
        if changed {call["function"]["arguments"]=json!(args.to_string());}
    }
    result
}

/// Equivalent Windows paths must name the same host document URI as well as
/// compare equally in the guard. Never rewrite source text or relative paths.
pub fn canonical_paths(payload:&Value,message:&Value)->Value {
    if catalog(payload).is_none() {return message.clone();}
    fn canonical(path:&str)->Option<String> {
        let bytes=path.as_bytes();
        if !bytes.first().is_some_and(u8::is_ascii_alphabetic)||bytes.get(1)!=Some(&b':')||!matches!(bytes.get(2),Some(b'/'|b'\\')) {return None;}
        key(path)?;
        let rest=path[2..].split(['/', '\\']).filter(|s|!s.is_empty()).collect::<Vec<_>>().join("\\");
        Some(format!("{}:\\{rest}",path[..1].to_ascii_uppercase()))
    }
    let mut result=message.clone();
    for c in result["tool_calls"].as_array_mut().into_iter().flatten() {
        let Some(name)=c["function"]["name"].as_str() else {continue;};
        if !matches!(name,"read_file"|"create_file"|"replace_string_in_file"|"get_errors"|"list_dir") {continue;}
        let Ok(mut a)=serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) else {continue;};
        for field in ["filePath","path"] {if let Some(p)=a[field].as_str().and_then(canonical) {a[field]=json!(p);}}
        if let Some(paths)=a["filePaths"].as_array_mut() {for p in paths {if let Some(s)=p.as_str().and_then(canonical) {*p=json!(s);}}}
        c["function"]["arguments"]=json!(a.to_string());
    }
    result
}

fn successful_result(text:&str)->bool {
    let lower=text.trim_start().to_ascii_lowercase();
    !["error","string replacement failed:","tool execution failed","failed to"].iter().any(|s|lower.starts_with(s))
}

fn terminal_exit(text:&str)->Option<bool> {
    // Header-based hosts precede program output; VS Code's native terminal
    // appends its status after the output instead.
    text.lines().find_map(|line| {
        let line=line.trim().to_ascii_lowercase().replace(' ',"");
        line.strip_prefix("exitcode:").or_else(||line.strip_prefix("processexitedwithcode")).map(|code|code.trim_end_matches('.')=="0")
    }).or_else(||text.lines().rev().find(|line|!line.trim().is_empty()).and_then(|line| {
        let line=line.trim().to_ascii_lowercase().replace(' ',"");
        line.strip_prefix("commandexitedwithcode").map(|code|code.trim_end_matches('.')=="0")
    }))
}

pub fn compiler_failure(payload:&Value)->Option<String> {
    let h=history(payload)?;
    for result in h.iter().rev().filter(|m|m["role"]=="tool") {
        let Some(call)=h.iter().filter_map(|m|m["tool_calls"].as_array()).flatten().find(|c|c["id"]==result["tool_call_id"]) else {continue;};
        if call["function"]["name"]!="run_in_terminal" {continue;}
        let args:Value=serde_json::from_str(call["function"]["arguments"].as_str()?).ok()?;
        if !args["command"].as_str().is_some_and(validation_command) {continue;}
        let text=result["content"].as_str()?;
        return (terminal_exit(text)==Some(false)).then(||text.chars().take(3000).collect());
    }
    None
}

/// Host execution remains permission controlled. A language-service cache is
/// not a compiler result. Ask the model to select its project's real validator
/// when that host tool is available; otherwise report the verification gap.
pub fn compilation(payload:&Value,message:&Value)->Result<Option<Value>,String> {
    let Some(tools)=catalog(payload) else {return Ok(None);};
    if message["tool_calls"].as_array().is_some_and(|c|!c.is_empty()) {return Ok(None);}
    if message["content"].as_str().is_some_and(|s|s.starts_with("Verification incomplete:")) {return Ok(None);}
    let Some(h)=history(payload) else {return Ok(None);};
    // A broad coding catalog/policy is not evidence of a source change. An
    // API request or server-start command must not trigger unrelated builds.
    let mut needs=false;
    let mut verified=false;let mut attempts=0;
    for m in h {
        for c in m["tool_calls"].as_array().into_iter().flatten() {
            let Some(result)=h.iter().find(|r|r["role"]=="tool"&&r["tool_call_id"]==c["id"]).and_then(|r|r["content"].as_str()) else {continue;};
            if matches!(c["function"]["name"].as_str(),Some("create_file"|"replace_string_in_file")) && successful_result(result) {needs=true;verified=false;}
            if c["function"]["name"]=="run_in_terminal" {
                let Ok(a)=serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) else {continue;};
                if a["command"].as_str().is_some_and(validation_command) {
                    attempts+=1;
                    // The host's exit header wins over any code-looking text
                    // printed later by the program under test.
                    verified=terminal_exit(result).unwrap_or(false);
                }
            }
        }
    }
    if !needs||verified {return Ok(None);}
    if !tools.iter().any(|t|t["function"]["name"]=="run_in_terminal") {
        return Ok(Some(incomplete("file edits and editor diagnostics were checked, but compiler/build and runtime validation were not run because command execution is unavailable for this turn.")));
    }
    if attempts>=3 {return Ok(Some(incomplete("compiler/build validation did not pass within three attempts. Inspect the host's actual errors before continuing.")));}
    Err("native compilation verification required: editor diagnostics alone do not verify source. Choose the project's actual compiler/typecheck/build or test command using run_in_terminal; inspect its real errors and fix the source before claiming completion. Do not run unrelated commands or claim a command was executed without a host result".into())
}

fn validation_command(command:&str)->bool {
    command.split([';','\n']).flat_map(|part|part.split("&&")).any(|part| {
        let command=part.trim().to_ascii_lowercase();
        ["node --check ","node -c ","node --test","npx --no-install tsc","tsc ","cargo check","cargo test","python -m py_compile ","python3 -m py_compile ","python -m compileall ","python3 -m compileall ","pytest","python -m pytest","python3 -m pytest","npm test","npm run build","npm run typecheck","pnpm test","pnpm build","pnpm typecheck","mvn test","mvn compile","mvn verify","./gradlew build","./gradlew test","gradle build","gradle test","dotnet build","dotnet test","go build","go test","cmake --build ","make ","gcc ","g++ ","clang ","clang++ ","javac ","kotlinc ","swift build","swift test","ruby -c ","php -l "].iter().any(|p|command.starts_with(p))
    })
}

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
                Some("create_file"|"replace_string_in_file")=>{reads=0;rounds=0;paths.clear();},
                Some("get_errors")=>{reads+=1;rounds+=1;},
                Some("read_file"|"list_dir"|"file_search"|"grep_search")=>{
                    reads+=1;
                    if let Ok(a)=serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) {
                        if let Some(path)=a["filePath"].as_str() {if let Some(k)=key(path) {paths.insert(k,path.to_string());}}
                    }
                },_=>{}
            }
        }
    }
    if reads<8 || paths.is_empty() {return None;}
    if rounds>=1 {return Some(incomplete("inspection repeated without repairs after actionable host diagnostics."));}
    Some(call("get_errors",json!({"filePaths":paths.into_values().take(32).collect::<Vec<_>>()}),"call_verify_progress_"))
}

fn history(payload: &Value) -> Option<&[Value]> {
    let messages = payload["messages"].as_array()?;
    // VS Code can insert user-role reminders between host tool results. They
    // must not erase evidence of edits made for the current tagged request.
    let start = messages.iter().rposition(|m|m["role"]=="user" && m["content"].as_str().is_some_and(|s|s.contains("<userRequest>")))
        .or_else(||messages.iter().rposition(|m|m["role"]=="user"))? + 1;
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
    if reads >= 2 { return Some(incomplete(&format!("the proposed edit still failed validation after two recovery reads: {error}."))); }
    // First recovery expands a partial read; the second moves forward rather
    // than repeatedly reading the same initial ten lines.
    let mut read=call("read_file", json!({"filePath":path,"startLine":end+1,"endLine":end+400}), "call_grounding_");
    read["content"]=json!(format!("The proposed edit was NOT executed: {error}. Read the exact target returned by this call, then use replace_string_in_file with oldString copied exactly from observed source. Do not use create_file to replace an existing path. No mutation has been applied by this recovery step."));
    Some(read)
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
    let mut failed = std::collections::BTreeSet::new();
    let mut rounds = 0;
    for m in h {
        let Some(calls) = m["tool_calls"].as_array() else { continue; };
        for c in calls {
            let Ok(args) = serde_json::from_str::<Value>(c["function"]["arguments"].as_str().unwrap_or("")) else { continue; };
            let result = h.iter().find(|r| r["role"] == "tool" && r["tool_call_id"] == c["id"]);
            let Some(text) = result.and_then(|r| r["content"].as_str()) else { continue; };
            if !successful_result(text) {
                if matches!(c["function"]["name"].as_str(),Some("create_file"|"replace_string_in_file")) {if let Some(k)=args["filePath"].as_str().and_then(key) {failed.insert(k);}}
                continue;
            }
            match c["function"]["name"].as_str() {
                Some("create_file" | "replace_string_in_file") => {
                    if let Some(path) = args["filePath"].as_str() { if let Some(k) = key(path) { failed.remove(&k);pending.insert(k, path.to_string()); } }
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
    if !failed.is_empty() {return Some(incomplete("a file write failed and no subsequent successful repair of that target was observed."));}
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
        assert!(first["content"].as_str().unwrap().contains(error));
        assert!(first["content"].as_str().unwrap().contains("NOT executed"));
        assert!(first["content"].as_str().unwrap().contains("replace_string_in_file"));
        assert!(!first["content"].as_str().unwrap().contains("bad"));
        observe(&mut p,first,"partial observed source");
        let second=recover_read(&p,&edit,error).unwrap();assert_eq!(args(&second)["startLine"],401);
        observe(&mut p,second,"later source");
        let stopped=recover_read(&p,&edit,error).unwrap();assert!(stopped.get("tool_calls").is_none());
        assert!(stopped["content"].as_str().unwrap().contains("Verification incomplete"));
        assert!(stopped["content"].as_str().unwrap().contains(error));
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
    #[test]
    fn model_diagnostics_cannot_reset_an_inspection_loop_without_a_write() {
        let mut p=payload();
        let read=call("read_file",json!({"filePath":"/src/main.rs","startLine":1,"endLine":50}),"read_");
        for _ in 0..4 {
            observe(&mut p,read.clone(),"source");
            observe(&mut p,call("get_errors",json!({"filePaths":["/src/main.rs"]}),"model_check_"),"No errors found");
        }
        assert!(inspection_progress(&p,&read).unwrap()["content"].as_str().unwrap().contains("Verification incomplete"));
    }
    #[test]
    fn equivalent_windows_paths_use_one_host_document_without_rewriting_source() {
        let p=payload();
        let message=call("replace_string_in_file",json!({"filePath":r"d:\\sample\\main.py","oldString":r"literal d:\\path","newString":r"literal d:\\other"}),"edit_");
        let normalized=canonical_paths(&p,&message);let a=args(&normalized);
        assert_eq!(a["filePath"],r"D:\sample\main.py");
        assert_eq!(a["oldString"],args(&message)["oldString"]);
        let diagnostics=call("get_errors",json!({"filePaths":["d://sample//main.py","/src/main.rs"]}),"check_");
        assert_eq!(args(&canonical_paths(&p,&diagnostics))["filePaths"],json!([r"D:\sample\main.py","/src/main.rs"]));
    }
    #[test]
    fn failed_write_cannot_be_presented_as_verified_even_if_diagnostics_are_empty() {
        let mut p=payload();let path="/src/main.py";
        observe(&mut p,call("replace_string_in_file",json!({"filePath":path}),"bad_"),"String replacement failed: Input and output are identical");
        observe(&mut p,call("get_errors",json!({"filePaths":[path]}),"check_"),"No errors found");
        assert!(verification(&p,&json!({"content":"Fixed"})).unwrap()["content"].as_str().unwrap().contains("file write failed"));
        observe(&mut p,call("replace_string_in_file",json!({"filePath":path}),"fixed_"),"Applied edit");
        assert_eq!(verification(&p,&json!({"content":"Fixed"})).unwrap()["tool_calls"][0]["function"]["name"],"get_errors");
    }
    #[test]
    fn compiler_feedback_is_required_but_terminal_authority_is_never_added() {
        let mut p=payload();observe(&mut p,call("create_file",json!({"filePath":"/src/main.rs"}),"write_"),"Created");
        let answer=json!({"content":"Everything works"});
        assert!(compilation(&p,&answer).unwrap().unwrap()["content"].as_str().unwrap().contains("not run"));
        p["tools"].as_array_mut().unwrap().push(json!({"function":{"name":"run_in_terminal"}}));
        assert!(compilation(&p,&answer).unwrap_err().contains("verification required"));
        observe(&mut p,call("run_in_terminal",json!({"command":"echo cargo check"}),"not_validation_"),"Exit Code: 0");
        assert!(compilation(&p,&answer).is_err());
        observe(&mut p,call("run_in_terminal",json!({"command":"cargo check"}),"validation_"),"Exit Code: 1\ncompiler error");
        assert!(compilation(&p,&answer).is_err());
        observe(&mut p,call("run_in_terminal",json!({"command":"cargo check"}),"validation_"),"Exit Code: 1\nOutput:\nExit Code: 0");
        assert!(compilation(&p,&answer).is_err());
        // Use a fresh fixture to verify successful feedback independently.
        p=payload();observe(&mut p,call("create_file",json!({"filePath":"/src/main.rs"}),"write_"),"Created");
        p["tools"].as_array_mut().unwrap().push(json!({"function":{"name":"run_in_terminal"}}));
        observe(&mut p,call("run_in_terminal",json!({"command":"cargo check"}),"validation_"),"Exit Code: 0");
        assert!(compilation(&p,&answer).unwrap().is_none());
        observe(&mut p,call("replace_string_in_file",json!({"filePath":"/src/main.rs"}),"write_"),"Applied");
        assert!(compilation(&p,&answer).is_err());
    }
    #[test]
    fn api_terminal_work_does_not_require_an_unrelated_project_build() {
        let mut p=payload();p["messages"].as_array_mut().unwrap().insert(0,json!({"role":"system","name":"mundusx_native_coding_policy","content":"Generic coding guidance"}));
        p["tools"].as_array_mut().unwrap().push(json!({"function":{"name":"run_in_terminal"}}));
        observe(&mut p,call("run_in_terminal",json!({"command":"curl.exe -X POST http://localhost:3000/api/students"}),"http_"),"Exit Code: 0\nHTTP 201 Created");
        assert!(compilation(&p,&json!({"content":"Created the requested record"})).unwrap().is_none());
        observe(&mut p,call("create_file",json!({"filePath":"/src/server.js"}),"write_"),"Created");
        assert!(compilation(&p,&json!({"content":"Created"})).is_err(),"real source changes still require validation");
    }
    #[test]
    fn native_vscode_compiler_footer_and_node_tests_are_observed() {
        let mut p=payload();p["tools"].as_array_mut().unwrap().push(json!({"function":{"name":"run_in_terminal"}}));
        observe(&mut p,call("create_file",json!({"filePath":"/src/main.js"}),"write_"),"Created");
        let failure="/src/main.js:42\nSyntaxError: Invalid or unexpected token\n\nCommand exited with code 1\n";
        observe(&mut p,call("run_in_terminal",json!({"command":"node --check /src/main.js"}),"check_"),failure);
        assert_eq!(compiler_failure(&p).as_deref(),Some(failure));
        assert!(compilation(&p,&json!({"content":"Done"})).is_err());
        observe(&mut p,call("run_in_terminal",json!({"command":"node --test tests/course.test.js"}),"test_"),"tests 12\npass 12\nCommand exited with code 0\n");
        assert!(compiler_failure(&p).is_none());assert!(compilation(&p,&json!({"content":"Done"})).unwrap().is_none());
        assert_eq!(terminal_exit("Exit Code: 1\nOutput:\nCommand exited with code 0"),Some(false));
        assert_eq!(terminal_exit("output mentions Command exited with code 0, but is not a status"),None);
    }
    #[test]
    fn host_reminders_keep_source_verification_scoped_to_the_actual_request() {
        let mut p=payload();p["messages"][0]["content"]=json!("<userRequest>Create the project</userRequest>");
        p["tools"].as_array_mut().unwrap().push(json!({"function":{"name":"run_in_terminal"}}));
        observe(&mut p,call("create_file",json!({"filePath":"/src/server.js"}),"write_"),"Created");
        p["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":"Host reminder: continue the current task"}));
        assert!(compilation(&p,&json!({"content":"Done"})).is_err());
        p["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":"<userRequest>Send an API request using curl</userRequest>"}));
        assert!(compilation(&p,&json!({"content":"HTTP result"})).unwrap().is_none());
    }
}
