use atelier_core::Result;
use atelier_core::{application::projects, document::Document, domain::text};
use std::cell::RefCell;
#[path = "support/json.rs"]
mod json;

#[test]
fn historical_project_detection_preserves_sources_drafts_roles_caching_and_unicode() {
    let fixture: Document = serde_json::from_str(include_str!(
        "fixtures/rust-project-detection-reference.json"
    ))
    .unwrap();
    let default_project: Document = fixture.get("project").unwrap().unwrap();
    let detected: Document = fixture.get("detected").unwrap().unwrap();
    let cases: Vec<Document> = fixture.get("cases").unwrap().unwrap();
    assert_eq!(cases.len(), 24);
    for scenario in cases {
        let project = match scenario.raw("project") {
            Some(value) => serde_json::from_str::<Option<Document>>(value.get()).unwrap(),
            None => Some(default_project.clone()),
        };
        let events = RefCell::new(Vec::<Document>::new());
        let mut resolve = |value: &serde_json::value::RawValue| -> Result<String> {
            let mut event = Document::default();
            event.set("repository", &value)?;
            events.borrow_mut().push(event);
            let input: String = serde_json::from_str(value.get())
                .map_err(|_| "message.invalid_or_overly_long_text")?;
            let mut result = String::from("checked:");
            result.push_str(&text(&input, 2000)?);
            Ok(result)
        };
        let mut scan = |repo: &str| -> Result<Document> {
            let mut event = Document::default();
            event.set("scan", repo)?;
            events.borrow_mut().push(event);
            if scenario.flag("failScan")? {
                return Err("Synthetic scan failure".into());
            }
            Ok(detected.clone())
        };
        let result = projects::detect(
            project.as_ref(),
            &scenario.get::<Document>("body").unwrap().unwrap(),
            scenario.flag("useDraft").unwrap(),
            &mut resolve,
            &mut scan,
        );
        let mut actual = Document::default();
        match result {
            Ok(value) => actual.set("value", &value).unwrap(),
            Err(error) => actual.set("error", &error.to_string()).unwrap(),
        }
        actual.set("events", &*events.borrow()).unwrap();
        let actual = serde_json::value::to_raw_value(&actual).unwrap();
        assert!(
            json::equivalent(&actual, scenario.raw("expected").unwrap()),
            "{}: {actual}",
            scenario.get::<String>("name").unwrap().unwrap()
        );
    }
}
