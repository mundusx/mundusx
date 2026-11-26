use std::cell::RefCell;
use serde_json::Value;

pub const CAPABILITY: &str = "request_token_budget_v1";
#[derive(Clone, Copy)]
struct Budget { remaining: u64, reserved: Option<u64> }
thread_local! { static BUDGET: RefCell<Option<Budget>> = const { RefCell::new(None) }; }

pub struct Scope(Option<Budget>);
impl Scope {
    pub fn enter(allowance: Option<u64>) -> Self {
        Self(BUDGET.with(|slot| slot.replace(allowance.map(|remaining|Budget {remaining,reserved:None}))))
    }
}
impl Drop for Scope {
    fn drop(&mut self) { BUDGET.with(|slot| {slot.replace(self.0);}); }
}

/// Reserve a byte-conservative input bound and bounded output before each call.
/// Unknown usage keeps the entire reservation spent, including on HTTP failure.
pub fn prepare(mut payload: Value) -> Result<Value,String> {
    BUDGET.with(|slot| {
        let mut state=slot.borrow_mut();
        let Some(budget)=state.as_mut() else {return Ok(payload);};
        let input=100u64.saturating_add(serde_json::to_vec(&payload).map_err(|e|e.to_string())?.len() as u64);
        let output=payload["max_tokens"].as_u64().ok_or("Budgeted inference requires an output limit")?;
        let cap=budget.remaining.saturating_sub(input).min(output);
        if cap==0 {return Err("request token budget exhausted before model call".into());}
        payload["max_tokens"]=cap.into();
        let reserved=input+cap;
        budget.remaining-=reserved;budget.reserved=Some(reserved);
        Ok(payload)
    })
}

pub fn settle(usage: &Value) {
    BUDGET.with(|slot| {
        let mut state=slot.borrow_mut();
        let Some(budget)=state.as_mut() else {return;};
        let Some(reserved)=budget.reserved.take() else {return;};
        let measured=usage["prompt_tokens"].as_u64().zip(usage["completion_tokens"].as_u64())
            .and_then(|(input,output)|input.checked_add(output));
        if let Some(measured)=measured {
            if measured<=reserved {budget.remaining=budget.remaining.saturating_add(reserved-measured);}
            else {budget.remaining=0;}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unknown_usage_cannot_fund_a_retry() {
        let body=json!({"messages":[],"max_tokens":10});
        let input=100+serde_json::to_vec(&body).unwrap().len() as u64;
        let _scope=Scope::enter(Some(input+5));
        assert_eq!(prepare(body.clone()).unwrap()["max_tokens"],5);
        assert!(prepare(body).is_err());
    }
    #[test]
    fn measured_usage_funds_bounded_repair_and_scope_restores() {
        let body=json!({"messages":[],"max_tokens":10});
        {let _scope=Scope::enter(Some(1000));
         prepare(body.clone()).unwrap();settle(&json!({"prompt_tokens":2,"completion_tokens":3}));
         assert!(prepare(body.clone()).is_ok());}
        assert!(prepare(body).is_ok());
    }
    #[test]
    fn overspend_and_missing_usage_never_refund() {
        let body=json!({"messages":[],"max_tokens":10});
        for usage in [json!({}),json!({"prompt_tokens":9999,"completion_tokens":1})] {
            let input=100+serde_json::to_vec(&body).unwrap().len() as u64;
            let _scope=Scope::enter(Some(input+10));prepare(body.clone()).unwrap();settle(&usage);
            assert!(prepare(body.clone()).is_err());
        }
    }
    #[test]
    fn concurrent_assignments_do_not_share_allowances() {
        let tasks:Vec<_>=[0,1000].into_iter().map(|allowance|std::thread::spawn(move||{
            let _scope=Scope::enter(Some(allowance));
            prepare(json!({"messages":[],"max_tokens":10})).is_ok()
        })).collect();
        let results:Vec<_>=tasks.into_iter().map(|t|t.join().unwrap()).collect();
        assert_eq!(results,vec![false,true]);
    }
}
