use crate::config::{load_config,config_exists};
use crate::identity::{DeviceIdentity,resolved_identity_path,device_id_for_identity};
use serde_json::{json,Value};
use std::io::{self,Write,BufRead};
use std::time::{Duration,SystemTime,UNIX_EPOCH};

pub fn validate_code(code:&str)->Result<&str,String>{
 let code=code.trim();if code.len()<32||code.len()>256||!code.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'_'||c==b'-'){return Err("Invalid link code. Generate a fresh code in Nodara Chat settings.".into())}Ok(code)
}
fn read_code()->Result<String,String>{let mut text=String::new();io::stdin().lock().read_line(&mut text).map_err(|_|"Cannot read link code")?;Ok(validate_code(&text)?.to_string())}
fn existing()->Result<(crate::config::Config,DeviceIdentity),String>{
 if !config_exists(){return Err("Set up and register this contributor before linking it.".into())}
 let config=load_config().map_err(|_|"Cannot read contributor settings")?.ok_or("Contributor settings are missing")?;
 let data=std::fs::read(resolved_identity_path()).map_err(|_|"Existing device identity is missing. Linking never generates a replacement key.")?;
 let identity:DeviceIdentity=serde_json::from_slice(&data).map_err(|_|"Cannot read existing device identity")?;
 if device_id_for_identity(&identity)!=config.device_id{return Err("Device identity does not match this contributor. Existing settings were preserved.".into())}
 Ok((config,identity))
}
fn send(action:&str,code:Option<&str>)->Result<Value,String>{
 let (config,identity)=existing()?;
 let path=format!("/v1/contributor-ownership/{action}");
 let mut body=json!({"node_id":config.device_id});if let Some(code)=code{body["link_token"]=json!(validate_code(code)?);}
 let body=serde_json::to_string(&body).map_err(|_|"Cannot prepare ownership request")?;
 let stamp=SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|"System clock is invalid")?.as_secs().to_string();
 let message=format!("POST\n{path}\n{stamp}\n{body}");
 let signature=identity.sign_existing_hex(&message).map_err(|_|"Cannot sign with the existing device key. Unlock secure storage and retry.")?;
 let endpoint=crate::control_plane_endpoint(&config.control_plane_url,&path)?;
 let response=ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(15)).build().post(&endpoint)
 .set("Content-Type","application/json").set("X-MundusX-Node-Id",&config.device_id).set("X-MundusX-Timestamp",&stamp).set("X-MundusX-Signature",&signature).send_string(&body)
 .map_err(|error|match error{ureq::Error::Status(409,_)=>"Link code is expired, used, or cannot claim this contributor. Generate a new code and verify the node ID.".to_string(),ureq::Error::Status(403,_)=>"Device signature was rejected. Check the existing identity and system clock.".to_string(),ureq::Error::Status(status,_)=>format!("The control plane could not confirm ownership (HTTP {status}). Check the connection and run owner status before retrying."),ureq::Error::Transport(error)=>format!("The ownership connection failed ({:?}). Check the control plane URL and TLS trust.",error.kind())})?;
 let value:Value=response.into_json().map_err(|_|"Invalid ownership response")?;
 if value.get("node_id").and_then(Value::as_str)!=Some(config.device_id.as_str()){return Err("Ownership response does not match this contributor".into())}
 Ok(value)
}
pub fn run(action:&str,code_stdin:bool,yes:bool,json_output:bool)->Result<(),String>{
 let result=if action=="status"{send("status",None)?}else{
  if !code_stdin {eprint!("Paste the one-time code from Nodara Chat: ");io::stderr().flush().ok();}
  let code=read_code()?;
  let preview=send("inspect",Some(&code))?;
  if action=="inspect"{preview}else{
   if !yes{
    eprintln!("Link contributor {} to {}?",preview["node_id"].as_str().unwrap_or(""),preview["email"].as_str().ok_or("Missing account confirmation")?);
    eprint!("Confirm [y/N]: ");io::stderr().flush().ok();let mut answer=String::new();io::stdin().lock().read_line(&mut answer).map_err(|_|"Cannot read confirmation")?;
    if !["y","yes"].contains(&answer.trim().to_ascii_lowercase().as_str()){return Err("Ownership linking cancelled; no changes were made.".into())}
   }
   send("link",Some(&code))?
  }
 };
 if json_output{println!("{}",serde_json::to_string(&result).map_err(|_|"Cannot format response")?);}else{println!("{}",serde_json::to_string_pretty(&result).map_err(|_|"Cannot format response")?);}
 Ok(())
}
#[cfg(test)]mod tests{use super::*;#[test]fn validates_private_code_without_accepting_paths_or_commands(){assert!(validate_code(&"a".repeat(64)).is_ok());for s in ["short","../identity.json","abc; run command"]{assert!(validate_code(s).is_err());}assert!(validate_code(&"x".repeat(257)).is_err());}}
