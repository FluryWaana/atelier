use crate::{field, read, write};
use atelier_core::{
    application::extensions::{self, Method as ConnectorMethod, Registry, Report, TicketSync},
    contracts::{JiraBatchImport, JiraConnection, JiraSearch},
    document::Document,
};
use gloo_net::http::Method;
use serde_json::json;
use std::collections::BTreeSet;

pub enum Outcome {
    ConfigurationRequired,
    Complete,
}

pub async fn synchronize(
    credential: &str,
    provider: &str,
    project: Option<Document>,
) -> Result<Outcome, String> {
    if provider == "local" {
        return Ok(Outcome::Complete);
    }
    if provider == "jira" {
        let connection = read(credential, "/api/jira/config")
            .await?
            .json::<JiraConnection>()
            .await
            .map_err(|_| "message.invalid_server_response")?;
        if !connection.token_set || connection.configuration.url.is_empty() {
            return Ok(Outcome::ConfigurationRequired);
        }
        let project = project.ok_or("message.ticket_sync_select_project")?;
        let search = read(
            credential,
            &format!(
                "/api/jira/search?revision={}",
                connection.configuration.revision
            ),
        )
        .await?
        .json::<JiraSearch>()
        .await
        .map_err(|_| "message.invalid_server_response")?;
        for issues in search.issues.chunks(20) {
            let body = serde_json::to_value(JiraBatchImport {
                revision: search.connection_revision,
                issue_ids: issues.iter().map(|issue| issue.id.clone()).collect(),
                project_id: field(&project, "id"),
                confirmed: true,
            })
            .map_err(|_| "message.invalid_server_response")?;
            write(credential, Method::POST, "/api/jira/import-batch", &body).await?;
        }
        return Ok(Outcome::Complete);
    }
    let module = provider
        .strip_prefix("extension:")
        .ok_or("message.integration_unavailable")?;
    let registry = read(credential, "/api/extensions")
        .await?
        .json::<Registry>()
        .await
        .map_err(|_| "message.invalid_server_response")?;
    let connection = registry
        .modules
        .iter()
        .find(|connection| connection.installed.manifest.id == module)
        .ok_or("message.integration_unavailable")?;
    if !registry.native_available
        || !connection
            .installed
            .manifest
            .methods
            .contains(&ConnectorMethod::TicketsSync)
    {
        return Err("message.ticket_sync_unsupported".into());
    }
    let revision = connection.installed.revision;
    let mut cursor = None::<String>;
    let mut cursors = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let started = js_sys::Date::now();
    let mut bytes = 0usize;
    // shortcut: bounded incremental imports preserve completed pages on failure, retry to resume.
    for _ in 0..100 {
        if js_sys::Date::now() - started > 180_000.0 {
            return Err("message.ticket_sync_pagination_limit".into());
        }
        let report = write(credential, Method::POST, &format!("/api/extensions/{module}/invoke"),
            &json!({"expectedRevision":revision,"method":"ticketsSync","action":null,"input":{"cursor":cursor},"confirmed":true}))
            .await?.json::<Report>().await.map_err(|_| "message.invalid_server_response")?;
        bytes += report.output.to_string().len();
        if bytes > 8 * 1024 * 1024 {
            return Err("message.ticket_sync_pagination_limit".into());
        }
        if report.method != ConnectorMethod::TicketsSync {
            return Err("message.invalid_server_response".into());
        }
        let sync = serde_json::from_value::<TicketSync>(report.output.clone())
            .map_err(|_| "message.invalid_server_response")?;
        if matches!(sync, TicketSync::ConfigurationRequired) {
            return Ok(Outcome::ConfigurationRequired);
        }
        let project = project
            .as_ref()
            .ok_or("message.ticket_sync_select_project")?;
        let page = extensions::ticket_page(report.method, &report.output).map_err(str::to_owned)?;
        for ticket in &page.items {
            if !identities.insert(ticket.identity()) {
                return Err("message.invalid_or_duplicate_connector_ticket".into());
            }
        }
        for tickets in page.items.chunks(20) {
            write(credential, Method::POST, &format!("/api/extensions/{module}/tickets/import"),
                &json!({"expectedRevision":revision,"reportId":report.id,"project":project,"tickets":tickets.iter().map(|ticket|ticket.identity()).collect::<Vec<_>>(),"confirmed":true})).await?;
        }
        match page.next {
            None => return Ok(Outcome::Complete),
            Some(next) if cursors.insert(next.clone()) => cursor = Some(next),
            Some(_) => return Err("message.ticket_sync_pagination_limit".into()),
        }
    }
    Err("message.ticket_sync_pagination_limit".into())
}
