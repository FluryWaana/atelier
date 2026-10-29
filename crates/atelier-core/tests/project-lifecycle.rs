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

#[test]
fn jira_mapping_edits_preserve_unrelated_fields_and_reject_stale_snapshots() {
    let context = Context {
        scenario: Document::default(),
        projects: RefCell::new(vec![serde_json::from_str(r#"{"id":"project","name":"Synthetic","repo":"checked:repo","projectRules":"human rules"}"#).unwrap()]),
        cards: RefCell::new(vec![]),
        events: RefCell::default(),
    };
    let body: Document = serde_json::from_str(r#"{"jiraMode":"custom","jiraStatusMap":{"doing":"31"},"expected":{"jiraMode":"inherit","jiraStatusMap":{}}}"#).unwrap();
    let edit = projects::prepare_update(
        &context,
        "project",
        &body,
        false,
        |_| false,
        |_| false,
        false,
    )
    .unwrap();
    assert!(
        edit.project
            .string_equals("projectRules", "human rules")
            .unwrap()
    );
    assert!(edit.project.string_equals("jiraMode", "custom").unwrap());
    assert_eq!(
        edit.project
            .get::<serde_json::Value>("jiraStatusMap")
            .unwrap()
            .unwrap()["doing"],
        "31"
    );
    context.projects.borrow_mut()[0] = edit.project;
    assert!(
        projects::prepare_update(
            &context,
            "project",
            &body,
            false,
            |_| false,
            |_| false,
            false
        )
        .is_err()
    );
    for invalid in [
        r#"{"jiraMode":"custom","jiraStatusMap":{}}"#,
        r#"{"jiraStatusMap":{"unknown":"31"}}"#,
    ] {
        assert!(
            projects::prepare_update(
                &context,
                "project",
                &serde_json::from_str(invalid).unwrap(),
                false,
                |_| false,
                |_| false,
                false
            )
            .is_err()
        );
    }
}
#[path = "support/json.rs"]
mod json;

#[test]
fn jira_mapping_only_suggests_one_available_unambiguous_transition() {
    use atelier_core::contracts::{JiraStatus, JiraTransition};
    let mut options = vec![JiraTransition {
        id: "transition".into(),
        name: "Begin".into(),
        target: JiraStatus {
            id: "31".into(),
            name: "In Progress".into(),
        },
        requires_fields: false,
    }];
    let mut settings =
        serde_json::json!({"jiraAutoTransition":true,"jiraStatusMap":{"doing":"in progress"}});
    assert_eq!(
        projects::mapped_jira_transition(&settings, "doing", &options),
        Some("transition")
    );
    options.push(options[0].clone());
    assert_eq!(
        projects::mapped_jira_transition(&settings, "doing", &options),
        None
    );
    options.pop();
    options[0].requires_fields = true;
    assert_eq!(
        projects::mapped_jira_transition(&settings, "doing", &options),
        None
    );
    options[0].requires_fields = false;
    settings["jiraAutoTransition"] = serde_json::json!(false);
    assert_eq!(
        projects::mapped_jira_transition(&settings, "doing", &options),
        None
    );
}

#[test]
fn inverse_jira_mapping_refuses_ambiguous_and_disabled_columns() {
    let mut settings =
        serde_json::json!({"jiraAutoTransition":true,"jiraStatusMap":{"doing":"31"}});
    assert_eq!(
        projects::mapped_jira_status(&settings, "31", "In Progress").as_deref(),
        Some("doing")
    );
    settings["jiraStatusMap"]["todo"] = serde_json::json!("in progress");
    assert_eq!(
        projects::mapped_jira_status(&settings, "31", "In Progress"),
        None
    );
    settings["jiraAutoTransition"] = serde_json::json!(false);
    assert_eq!(projects::mapped_jira_status(&settings, "31", "Other"), None);
}

struct Context {
    scenario: Document,
    projects: RefCell<Vec<Document>>,
    cards: RefCell<Vec<Document>>,
    events: RefCell<Vec<Box<RawValue>>>,
}
impl CreationContext for Context {
    fn settings(&self) -> Result<Document> {
        Ok(self.scenario.get("settings")?.unwrap_or_default())
    }
    fn agent_for(&self, _: &Document) -> Result<JsonString> {
        panic!("Project lifecycle does not select agents");
    }
    fn repository(&self, value: &RawValue) -> Result<JsonString> {
        let mut event = Document::default();
        event.set("repository", &value)?;
        self.events
            .borrow_mut()
            .push(serde_json::value::to_raw_value(&event)?);
        let value: JsonString =
            serde_json::from_str(value.get()).map_err(|_| "message.invalid_or_overly_long_text")?;
        let value = text_units(&value, 2000)?;
        let mut repo = JsonString::from("checked:");
        repo.0.extend(value.0);
        Ok(repo)
    }
    fn new_id(&self) -> String {
        "new-project".into()
    }
    fn timestamp(&self) -> Result<String> {
        self.events
            .borrow_mut()
            .push(serde_json::value::to_raw_value(&"timestamp")?);
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
        let id = project.get::<String>("id")?.unwrap();
        self.events
            .borrow_mut()
            .push(serde_json::value::to_raw_value(&format!("project:{id}"))?);
        let mut projects = self.projects.borrow_mut();
        if let Some(existing) = projects
            .iter_mut()
            .find(|project| project.string_equals("id", &id).unwrap())
        {
            *existing = project.clone();
        } else {
            projects.push(project.clone());
        }
        Ok(())
    }
    fn save_card(&self, card: &Document) -> Result<()> {
        let id = card.get::<String>("id")?.unwrap();
        self.events
            .borrow_mut()
            .push(serde_json::value::to_raw_value(&format!("card:{id}"))?);
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
        "fixtures/rust-project-lifecycle-reference.json"
    ))
    .unwrap();
    assert!(cases.len() >= 35);
    for (index, scenario) in cases.into_iter().enumerate() {
        let context = Context {
            projects: RefCell::new(scenario.get("projects").unwrap().unwrap()),
            cards: RefCell::new(scenario.get("cards").unwrap().unwrap()),
            events: RefCell::new(Vec::new()),
            scenario,
        };
        let previous_projects = context.projects.borrow().clone();
        let previous_cards = context.cards.borrow().clone();
        let body = context.scenario.get::<Document>("body").unwrap().unwrap();
        // Renaming is now supported; the former ignored-name contract is retired below.
        if context.scenario.string_equals("action", "update").unwrap() && body.raw("name").is_some()
        {
            continue;
        }
        let active: Vec<String> = context.scenario.get("active").unwrap().unwrap_or_default();
        let busy = |id: &str| active.iter().any(|active| active == id);
        let windows = context.scenario.truthy("windows").unwrap();
        let result = (|| -> Result<Document> {
            match context.scenario.get::<String>("action")?.unwrap().as_str() {
                "create" => projects::create(&context, &body, windows),
                "update" => {
                    let id = context
                        .scenario
                        .get::<String>("id")?
                        .unwrap_or("project".into());
                    let edit = projects::prepare_update(
                        &context,
                        &id,
                        &body,
                        windows,
                        busy,
                        |_| context.scenario.truthy("connectorAvailable").unwrap(),
                        context.scenario.truthy("running")?,
                    )?;
                    if edit.moved {
                        context
                            .events
                            .borrow_mut()
                            .push(serde_json::value::to_raw_value(&"backup")?);
                        if context.scenario.truthy("failBackup")? {
                            return Err("Synthetic backup failure".into());
                        }
                        context
                            .events
                            .borrow_mut()
                            .push(serde_json::value::to_raw_value(&"transaction")?);
                    }
                    context.save_project(&edit.project)?;
                    if edit.moved {
                        for mut card in edit.cards {
                            card.set("repo", &edit.project.raw("repo").unwrap())?;
                            card.set("updatedAt", &context.timestamp()?)?;
                            context.save_card(&card)?;
                            let id = card.get::<String>("id")?.unwrap();
                            let mut event = Document::default();
                            event.set("invalidate", &id)?;
                            event.set("reason", &projects::LOCATION_CHANGE_REASON)?;
                            context
                                .events
                                .borrow_mut()
                                .push(serde_json::value::to_raw_value(&event)?);
                            if context.scenario.string_equals("failReview", &id)? {
                                return Err("Synthetic review failure".into());
                            }
                        }
                    }
                    Ok(edit.project)
                }
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
        if context.scenario.string_equals("action", "update").unwrap() {
            actual.set("events", &*context.events.borrow()).unwrap();
        }
        let actual = serde_json::value::to_raw_value(&actual).unwrap();
        let expected = context.scenario.raw("expected").unwrap();
        assert!(
            json::equivalent(&actual, expected),
            "case {index}\nactual: {actual}\nexpected: {expected}"
        );
    }
}

#[test]
fn project_rename_validates_without_moving_cards_or_changing_settings() {
    let context = Context {
        scenario: Document::default(),
        projects: RefCell::new(vec![serde_json::from_str(r#"{"id":"project","name":"Before","repo":"checked:repo","projectRules":"Keep rules"}"#).unwrap()]),
        cards: RefCell::new(vec![]), events: RefCell::default(),
    };
    for name in ["After", "  Nouveau projet é 🔧  "] {
        let body = serde_json::from_str::<Document>(&serde_json::json!({"name":name}).to_string())
            .unwrap();
        let edit = projects::prepare_update(
            &context,
            "project",
            &body,
            false,
            |_| false,
            |_| false,
            false,
        )
        .unwrap();
        assert!(edit.project.string_equals("name", name.trim()).unwrap());
        assert!(edit.project.string_equals("repo", "checked:repo").unwrap());
        assert!(
            edit.project
                .string_equals("projectRules", "Keep rules")
                .unwrap()
        );
        assert!(!edit.moved);
        assert!(context.events.borrow().is_empty());
    }
    for name in ["", " ", &"x".repeat(101)] {
        let body = serde_json::from_str::<Document>(&serde_json::json!({"name":name}).to_string())
            .unwrap();
        assert!(
            projects::prepare_update(
                &context,
                "project",
                &body,
                false,
                |_| false,
                |_| false,
                false
            )
            .is_err()
        );
    }
}

#[test]
fn verification_command_edits_require_the_observed_project_value() {
    let context = Context {
        scenario: Document::default(),
        projects: RefCell::new(vec![
            serde_json::from_str(r#"{"id":"project","name":"Synthetic","repo":"checked:repo"}"#)
                .unwrap(),
        ]),
        cards: RefCell::new(vec![]),
        events: RefCell::default(),
    };
    let body: Document =
        serde_json::from_str(r#"{"testCommand":"synthetic verify","expected":{"testCommand":""}}"#)
            .unwrap();
    let edit = projects::prepare_update(
        &context,
        "project",
        &body,
        false,
        |_| false,
        |_| false,
        false,
    )
    .unwrap();
    assert!(
        edit.project
            .string_equals("testCommand", "synthetic verify")
            .unwrap()
    );
    context.projects.borrow_mut()[0] = edit.project;
    assert!(
        projects::prepare_update(
            &context,
            "project",
            &body,
            false,
            |_| false,
            |_| false,
            false
        )
        .is_err()
    );
}
