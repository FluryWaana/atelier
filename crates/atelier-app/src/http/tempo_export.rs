use super::{Shared, responses::response};
use crate::{
    Result,
    infrastructure::{jira, store::Documents, tempo, tempo_export as workflow},
};
use atelier_core::{
    contracts::{
        ForgeWriteState as Phase, JiraConfiguration, JiraMode, TempoExportIntent,
        TempoExportPreview, TempoExportRequest, TempoPayload,
    },
    document::Document,
};
use axum::{http::StatusCode, response::Response};

pub(super) async fn action(state: Shared, id: String, action: &str, body: Document) -> Response {
    let input: TempoExportRequest = match serde_json::to_string(&body)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
    {
        Some(input) => input,
        None => {
            return super::failure(
                StatusCode::BAD_REQUEST,
                "message.invalid_tempo_export_parameters",
            );
        }
    };
    if input.expected.get::<String>("id").ok().flatten().as_deref() != Some(id.as_str()) {
        return super::failure(
            StatusCode::BAD_REQUEST,
            "message.invalid_local_time_identity",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return super::failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    if action == "export-preview" {
        return response(preview(&state, input).await);
    }
    if !input.confirmed {
        return super::failure(
            StatusCode::BAD_REQUEST,
            "message.confirm_the_author_ticket_and_time_data_to_send",
        );
    }
    let reconcile = action == "reconcile";
    let requested = input.clone();
    let vault = state.vault.clone();
    let operation_id = uuid::Uuid::new_v4().to_string();
    let reserved = state
        .database
        .call(move |store| {
            let (configuration, token) =
                tempo::credentials(store, requested.tempo_revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })?;
            jira::require_revision(store, requested.jira_revision)?;
            let preview = if reconcile {
                requested
                    .expected
                    .get::<TempoExportIntent>("tempoExport")?
                    .map(|intent| intent.preview)
            } else {
                requested
                    .expected
                    .get::<TempoExportPreview>("tempoExportPreview")?
            }
            .ok_or("message.prepare_a_preview_before_exporting_to_tempo")?;
            let jira = store
                .settings::<Document>()?
                .get::<JiraConfiguration>("jiraConnection")?
                .unwrap_or_default();
            if preview.tempo.base != configuration.base
                || preview.jira_url != jira.url
                || jira.mode != JiraMode::Cloud
            {
                return Err(
                    "message.reconnect_the_jira_and_tempo_sites_of_the_uncertain_export".into(),
                );
            }
            let entry = if !reconcile {
                workflow::reserve(store, &requested, &operation_id)?
            } else {
                let entry = store
                    .document::<Document>(Documents::Times, &id)?
                    .ok_or("message.time_entry_not_found")?;
                if serde_json::to_value(&entry)? != serde_json::to_value(&requested.expected)?
                    || !entry.string_equals("exportState", "uncertain")?
                {
                    return Err("message.the_time_entry_changed_read_it_again_before_export".into());
                }
                entry
            };
            let intent = entry
                .get::<TempoExportIntent>("tempoExport")?
                .ok_or("message.missing_tempo_export_intent")?;
            if reconcile && intent.state != Phase::Uncertain {
                return Err("message.no_uncertain_tempo_export_to_check".into());
            }
            Ok((entry, intent, configuration, token))
        })
        .await;
    let (entry, intent, configuration, token) = match reserved {
        Ok(reserved) => reserved,
        Err(error) => return response::<Document>(Err(error)),
    };
    let _ = state.changed.send(());
    let entry_id = entry
        .get::<String>("id")
        .ok()
        .flatten()
        .expect("reserved time identity");
    let work = async {
        let client = tempo::Tempo::new(&configuration.base, token)?;
        let worklog = if !reconcile {
            let worklog = client.create(&intent.preview.payload).await?;
            let entry_id = entry_id.clone();
            let operation_id = intent.id.clone();
            state
                .database
                .call(move |store| workflow::remember(store, &entry_id, &operation_id, worklog))
                .await?;
            client.read(worklog, &intent.preview.payload).await?
        } else if let Some(worklog) = intent.worklog_id {
            client.read(worklog, &intent.preview.payload).await?
        } else {
            client
                .find(&entry_id, &intent.preview.payload)
                .await?
                .ok_or("message.no_tempo_worklog_found_export_remains_uncertain_no_automatic_res")?
        };
        let entry_id = entry_id.clone();
        let operation_id = intent.id.clone();
        state
            .database
            .call(move |store| {
                workflow::record(
                    store,
                    &entry_id,
                    &operation_id,
                    worklog,
                    configuration.revision,
                    input.jira_revision,
                )
            })
            .await
    };
    let mut closing = state.shutdown.subscribe();
    let stopping = *closing.borrow();
    let result = if stopping {
        drop(work);
        Err("message.atelier_is_shutting_down".into())
    } else {
        tokio::select! {
            result=tokio::time::timeout(std::time::Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err("message.tempo_export_interrupted_or_timed_out_read_the_worklog_before_tr".into())),
            _=closing.changed()=>Err("message.atelier_is_shutting_down".into()),
        }
    };
    let result = match result {
        Err(error) if !reconcile => {
            let operation_id = intent.id;
            match state
                .database
                .call(move |store| workflow::uncertain(store, &entry_id, &operation_id))
                .await
            {
                Ok(_) => Err(error),
                Err(recording) => Err(recording),
            }
        }
        result => result,
    };
    let _ = state.changed.send(());
    response(result)
}

async fn preview(state: &Shared, input: TempoExportRequest) -> Result<Document> {
    let operation = async {
        let requested = input.clone();
        let vault = state.vault.clone();
        let (entry, ticket, configuration, tempo_token, jira_configuration, jira_token) = state
            .database
            .call(move |store| {
                let (entry, _, ticket) = workflow::selected(store, &requested)?;
                if !entry.string_equals("exportState", "draft")?
                    || entry.has_text("runningSince")?
                {
                    return Err("message.stop_the_timer_only_unsent_time_can_be_prepared".into());
                }
                let (configuration, tempo_token) =
                    tempo::credentials(store, requested.tempo_revision, &mut |value, decrypt| {
                        vault.protect(value, decrypt)
                    })?;
                let (jira_configuration, jira_token) =
                    jira::credentials(store, requested.jira_revision, &mut |value, decrypt| {
                        vault.protect(value, decrypt)
                    })?;
                Ok((
                    entry,
                    ticket,
                    configuration,
                    tempo_token,
                    jira_configuration,
                    jira_token,
                ))
            })
            .await?;
        let id = entry
            .get::<String>("id")?
            .ok_or("message.invalid_local_time_identity")?;
        uuid::Uuid::parse_str(&id).map_err(|_| "message.invalid_local_time_identity")?;
        let client = jira::Jira::new(jira_configuration, jira_token)?;
        let issue_id = ticket
            .get::<String>("id")?
            .ok_or("message.invalid_jira_issue_identifier")?;
        let issue = client.issue_by_id(&issue_id).await?;
        if !ticket.string_equals("key", &issue.key)? {
            return Err("message.the_jira_key_changed_read_the_ticket_again_before_export".into());
        }
        let account = client.account_id().await?;
        let fields = atelier_core::domain::time::time_fields(&entry)?;
        let payload = TempoPayload {
            issue_id: issue
                .id
                .parse()
                .map_err(|_| "message.invalid_jira_issue_identifier")?,
            author_account_id: account,
            time_spent_seconds: fields.seconds,
            start_date: fields.date,
            start_time: fields.start_time,
            description: format!("{}\n[atelier:{id}]", fields.description.clone()),
        };
        tempo::Tempo::new(&configuration.base, tempo_token)?.validate(&payload)?;
        state
            .database
            .call(move |store| workflow::save_preview(store, &input, configuration, payload))
            .await
    };
    tokio::time::timeout(std::time::Duration::from_secs(90), operation)
        .await
        .unwrap_or_else(|_| Err("message.tempo_export_preparation_timed_out".into()))
}
