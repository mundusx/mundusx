//! Narrow protection for explicit stop-localhost requests. Never executes tools.
use serde_json::{json,Value};

fn active_port(payload:&Value)->Option<u16> {
    let m=payload["messages"].as_array()?.iter().rev().find(|m|m["role"]=="user")?;
    let raw=m["content"].as_str()?;
    let request=if raw.contains("<userRequest>") {raw.rsplit_once("<userRequest>")?.1.split_once("</userRequest>")?.0} else {raw};
    let s=request.trim().to_ascii_lowercase();
    if !["kill ","stop ","close ","terminate "].iter().any(|v|s.starts_with(v)){return None}
    let tail=s.split_once("localhost:").or_else(||s.split_once("127.0.0.1:"))?.1;
    let digits=tail.trim_end_matches('/');
    if !digits.bytes().all(|c|c.is_ascii_digit()){return None}
    digits.parse::<u16>().ok().filter(|p|*p>0)
}
fn probe(port:u16)->String {
    format!("$ErrorActionPreference='Stop'; $listeners=@(Get-NetTCPConnection -State Listen -ErrorAction Stop | Where-Object {{$_.LocalPort -eq {port}}}); if($listeners.Count -eq 0){{Write-Output 'MUNDUSX_NO_LISTENER:{port}'}}else{{$listeners | Select-Object LocalAddress,LocalPort,OwningProcess}}")
}
pub fn applies(payload:&Value)->bool { active_port(payload).is_some() }
pub fn guard(payload:&Value,message:&Value)->Option<Value> {
    let port=active_port(payload)?;
    let expected=probe(port);
    let messages=payload["messages"].as_array()?;
    // Only accept evidence from a result tied to our exact read-only command.
    if let Some(result)=messages.last().filter(|m|m["role"]=="tool") {
        let id=result["tool_call_id"].as_str()?;
        let matched=messages.iter().filter_map(|m|m["tool_calls"].as_array()).flatten().any(|call|{
            call["id"]==id && call["function"]["name"]=="run_in_terminal" &&
            call["function"]["arguments"].as_str().and_then(|s|serde_json::from_str::<Value>(s).ok()).is_some_and(|a|a["command"]==expected)
        });
        if matched && result["content"].as_str().is_some_and(|s|s.lines().any(|l|l.trim()==format!("MUNDUSX_NO_LISTENER:{port}"))) {
            return Some(json!({"role":"assistant","content":format!("Nothing is listening on local port {port}. No further process termination is needed.")}));
        }
    }
    let mut out=message.clone();let mut changed=false;
    for call in out["tool_calls"].as_array_mut()? {
        if call["function"]["name"]!="run_in_terminal" {continue}
        let mut args:Value=serde_json::from_str(call["function"]["arguments"].as_str()?).ok()?;
        let cmd=args["command"].as_str()?.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
        if ["taskkill /f /im node.exe","taskkill /f /im npm.cmd"].contains(&cmd.as_str()) {
            args["command"]=expected.clone().into();args["mode"]="sync".into();
            args["explanation"]="Check the listening socket before stopping a process".into();
            call["function"]["arguments"]=args.to_string().into();changed=true;
        }
    }
    if changed {out["content"]=format!("I'll verify which process is listening on local port {port} before stopping anything.").into();Some(out)}else{None}
}

#[cfg(test)]mod tests{
 use super::*;
 fn body()->Value{json!({"messages":[{"role":"user","content":"kill this http://localhost:3000/"}]})}
 fn call(command:&str)->Value{json!({"role":"assistant","tool_calls":[{"id":"c","type":"function","function":{"name":"run_in_terminal","arguments":json!({"command":command,"mode":"sync"}).to_string()}}]})}
 #[test]fn broad_kill_becomes_read_only_port_check(){let result=guard(&body(),&call("taskkill /F /IM node.exe")).unwrap();let args:Value=serde_json::from_str(result["tool_calls"][0]["function"]["arguments"].as_str().unwrap()).unwrap();assert_eq!(args["command"],probe(3000));assert!(!result.to_string().contains("taskkill"));}
 #[test]fn matching_no_listener_evidence_ends_the_loop(){let mut p=body();p["messages"].as_array_mut().unwrap().extend([call(&probe(3000)),json!({"role":"tool","tool_call_id":"c","content":"MUNDUSX_NO_LISTENER:3000"})]);let result=guard(&p,&call("taskkill /F /IM npm.cmd")).unwrap();assert!(result.get("tool_calls").is_none());p["messages"][2]["tool_call_id"]="wrong".into();assert!(guard(&p,&json!({"role":"assistant","content":"x"})).is_none());}
 #[test]fn other_tasks_and_injected_ports_are_untouched(){for text in ["hello","kill localhost:3000; anything","stop localhost:99999"]{let p=json!({"messages":[{"role":"user","content":text}]});assert!(guard(&p,&call("taskkill /F /IM node.exe")).is_none());}}
}
