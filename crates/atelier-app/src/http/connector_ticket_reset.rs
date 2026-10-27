use super::{Shared, responses::response};
use crate::infrastructure::{connector_ticket_reset as reset, connectors, extensions::Executable};
use atelier_core::application::extensions::{
    Invocation, Method, Report, Status, TicketResetApply, TicketResetRead,
};
use axum::{
    extract::{Path, State},
    response::Response,
};
use std::{path::PathBuf, time::Duration};

pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    let input: TicketResetRead = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let operation = async {
        let vault = state.vault.clone();
        let module = id.clone();
        let (profile, mut request, source) = state
            .database
            .call(move |store| {
                let (profile, request) = connectors::invocation(
                    store,
                    &module,
                    &Invocation {
                        expected_revision: input.expected_revision,
                        method: Method::TicketsSearch,
                        action: None,
                        input: serde_json::json!({"cursor":null}),
                        confirmed: input.confirmed,
                    },
                    &mut |value, decrypt| vault.protect(value, decrypt),
                )?;
                if !profile.manifest.methods.contains(&Method::TicketsReset) {
                    return Err(
                        "Ce connecteur ne déclare pas la resynchronisation complète.".into(),
                    );
                }
                let source = reset::source(store, &profile)?;
                Ok((profile, request, source))
            })
            .await?;
        let mut traversal = reset::Traversal::default();
        loop {
            request.request_id = format!("request-{}", uuid::Uuid::new_v4());
            let executable = Executable::open(
                PathBuf::from(&profile.binary.path),
                Some(profile.binary.sha256.clone()),
            )
            .await?;
            let reply = executable
                .prepare(&profile.manifest, request.clone(), Duration::from_secs(30))?
                .dispatch()
                .await?;
            if reply.status != Status::Ok {
                return Err(
                    "Lecture complète interrompue par le connecteur ; aucune carte modifiée."
                        .into(),
                );
            }
            let report = Report {
                id: uuid::Uuid::new_v4().to_string(),
                method: reply.method,
                action: None,
                message: reply.message,
                output: reply.output,
                observed_at: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)?,
            };
            match traversal.page(&report)? {
                Some(cursor) => request.input = serde_json::json!({"cursor":cursor}),
                None => break,
            }
        }
        state
            .database
            .call(move |store| traversal.save(store, &profile, source))
            .await
    };
    response(match tokio::time::timeout(Duration::from_secs(180),operation).await {
        Ok(result)=>result,
        Err(_)=>Err("Lecture complète trop longue ; aucune carte modifiée, aucun résultat partiel appliqué.".into()),
    })
}

pub(super) async fn apply(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    let input: TicketResetApply = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let result = state
        .database
        .call(move |store| reset::apply(store, &id, &input))
        .await;
    if result.is_ok() {
        let _ = state.changed.send(());
    }
    response(result)
}
