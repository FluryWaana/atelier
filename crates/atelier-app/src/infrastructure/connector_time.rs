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
        return Err("Destination d’export de temps invalide.".into());
    }
    Ok(proposal)
}

fn source(entry: &Document, card: &Card) -> Result<TimeSource> {
    let fields = atelier_core::domain::time::time_fields(entry)?;
    let id = entry
        .get::<String>("id")?
        .ok_or("Identité de temps absente.")?;
    uuid::Uuid::parse_str(&id).map_err(|_| "Identité de temps invalide.")?;
    if !entry.string_equals("cardId", &card.id)? || entry.truthy("runningSince")? {
        return Err("Arrête le chronomètre et relis la tâche avant cet export.".into());
    }
    let ticket = card
        .fields
        .get::<Document>("ticket")?
        .map(|ticket| -> Result<_> {
            Ok(TimeTicket {
                connector: ticket
                    .get::<String>("connector")?
                    .ok_or("Connecteur de ticket absent.")?,
                instance: ticket
                    .get::<String>("instance")?
                    .ok_or("Instance de ticket absente.")?,
                id: ticket
                    .get::<String>("id")?
                    .ok_or("Identité de ticket absente.")?,
                key: ticket
                    .get::<String>("key")?
                    .ok_or("Clé de ticket absente.")?,
            })
        })
        .transpose()?;
    Ok(TimeSource {
        id,
        card_id: card.id.clone(),
        card_title: card.title.utf8()?,
        ticket,
        seconds: fields.seconds,
        date: fields.date,
        start_time: fields.start_time,
        description: fields.description.utf8()?,
    })
}

fn select(store: &Store<'_>, value: &Value) -> Result<(Selection, TimeSource)> {
    let selection: Selection = serde_json::from_str(&value.to_string())?;
    let id = selection
        .expected
        .get::<String>("id")?
        .ok_or("Identité de temps absente.")?;
    let entry = store
        .document::<Document>(Documents::Times, &id)?
        .ok_or("Entrée de temps introuvable.")?;
    if serde_json::to_value(&entry)? != serde_json::to_value(&selection.expected)?
        || !entry.string_equals("exportState", "draft")?
    {
        return Err("La saisie a changé ou a déjà été envoyée. Relis-la avant l’export.".into());
    }
    let card = super::forge_association::snapshot(store, &selection.expected_card)?;
    Ok((selection, source(&entry, &card)?))
}

pub fn preview_input(store: &Store<'_>, value: &Value) -> Result<Value> {
    let (selection, source) = select(store, value)?;
    if selection.report_id.is_some() {
        return Err("Requête de lecture d’export invalide.".into());
    }
    Ok(serde_json::to_value(source)?)
}

pub fn validate_report(store: &Store<'_>, report: &Report) -> Result<()> {
    let proposal = proposal(&report.output)?;
    let entry = require_source(store, &proposal.source)?;
    if !entry.string_equals("exportState", "draft")? {
        return Err("Cette saisie ne peut plus être préparée.".into());
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
        .ok_or("Relis l’aperçu de temps de ce module avant de confirmer.")?;
    let age = time::OffsetDateTime::now_utc()
        - time::OffsetDateTime::parse(
            &report.observed_at,
            &time::format_description::well_known::Rfc3339,
        )?;
    if age > time::Duration::minutes(10) || age < time::Duration::seconds(-60) {
        return Err("Aperçu d’export de temps expiré. Relis sa destination.".into());
    }
    let proposal = proposal(&report.output)?;
    if proposal.source != source {
        return Err("La saisie ou le ticket a changé depuis l’aperçu du module.".into());
    }
    if super::connector_effects::read(&store.settings::<Document>()?)?
        .iter()
        .any(|operation| {
            operation.method == Method::TimeSend
                && matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
                && operation.input["source"]["id"].as_str() == Some(source.id.as_str())
        })
    {
        return Err("Cette saisie possède déjà un export confirmé dans le journal.".into());
    }
    Ok(serde_json::to_value(proposal)?)
}

fn require_source(store: &Store<'_>, expected: &TimeSource) -> Result<Document> {
    let entry = store
        .document::<Document>(Documents::Times, &expected.id)?
        .ok_or("Entrée de temps introuvable.")?;
    let card = store
        .document::<Card>(Documents::Cards, &expected.card_id)?
        .ok_or("Carte introuvable.")?;
    let card = super::forge_association::snapshot(store, &card)?;
    if source(&entry, &card)? != *expected {
        return Err(
            "La saisie ou la portée du ticket a changé. Aucun résultat d’export appliqué.".into(),
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
        return Err(
            "Cette saisie conserve déjà une intention d’export. Vérifie son historique.".into(),
        );
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
        .ok_or("Entrée de temps introuvable.")?;
    if entry
        .get::<String>("connectorExport")?
        .is_some_and(|id| id != operation.id)
    {
        return Err("Cette saisie est liée à une autre opération distante.".into());
    }
    if matches!(operation.state, Phase::Confirmed | Phase::Reconciled) {
        let fields = atelier_core::domain::time::time_fields(&entry)?;
        if fields.seconds != proposal.source.seconds
            || fields.date != proposal.source.date
            || fields.start_time != proposal.source.start_time
            || fields.description.utf8()? != proposal.source.description
            || !entry.string_equals("cardId", &proposal.source.card_id)?
        {
            return Err(
                "La saisie locale diffère du temps réellement observé. Aucun succès appliqué."
                    .into(),
            );
        }
        let reply = operation
            .reply
            .as_ref()
            .ok_or("Observation d’export absente.")?;
        if reply
            .output
            .get("time")
            .map(self::proposal)
            .transpose()?
            .as_ref()
            != Some(&proposal)
        {
            return Err("Le module n’a pas observé la saisie et sa destination exactes.".into());
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
