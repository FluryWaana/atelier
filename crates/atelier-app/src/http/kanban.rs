use super::{Shared, responses::response};
use crate::infrastructure::store::Documents;
use atelier_core::{
    application::{jira::JiraFailure, kanban::Configuration, projects::ProjectFailure},
    contracts::JiraConfiguration,
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    response::Response,
};
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    configuration: Configuration,
    expected: Option<Configuration>,
    jira_revision: Option<u64>,
}
pub(super) async fn save(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Input = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let result = state
        .database
        .call(move |store| persist(store, &id, input))
        .await;
    if result.is_ok() {
        let _ = state.changed.send(());
    }
    response(result)
}

fn persist(
    store: &crate::infrastructure::store::Store<'_>,
    id: &str,
    input: Input,
) -> crate::Result<Document> {
    input
        .configuration
        .validate()
        .map_err(ProjectFailure::Invalid)?;
    let mut project = store
        .document::<Document>(Documents::Projects, id)?
        .ok_or(ProjectFailure::Missing)?;
    if project.get::<Configuration>("kanban")? != input.expected {
        return Err(ProjectFailure::Conflict("message.kanban_configuration_changed").into());
    }
    if let Some(mapping) = &input.configuration.jira {
        let current = store
            .settings::<Document>()?
            .get::<JiraConfiguration>("jiraConnection")?
            .unwrap_or_default();
        if input.jira_revision != Some(current.revision)
            || mapping.instance != current.url
            || mapping.account != current.email
            || mapping.mode != current.mode
        {
            return Err(JiraFailure::Conflict.into());
        }
    }
    project.set("kanban", &input.configuration)?;
    store.put(Documents::Projects, &project)?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn saves_are_conflict_checked_and_preserve_cards_on_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("synthetic.sqlite");
        let database = crate::infrastructure::database::Database::open(&path).unwrap();
        let (before,configuration,card)=database.call(|store|{
            store.put(Documents::Projects,&json!({"id":"synthetic","name":"Keep project","repo":"synthetic-repository"}))?;
            let card=json!({"id":"synthetic-card","title":"Keep card","status":"review","repo":"synthetic-repository"});store.put(Documents::Cards,&card)?;
            let mut configuration=Configuration::default();configuration.columns[0].name="Ready".into();
            persist(store,"synthetic",Input{configuration:configuration.clone(),expected:None,jira_revision:None})?;
            let before=store.document::<Document>(Documents::Projects,"synthetic")?.unwrap();
            let stale=persist(store,"synthetic",Input{configuration:Configuration::default(),expected:None,jira_revision:None}).unwrap_err();
            assert!(matches!(stale.downcast_ref::<ProjectFailure>(),Some(ProjectFailure::Conflict(_))));
            Ok((before,configuration,card))
        }).await.unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TRIGGER refuse_columns BEFORE UPDATE ON projects BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        database
            .call(move |store| {
                assert!(
                    persist(
                        store,
                        "synthetic",
                        Input {
                            configuration: Configuration::default(),
                            expected: Some(configuration),
                            jira_revision: None
                        }
                    )
                    .is_err()
                );
                assert_eq!(
                    serde_json::to_value(
                        store
                            .document::<Document>(Documents::Projects, "synthetic")?
                            .unwrap()
                    )?,
                    serde_json::to_value(before)?
                );
                assert_eq!(
                    store.document::<serde_json::Value>(Documents::Cards, "synthetic-card")?,
                    Some(card)
                );
                Ok(())
            })
            .await
            .unwrap();
        database.close().await.unwrap();
    }
}
