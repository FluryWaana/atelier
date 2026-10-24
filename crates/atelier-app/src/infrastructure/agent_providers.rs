use super::{
    settings,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{application::agent_providers, document::Document};

pub fn describe(store: &Store<'_>) -> Result<Vec<Document>> {
    agent_providers::describe(
        &store.settings()?,
        &settings::load(store, &mut settings::prototype_protect)?,
    )
}

pub fn save(store: &Store<'_>, body: &Document) -> Result<Vec<Document>> {
    // shortcut: prototype cannot launch/update agents; wire M04 reservations and cache invalidation before enabling those operations.
    let next = agent_providers::prepare_save(
        &store.settings()?,
        &settings::load(store, &mut settings::prototype_protect)?,
        body,
        false,
    )?;
    let response = agent_providers::describe(&next.settings, &next.tokens)?;
    settings::persist(store, &next, &mut settings::prototype_protect)?;
    Ok(response)
}

pub fn remove(store: &Store<'_>, id: &str) -> Result<Vec<Document>> {
    let next = agent_providers::prepare_remove(
        &store.settings()?,
        &settings::load(store, &mut settings::prototype_protect)?,
        id,
        &store.documents(Documents::Cards)?,
        &store.documents(Documents::Projects)?,
        false,
    )?;
    let response = agent_providers::describe(&next.settings, &next.tokens)?;
    settings::persist(store, &next, &mut settings::prototype_protect)?;
    Ok(response)
}
