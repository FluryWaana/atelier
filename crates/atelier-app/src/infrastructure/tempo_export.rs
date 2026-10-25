use super::{
    forge_association, jira,
    store::{Documents, Store},
    tempo,
};
use crate::Result;
use atelier_core::{
    contracts::{
        Card, ForgeWriteState as Phase, JiraConfiguration, JiraMode, TempoConfiguration,
        TempoExportIntent, TempoExportPreview, TempoExportRequest, TempoPayload,
    },
    document::Document,
};

fn entry_snapshot(entry: &Document) -> Result<Document> {
    let mut snapshot = Document::default();
    for key in [
        "id",
        "cardId",
        "seconds",
        "date",
        "startTime",
        "description",
        "runningSince",
        "exportState",
    ] {
        if let Some(raw) = entry.raw(key) {
            snapshot.set(key, &raw)?;
        }
    }
    Ok(snapshot)
}
pub fn selected(
    store: &Store<'_>,
    input: &TempoExportRequest,
) -> Result<(Document, Card, Document)> {
    let id = input
        .expected
        .get::<String>("id")?
        .ok_or("Identité de temps locale invalide.")?;
    let entry = store
        .document::<Document>(Documents::Times, &id)?
        .ok_or("Entrée de temps introuvable.")?;
    if serde_json::to_value(&entry)? != serde_json::to_value(&input.expected)?
        || !entry.string_equals("cardId", &input.expected_card.id)?
    {
        return Err("La saisie de temps a changé. Relis-la avant l’export.".into());
    }
    let card = forge_association::snapshot(store, &input.expected_card)?;
    let ticket = card
        .fields
        .get::<Document>("ticket")?
        .filter(|ticket| ticket.string_equals("connector", "jira").unwrap_or(false))
        .ok_or("Associe un ticket Jira à cette tâche avant l’export.")?;
    jira::require_revision(store, input.jira_revision)?;
    let configuration = store
        .settings::<Document>()?
        .get::<JiraConfiguration>("jiraConnection")?
        .unwrap_or_default();
    if configuration.mode != JiraMode::Cloud
        || !ticket.string_equals("instance", &configuration.url)?
    {
        return Err("Reconnecte le site Jira Cloud associé à ce ticket.".into());
    }
    Ok((entry, card, ticket))
}
pub fn save_preview(
    store: &Store<'_>,
    input: &TempoExportRequest,
    configuration: TempoConfiguration,
    payload: TempoPayload,
) -> Result<Document> {
    store.transaction(|store| {
        tempo::validate_payload(&payload)?;
        let (mut entry, card, ticket) = selected(store, input)?;
        if !entry.string_equals("exportState", "draft")? || entry.truthy("runningSince")? {
            return Err(
                "Arrête le chronomètre ; seul un temps non envoyé peut être préparé.".into(),
            );
        }
        let id = entry
            .get::<String>("id")?
            .ok_or("Identité de temps locale invalide.")?;
        uuid::Uuid::parse_str(&id).map_err(|_| "Identité de temps locale invalide.")?;
        let fields = atelier_core::domain::time::time_fields(&entry)?;
        let issue = ticket
            .get::<String>("id")?
            .and_then(|id| id.parse::<u64>().ok())
            .ok_or("Identifiant Jira invalide.")?;
        if payload.issue_id != issue
            || payload.time_spent_seconds != fields.seconds
            || payload.start_date != fields.date
            || payload.start_time != fields.start_time
            || payload.description != format!("{}\n[atelier:{id}]", fields.description.utf8()?)
        {
            return Err("L’aperçu Tempo ne correspond pas à la saisie et au ticket.".into());
        }
        let settings = store.settings::<Document>()?;
        let current = settings
            .get::<TempoConfiguration>("tempoConnection")?
            .unwrap_or_default();
        let jira = settings
            .get::<JiraConfiguration>("jiraConnection")?
            .unwrap_or_default();
        if current != configuration || configuration.revision != input.tempo_revision {
            return Err("La connexion Tempo a changé. Recharge avant de réessayer.".into());
        }
        let preview = TempoExportPreview {
            entry: entry_snapshot(&entry)?,
            card_id: card.id,
            ticket,
            jira_url: jira.url,
            jira_revision: jira.revision,
            tempo: configuration,
            payload,
        };
        entry.set("tempoExportPreview", &preview)?;
        store.put(Documents::Times, &entry)?;
        Ok(entry)
    })
}
pub fn reserve(store: &Store<'_>, input: &TempoExportRequest, id: &str) -> Result<Document> {
    store.transaction(|store| {
        if !input.confirmed || uuid::Uuid::parse_str(id).is_err() {
            return Err("Confirme l’auteur, le ticket et les données du temps à envoyer.".into());
        }
        let (mut entry, card, ticket) = selected(store, input)?;
        let preview = entry
            .get::<TempoExportPreview>("tempoExportPreview")?
            .ok_or("Prépare un aperçu avant l’export Tempo.")?;
        let configuration = store
            .settings::<Document>()?
            .get::<TempoConfiguration>("tempoConnection")?
            .unwrap_or_default();
        if !entry.string_equals("exportState", "draft")?
            || entry.truthy("runningSince")?
            || serde_json::to_value(entry_snapshot(&entry)?)?
                != serde_json::to_value(&preview.entry)?
            || serde_json::to_value(&ticket)? != serde_json::to_value(&preview.ticket)?
            || preview.card_id != card.id
            || configuration != preview.tempo
            || configuration.revision != input.tempo_revision
            || preview.jira_revision != input.jira_revision
        {
            return Err("L’aperçu de l’export est périmé. Prépare-le à nouveau.".into());
        }
        entry.set(
            "tempoExport",
            &TempoExportIntent {
                id: id.into(),
                state: Phase::Sending,
                preview,
                worklog_id: None,
                observed_at: None,
            },
        )?;
        entry.set("exportState", &"sending")?;
        entry.remove("error");
        store.put(Documents::Times, &entry)?;
        Ok(entry)
    })
}
fn active(store: &Store<'_>, entry_id: &str, id: &str) -> Result<(Document, TempoExportIntent)> {
    let entry = store
        .document::<Document>(Documents::Times, entry_id)?
        .ok_or("Entrée de temps introuvable.")?;
    let intent = entry
        .get::<TempoExportIntent>("tempoExport")?
        .filter(|intent| {
            intent.id == id && matches!(intent.state, Phase::Sending | Phase::Uncertain)
        })
        .ok_or("L’intention d’export Tempo a changé.")?;
    Ok((entry, intent))
}
pub fn remember(store: &Store<'_>, entry_id: &str, id: &str, worklog: u64) -> Result<()> {
    store.transaction(|store| {
        let (mut entry, mut intent) = active(store, entry_id, id)?;
        if worklog == 0
            || worklog > i64::MAX as u64
            || intent
                .worklog_id
                .is_some_and(|previous| previous != worklog)
        {
            return Err("Identité de journal Tempo invalide.".into());
        }
        intent.worklog_id = Some(worklog);
        entry.set("tempoExport", &intent)?;
        store.put(Documents::Times, &entry)
    })
}
pub fn uncertain(store: &Store<'_>, entry_id: &str, id: &str) -> Result<Document> {
    store.transaction(|store| {
        let (mut entry, mut intent) = active(store, entry_id, id)?;
        intent.state = Phase::Uncertain;
        entry.set("tempoExport", &intent)?;
        entry.set("exportState", &"uncertain")?;
        entry.set(
            "error",
            &"Envoi Tempo incertain. Relis le journal avant toute nouvelle tentative.",
        )?;
        store.put(Documents::Times, &entry)?;
        Ok(entry)
    })
}
pub fn record(
    store: &Store<'_>,
    entry_id: &str,
    id: &str,
    worklog: u64,
    tempo_revision: u64,
    jira_revision: u64,
) -> Result<Document> {
    store.transaction(|store| {
        let (mut entry, mut intent) = active(store, entry_id, id)?;
        jira::require_revision(store, jira_revision)?;
        let settings = store.settings::<Document>()?;
        let tempo = settings
            .get::<TempoConfiguration>("tempoConnection")?
            .unwrap_or_default();
        let jira = settings
            .get::<JiraConfiguration>("jiraConnection")?
            .unwrap_or_default();
        if tempo.revision != tempo_revision
            || tempo.base != intent.preview.tempo.base
            || jira.url != intent.preview.jira_url
            || jira.mode != JiraMode::Cloud
        {
            return Err("Reconnecte les sites Jira et Tempo de l’envoi incertain.".into());
        }
        if intent
            .worklog_id
            .is_some_and(|previous| previous != worklog)
            || worklog == 0
            || worklog > i64::MAX as u64
        {
            return Err("Le journal Tempo relu ne correspond pas à l’export.".into());
        }
        intent.state = Phase::Confirmed;
        intent.observed_at = Some(
            time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
        );
        intent.worklog_id = Some(worklog);
        entry.set("tempoExport", &intent)?;
        entry.set("tempoExportEffect", &intent)?;
        entry.set("tempoId", &worklog)?;
        entry.set("exportState", &"exported")?;
        entry.remove("tempoExportPreview");
        entry.remove("error");
        store.put(Documents::Times, &entry)?;
        Ok(entry)
    })
}

pub fn blocks(store: &Store<'_>, path: &str) -> Result<bool> {
    let card_id = path
        .strip_prefix("/api/cards/")
        .map(|tail| tail.split('/').next().unwrap_or_default());
    for entry in store.documents::<Document>(Documents::Times)? {
        if (entry.string_equals("exportState", "sending")?
            || entry.string_equals("exportState", "uncertain")?)
            && (path.starts_with("/api/backups")
                || path.starts_with("/api/projects")
                || card_id.is_some_and(|id| entry.string_equals("cardId", id).unwrap_or(true)))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn reset(store: &Store<'_>, entry_id: &str, body: &Document) -> Result<Document> {
    store.transaction(|store| {
        let mut entry=store.document::<Document>(Documents::Times,entry_id)?.ok_or("Entrée de temps introuvable.")?;
        let expected=body.get::<Document>("expected")?.ok_or("Relis la saisie de temps avant de la débloquer.")?;
        let reason=body.get::<String>("manualEvidence")?.filter(|reason|!reason.trim().is_empty()&&reason.encode_utf16().count()<=1000&&!reason.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\r'|'\t'))).ok_or("Décris la vérification manuelle effectuée dans Tempo.")?;
        if serde_json::to_value(&entry)?!=serde_json::to_value(expected)? || !entry.string_equals("exportState","uncertain")?
            || body.get::<bool>("confirmed")?!=Some(true)||body.get::<bool>("acceptDuplicateRisk")?!=Some(true) {
            return Err("Confirme la vérification manuelle et le risque de doublon avant de débloquer l’export.".into());
        }
        let intent=entry.get::<TempoExportIntent>("tempoExport")?.filter(|intent|intent.state==Phase::Uncertain).ok_or("Aucun export Tempo incertain à vérifier.")?;
        let mut history=entry.get::<Vec<serde_json::Value>>("tempoExportHistory")?.unwrap_or_default();
        if history.len()>=16 {return Err("L’historique d’exports est plein. Résous les envois incertains avant de continuer.".into());}
        history.push(serde_json::json!({"intent":intent,"manualEvidence":reason,"acknowledgedAt":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?}));
        if serde_json::to_vec(&history)?.len()>64*1024 {return Err("L’historique d’exports est plein. Résous les envois incertains avant de continuer.".into());}
        entry.set("tempoExportHistory",&history)?;entry.set("exportState",&"draft")?;
        for key in ["tempoExport","tempoExportPreview","error"] {entry.remove(key);}
        store.put(Documents::Times,&entry)?;Ok(entry)
    })
}
