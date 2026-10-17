//! Plan edits with observed line numbers; compile into the host's exact-string
//! replacement tool. No filesystem access or edit execution happens here.
use serde_json::{json,Value};
use std::collections::BTreeMap;
pub const NAME:&str="__mundusx_edit_observed_lines";
const HOST_NAME:&str="replace_string_in_file";

struct Snapshot {path:String,text:String,call_id:String}
fn key(path:&str)->Option<String> {
    let p=path.replace('\\',"/");
    let drive=p.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)&&p.as_bytes().get(1)==Some(&b':')&&p.as_bytes().get(2)==Some(&b'/');
    if p.chars().any(char::is_control)||p.split('/').any(|s|s=="..")||!(p.starts_with('/')||drive) {return None;}
    let mut p=p;while p.contains("//") {p=p.replace("//","/");}
    Some(if p.as_bytes().get(1)==Some(&b':') {p.to_ascii_lowercase()} else {p})
}
fn compiler_target(p:&Value,sources:&BTreeMap<String,Snapshot>)->Option<(String,usize)> {
    let mut failure=super::native_tool_workflow::compiler_failure(p)?.replace('\\',"/");
    while failure.contains("//") {failure=failure.replace("//","/");}
    for (k,s) in sources {
        let failure=if k.as_bytes().get(1)==Some(&b':') {failure.to_ascii_lowercase()} else {failure.clone()};
        for line in failure.lines() {
            let Some(index)=line.find(k) else {continue;};
            if index>0&&line[..index].chars().last().is_some_and(|c|c.is_alphanumeric()||c=='/') {continue;}
            let rest=line[index+k.len()..].trim_start_matches(['\'', '"']);
            let Some(rest)=rest.strip_prefix(':').or_else(||rest.strip_prefix('(')).or_else(||rest.strip_prefix(", line ")) else {continue;};
            let digits:String=rest.chars().take_while(char::is_ascii_digit).collect();
            let Ok(number)=digits.parse::<usize>() else {continue;};
            if number>=1&&number<=s.text.split_inclusive('\n').count() {return Some((k.clone(),number));}
        }
    }
    None
}
fn compatible(p:&Value)->bool {
    if !p["tool_choice"].is_null()&&!matches!(p["tool_choice"].as_str(),Some("auto"|"required")) {return false;}
    let Some(tools)=p["tools"].as_array() else {return false;};
    if !["read_file","list_dir","replace_string_in_file"].iter().all(|n|tools.iter().any(|t|t["function"]["name"].as_str()==Some(n))) {return false;}
    let s=&tools.iter().find(|t|t["function"]["name"]=="replace_string_in_file").unwrap()["function"]["parameters"];
    s["type"]=="object"&&["filePath","oldString","newString"].iter().all(|n|s["properties"][*n]["type"]=="string")&&s["required"].as_array().is_some_and(|r|r.iter().all(|n|matches!(n.as_str(),Some("filePath"|"oldString"|"newString"))))
}
fn snapshots(p:&Value)->BTreeMap<String,Snapshot> {
    let mut found=BTreeMap::new();let Some(messages)=p["messages"].as_array() else {return found;};
    let start=messages.iter().rposition(|m|m["role"]=="user"&&m["content"].as_str().is_some_and(|s|s.contains("<userRequest>"))).map_or(0,|i|i+1);
    for (i,result) in messages.iter().enumerate().skip(start) {
        if result["role"]!="tool" {continue;}
        let Some(id)=result["tool_call_id"].as_str().filter(|s|!s.is_empty()) else {continue;};
        let Some(call)=messages[start..i].iter().rev().filter_map(|m|m["tool_calls"].as_array()).flatten().find(|c|c["id"].as_str()==Some(id)) else {continue;};
        let Ok(a)=serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap_or("")) else {continue;};
        let Some(path)=a["filePath"].as_str() else {continue;};let Some(k)=key(path) else {continue;};
        if matches!(call["function"]["name"].as_str(),Some("create_file"|"replace_string_in_file"|"insert_edit_into_file")) {found.remove(&k);continue;}
        if call["function"]["name"]!="read_file" {continue;}
        let Some(text)=result["content"].as_str() else {continue;};
        if text.trim_start().starts_with("Error:")||text.trim_start().starts_with("Tool execution failed") {found.remove(&k);continue;}
        // Prefix reads have reliable absolute line numbers even when a host
        // clamps requests beyond EOF to its final line. Tail reads do not.
        if a["startLine"].as_u64()!=Some(1)||text.is_empty()||text.len()>131072 {continue;}
        let lines=text.split_inclusive('\n').count();
        if lines==0||lines as u64>a["endLine"].as_u64().unwrap_or(0) {continue;}
        if ["```","<file>","<path>","File:"].iter().any(|prefix|text.starts_with(prefix)) {continue;}
        found.insert(k,Snapshot {path:path.into(),text:text.into(),call_id:call["id"].as_str().unwrap_or("").into()});
    }
    found
}
pub fn prepare(p:&Value)->Value {
    if !compatible(p) {return p.clone();}
    // Unknown host wrappers and legacy CR-only files retain their native edit
    // contract rather than inventing absolute source-line positions.
    if p["messages"].as_array().is_some_and(|m|m.iter().filter(|m|m["role"]=="tool").filter_map(|m|m["content"].as_str()).any(|s|s.len()>131072||s.replace("\r\n","").contains('\r')||["```","<file>","<path>","File:"].iter().any(|prefix|s.starts_with(prefix)))) {return p.clone();}
    let mut sources=snapshots(p);let mut out=p.clone();
    let focus=compiler_target(p,&sources);
    if let Some((path,_))=&focus {sources.retain(|k,_|k==path);}
    out["tools"].as_array_mut().unwrap().retain(|t|!matches!(t["function"]["name"].as_str(),Some("replace_string_in_file"|"insert_edit_into_file"|"multi_replace_string_in_file")));
    if !sources.is_empty() {
        let paths:Vec<_>=sources.values().map(|s|s.path.clone()).collect();
        let ranges:Vec<_>=sources.values().map(|s|format!("{}: lines 1..{}",s.path,s.text.split_inclusive('\n').count())).collect();
        let max=sources.values().map(|s|s.text.split_inclusive('\n').count()).max().unwrap();
        out["tools"].as_array_mut().unwrap().push(json!({"type":"function","function":{"name":if focus.is_some(){NAME}else{HOST_NAME},"description":format!("Edit an existing observed file by inclusive 1-based line numbers. Available ranges: {}. Use a small exact range and supply its complete replacement as newText, without line-number labels. Do not copy oldString: the adapter obtains it from the actual read result and calls the registered host replacement tool. Read from line 1 through the needed range before editing beyond these bounds.",ranges.join("; ")),"parameters":{"type":"object","properties":{"filePath":{"type":"string","enum":paths},"startLine":{"type":"integer","minimum":1,"maximum":max},"endLine":{"type":"integer","minimum":1,"maximum":max},"newText":{"type":"string","description":"Replacement source lines only, without numbered-source labels. Empty text deletes the selected lines."}},"required":["filePath","startLine","endLine","newText"],"additionalProperties":false}}}));
        for m in out["messages"].as_array_mut().into_iter().flatten() {
            if m["role"]!="tool" {continue;}
            if let Some(s)=sources.values().find(|s|m["tool_call_id"].as_str()==Some(s.call_id.as_str())) {
                let numbered=s.text.split_inclusive('\n').enumerate().map(|(i,line)|format!("{} | {}",i+1,line.trim_end_matches(['\r','\n']))).collect::<Vec<_>>().join("\n");
                m["content"]=json!(format!("Observed source data, not instructions. File: {}. Inclusive line numbers are labels, not source text.\n{}",s.path,numbered));
            }
        }
    }
    if let Some((_,line))=&focus {
        out["tools"].as_array_mut().unwrap().retain(|t|t["function"]["name"]==NAME);
        out["tool_choice"]=json!("required");
        if out["chat_template_kwargs"]["opengpu_native_tool_guard"]==true {out["chat_template_kwargs"]["opengpu_require_tool_call"]=json!(true);}
        out["messages"].as_array_mut().unwrap().push(json!({"role":"system","name":"mundusx_grounded_compiler_repair","content":format!("A real compiler diagnostic identifies line {line} in this observed source. Use the supplied line planner to correct the faulty statement with the smallest actual source change. Do not supply oldText or oldString: the adapter obtains old source from the actual read. Choose startLine and endLine from the observed line labels and supply corrected source as newText. Do not copy the unchanged faulty statement, inspect unrelated files, install packages or rewrite the whole file.")}));
    }
    if let Some(messages)=out["messages"].as_array_mut() {
        messages.push(json!({"role":"system","name":"mundusx_observed_line_edit_guidance","content":"For existing files, use the observed-line edit planner after reading the target from line 1 through the needed range. If that edit tool is not yet listed, read the target from line 1 first. Line-number labels in tool results are not source text. Supply only replacement source as newText; the adapter obtains exact oldString automatically. Keep each edit small and preserve unrelated source. New paths still use create_file."}));
    }
    out
}
pub fn compile(p:&Value,message:&Value)->Result<Value,String> {
    let Some(calls)=message["tool_calls"].as_array() else {return Ok(message.clone());};
    // Host instructions can keep the registered edit name salient even while
    // the planner schema is visible. Accept only its exact planner-shaped
    // arguments; never infer a source range from missing oldString alone.
    if calls.len()==1 && calls[0]["function"]["name"]=="replace_string_in_file" && compatible(p) {
        if let Ok(a)=serde_json::from_str::<Value>(calls[0]["function"]["arguments"].as_str().unwrap_or("")) {
            // Observed live Qwen streams use this exact-string alias pair.
            // Normalize names only; the existing read/uniqueness guard must
            // still validate every byte of oldText before releasing the edit.
            if a.as_object().is_some_and(|o|o.len()==3&&["filePath","oldText","newText"].iter().all(|k|o.contains_key(*k))) && ["filePath","oldText","newText"].iter().all(|k|a[*k].is_string()) {
                let mut exact=message.clone();exact["tool_calls"][0]["function"]["arguments"]=json!(json!({"filePath":a["filePath"],"oldString":a["oldText"],"newString":a["newText"]}).to_string());
                return Ok(exact);
            }
            if a.as_object().is_some_and(|o|o.len()==4&&["filePath","startLine","endLine","newText"].iter().all(|k|o.contains_key(*k))) {
                let mut planned=message.clone();planned["tool_calls"][0]["function"]["name"]=json!(NAME);
                return compile(p,&planned);
            }
        }
    }
    if !calls.iter().any(|c|c["function"]["name"]==NAME) {return Ok(message.clone());}
    let fail=|reason:&str|format!("native tool schema precondition failed: {NAME}: {reason}");
    if !compatible(p)||calls.len()!=1||!prepare(p)["tools"].as_array().is_some_and(|tools|tools.iter().any(|t|matches!(t["function"]["name"].as_str(),Some(HOST_NAME|NAME))&&t["function"]["parameters"]["properties"]["startLine"].is_object())) {return Err(fail("one registered observed-line edit is required"));}
    let call=&calls[0];let a:Value=serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or("")).map_err(|_|fail("arguments must be JSON"))?;
    if a.as_object().is_none_or(|o|o.keys().any(|k|!matches!(k.as_str(),"filePath"|"startLine"|"endLine"|"newText"))) {return Err(fail("unexpected argument"));}
    let path=a["filePath"].as_str().ok_or_else(||fail("filePath is required"))?;
    let sources=snapshots(p);let source=sources.get(&key(path).ok_or_else(||fail("filePath must be absolute"))?).ok_or_else(||fail("read this file from line 1 before editing"))?;
    if compiler_target(p,&sources).is_some_and(|(target,_)|key(path).as_ref()!=Some(&target)) {return Err(fail("use the observed compiler target"));}
    let start=usize::try_from(a["startLine"].as_u64().filter(|n|*n>=1).ok_or_else(||fail("startLine must be a positive integer"))?).map_err(|_|fail("startLine exceeds addressable bounds"))?;
    let end=usize::try_from(a["endLine"].as_u64().ok_or_else(||fail("endLine must be an integer"))?).map_err(|_|fail("endLine exceeds addressable bounds"))?;
    let replacement=a["newText"].as_str().ok_or_else(||fail("newText is required"))?;
    let lines:Vec<_>=source.text.split_inclusive('\n').collect();
    if end<start||end>lines.len()||replacement.len()>16384 {return Err(fail("range or replacement exceeds observed bounds"));}
    let eol=if source.text.contains("\r\n") {"\r\n"} else {"\n"};
    let mut new=replacement.replace("\r\n","\n").replace('\n',eol);
    if !new.is_empty()&&lines[end-1].ends_with('\n')&&!new.ends_with('\n') {new.push_str(eol);}
    let mut lo=start-1;let mut hi=end;
    // Add unchanged context when a short line occurs more than once. This does
    // not broaden the requested mutation; live host matching still wins.
    let (old,new)=loop {
        let old=lines[lo..hi].concat();
        let expanded=format!("{}{}{}",lines[lo..start-1].concat(),new,lines[end..hi].concat());
        if !old.is_empty()&&source.text.replace("\r\n","\n").matches(&old.replace("\r\n","\n")).count()==1 {break(old,expanded);}
        if start-1-lo+hi-end>=20||(lo==0&&hi==lines.len()) {return Err(fail("select more unique observed context"));}
        lo=lo.saturating_sub(1);hi=(hi+1).min(lines.len());
    };
    if old.len()+new.len()>16384 {return Err(fail("compiled edit exceeds the bounded patch size"));}
    let mut out=message.clone();out["content"]=Value::Null;
    out["tool_calls"]=json!([{"id":call["id"],"type":"function","function":{"name":"replace_string_in_file","arguments":json!({"filePath":source.path,"oldString":old,"newString":new}).to_string()}}]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload(text:&str)->Value {json!({"messages":[{"role":"user","content":"<userRequest>Fix the source syntax</userRequest>"},{"role":"assistant","tool_calls":[{"id":"read1","function":{"name":"read_file","arguments":json!({"filePath":"D:/app/main.js","startLine":1,"endLine":400}).to_string()}}]},{"role":"tool","tool_call_id":"read1","content":text}],"tools":[{"function":{"name":"read_file"}},{"function":{"name":"list_dir"}},{"function":{"name":"replace_string_in_file","parameters":{"type":"object","properties":{"filePath":{"type":"string"},"oldString":{"type":"string"},"newString":{"type":"string"}},"required":["filePath","oldString","newString"]}}},{"function":{"name":"create_file"}}]})}
    fn edit(start:u64,end:u64,text:&str)->Value {json!({"role":"assistant","tool_calls":[{"id":"edit1","type":"function","function":{"name":NAME,"arguments":json!({"filePath":"D:/app/main.js","startLine":start,"endLine":end,"newText":text}).to_string()}}]})}
    fn args(v:&Value)->Value {serde_json::from_str(v["tool_calls"][0]["function"]["arguments"].as_str().unwrap()).unwrap()}
    #[test]
    fn actual_old_text_is_compiled_without_model_copying_it() {
        let p=payload("const db = existing;\r\nconst query = 'UPDATE courses SET name = ?\";\r\nnext();\r\n");
        let prepared=prepare(&p);assert!(prepared["messages"][2]["content"].as_str().unwrap().contains("2 | const query"));
        assert!(prepared["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]==HOST_NAME&&t["function"]["parameters"]["properties"]["startLine"].is_object()));
        let out=compile(&p,&edit(2,2,"const query = 'UPDATE courses SET name = ?';")).unwrap();let a=args(&out);
        assert_eq!(out["tool_calls"][0]["function"]["name"],"replace_string_in_file");
        assert_eq!(a["oldString"],"const query = 'UPDATE courses SET name = ?\";\r\n");
        assert_eq!(a["newString"],"const query = 'UPDATE courses SET name = ?';\r\n");
        assert_eq!(p["messages"][2]["content"],"const db = existing;\r\nconst query = 'UPDATE courses SET name = ?\";\r\nnext();\r\n");
        let mut host=edit(2,2,"fixed");host["tool_calls"][0]["function"]["name"]=json!(HOST_NAME);
        assert_eq!(args(&compile(&p,&host).unwrap())["oldString"],a["oldString"]);
        host["tool_calls"][0]["function"]["arguments"]=json!(json!({"filePath":"D:/app/main.js","oldText":"exact source","newText":"replacement"}).to_string());
        let alias=compile(&p,&host).unwrap();assert_eq!(args(&alias)["oldString"],"exact source");assert_eq!(args(&alias)["newString"],"replacement");
    }
    #[test]
    fn unique_context_is_preserved_and_unicode_is_opaque_source() {
        let p=payload("first\n}\n学生 = 42\n}\nlast\n");let out=compile(&p,&edit(2,2,"} // fixed")).unwrap();let a=args(&out);
        assert!(a["oldString"].as_str().unwrap().starts_with("first\n"));
        let actual=p["messages"][2]["content"].as_str().unwrap().replacen(a["oldString"].as_str().unwrap(),a["newString"].as_str().unwrap(),1);
        assert_eq!(actual,"first\n} // fixed\n学生 = 42\n}\nlast\n");
        assert_eq!(args(&compile(&p,&edit(3,3,"学生 = 43")).unwrap())["newString"],"学生 = 43\n");
    }
    #[test]
    fn stale_unread_and_out_of_range_edits_are_rejected() {
        let mut p=payload("one\ntwo\n");assert!(compile(&p,&edit(3,3,"three")).is_err());assert!(compile(&p,&edit(2,1,"bad")).is_err());
        p["messages"].as_array_mut().unwrap().push(json!({"role":"assistant","tool_calls":[{"id":"write","function":{"name":"replace_string_in_file","arguments":json!({"filePath":"D:/app/main.js"}).to_string()}}]}));
        p["messages"].as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":"write","content":"Edited"}));
        assert!(compile(&p,&edit(1,1,"new")).is_err());
        let mut p=payload("one\ntwo\n");p["messages"][1]["tool_calls"][0]["function"]["arguments"]=json!(json!({"filePath":"D:/app/main.js","startLine":401,"endLine":800}).to_string());
        assert!(compile(&p,&edit(1,1,"new")).is_err());
        for choice in [json!("none"),json!({"type":"function","function":{"name":"replace_string_in_file"}})] {let mut p=payload("x");p["tool_choice"]=choice;assert_eq!(prepare(&p),p);}
    }
    #[test]
    fn observed_compiler_locations_focus_only_the_grounded_line_planner() {
        for diagnostic in ["D:\\app\\main.js:2\nSyntaxError", " --> D:/app/main.js:2:5\nerror", "File \"D:/app/main.js\", line 2\nSyntaxError", "D:/app/main.js(2,5): error"] {
            let mut p=payload("one\ntwo\nthree\n");p["chat_template_kwargs"]=json!({"opengpu_native_tool_guard":true});
            p["messages"].as_array_mut().unwrap().push(json!({"role":"assistant","tool_calls":[{"id":"check1","function":{"name":"run_in_terminal","arguments":json!({"command":"node --check D:/app/main.js"}).to_string()}}]}));
            p["messages"].as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":"check1","content":format!("{diagnostic}\nCommand exited with code 1")}));
            let prepared=prepare(&p);assert_eq!(prepared["tools"].as_array().unwrap().len(),1,"{diagnostic}");assert_eq!(prepared["tools"][0]["function"]["name"],NAME);assert_eq!(prepared["tool_choice"],"required");assert_eq!(prepared["chat_template_kwargs"]["opengpu_require_tool_call"],true);
            assert_eq!(args(&compile(&p,&edit(2,2,"fixed")).unwrap())["oldString"],"two\n");
            let mut explicit=p.clone();explicit["tool_choice"]=json!({"type":"function","function":{"name":"read_file"}});assert_eq!(prepare(&explicit),explicit);
            p["messages"].as_array_mut().unwrap().push(json!({"role":"assistant","tool_calls":[{"id":"write1","function":{"name":"replace_string_in_file","arguments":json!({"filePath":"D:/app/main.js"}).to_string()}}]}));
            p["messages"].as_array_mut().unwrap().push(json!({"role":"tool","tool_call_id":"write1","content":"Applied edit"}));
            assert!(super::super::native_tool_workflow::compiler_failure(&p).is_none());
            assert!(!prepare(&p)["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]==NAME));
        }
    }
}
