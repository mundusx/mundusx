//! Existing device identity only; never request the operator-wide ledger.
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
const PATH: &str = "/v1/contributor/credits";
pub fn validate(payload: &Value, node: &str) -> Result<(), String> {
    if payload["node_id"].as_str() != Some(node) || payload["unit"] != "legacy_work_points" {
        return Err("Reward response does not match this contributor or its unit".into());
    }
    for field in [
        "remaining_work_points",
        "earned_work_points",
        "used_work_points",
    ] {
        if !payload[field].as_f64().is_some_and(f64::is_finite) {
            return Err("Reward response is incomplete".into());
        }
    }
    Ok(())
}
pub fn run() -> Result<(), String> {
    let (config, identity) = crate::ownership::existing()?;
    let endpoint = crate::control_plane_endpoint(&config.control_plane_url, PATH)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid system clock")?
        .as_secs()
        .to_string();
    let signature = identity
        .sign_existing_hex(&format!("GET\n{PATH}\n{stamp}\n"))
        .map_err(|_| "Cannot sign rewards request. Unlock existing secure storage and retry.")?;
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(15))
        .build()
        .get(&endpoint)
        .set("X-MundusX-Node-Id", &config.device_id)
        .set("X-MundusX-Timestamp", &stamp)
        .set("X-MundusX-Signature", &signature)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(status, _) => {
                format!("Signed contributor rewards unavailable (HTTP {status})")
            }
            _ => "Contributor rewards connection failed".into(),
        })?;
    let payload: Value = response
        .into_json()
        .map_err(|_| "Invalid contributor rewards response")?;
    validate(&payload, &config.device_id)?;
    crate::print_json(&payload)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_other_devices_wrong_units_and_missing_balances() {
        let mut p = serde_json::json!({"node_id":"mine","unit":"legacy_work_points","remaining_work_points":6.0,"earned_work_points":7.0,"used_work_points":1.0});
        assert!(validate(&p, "mine").is_ok());
        assert!(validate(&p, "other").is_err());
        p["unit"] = serde_json::json!("usd");
        assert!(validate(&p, "mine").is_err());
        p["unit"] = serde_json::json!("legacy_work_points");
        p.as_object_mut().unwrap().remove("remaining_work_points");
        assert!(validate(&p, "mine").is_err());
    }
}
