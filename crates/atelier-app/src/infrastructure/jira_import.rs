use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::{jira::JiraFailure, task_creation},
    contracts::{Card, JiraConfiguration, JiraImport, JiraImported, JiraIssueDetail},
    document::Document,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Reservation {
    id: String,
    input: JiraImport,
    base: String,
    project: Document,
}
fn conflict() -> crate::Error {
    JiraFailure::Conflict.into()
}

pub fn reserve(store: &Store<'_>, input: JiraImport, id: String) -> Result<()> {
    store.transaction(|store| {
        if input.issue_id.is_empty()
            || input.issue_id.len() > 32
            || !input.issue_id.bytes().all(|byte| byte.is_ascii_digit())
            || input.project_id.is_empty()
            || input.project_id.len() > 100
        {
            return Err("Ticket ou projet Jira invalide.".into());
        }
        let mut settings = store.settings::<Document>()?;
        if settings.raw("jiraImportReservation").is_some() {
            return Err(conflict());
        }
        let connection = settings
            .get::<JiraConfiguration>("jiraConnection")?
            .ok_or_else(conflict)?;
        if connection.revision != input.revision {
            return Err(conflict());
        }
        let project = store
            .document::<Document>(Documents::Projects, &input.project_id)?
            .ok_or("Projet introuvable.")?;
        let reservation = Reservation {
            id,
            input,
            base: connection.url,
            project,
        };
        settings.set("jiraImportReservation", &reservation)?;
        store.save_settings(&settings)
    })
}

pub fn release(store: &Store<'_>, id: &str) -> Result<()> {
    store.transaction(|store| {
        let mut settings = store.settings::<Document>()?;
        if settings
            .get::<Reservation>("jiraImportReservation")?
            .is_some_and(|reservation| reservation.id == id)
        {
            settings.remove("jiraImportReservation");
            store.save_settings(&settings)?;
        }
        Ok(())
    })
}

/// Reads have no remote effect; an interrupted reservation can be discarded before serving requests.
pub fn recover(store: &Store<'_>) -> Result<()> {
    let mut settings = store.settings::<Document>()?;
    if settings.raw("jiraImportReservation").is_some() {
        settings.remove("jiraImportReservation");
        store.save_settings(&settings)?;
    }
    Ok(())
}

pub fn finish(store: &Store<'_>, id: &str, detail: JiraIssueDetail) -> Result<JiraImported> {
    store.transaction(|store| {
        if !atelier_core::application::jira::issue_identity(&detail.issue.id,&detail.issue.key) {
            return Err("Ticket Jira invalide.".into());
        }
        let mut settings=store.settings::<Document>()?;
        let reservation=settings.get::<Reservation>("jiraImportReservation")?.filter(|reservation|reservation.id==id).ok_or_else(conflict)?;
        let connection=settings.get::<JiraConfiguration>("jiraConnection")?.ok_or_else(conflict)?;
        if connection.revision!=reservation.input.revision || detail.connection_revision!=connection.revision || detail.issue.id!=reservation.input.issue_id || connection.url!=reservation.base {return Err(conflict());}
        if detail.issue.url!=format!("{}/browse/{}",reservation.base.trim_end_matches('/'),detail.issue.key) {
            return Err("Adresse du ticket Jira incohérente.".into());
        }
        let project=store.document::<Document>(Documents::Projects,&reservation.input.project_id)?.ok_or_else(conflict)?;
        if serde_json::to_value(&project)?!=serde_json::to_value(&reservation.project)? {return Err(conflict());}
        let mut existing=None;
        for card in store.documents::<Card>(Documents::Cards)? {
            if let Some(ticket)=card.fields.get::<Document>("ticket")? {
                let base=ticket.get::<String>("instance")?.or(ticket.get::<String>("base")?);
                if ticket.string_equals("connector","jira")? && base.as_deref()==Some(reservation.base.as_str()) && ticket.string_equals("id",&detail.issue.id)? {
                    if existing.is_some() {return Err("Plusieurs cartes sont liées au même ticket Jira.".into());}
                    existing=Some(card);
                }
            }
        }
        let result=if let Some(card)=existing { JiraImported {card,created:false} } else {
            let mut body=Document::default();
            body.set("title",&detail.issue.title)?;
            body.set("description",&detail.description)?;
            body.set("repo",&project.get::<String>("repo")?.ok_or("Dépôt du projet absent.")?)?;
            let mut card=task_creation::create(store,&body,false)?;
            card.fields.set("ticket",&serde_json::json!({"connector":"jira","instance":reservation.base,"id":detail.issue.id,"key":detail.issue.key,"url":detail.issue.url,"status":detail.issue.status}))?;
            store.put(Documents::Cards,&card)?;
            JiraImported {card,created:true}
        };
        settings.remove("jiraImportReservation");
        store.save_settings(&settings)?;
        Ok(result)
    })
}
