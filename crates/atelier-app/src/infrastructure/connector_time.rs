use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::extensions::{
        self as protocol, Installed, Method, RemoteOperation, Report, TimeProposal, TimeSource,
        TimeTicket,
    },
    contracts::{Card, ForgeWriteState as Phase},
    document::Document,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    expected: Document,
    expected_card: Card,
    #[serde(default)]
    report_id: Option<String>,
}

pub fn proposal(value: &Value) -> Result<TimeProposal> {
    let proposal = protocol::time_proposal(value)?;
    if proposal.instance.chars().any(char::is_control)
        || proposal.instance.contains(['\\', '%'])
        || super::jira::normalize_url(&proposal.instance)? != proposal.instance
    {
        return Err("message.invalid_time_export_destination".into());
    }
    Ok(proposal)
}

fn source(entry: &Document, card: &Card) -> Result<TimeSource> {
    let fields = atelier_core::domain::time::time_fields(entry)?;
    let id = entry
        .get::<String>("id")?
        .ok_or("message.time_identity_missing")?;
    uuid::Uuid::parse_str(&id).map_err(|_| "message.time_identity_invalid")?;
    if !entry.string_equals("cardId", &card.id)? || entry.has_text("runningSince")? {
        return Err("message.stop_the_timer_and_reread_the_task_before_this_export".into());
    }
    let ticket = card
        .fields
        .get::<Document>("ticket")?
        .map(|ticket| -> Result<_> {
            Ok(TimeTicket {
                connector: ticket
                    .get::<String>("connector")?
                    .ok_or("message.ticket_connector_missing")?,
                instance: ticket
                    .get::<String>("instance")?
                    .ok_or("message.ticket_instance_missing")?,
                id: ticket
                    .get::<String>("id")?
                    .ok_or("message.ticket_identity_missing")?,
                key: ticket
                    .get::<String>("key")?
                    .ok_or("message.ticket_key_absent")?,
            })
        })
        .transpose()?;
    Ok(TimeSource {
        id,
        card_id: card.id.clone(),
        card_title: card.title.clone(),
        ticket,
        seconds: fields.seconds,
        date: fields.date,
        start_time: fields.start_time,
        description: fields.description.clone(),
    })
}

fn select(store: &Store<'_>, value: &Value) -> Result<(Selection, TimeSource)> {
    let selection: Selection = serde_json::from_str(&value.to_string())?;
    let id = selection
        .expected
        .get::<String>("id")?
        .ok_or("message.time_identity_missing")?;
    let entry = store
        .document::<Document>(Documents::Times, &id)?
        .ok_or("message.time_entry_not_found")?;
    if serde_json::to_value(&entry)? != serde_json::to_value(&selection.expected)?
        || !entry.string_equals("exportState", "draft")?
    {
        return Err(
            "message.the_entry_changed_or_was_already_sent_reread_it_before_exporting".into(),
        );
    }
    let card = super::forge_association::snapshot(store, &selection.expected_card)?;
    Ok((selection, source(&entry, &card)?))
}

pub fn preview_input(store: &Store<'_>, value: &Value) -> Result<Value> {
    let (selection, source) = select(store, value)?;
    if selection.report_id.is_some() {
        return Err("message.export_read_request_invalid".into());
    }
    Ok(serde_json::to_value(source)?)
}

pub fn validate_report(store: &Store<'_>, report: &Report) -> Result<()> {
    let proposal = proposal(&report.output)?;
    let entry = require_source(store, &proposal.source)?;
    if !entry.string_equals("exportState", "draft")? {
        return Err("message.this_entry_can_no_longer_be_prepared".into());
    }
    Ok(())
}

pub fn prepare_preview(store: &Store<'_>, profile: &Installed, value: &Value) -> Result<Value> {
    let (selection, source) = select(store, value)?;
    let report = profile
        .report
        .as_ref()
        .filter(|report| {
            report.method == Method::TimePreview && Some(&report.id) == selection.report_id.as_ref()
        })
        .ok_or("message.reread_this_module_s_time_preview_before_confirming")?;
    let age = time::OffsetDateTime::now_utc()
        - time::OffsetDateTime::parse(
            &report.observed_at,
            &time::format_description::well_known::Rfc3339,
        )?;
    if age > time::Duration::minutes(10) || age < time::Duration::seconds(-60) {
        return Err("message.time_export_preview_expired_reread_its_destination".into());
    }
    let proposal = proposal(&report.output)?;
    if proposal.source != source {
        return Err("message.the_entry_or_ticket_changed_since_the_module_preview".into());
    }
    if super::connector_effects::read(&store.settings::<Document>()?)?
        .iter()
        .any(|operation| {
            operation.method == Method::TimeSend
                && matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
                && operation.input["source"]["id"].as_str() == Some(source.id.as_str())
        })
    {
        return Err("message.this_entry_already_has_a_confirmed_export_in_the_journal".into());
    }
    Ok(serde_json::to_value(proposal)?)
}

fn require_source(store: &Store<'_>, expected: &TimeSource) -> Result<Document> {
    let entry = store
        .document::<Document>(Documents::Times, &expected.id)?
        .ok_or("message.time_entry_not_found")?;
    let card = store
        .document::<Card>(Documents::Cards, &expected.card_id)?
        .ok_or("message.card_not_found")?;
    let card = super::forge_association::snapshot(store, &card)?;
    if source(&entry, &card)? != *expected {
        return Err(
            "message.the_entry_or_ticket_scope_changed_no_export_result_was_applied".into(),
        );
    }
    Ok(entry)
}

pub fn before_send(store: &Store<'_>, operation: &RemoteOperation) -> Result<()> {
    if operation.method != Method::TimeSend {
        return Ok(());
    }
    let proposal = proposal(&operation.input)?;
    let entry = require_source(store, &proposal.source)?;
    if !entry.string_equals("exportState", "draft")?
        || entry.raw("tempoExport").is_some()
        || entry.raw("connectorExport").is_some()
    {
        return Err("message.this_entry_already_retains_an_export_intent_check_its_history".into());
    }
    Ok(())
}

pub fn synchronize(store: &Store<'_>, operation: &RemoteOperation) -> Result<()> {
    if operation.method != Method::TimeSend {
        return Ok(());
    }
    let proposal = proposal(&operation.input)?;
    if operation.state == Phase::Preparing
        || operation.state == Phase::Failed && operation.request_id.is_none()
    {
        return Ok(());
    }
    let mut entry = store
        .document::<Document>(Documents::Times, &proposal.source.id)?
        .ok_or("message.time_entry_not_found")?;
    if entry
        .get::<String>("connectorExport")?
        .is_some_and(|id| id != operation.id)
    {
        return Err("message.this_entry_is_linked_to_another_remote_operation".into());
    }
    if matches!(operation.state, Phase::Confirmed | Phase::Reconciled) {
        let fields = atelier_core::domain::time::time_fields(&entry)?;
        if fields.seconds != proposal.source.seconds
            || fields.date != proposal.source.date
            || fields.start_time != proposal.source.start_time
            || fields.description.clone() != proposal.source.description
            || !entry.string_equals("cardId", &proposal.source.card_id)?
        {
            return Err(
                "message.the_local_entry_differs_from_the_time_actually_observed_no_succe".into(),
            );
        }
        let reply = operation
            .reply
            .as_ref()
            .ok_or("message.export_observation_missing")?;
        if reply
            .output
            .get("time")
            .map(self::proposal)
            .transpose()?
            .as_ref()
            != Some(&proposal)
        {
            return Err(
                "message.the_module_did_not_observe_the_exact_entry_and_destination".into(),
            );
        }
    }
    let state = match operation.state {
        Phase::Sending => "sending",
        Phase::Uncertain => "uncertain",
        Phase::Confirmed | Phase::Reconciled => "exported",
        Phase::Failed => "draft",
        Phase::Preparing => unreachable!(),
    };
    if operation.state == Phase::Failed {
        entry.remove("connectorExport");
        entry.remove("exportConnector");
    } else {
        entry.set("connectorExport", &operation.id)?;
        entry.set("exportConnector", &operation.module)?;
    }
    entry.set("exportState", &state)?;
    entry.remove("tempoExportPreview");
    entry.remove("error");
    store.put(Documents::Times, &entry)
}
