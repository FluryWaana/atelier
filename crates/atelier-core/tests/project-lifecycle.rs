use atelier_core::{
    application::{
        projects::{self, ProjectRepository},
        task_creation::{CreationContext, Result},
    },
    document::{Document, JsonString},
    domain::text_units,
};
use serde_json::value::RawValue;
use std::cell::RefCell;
#[path = "support/json.rs"]
mod json;

struct Context {
    scenario: Document,
    projects: RefCell<Vec<Document>>,
    cards: RefCell<Vec<Document>>,
}
impl CreationContext for Context {
    fn settings(&self) -> Result<Document> {
        panic!("Project lifecycle does not read settings");
    }
    fn agent_for(&self, _: &Document) -> Result<JsonString> {
        panic!("Project lifecycle does not select agents");
    }
    fn repository(&self, value: &RawValue) -> Result<JsonString> {
        let value: JsonString =
            serde_json::from_str(value.get()).map_err(|_| "Texte invalide ou trop long.")?;
        let value = text_units(&value, 2000)?;
        let mut repo = JsonString::from("checked:");
        repo.0.extend(value.0);
        Ok(repo)
    }
    fn new_id(&self) -> String {
        "new-project".into()
    }
    fn timestamp(&self) -> Result<String> {
        Ok("now".into())
    }
}
impl ProjectRepository for Context {
    fn projects(&self) -> Result<Vec<Document>> {
        Ok(self.projects.borrow().clone())
    }
    fn cards(&self) -> Result<Vec<Document>> {
        Ok(self.cards.borrow().clone())
    }
    fn times(&self) -> Result<Vec<Document>> {
        Ok(self.scenario.get("times")?.unwrap_or_default())
    }
    fn save_project(&self, project: &Document) -> Result<()> {
        self.projects.borrow_mut().push(project.clone());
        Ok(())
    }
    fn save_card(&self, card: &Document) -> Result<()> {
        let id = card.get::<String>("id")?.unwrap();
        if self.scenario.string_equals("failCard", &id)? {
            return Err("Synthetic card write failure".into());
        }
        for existing in self.cards.borrow_mut().iter_mut() {
            if existing.string_equals("id", &id)? {
                *existing = card.clone();
            }
        }
        Ok(())
    }
    fn delete_project(&self, id: &str) -> Result<()> {
        self.projects
            .borrow_mut()
            .retain(|project| !project.string_equals("id", id).unwrap());
        Ok(())
    }
}

#[test]
fn historical_project_lifecycle_preserves_guards_utf16_archive_scope_and_rollback() {
    let cases: Vec<Document> = serde_json::from_str(include_str!(
        "../../../test/fixtures/rust-project-lifecycle-reference.json"
    ))
    .unwrap();
    assert!(cases.len() >= 35);
    for (index, scenario) in cases.into_iter().enumerate() {
        let context = Context {
            projects: RefCell::new(scenario.get("projects").unwrap().unwrap()),
            cards: RefCell::new(scenario.get("cards").unwrap().unwrap()),
            scenario,
        };
        let previous_projects = context.projects.borrow().clone();
        let previous_cards = context.cards.borrow().clone();
        let body = context.scenario.get::<Document>("body").unwrap().unwrap();
        let active: Vec<String> = context.scenario.get("active").unwrap().unwrap_or_default();
        let busy = |id: &str| active.iter().any(|active| active == id);
        let windows = context.scenario.truthy("windows").unwrap();
        let result = (|| -> Result<Document> {
            match context.scenario.get::<String>("action")?.unwrap().as_str() {
                "create" => projects::create(&context, &body, windows),
                "archive" => {
                    let count = projects::archive_tasks(&context, &body, windows, busy)?;
                    let mut result = Document::default();
                    result.set("count", &count)?;
                    Ok(result)
                }
                "remove" => {
                    let id = context
                        .scenario
                        .get::<String>("id")?
                        .unwrap_or("project".into());
                    let archived = projects::remove(
                        &context,
                        &id,
                        &body,
                        windows,
                        busy,
                        context.scenario.truthy("running")?,
                    )?;
                    let mut result = Document::default();
                    result.set("ok", &true)?;
                    result.set("archived", &archived)?;
                    Ok(result)
                }
                _ => panic!("Unknown project operation"),
            }
        })();
        let mut actual = Document::default();
        match result {
            Ok(value) => actual.set("value", &value).unwrap(),
            Err(error) => {
                *context.projects.borrow_mut() = previous_projects;
                *context.cards.borrow_mut() = previous_cards;
                actual.set("error", &error.to_string()).unwrap();
            }
        }
        actual.set("projects", &*context.projects.borrow()).unwrap();
        actual.set("cards", &*context.cards.borrow()).unwrap();
        let actual = serde_json::value::to_raw_value(&actual).unwrap();
        let expected = context.scenario.raw("expected").unwrap();
        assert!(
            json::equivalent(&actual, expected),
            "case {index}\nactual: {actual}\nexpected: {expected}"
        );
    }
}
