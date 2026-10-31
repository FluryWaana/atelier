use atelier_core::{
    application::{
        jira,
        kanban::{Configuration, JiraMapping},
        projects::mapped_jira_status,
    },
    contracts::{JiraBoardColumn, JiraConfiguration, JiraMode},
};
use serde_json::json;
fn mapping() -> Configuration {
    Configuration {
        jira: Some(JiraMapping {
            instance: "https://synthetic.example.test".into(),
            account: "synthetic@example.test".into(),
            mode: JiraMode::Cloud,
            board_id: 42,
            name: "Synthetic board".into(),
            columns: vec![JiraBoardColumn {
                name: "In progress".into(),
                status_ids: vec!["10".into(), "11".into()],
            }],
            targets: [("10".into(), "doing".into()), ("11".into(), "doing".into())].into(),
        }),
        ..Configuration::default()
    }
}
#[test]
fn board_reads_preserve_column_order_and_all_status_ids() {
    let value = json!({"id":42,"name":"Synthetic board","columnConfig":{"columns":[{"name":"Ready","statuses":[]},{"name":"In progress","statuses":[{"id":"10","self":"https://unused.example.test"},{"id":"11"}]}]},"ignored":"not projected"});
    let board = jira::board_configuration(&value, 42, 7).unwrap();
    assert_eq!(board.connection_revision, 7);
    assert_eq!(board.columns[0].name, "Ready");
    assert_eq!(board.columns[1].status_ids, vec!["10", "11"]);
    for invalid in [
        json!({}),
        json!({"id":41,"name":"Other","columnConfig":{"columns":[]}}),
        json!({"id":42,"name":"Invalid\nname","columnConfig":{"columns":[{"name":"Column","statuses":[]}]}}),
        json!({"id":42,"name":"Board","columnConfig":{"columns":[{"name":"A","statuses":[{"id":"10"}]},{"name":"B","statuses":[{"id":"10"}]}]}}),
        json!({"id":42,"name":"Board","columnConfig":{"columns":[{"name":"A","statuses":[{"id":"../10"}]}]}}),
    ] {
        assert!(jira::board_configuration(&invalid, 42, 7).is_err());
    }
}
#[test]
fn renaming_and_reordering_keep_every_workflow_stage() {
    let mut configuration = Configuration::default();
    configuration.columns[0].name = "Ready for development".into();
    configuration.columns.swap(0, 2);
    assert!(configuration.validate().is_ok());
    let mut missing = configuration.clone();
    missing.columns.pop();
    assert!(missing.validate().is_err());
    let mut duplicated = configuration.clone();
    duplicated.columns[1].status = duplicated.columns[0].status.clone();
    assert!(duplicated.validate().is_err());
    let mut invalid = configuration;
    invalid.columns[0].name = "x".repeat(101);
    assert!(invalid.validate().is_err());
}
#[test]
fn multiple_jira_statuses_are_mapped_only_by_explicit_ids_and_matching_scope() {
    let configuration = mapping();
    assert!(configuration.validate().is_ok());
    let connection = JiraConfiguration {
        url: "https://synthetic.example.test".into(),
        email: "synthetic@example.test".into(),
        revision: 7,
        ..Default::default()
    };
    for id in ["10", "11"] {
        assert_eq!(
            configuration.mapped_status(&connection, id).as_deref(),
            Some("doing")
        );
    }
    assert!(configuration.mapped_status(&connection, "12").is_none());
    let mut other = connection.clone();
    other.url = "https://other.example.test".into();
    assert!(configuration.mapped_status(&other, "10").is_none());
    other = connection.clone();
    other.email = "other@example.test".into();
    assert!(configuration.mapped_status(&other, "10").is_none());
    other = connection.clone();
    other.mode = JiraMode::DataCenter;
    assert!(configuration.mapped_status(&other, "10").is_none());
    let settings =
        json!({"kanban":configuration,"jiraConnection":connection,"jiraAutoTransition":false});
    assert_eq!(
        mapped_jira_status(&settings, "11", "Unrelated name").as_deref(),
        Some("doing")
    );
    assert!(mapped_jira_status(&settings, "unknown", "In progress").is_none());
}
#[test]
fn unknown_remote_statuses_and_local_stages_are_rejected() {
    let mut configuration = mapping();
    configuration
        .jira
        .as_mut()
        .unwrap()
        .targets
        .insert("unknown".into(), "doing".into());
    assert!(configuration.validate().is_err());
    let mut configuration = mapping();
    configuration
        .jira
        .as_mut()
        .unwrap()
        .targets
        .insert("10".into(), "custom-stage".into());
    assert!(configuration.validate().is_err());
}

#[test]
fn changed_remote_columns_require_review_instead_of_partial_matching() {
    let mut configuration = mapping();
    configuration.jira.as_mut().unwrap().columns[0]
        .status_ids
        .push("12".into());
    assert_eq!(
        configuration.validate(),
        Err("message.column_mapping_needs_review")
    );
}

#[test]
fn explicit_board_mapping_never_falls_back_to_old_name_matching() {
    let connection = JiraConfiguration {
        url: "https://synthetic.example.test".into(),
        email: "synthetic@example.test".into(),
        ..Default::default()
    };
    let settings = json!({"kanban":mapping(),"jiraConnection":connection,"jiraAutoTransition":true,"jiraStatusMap":{"todo":"In progress"}});
    assert!(mapped_jira_status(&settings, "unmapped", "In progress").is_none());
    let settings = json!({"kanban":Configuration::default(),"jiraConnection":connection,"jiraAutoTransition":true,"jiraStatusMap":{"todo":"In progress"}});
    assert!(mapped_jira_status(&settings, "10", "In progress").is_none());
}

#[test]
fn invalid_explicit_column_configuration_never_enables_legacy_matching() {
    let settings = json!({"kanban":{"columns":[],"jira":null},"jiraConnection":{},"jiraAutoTransition":true,"jiraStatusMap":{"todo":"Open"}});
    assert!(mapped_jira_status(&settings, "1", "Open").is_none());
}
