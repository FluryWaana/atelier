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
                "Paramètres d’export Tempo invalides.",
            );
        }
    };
    if input.expected.get::<String>("id").ok().flatten().as_deref() != Some(id.as_str()) {
        return super::failure(
            StatusCode::BAD_REQUEST,
            "Identité de temps locale invalide.",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return super::failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    if action == "export-preview" {
        return response(preview(&state, input).await);
    }
    if !input.confirmed {
        return super::failure(
            StatusCode::BAD_REQUEST,
            "Confirme l’auteur, le ticket et les données du temps à envoyer.",
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
            .ok_or("Prépare un aperçu avant l’export Tempo.")?;
            let jira = store
                .settings::<Document>()?
                .get::<JiraConfiguration>("jiraConnection")?
                .unwrap_or_default();
            if preview.tempo.base != configuration.base
                || preview.jira_url != jira.url
                || jira.mode != JiraMode::Cloud
            {
                return Err("Reconnecte les sites Jira et Tempo de l’envoi incertain.".into());
            }
            let entry = if !reconcile {
                workflow::reserve(store, &requested, &operation_id)?
            } else {
                let entry = store
                    .document::<Document>(Documents::Times, &id)?
                    .ok_or("Entrée de temps introuvable.")?;
                if serde_json::to_value(&entry)? != serde_json::to_value(&requested.expected)?
                    || !entry.string_equals("exportState", "uncertain")?
                {
                    return Err("La saisie de temps a changé. Relis-la avant l’export.".into());
                }
                entry
            };
            let intent = entry
                .get::<TempoExportIntent>("tempoExport")?
                .ok_or("Intention d’export Tempo absente.")?;
            if reconcile && intent.state != Phase::Uncertain {
                return Err("Aucun export Tempo incertain à vérifier.".into());
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
            client.find(&entry_id,&intent.preview.payload).await?.ok_or("Aucun journal Tempo retrouvé. L’envoi reste incertain ; aucun renvoi automatique.")?
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
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! {
            result=tokio::time::timeout(std::time::Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err("Export Tempo interrompu ou trop long. Relis le journal avant toute nouvelle tentative.".into())),
            _=closing.changed()=>Err("Atelier est en cours d’arrêt.".into()),
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
                if !entry.string_equals("exportState", "draft")? || entry.truthy("runningSince")? {
                    return Err(
                        "Arrête le chronomètre ; seul un temps non envoyé peut être préparé."
                            .into(),
                    );
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
            .ok_or("Identité de temps locale invalide.")?;
        uuid::Uuid::parse_str(&id).map_err(|_| "Identité de temps locale invalide.")?;
        let client = jira::Jira::new(jira_configuration, jira_token)?;
        let issue_id = ticket
            .get::<String>("id")?
            .ok_or("Identifiant Jira invalide.")?;
        let issue = client.issue_by_id(&issue_id).await?;
        if !ticket.string_equals("key", &issue.key)? {
            return Err("La clé Jira a changé. Relis le ticket avant l’export.".into());
        }
        let account = client.account_id().await?;
        let fields = atelier_core::domain::time::time_fields(&entry)?;
        let payload = TempoPayload {
            issue_id: issue.id.parse().map_err(|_| "Identifiant Jira invalide.")?,
            author_account_id: account,
            time_spent_seconds: fields.seconds,
            start_date: fields.date,
            start_time: fields.start_time,
            description: format!("{}\n[atelier:{id}]", fields.description.utf8()?),
        };
        tempo::Tempo::new(&configuration.base, tempo_token)?.validate(&payload)?;
        state
            .database
            .call(move |store| workflow::save_preview(store, &input, configuration, payload))
            .await
    };
    tokio::time::timeout(std::time::Duration::from_secs(90), operation)
        .await
        .unwrap_or_else(|_| Err("Préparation de l’export Tempo trop longue.".into()))
}
