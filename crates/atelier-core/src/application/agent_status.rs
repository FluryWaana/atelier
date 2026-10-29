use crate::{contracts::AgentDiagnostic, document::Document};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Checks {
    next: u64,
    entries: BTreeMap<String, Entry>,
}

struct Entry {
    request: u64,
    profile: serde_json::Value,
    diagnostic: Option<AgentDiagnostic>,
}

fn configuration(profile: &Document) -> serde_json::Value {
    let mut value = serde_json::to_value(profile).expect("valid profile document");
    if let Some(fields) = value.as_object_mut() {
        for key in ["apiKeySet", "updating", "updatePhase"] {
            fields.remove(key);
        }
    }
    value
}

impl Checks {
    pub fn begin(&mut self, id: &str, profile: &Document) -> u64 {
        self.next = self
            .next
            .checked_add(1)
            .expect("agent check counter exhausted");
        self.entries.insert(
            id.into(),
            Entry {
                request: self.next,
                profile: configuration(profile),
                diagnostic: None,
            },
        );
        self.next
    }

    pub fn finish(&mut self, id: &str, request: u64, diagnostic: Option<AgentDiagnostic>) {
        if let Some(entry) = self
            .entries
            .get_mut(id)
            .filter(|entry| entry.request == request)
        {
            entry.diagnostic = diagnostic;
        }
    }

    pub fn diagnostic(&self, id: &str, profile: &Document) -> Option<&AgentDiagnostic> {
        self.entries
            .get(id)
            .filter(|entry| configuration(profile) == entry.profile)
            .and_then(|entry| entry.diagnostic.as_ref())
    }
}
