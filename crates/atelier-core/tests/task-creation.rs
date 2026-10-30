use atelier_core::{
    application::task_creation::{self, CreationContext, Result},
    document::{Document, JsonString},
    domain::{
        checklist::{Criterion, checklist_text},
        text_units,
    },
};
use serde_json::value::RawValue;
use std::cell::{Cell, RefCell};
#[path = "support/json.rs"]
mod json;
use json::equivalent;

#[test]
fn default_agent_validates_repository_text_and_matches_native_paths() {
    let settings: Document =
        serde_json::from_str(r#"{"repo":"C:/Synthetic/Repo","agent":"codex"}"#).unwrap();
    let project: Document =
        serde_json::from_str(r#"{"repo":"c:\\synthetic\\repo","agent":"claude"}"#).unwrap();
    assert_eq!(
        task_creation::default_agent(&settings, &[project], &Document::default(), true).unwrap(),
        JsonString::from("claude")
    );
    for invalid in ["true", "1", "[]", "{}", "null", r#""\ud800""#] {
        let body: Document = serde_json::from_str(&format!(r#"{{"repo":{invalid}}}"#)).unwrap();
        assert_eq!(
            task_creation::default_agent(&settings, &[], &body, false)
                .unwrap_err()
                .to_string(),
            "message.invalid_or_overly_long_text"
        );
        let settings: Document =
            serde_json::from_str(&format!(r#"{{"agent":{invalid}}}"#)).unwrap();
        assert_eq!(
            task_creation::default_agent(&settings, &[], &Document::default(), false)
                .unwrap_err()
                .to_string(),
            "message.invalid_agent"
        );
    }
}

struct Context {
    scenario: Document,
    events: RefCell<Vec<Box<RawValue>>>,
    clock: Cell<u8>,
}
impl Context {
    fn event(&self, value: &impl serde::Serialize) {
        self.events
            .borrow_mut()
            .push(serde_json::value::to_raw_value(value).unwrap());
    }
}
impl CreationContext for Context {
    fn settings(&self) -> Result<Document> {
        let mut settings = self
            .scenario
            .get::<Document>("settings")?
            .unwrap_or_default();
        if settings.raw("repo").is_none() {
            settings.set("repo", &"")?;
        }
        Ok(settings)
    }
    fn agent_for(&self, body: &Document) -> Result<JsonString> {
        self.event(&("agentFor", body.raw("repo")));
        if let Some(projects) = self.scenario.get::<Vec<Document>>("projects")? {
            return task_creation::default_agent(&self.settings()?, &projects, body, cfg!(windows));
        }
        Ok(JsonString::from("claude"))
    }
    fn repository(&self, value: &RawValue) -> Result<JsonString> {
        self.event(&("repository", value));
        let value: JsonString =
            serde_json::from_str(value.get()).map_err(|_| "message.invalid_or_overly_long_text")?;
        let value = text_units(&value, 2000)?;
        if self.scenario.truthy("repositoryError")? {
            return Err("Synthetic repository failure".into());
        }
        let mut path = JsonString::from("checked:");
        path.0.extend(value.0);
        Ok(path)
    }
    fn new_id(&self) -> String {
        self.event(&"id");
        "created".into()
    }
    fn timestamp(&self) -> Result<String> {
        self.event(&"timestamp");
        let clock = self.clock.get();
        self.clock.set(clock + 1);
        Ok(format!("2026-10-24T00:00:00.00{clock}Z"))
    }
}

#[test]
fn historical_task_creation_preserves_fields_utf16_and_validation_order() {
    let cases: Vec<Document> =
        serde_json::from_str(include_str!("fixtures/rust-task-creation-reference.json")).unwrap();
    assert!(cases.len() >= 70);
    for (index, scenario) in cases.into_iter().enumerate() {
        let context = Context {
            scenario,
            events: RefCell::default(),
            clock: Cell::new(0),
        };
        let body = context.scenario.get::<Document>("body").unwrap().unwrap();
        let mut actual = Document::default();
        match task_creation::create(
            &context,
            &body,
            context.scenario.truthy("brainstorming").unwrap(),
        ) {
            Ok(card) => {
                let criteria = card
                    .fields
                    .get::<Vec<Criterion>>("checklist")
                    .unwrap()
                    .unwrap();
                actual.set("prompt", &checklist_text(&criteria)).unwrap();
                actual.set("value", &card).unwrap();
            }
            Err(error) => {
                actual.set("error", &error.to_string()).unwrap();
            }
        }
        actual.set("events", &*context.events.borrow()).unwrap();
        let actual = serde_json::value::to_raw_value(&actual).unwrap();
        let expected = context.scenario.raw("expected").unwrap();
        assert!(
            equivalent(&actual, expected),
            "case {index}\nactual: {actual}\nexpected: {expected}"
        );
    }
}
