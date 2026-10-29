//! Provider-independent host-tool recovery. Never executes or invents a tool.
use serde_json::{json,Value};

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
 #[test]fn repair_uses_the_same_contract_for_every_provider(){for model in ["qwen3-coder","nemotron","other"] {let p=json!({"model":model,"messages":[{"role":"user","content":"start the server"}],"tools":[{"function":{"name":"read_file"}},{"function":{"name":"run_in_terminal","parameters":{"required":["command","mode"],"properties":{"mode":{"enum":["sync","async"]}}}}}]});let r=repair_request(&p,"native tool schema precondition failed: run_in_terminal.mode is required");assert_eq!(r["model"],model);assert_eq!(r["tools"],json!([p["tools"][1]]));assert_eq!(r["messages"][0],p["messages"][0]);assert!(r["messages"][1]["content"].as_str().unwrap().contains("async"));assert!(r["messages"][1]["content"].as_str().unwrap().contains("sync"));}}
 #[test]fn explicit_none_and_selected_function_are_preserved(){let mut p=json!({"messages":[],"tools":[{"function":{"name":"read_file"}}],"tool_choice":"none"});assert_eq!(repair_request(&p,"native tool schema precondition failed: read_file.path is required")["tool_choice"],"none");p["tool_choice"]=json!({"function":{"name":"read_file"}});assert_eq!(repair_request(&p,"native tool schema precondition failed: run_in_terminal.mode is required")["tool_choice"]["function"]["name"],"read_file");}
 fn call(name:&str,id:&str,file:&str)->Value{json!({"id":id,"function":{"name":name,"arguments":json!({"filePath":file}).to_string()}})}
 #[test]fn existing_target_recreation_is_blocked_but_a_fresh_missing_read_invalidates_it(){let proposed=json!({"tool_calls":[call("create_file","new","D:/p/script.js")]});let mut p=json!({"messages":[{"role":"assistant","tool_calls":[call("create_file","old","d:\\p\\script.js")]},{"role":"tool","tool_call_id":"old","content":"Error: file already exists"}]});assert!(reject_existing_create(&p,&proposed).is_err());assert!(reject_existing_create(&p,&json!({"tool_calls":[call("create_file","other","D:/p/other.js")]})).is_ok());p["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","tool_calls":[call("read_file","read","D:/p/script.js")]}),json!({"role":"tool","tool_call_id":"read","content":"const errorCode = 'ENOENT';"})]);assert!(reject_existing_create(&p,&proposed).is_err());p["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","tool_calls":[call("read_file","missing","D:/p/script.js")]}),json!({"role":"tool","tool_call_id":"missing","content":"ENOENT: file not found"})]);assert!(reject_existing_create(&p,&proposed).is_ok());}
}
