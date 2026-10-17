//! Operation identity and lifetime belong to Rust, independently of a WebView.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Select {
        target: String,
        #[serde(rename = "soundId")]
        sound_id: String,
    },
    Register {},
    Rename {
        id: String,
        name: String,
    },
    Unregister {
        id: String,
    },
}
#[derive(Clone)]
struct Entry {
    session: String,
    base: u64,
    id: String,
    action: Action,
    outcome: Value,
}
#[derive(Default)]
struct Inner {
    seq: u64,
    blocked: bool,
    entries: VecDeque<Entry>,
}
#[derive(Default)]
pub struct Operations {
    inner: Mutex<Inner>,
}
impl Operations {
    pub fn begin(
        &self,
        session: &str,
        base: u64,
        id: &str,
        action: &Action,
    ) -> Result<bool, String> {
        if id.is_empty() || id.len() > 128 {
            return Err("InvalidRequestId".into());
        }
        let mut i = self.inner.lock().map_err(|_| "SoundOperationPoisoned")?;
        if let Some(e) = i.entries.iter().find(|e| e.id == id) {
            return if e.session == session && e.base == base && e.action == *action {
                Ok(false)
            } else {
                Err("RequestIdReused".into())
            };
        }
        if i.blocked
            || i.entries
                .back()
                .is_some_and(|e| e.outcome["state"] == "pending")
        {
            return Err("AudioManagementBusy".into());
        }
        if i.entries.len() >= 256 {
            i.entries.pop_front();
        }
        i.entries.push_back(Entry {
            session: session.into(),
            base,
            id: id.into(),
            action: action.clone(),
            outcome: json!({"state":"pending"}),
        });
        i.seq += 1;
        Ok(true)
    }
    pub fn finish(&self, id: &str, outcome: Value) {
        let mut i = self.inner.lock().unwrap();
        i.blocked = outcome["uncertain"] == true;
        if let Some(e) = i.entries.iter_mut().find(|e| e.id == id) {
            e.outcome = outcome;
        }
        i.seq += 1;
    }
    pub fn receipt(&self, session: &str, id: &str) -> Result<Value, String> {
        let i = self.inner.lock().map_err(|_| "SoundOperationPoisoned")?;
        let e = i
            .entries
            .iter()
            .find(|e| e.id == id && e.session == session)
            .ok_or("UnknownReceipt")?;
        Ok(e.outcome.clone())
    }
    pub fn snapshot(&self) -> Value {
        let i = self.inner.lock().unwrap();
        json!({"eventSeq":i.seq,"busy":i.blocked||i.entries.back().is_some_and(|e|e.outcome["state"]=="pending"),"operation":i.entries.back().map(|e|json!({"requestId":e.id,"outcome":e.outcome}))})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_pending_and_uncertain_are_not_released_by_duplicate_requests() {
        let o = Operations::default();
        let a = Action::Register {};
        assert!(o.begin("s", 1, "a", &a).unwrap());
        assert!(!o.begin("s", 1, "a", &a).unwrap());
        assert!(o.begin("s", 1, "b", &a).is_err());
        assert!(o.begin("old", 1, "a", &a).is_err());
        o.finish("a", json!({"state":"failed","uncertain":true}));
        assert!(o.begin("s", 1, "b", &a).is_err());
        assert_eq!(o.receipt("s", "a").unwrap()["state"], "failed");
    }
    #[test]
    fn known_failure_and_cancel_allow_explicit_new_request() {
        let o = Operations::default();
        let a = Action::Unregister { id: "dummy".into() };
        o.begin("s", 1, "one", &a).unwrap();
        o.finish("one", json!({"state":"failed","uncertain":false}));
        assert!(o.begin("s", 1, "two", &a).unwrap());
        o.finish("two", json!({"state":"cancelled"}));
        assert_eq!(o.snapshot()["busy"], false);
    }
}
