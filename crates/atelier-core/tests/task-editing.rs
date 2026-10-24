use atelier_core::{
    application::{
        card_lifecycle::CardFailure,
        task_creation::{CreationContext, Result},
        task_editing::{self, EditingContext},
    },
    contracts::Card,
    document::{Document, JsonString},
    domain::text_units,
};
use serde_json::value::RawValue;
use std::cell::RefCell;
#[path = "support/json.rs"]
mod json;

struct Context {
    scenario: Document,
    events: RefCell<Vec<Box<RawValue>>>,
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
        Ok(Document::default())
    }
    fn agent_for(&self, _: &Document) -> Result<JsonString> {
        panic!("Edits do not inherit the project agent");
    }
    fn new_id(&self) -> String {
        panic!("Edits do not allocate an id");
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
    fn timestamp(&self) -> Result<String> {
        self.event(&"timestamp");
        Ok("now".into())
    }
}
impl EditingContext for Context {
    fn cards(&self) -> Result<Vec<Document>> {
        let mut cards = self
            .scenario
            .get::<Vec<Document>>("others")?
            .unwrap_or_else(|| {
                vec![serde_json::from_str(r#"{"id":"other","status":"done"}"#).unwrap()]
            });
        cards.insert(0, self.scenario.get::<Document>("card")?.unwrap());
        Ok(cards)
    }
}

#[test]
fn historical_task_edits_preserve_guards_fields_review_invalidation_and_effect_order() {
    let cases: Vec<Document> = serde_json::from_str(include_str!(
        "../../../test/fixtures/rust-task-editing-reference.json"
    ))
    .unwrap();
    assert!(cases.len() >= 60);
    for (index, scenario) in cases.into_iter().enumerate() {
        let context = Context {
            scenario,
            events: RefCell::default(),
        };
        let mut saved = context.scenario.get::<Card>("card").unwrap().unwrap();
        let body = context.scenario.get::<Document>("body").unwrap().unwrap();
        let result = (|| -> Result<Card> {
            if context.scenario.truthy("missing")? {
                return Err(CardFailure::Missing.into());
            }
            let edit =
                task_editing::prepare(&context, &saved, &body, context.scenario.truthy("busy")?)?;
            if edit.auto_start {
                let mut options = Document::default();
                for key in ["agent", "mode", "instructions"] {
                    if let Some(raw) = edit.card.fields.raw(key) {
                        options.set(key, &raw)?;
                    }
                }
                context.event(&("launch", &edit.card, &options));
                if context.scenario.truthy("launchError")? {
                    return Err("Synthetic launch failure".into());
                }
                saved = edit.card;
                saved.fields.set("status", &"doing")?;
            } else {
                context.event(&("put", &edit.card));
                saved = edit.card;
            }
            if edit.requirements_changed {
                context.event(&"invalidate");
            }
            if edit.sync_status {
                context.event(&"sync");
            }
            Ok(saved.clone())
        })();
        let mut actual = Document::default();
        match result {
            Ok(value) => actual.set("value", &value).unwrap(),
            Err(error) => actual.set("error", &error.to_string()).unwrap(),
        }
        actual.set("saved", &saved).unwrap();
        actual.set("events", &*context.events.borrow()).unwrap();
        let actual = serde_json::value::to_raw_value(&actual).unwrap();
        let expected = context.scenario.raw("expected").unwrap();
        assert!(
            json::equivalent(&actual, expected),
            "case {index}\nactual: {actual}\nexpected: {expected}"
        );
    }
}
