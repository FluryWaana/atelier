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
fn task_agent_preferences_are_conflict_checked_and_reset_options_on_profile_change() {
    let context = Context {
        scenario: Document::default(),
        events: RefCell::default(),
    };
    let card:Card=serde_json::from_str(r#"{"id":"task","title":"Synthetic","agent":"codex","model":"old-model","effort":"high","status":"todo"}"#).unwrap();
    let body: Document =
        serde_json::from_str(r#"{"model":"new-model","expected":{"model":"old-model"}}"#).unwrap();
    let edited = task_editing::prepare(&context, &card, &body, false).unwrap();
    assert!(
        edited
            .card
            .fields
            .string_equals("model", "new-model")
            .unwrap()
    );
    assert!(!edited.requirements_changed);
    assert!(task_editing::prepare(&context, &edited.card, &body, false).is_err());
    let change: Document =
        serde_json::from_str(r#"{"agent":"claude","expected":{"agent":"codex"}}"#).unwrap();
    let edited = task_editing::prepare(&context, &edited.card, &change, false).unwrap();
    assert!(edited.card.fields.string_equals("agent", "claude").unwrap());
    assert!(edited.card.fields.string_equals("model", "").unwrap());
    assert!(edited.card.fields.string_equals("effort", "").unwrap());
    let invalid: Document =
        serde_json::from_str(r#"{"effort":"none","expected":{"effort":""}}"#).unwrap();
    assert!(task_editing::prepare(&context, &edited.card, &invalid, false).is_err());
}

#[test]
fn unchanged_edit_vectors_preserve_validation_and_review_invalidation() {
    let cases: Vec<Document> =
        serde_json::from_str(include_str!("fixtures/rust-task-editing-reference.json")).unwrap();
    assert!(cases.len() >= 60);
    for (index, scenario) in cases.into_iter().enumerate() {
        let context = Context {
            scenario,
            events: RefCell::default(),
        };
        let mut saved = context.scenario.get::<Card>("card").unwrap().unwrap();
        let body = context.scenario.get::<Document>("body").unwrap().unwrap();
        // The rebuild deliberately separates manual status changes from agent execution.
        if body.string_equals("status", "doing").unwrap()
            && !saved.fields.string_equals("status", "doing").unwrap()
        {
            continue;
        }
        let result = (|| -> Result<Card> {
            if context.scenario.truthy("missing")? {
                return Err(CardFailure::Missing.into());
            }
            let edit =
                task_editing::prepare(&context, &saved, &body, context.scenario.truthy("busy")?)?;
            context.event(&("put", &edit.card));
            saved = edit.card;
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

#[test]
fn manual_status_changes_save_without_launching_an_agent() {
    let card: Card = serde_json::from_str(
        r#"{"id":"manual","title":"Task","status":"todo","agent":"codex","updatedAt":"before"}"#,
    )
    .unwrap();
    let context = Context {
        scenario: Document::default(),
        events: RefCell::default(),
    };
    for status in ["todo", "doing", "review", "done", "standby"] {
        context.events.borrow_mut().clear();
        let body =
            serde_json::from_str::<Document>(&serde_json::json!({"status":status}).to_string())
                .unwrap();
        let edit = task_editing::prepare(&context, &card, &body, false).unwrap();
        assert!(edit.card.fields.string_equals("status", status).unwrap());
        assert!(edit.card.fields.string_equals("updatedAt", "now").unwrap());
        assert_eq!(edit.sync_status, status != "todo");
        assert!(!edit.requirements_changed);
        assert_eq!(context.events.borrow().len(), 1);
        assert_eq!(context.events.borrow()[0].get(), "\"timestamp\"");
        assert!(task_editing::prepare(&context, &card, &body, true).is_err());
        let mut archived = card.clone();
        archived.fields.set("archived", &true).unwrap();
        assert!(task_editing::prepare(&context, &archived, &body, false).is_err());
    }
}
