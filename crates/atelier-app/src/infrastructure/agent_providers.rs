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

pub fn execution_profile(
    store: &Store<'_>,
    id: &str,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<(Document, String)> {
    let tokens = settings::load(store, protect)?;
    let profile = agent_providers::describe(&store.settings()?, &tokens)?
        .into_iter()
        .find(|profile| profile.string_equals("id", id).unwrap_or(false))
        .ok_or("message.invalid_agent")?;
    let key = if profile.string_equals("auth", "api")? {
        let name = format!("agent:{id}:key");
        let alias = format!("connector:{name}:token");
        let selected = if tokens
            .raw(&alias)
            .is_some_and(|value| value.get() != "null")
        {
            alias
        } else {
            name
        };
        tokens
            .get::<String>(&selected)?
            .filter(|key| !key.is_empty())
            .ok_or("message.enter_this_profile_s_api_key_before_starting_an_agent")?
    } else {
        String::new()
    };
    Ok((profile, key))
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
    let busy = store
        .documents::<Document>(Documents::Runs)?
        .iter()
        .map(super::runs::active)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .any(|active| active);
    let next = agent_providers::prepare_save(
        &store.settings()?,
        &settings::load(store, protect)?,
        body,
        busy,
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
    let busy = store
        .documents::<Document>(Documents::Runs)?
        .iter()
        .map(super::runs::active)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .any(|active| active);
    let next = agent_providers::prepare_remove(
        &store.settings()?,
        &settings::load(store, protect)?,
        id,
        body,
        &store.documents(Documents::Cards)?,
        &store.documents(Documents::Projects)?,
        busy,
    )?;
    let response = agent_providers::describe(&next.settings, &next.tokens)?;
    settings::persist(store, &next, protect)?;
    Ok(response)
}
