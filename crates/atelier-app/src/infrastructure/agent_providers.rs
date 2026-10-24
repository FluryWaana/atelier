use super::{
    settings,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{application::agent_providers, document::Document};

pub fn describe(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Vec<Document>> {
    agent_providers::describe(&store.settings()?, &settings::load(store, protect)?)
}

pub fn diagnostic_profile(
    store: &Store<'_>,
    body: &Document,
    id: &str,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Document> {
    agent_providers::diagnostic_profile(
        &store.settings()?,
        &settings::load(store, protect)?,
        body,
        id,
    )
}

pub fn save(
    store: &Store<'_>,
    body: &Document,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Vec<Document>> {
    // shortcut: prototype cannot launch/update agents; wire M04 reservations and cache invalidation before enabling those operations.
    let next = agent_providers::prepare_save(
        &store.settings()?,
        &settings::load(store, protect)?,
        body,
        false,
    )?;
    let response = agent_providers::describe(&next.settings, &next.tokens)?;
    settings::persist(store, &next, protect)?;
    Ok(response)
}

pub fn remove(
    store: &Store<'_>,
    id: &str,
    body: &Document,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Vec<Document>> {
    let next = agent_providers::prepare_remove(
        &store.settings()?,
        &settings::load(store, protect)?,
        id,
        body,
        &store.documents(Documents::Cards)?,
        &store.documents(Documents::Projects)?,
        false,
    )?;
    let response = agent_providers::describe(&next.settings, &next.tokens)?;
    settings::persist(store, &next, protect)?;
    Ok(response)
}
