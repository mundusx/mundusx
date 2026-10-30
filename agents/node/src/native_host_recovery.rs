//! Provider-independent host-tool recovery. Never executes or invents a tool.
use serde_json::{json,Value};

fn native_edits(payload:&Value)->bool {
    payload["tools"].as_array().is_some_and(|tools|tools.iter().any(|t|
        matches!(t["function"]["name"].as_str(),Some("replace_string_in_file"|"multi_replace_string_in_file"|"insert_edit_into_file"))))
}

// Detect source embedded in shell writes, not ordinary build/test commands or
// file names mentioned as arguments. Keep structured source in the host editor.
fn source_write(command:&str)->bool {
    let c=command.to_ascii_lowercase();
    let write=["set-content", "add-content", "out-file", "writealltext", "writeallbytes"].iter().any(|s|c.contains(s))
        || (c.contains('>') && ["echo ", "printf ", "cat "].iter().any(|s|c.contains(s)));
    write && (command.contains('\n') || ["<!doctype", "<html", "<script", "<body"].iter().any(|s|c.contains(s)))
}

fn current(payload:&Value)->&[Value] {
    let Some(messages)=payload["messages"].as_array() else{return &[]};
    let start=super::native_task_progress::request_index(messages).map_or(0,|i|i+1);
    &messages[start..]
}

fn matched<'a>(messages:&'a [Value],index:usize,result:&Value)->Option<&'a Value> {
    let id=result["tool_call_id"].as_str()?;
    messages[..index].iter().rev().filter(|m|m["role"]=="assistant")
        .filter_map(|m|m["tool_calls"].as_array()).flatten().find(|c|c["id"]==id)
}

fn failed(text:&str)->bool {
    let text=text.trim_start().to_ascii_lowercase();
    ["error", "failed", "string replacement failed", "tool execution failed"].iter().any(|s|text.starts_with(s))
}

fn background_id(text:&str)->Option<&str> {
    let id=["moved to background terminal ID ","moved to the background using the ID "].iter()
        .find_map(|prefix|text.split_once(prefix).map(|p|p.1.split_whitespace().next().unwrap_or("").trim_end_matches([';',',','.'])))?;
    (id.len()==36 && id.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_hexdigit()})).then_some(id)
}

fn pending_source_writes(payload:&Value)->Vec<String> {
    let messages=current(payload);let mut pending=std::collections::BTreeSet::new();
    for (i,result) in messages.iter().enumerate().filter(|(_,m)|m["role"]=="tool") {
        let Some(call)=matched(messages,i,result) else{continue};
        let Some(a)=args(call) else{continue};
        let text=result["content"].as_str().unwrap_or("");
        if call["function"]["name"]=="run_in_terminal" && a["command"].as_str().is_some_and(source_write) {
            if let Some(id)=background_id(text){pending.insert(id.to_owned());}
        }
        if call["function"]["name"]=="kill_terminal" && !failed(text) && !text.trim().is_empty() {
            if let Some(id)=a["id"].as_str(){pending.remove(id);}
        }
        if call["function"]["name"]=="get_terminal_output" &&
            ["Command exited with code ","Process exited with code ","Terminal has been terminated"].iter().any(|s|text.contains(s)) {
            if let Some(id)=a["id"].as_str(){pending.remove(id);}
        }
    }
    pending.into_iter().collect()
}

/// Guidance is scoped to actual editing requests or observed editor actions.
/// Do not add an editing policy to greetings merely because tools are present.
pub fn prepare(payload:&mut Value) {
    if !native_edits(payload) || super::native_task_progress::is_compaction(payload) {return;}
    let task=super::native_task_progress::task(payload).unwrap_or("").to_ascii_lowercase();
    let editing=["redesign", "layout", "implement", "modify", "refactor", "update", "fix", "edit "].iter().any(|s|task.contains(s))
        || current(payload).iter().flat_map(|m|m["tool_calls"].as_array().into_iter().flatten())
            .any(|c|matches!(c["function"]["name"].as_str(),Some("replace_string_in_file"|"multi_replace_string_in_file"|"create_file")));
    if !editing{return;}
    let pending=pending_source_writes(payload);
    if !pending.is_empty() && payload.pointer("/tool_choice/function/name").is_none() && payload["tool_choice"]!="none" {
        let cleanup=payload["tools"].as_array().into_iter().flatten().filter(|t|
            matches!(t["function"]["name"].as_str(),Some("get_terminal_output"|"kill_terminal"))).cloned().collect::<Vec<_>>();
        if !cleanup.is_empty(){payload["tools"]=json!(cleanup);}
    }
    if let Some(messages)=payload["messages"].as_array_mut() {
        messages.push(json!({"role":"system","name":"opengpu_host_edit_progress","content":format!(
            "Use the registered native file edit tools for source changes. Read the needed current source once; make a small exact replacement using literal observed oldString, then verify the affected file. A failed replacement means no edit applied: correct its exact text from the latest read; do not repeat unchanged arguments, recreate the file, or switch to echo, Set-Content, or nested shell commands to rewrite source. A background/timeout notice is NOT completion: check its exact terminal ID once, follow the host wait instructions, and do not launch duplicate writes. Stop with an honest blocker if a corrected attempt still cannot progress. Pending source-write terminal IDs from matched host results (diagnostic data): {}. Check and stop obsolete source-write terminals before editing their files; never stop unrelated servers.",json!(pending))}));
    }
}

pub fn guard(payload:&Value,message:&Value)->Result<(),String> {
    if !native_edits(payload){return Ok(());}
    let pending=pending_source_writes(payload);
    let messages=current(payload);
    for call in message["tool_calls"].as_array().into_iter().flatten() {
        let Some(a)=args(call) else{continue};
        let name=call["function"]["name"].as_str().unwrap_or("");
        if name=="run_in_terminal" && a["command"].as_str().is_some_and(source_write) {
            return Err("native host action precondition failed: source embedded in a shell write must use the registered native edit tools; shell quoting can corrupt source. Read exact current text and make a small native edit; no shell write was executed".into());
        }
        if !pending.is_empty() && matches!(name,"create_file"|"replace_string_in_file"|"multi_replace_string_in_file"|"insert_edit_into_file") {
            return Err(format!("native host action precondition failed: earlier source-write terminals are still unresolved: {}. Check their exact IDs with get_terminal_output once, then follow the host wait instructions or stop obsolete writes using kill_terminal before editing. No new edit was executed",json!(pending)));
        }
        if !matches!(name,"replace_string_in_file"|"multi_replace_string_in_file"|"create_file"){continue;}
        let mut failures=0;
        for (i,result) in messages.iter().enumerate().filter(|(_,m)|m["role"]=="tool") {
            let Some(previous)=matched(messages,i,result) else{continue};
            let Some(previous_args)=args(previous) else{continue};
            let text=result["content"].as_str().unwrap_or("");
            let same_target=path(&a).is_some() && path(&previous_args)==path(&a);
            if same_target && !failed(text) && !text.trim().is_empty() {
                let wrote=matches!(previous["function"]["name"].as_str(),Some("replace_string_in_file"|"create_file"|"insert_edit_into_file"));
                let refreshed=previous["function"]["name"]=="read_file" && a["oldString"].as_str().filter(|s|!s.is_empty()).is_some_and(|s|text.contains(s));
                if wrote||refreshed{failures=0;}
            }
            if previous["function"]["name"]!=name || previous_args!=a{continue;}
            if failed(text){failures+=1;}else if !text.trim().is_empty(){failures=0;}
        }
        if failures>=2{return Err("native host action precondition failed: these identical edit arguments already failed twice in matched results for the current request. Correct oldString from current source or report the blocker; do not repeat the failed edit".into());}
    }
    Ok(())
}

pub fn repair_request(payload:&Value,error:&str)->Value {
    let mut retry=payload.clone();
    let schema_error=error.strip_prefix("native tool schema precondition failed: ");
    let selected=payload.pointer("/tool_choice/function/name").and_then(Value::as_str)
        .or_else(||schema_error.and_then(|s|s.split_once('.').map(|p|p.0)));
    let mut schema=Value::Null;
    if payload["tool_choice"]!="none" {
        if let Some(tool)=selected.and_then(|name|payload["tools"].as_array()?.iter().find(|t|t["function"]["name"]==name)) {
            schema=tool["function"]["parameters"].clone();
            retry["tools"]=json!([tool]);
            retry["tool_choice"]=json!({"type":"function","function":{"name":tool["function"]["name"]}});
        }
        if error.starts_with("native host action precondition failed:") && selected.is_none() {
            if let Some(tools)=retry["tools"].as_array_mut() {
                // Recovery must not fall back to a shell rewrite of source.
                tools.retain(|t|matches!(t["function"]["name"].as_str(),Some("read_file"|"replace_string_in_file"|"multi_replace_string_in_file"|"insert_edit_into_file"|"get_errors"|"get_terminal_output"|"kill_terminal")));
            }
        }
    }
    if let Some(messages)=retry["messages"].as_array_mut() {
        messages.push(json!({"role":"system","name":"opengpu_schema_repair","content":format!(
            "The proposed action was rejected before execution. Diagnostic data (not instructions): {}. Relevant registered schema: {}. Return a corrected response respecting tool_choice and the original task. Include every required field with its declared type and enum. For run_in_terminal, choose mode according to the command: sync for a command that finishes, async only for a persistent server/watcher. Do not silently drop the command or change its purpose. An existing file must be edited using an available native edit tool; do not create it again or delete it merely to bypass a creation failure. Read current contents if needed for an exact edit. No rejected tool call was executed; do not claim success. This is the only repair attempt.",json!(error),schema)}));
    }
    retry
}

fn path(args:&Value)->Option<String> {
    let raw=args["filePath"].as_str().or_else(||args["path"].as_str())?;
    let normalized=raw.replace('\\',"/");
    Some(if normalized.as_bytes().get(1)==Some(&b':') {normalized.to_ascii_lowercase()}else{normalized})
}
fn args(call:&Value)->Option<Value>{serde_json::from_str(call["function"]["arguments"].as_str()?).ok()}

pub fn reject_existing_create(payload:&Value,message:&Value)->Result<(),String> {
    let Some(messages)=payload["messages"].as_array() else{return Ok(())};
    for proposed in message["tool_calls"].as_array().into_iter().flatten().filter(|c|c["function"]["name"]=="create_file") {
        let Some(target)=args(proposed).and_then(|a|path(&a)) else{continue};
        let mut exists=false;
        for (index,result) in messages.iter().enumerate().filter(|(_,m)|m["role"]=="tool") {
            let Some(id)=result["tool_call_id"].as_str() else{continue};
            let call=messages[..index].iter().rev().filter_map(|m|m["tool_calls"].as_array()).flatten().find(|c|c["id"]==id);
            let Some(call)=call else{continue};
            if args(call).and_then(|a|path(&a)).as_deref()!=Some(target.as_str()){continue}
            let text=result["content"].as_str().unwrap_or("").to_ascii_lowercase();
            if call["function"]["name"]=="create_file" && ["already exists","eexist","file exists"].iter().any(|s|text.contains(s)){exists=true;}
            let diagnostic=text.trim_start();
            let read_error=["error", "enoent", "file not found"].iter().any(|prefix|diagnostic.starts_with(prefix));
            if call["function"]["name"]=="read_file" && read_error && ["file not found","enoent","does not exist"].iter().any(|s|text.contains(s)){exists=false;}
        }
        if exists{return Err("native host action precondition failed: create_file already failed because this target exists; use a registered edit tool, or read current contents before editing; do not delete and recreate it".into())}
    }
    Ok(())
}

#[cfg(test)]mod tests {
 use super::*;
 fn fixture()->Value {json!({"messages":[{"role":"user","content":"<userRequest>Redesign the layout</userRequest>"}],"tools":[{"function":{"name":"run_in_terminal"}},{"function":{"name":"create_file"}},{"function":{"name":"replace_string_in_file"}},{"function":{"name":"read_file"}},{"function":{"name":"get_terminal_output"}},{"function":{"name":"kill_terminal"}}]})}
 fn action(name:&str,id:&str,a:Value)->Value {json!({"id":id,"function":{"name":name,"arguments":a.to_string()}})}
 fn result(p:&mut Value,call:Value,text:&str){let id=call["id"].clone();p["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","tool_calls":[call]}),json!({"role":"tool","tool_call_id":id,"content":text})]);}
 fn proposed(call:Value)->Value {json!({"role":"assistant","tool_calls":[call]})}
 #[test]fn shell_source_writes_are_rejected_without_blocking_normal_terminal_work(){let p=fixture();for command in ["powershell -Command \"Set-Content -Path index.html -Value '<html lang=\"en\">\n</html>'\"", "echo '<!DOCTYPE html>' > index.html"] {assert!(guard(&p,&proposed(action("run_in_terminal","new",json!({"command":command})))).is_err());}for command in ["npm test", "npm start", "Get-Content index.html", "Set-Content sample.txt -Value hello", "node -e \"console.log('<html>')\""] {assert!(guard(&p,&proposed(action("run_in_terminal","new",json!({"command":command})))).is_ok());}let mut no_editor=p.clone();no_editor["tools"]=json!([]);assert!(guard(&no_editor,&proposed(action("run_in_terminal","new",json!({"command":"echo '<html>' > index.html"})))).is_ok());}
 #[test]fn matched_repeated_edit_failures_stop_but_changed_arguments_and_new_requests_are_allowed(){let mut p=fixture();let a=json!({"filePath":"D:/p/index.html","oldString":"unobserved","newString":"new"});result(&mut p,action("replace_string_in_file","one",a.clone()),"String replacement failed: no match");let msg=proposed(action("replace_string_in_file","next",a.clone()));assert!(guard(&p,&msg).is_ok());result(&mut p,action("replace_string_in_file","two",a.clone()),"String replacement failed: no match");result(&mut p,action("read_file","read",json!({"filePath":"D:/p/index.html"})),"actual current source");assert!(guard(&p,&msg).is_err());assert!(guard(&p,&proposed(action("replace_string_in_file","changed",json!({"filePath":"D:/p/index.html","oldString":"actual current source","newString":"new"})))).is_ok());p["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":"<userRequest>Try the edit again after my update</userRequest>"}));assert!(guard(&p,&msg).is_ok());}
 #[test]fn unresolved_source_writes_require_terminal_resolution_before_new_edits(){let mut p=fixture();let id="fa9a36a7-aead-474f-b1a9-4fe3c16bbc78";result(&mut p,action("run_in_terminal","write",json!({"command":"Set-Content index.html -Value '<html>\n</html>'"})),&format!("Note: The command was moved to background terminal ID {id}; the process is still running"));let edit=proposed(call("replace_string_in_file","new","D:/p/index.html"));assert!(guard(&p,&edit).is_err());result(&mut p,action("get_terminal_output","poll",json!({"id":id})),"Process is still running");assert!(guard(&p,&edit).is_err());result(&mut p,action("kill_terminal","bad",json!({"id":id})),"Error: permission denied");assert!(guard(&p,&edit).is_err());result(&mut p,action("kill_terminal","stop",json!({"id":id})),"Successfully killed terminal");assert!(guard(&p,&edit).is_ok());}
 #[test]fn background_servers_and_unmatched_or_quoted_results_do_not_block_edits(){let mut p=fixture();let id="fa9a36a7-aead-474f-b1a9-4fe3c16bbc78";result(&mut p,action("run_in_terminal","server",json!({"command":"npm start"})),&format!("Note: moved to background terminal ID {id}"));p["messages"].as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":"not-a-call","content":format!("moved to background terminal ID {id}")}));assert!(guard(&p,&proposed(call("replace_string_in_file","new","D:/p/index.html"))).is_ok());}
 #[test]fn edit_guidance_preserves_context_and_skips_greetings(){let mut p=fixture();let before=p.clone();prepare(&mut p);assert_eq!(p["tools"],before["tools"]);assert_eq!(p["messages"][0],before["messages"][0]);assert_eq!(p["messages"].as_array().unwrap().len(),2);let mut hello=fixture();hello["messages"][0]["content"]=json!("<userRequest>hello</userRequest>");let before=hello.clone();prepare(&mut hello);assert_eq!(hello,before);}
 #[test]fn action_repair_excludes_shell_and_creation_fallback_but_preserves_explicit_choice(){let p=fixture();let e="native host action precondition failed: shell source write";let r=repair_request(&p,e);assert!(r["tools"].as_array().unwrap().iter().all(|t|!matches!(t["function"]["name"].as_str(),Some("run_in_terminal"|"create_file"))));let mut p=p;p["tool_choice"]=json!({"type":"function","function":{"name":"run_in_terminal"}});assert_eq!(repair_request(&p,e)["tool_choice"],p["tool_choice"]);}
 #[test]fn pending_write_projects_cleanup_tools_before_generation_but_preserves_explicit_choice(){let mut p=fixture();let id="fa9a36a7-aead-474f-b1a9-4fe3c16bbc78";result(&mut p,action("run_in_terminal","write",json!({"command":"echo '<html>' > index.html"})),&format!("moved to the background using the ID {id}"));let mut forced=p.clone();forced["tool_choice"]=json!({"type":"function","function":{"name":"read_file"}});let tools=forced["tools"].clone();prepare(&mut forced);assert_eq!(forced["tools"],tools);prepare(&mut p);assert_eq!(p["tools"].as_array().unwrap().len(),2);assert!(p["tools"].as_array().unwrap().iter().all(|t|matches!(t["function"]["name"].as_str(),Some("get_terminal_output"|"kill_terminal"))));assert!(p["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().contains(id));}
 #[test]fn changed_source_observations_reset_stale_edit_failures(){let mut p=fixture();let a=json!({"filePath":"D:/p/index.html","oldString":"now present","newString":"replacement"});for id in ["one","two"]{result(&mut p,action("replace_string_in_file",id,a.clone()),"String replacement failed: no match");}let edit=proposed(action("replace_string_in_file","next",a.clone()));assert!(guard(&p,&edit).is_err());result(&mut p,action("read_file","fresh",json!({"filePath":"D:/p/index.html"})),"External edit: now present");assert!(guard(&p,&edit).is_ok());for id in ["three","four"]{result(&mut p,action("replace_string_in_file",id,a.clone()),"String replacement failed: no match");}assert!(guard(&p,&edit).is_err());result(&mut p,action("replace_string_in_file","other",json!({"filePath":"D:/p/index.html","oldString":"earlier","newString":"now present"})),"Successfully applied edit");assert!(guard(&p,&edit).is_ok());}
 #[test]fn repair_uses_the_same_contract_for_every_provider(){for model in ["qwen3-coder","nemotron","other"] {let p=json!({"model":model,"messages":[{"role":"user","content":"start the server"}],"tools":[{"function":{"name":"read_file"}},{"function":{"name":"run_in_terminal","parameters":{"required":["command","mode"],"properties":{"mode":{"enum":["sync","async"]}}}}}]});let r=repair_request(&p,"native tool schema precondition failed: run_in_terminal.mode is required");assert_eq!(r["model"],model);assert_eq!(r["tools"],json!([p["tools"][1]]));assert_eq!(r["messages"][0],p["messages"][0]);assert!(r["messages"][1]["content"].as_str().unwrap().contains("async"));assert!(r["messages"][1]["content"].as_str().unwrap().contains("sync"));}}
 #[test]fn explicit_none_and_selected_function_are_preserved(){let mut p=json!({"messages":[],"tools":[{"function":{"name":"read_file"}}],"tool_choice":"none"});assert_eq!(repair_request(&p,"native tool schema precondition failed: read_file.path is required")["tool_choice"],"none");p["tool_choice"]=json!({"function":{"name":"read_file"}});assert_eq!(repair_request(&p,"native tool schema precondition failed: run_in_terminal.mode is required")["tool_choice"]["function"]["name"],"read_file");}
 fn call(name:&str,id:&str,file:&str)->Value{json!({"id":id,"function":{"name":name,"arguments":json!({"filePath":file}).to_string()}})}
 #[test]fn existing_target_recreation_is_blocked_but_a_fresh_missing_read_invalidates_it(){let proposed=json!({"tool_calls":[call("create_file","new","D:/p/script.js")]});let mut p=json!({"messages":[{"role":"assistant","tool_calls":[call("create_file","old","d:\\p\\script.js")]},{"role":"tool","tool_call_id":"old","content":"Error: file already exists"}]});assert!(reject_existing_create(&p,&proposed).is_err());assert!(reject_existing_create(&p,&json!({"tool_calls":[call("create_file","other","D:/p/other.js")]})).is_ok());p["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","tool_calls":[call("read_file","read","D:/p/script.js")]}),json!({"role":"tool","tool_call_id":"read","content":"const errorCode = 'ENOENT';"})]);assert!(reject_existing_create(&p,&proposed).is_err());p["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","tool_calls":[call("read_file","missing","D:/p/script.js")]}),json!({"role":"tool","tool_call_id":"missing","content":"ENOENT: file not found"})]);assert!(reject_existing_create(&p,&proposed).is_ok());}
}
