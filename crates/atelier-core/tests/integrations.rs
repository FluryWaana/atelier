use atelier_core::{
    application::{
        integrations::{self, Kind},
        settings::SettingsFailure,
    },
    document::Document,
};
use serde_json::json;

fn settings() -> Document {
    serde_json::from_value(json!({"processConnectors":[{
        "manifest":{"id":"synthetic-tracker","name":"Synthetic tracker","description":"Synthetic module","sourceLocale":"en-US","methods":["check","ticketsSearch","ticketsRead","timePreview","timeSend","timeReconcile"],"fields":[],"tabs":[]},
        "binary":{"path":"synthetic-agent","sha256":"a".repeat(64)},"revision":1,"values":{},"report":null
    }],"ticketLink":{"connector":"jira","id":"42"},"timeOperation":{"provider":"tempo","state":"uncertain"}})).unwrap()
}

#[test]
fn defaults_are_local_without_modules_or_credentials() {
    for kind in [Kind::Tickets, Kind::Time] {
        assert_eq!(
            integrations::selected(&Document::default(), kind).unwrap(),
            "local"
        );
        assert!(integrations::available(&Document::default(), kind, "local").unwrap());
    }
}

#[test]
fn catalog_uses_capabilities_and_never_exposes_module_configuration() {
    let mut settings = settings();
    let mut installed = settings
        .get::<Vec<atelier_core::application::extensions::Installed>>("processConnectors")
        .unwrap()
        .unwrap();
    installed[0]
        .values
        .insert("private-fixture".into(), json!("synthetic-not-public"));
    settings.set("processConnectors", &installed).unwrap();
    let catalog = integrations::catalog(&settings).unwrap();
    let provider = catalog
        .iter()
        .find(|provider| provider.id == "extension:synthetic-tracker")
        .unwrap();
    assert_eq!(provider.kinds, vec![Kind::Tickets, Kind::Time]);
    assert!(
        !serde_json::to_string(&catalog)
            .unwrap()
            .contains("synthetic-not-public")
    );
    assert!(!integrations::available(&settings, Kind::Time, "jira").unwrap());
    assert!(!integrations::available(&settings, Kind::Tickets, "tempo").unwrap());
    installed[0].manifest.methods = vec![
        atelier_core::application::extensions::Method::Check,
        atelier_core::application::extensions::Method::TimeSend,
    ];
    settings.set("processConnectors", &installed).unwrap();
    assert!(
        !integrations::available(&settings, Kind::Time, "extension:synthetic-tracker").unwrap()
    );
}

#[test]
fn selection_is_conflict_checked_and_preserves_existing_sources() {
    let original = settings();
    let next = integrations::choose(
        &original,
        Kind::Tickets,
        "extension:synthetic-tracker",
        "local",
    )
    .unwrap();
    assert_eq!(
        integrations::selected(&next, Kind::Tickets).unwrap(),
        "extension:synthetic-tracker"
    );
    assert_eq!(integrations::selected(&next, Kind::Time).unwrap(), "local");
    for key in ["ticketLink", "timeOperation", "processConnectors"] {
        assert_eq!(
            original.get::<serde_json::Value>(key).unwrap(),
            next.get::<serde_json::Value>(key).unwrap()
        );
    }
    let stale = integrations::choose(&next, Kind::Tickets, "jira", "local").unwrap_err();
    assert!(matches!(
        stale.downcast_ref::<SettingsFailure>(),
        Some(SettingsFailure::Conflict)
    ));
    for invalid in ["extension:absent", "tempo", "", "extension:../../jira"] {
        assert!(integrations::choose(&original, Kind::Tickets, invalid, "local").is_err());
    }
}

#[test]
fn removing_selected_module_resets_only_future_defaults() {
    let mut original = settings();
    original = integrations::choose(
        &original,
        Kind::Tickets,
        "extension:synthetic-tracker",
        "local",
    )
    .unwrap();
    original = integrations::choose(
        &original,
        Kind::Time,
        "extension:synthetic-tracker",
        "local",
    )
    .unwrap();
    let mut removed = original.clone();
    integrations::removed(&mut removed, "other").unwrap();
    assert_eq!(
        integrations::selected(&removed, Kind::Tickets).unwrap(),
        "extension:synthetic-tracker"
    );
    integrations::removed(&mut removed, "synthetic-tracker").unwrap();
    assert_eq!(
        integrations::selected(&removed, Kind::Tickets).unwrap(),
        "local"
    );
    assert_eq!(
        integrations::selected(&removed, Kind::Time).unwrap(),
        "local"
    );
    assert_eq!(
        original.get::<serde_json::Value>("ticketLink").unwrap(),
        removed.get::<serde_json::Value>("ticketLink").unwrap()
    );
    assert_eq!(
        original.get::<serde_json::Value>("timeOperation").unwrap(),
        removed.get::<serde_json::Value>("timeOperation").unwrap()
    );
}
