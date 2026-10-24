use atelier_core::{
    application::task_creation::{self, CreationContext, Result},
    document::{Document, JsonString},
    domain::{
        checklist::{Criterion, checklist_text},
        text_units,
    },
};
use serde_json::{Value, value::RawValue};
use std::cell::{Cell, RefCell};

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
    fn agent_for(&self, repo: Option<&RawValue>) -> Result<JsonString> {
        self.event(&("agentFor", repo));
        Ok(JsonString::from("claude"))
    }
    fn repository(&self, value: &RawValue) -> Result<JsonString> {
        self.event(&("repository", value));
        let value: JsonString =
            serde_json::from_str(value.get()).map_err(|_| "Texte invalide ou trop long.")?;
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

fn equivalent(actual: &RawValue, expected: &RawValue) -> bool {
    match (actual.get().as_bytes()[0], expected.get().as_bytes()[0]) {
        (b'{', b'{') => {
            let actual: Document = serde_json::from_str(actual.get()).unwrap();
            let expected: Document = serde_json::from_str(expected.get()).unwrap();
            actual.keys().count() == expected.keys().count()
                && actual.keys().all(|key| {
                    let key = key.utf8().unwrap();
                    expected
                        .raw(&key)
                        .is_some_and(|expected| equivalent(actual.raw(&key).unwrap(), expected))
                })
        }
        (b'[', b'[') => {
            let actual: Vec<Box<RawValue>> = serde_json::from_str(actual.get()).unwrap();
            let expected: Vec<Box<RawValue>> = serde_json::from_str(expected.get()).unwrap();
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected.iter())
                    .all(|(a, e)| equivalent(a, e))
        }
        (b'"', b'"') => {
            serde_json::from_str::<JsonString>(actual.get()).unwrap()
                == serde_json::from_str::<JsonString>(expected.get()).unwrap()
        }
        _ => {
            serde_json::from_str::<Value>(actual.get()).unwrap()
                == serde_json::from_str::<Value>(expected.get()).unwrap()
        }
    }
}

#[test]
fn historical_task_creation_preserves_fields_utf16_and_validation_order() {
    let cases: Vec<Document> = serde_json::from_str(include_str!(
        "../../../test/fixtures/rust-task-creation-reference.json"
    ))
    .unwrap();
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
