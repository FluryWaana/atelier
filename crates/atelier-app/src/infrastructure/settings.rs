use super::{store::Store, token_vault};
use crate::Result;
use atelier_core::{
    application::{settings, task_creation::CreationContext},
    document::Document,
};

#[derive(Debug)]
pub struct VaultFailure;
impl std::fmt::Display for VaultFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("message.credentials_unavailable_no_settings_changed")
    }
}
impl std::error::Error for VaultFailure {}

pub fn platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

pub fn jira_base(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(&value).map_err(|_| "Invalid URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some_and(|value| !value.is_empty())
        || url.query().is_some_and(|value| !value.is_empty())
        || url.fragment().is_some_and(|value| !value.is_empty())
    {
        return Err(
            "message.an_https_jira_address_without_credentials_or_parameters_is_requi".into(),
        );
    }
    let value = url.as_str();
    Ok(String::from(value.strip_suffix('/').unwrap_or(value)))
}

pub(super) fn load(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Document> {
    match token_vault::read_encrypted(store)? {
        Some(encrypted) => {
            let plain = protect(&encrypted, true).map_err(|_| VaultFailure)?;
            serde_json::from_str(&plain).map_err(|_| VaultFailure.into())
        }
        None => Ok(Document::default()),
    }
}

fn encrypt(
    tokens: &Document,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Option<String>> {
    token_vault::tokens_plaintext(tokens)?
        .map(|plain| protect(&plain, false).map_err(|_| VaultFailure.into()))
        .transpose()
}

pub fn public_config(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Document> {
    let stored = store.settings()?;
    match load(store, protect) {
        Ok(tokens) => settings::public_config(&stored, &tokens, platform()),
        Err(error) if error.is::<VaultFailure>() => {
            let mut config = settings::public_config(&stored, &Document::default(), platform())?;
            for flag in ["tokenSet", "azureTokenSet", "tempoTokenSet"] {
                config.set(flag, &Option::<bool>::None)?;
            }
            config.set(
                "credentialError",
                &"message.credentials_unavailable_local_data_remain_available",
            )?;
            Ok(config)
        }
        Err(error) => Err(error),
    }
}

// The caller owns one SQLite worker operation: no other settings/template write can interleave.
pub fn update(
    store: &Store<'_>,
    body: &Document,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Document> {
    let stored = store.settings()?;
    let tokens = load(store, protect)?;
    let mut next = settings::prepare_update(
        &stored,
        &tokens,
        body,
        |raw| store.repository(raw),
        jira_base,
    )?;
    if stored.raw("jiraConnection").is_some()
        && tokens.get::<String>("jira")? != next.tokens.get::<String>("jira")?
    {
        super::jira::invalidate_configuration(&mut next.settings, None)?;
    }
    if tokens.get::<String>("tempo")? != next.tokens.get::<String>("tempo")? {
        super::tempo::invalidate(&mut next.settings, None)?;
    }
    let response = settings::public_config(&next.settings, &next.tokens, platform())?;
    persist(store, &next, protect)?;
    Ok(response)
}

pub(super) fn persist(
    store: &Store<'_>,
    next: &settings::SettingsUpdate,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<()> {
    let encrypted = encrypt(&next.tokens, protect)?;
    store.transaction(|store| {
        super::connector_repositories::invalidate_connections(
            store,
            &store.settings::<Document>()?,
            &next.settings,
        )?;
        token_vault::save_encrypted(store, encrypted.as_deref())?;
        store.save_settings(&next.settings)
    })
}

pub fn remove_token(
    store: &Store<'_>,
    service: &str,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Document> {
    // Reject an unknown service before asking the native vault to decrypt anything.
    settings::remove_token(&Document::default(), service)?;
    let next = settings::remove_token(&load(store, protect)?, service)?;
    let encrypted = encrypt(&next, protect)?;
    let mut stored = store.settings::<Document>()?;
    let mut changed =
        service == "jira" && super::jira::invalidate_configuration(&mut stored, None)?;
    if service == "tempo" {
        super::tempo::invalidate(&mut stored, None)?;
        changed = true;
    }
    let response = settings::public_config(&stored, &next, platform())?;
    store.transaction(|store| {
        token_vault::save_encrypted(store, encrypted.as_deref())?;
        if changed {
            store.save_settings(&stored)?;
        }
        Ok(())
    })?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unavailable_vault_keeps_local_settings_readable_without_claiming_tokens_are_absent() {
        let connection = super::super::store::open(std::path::Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        let saved = json!({"baseBranch":"local","taskTemplates":[{"id":"keep"}]});
        store.save_settings(&saved).unwrap();
        token_vault::save_encrypted(&store, Some("synthetic-opaque-credentials")).unwrap();
        let mut unavailable =
            |_: &str, _: bool| -> Result<String> { Err("synthetic locked vault".into()) };
        let public = public_config(&store, &mut unavailable).unwrap();
        assert_eq!(
            public.get::<String>("baseBranch").unwrap().as_deref(),
            Some("local")
        );
        for flag in ["tokenSet", "azureTokenSet", "tempoTokenSet"] {
            assert_eq!(public.raw(flag).unwrap().get(), "null");
        }
        assert!(public.has_text("credentialError").unwrap());
        assert!(
            !serde_json::to_string(&public)
                .unwrap()
                .contains("synthetic-opaque")
        );
        let body = serde_json::from_str(r#"{"baseBranch":"changed"}"#).unwrap();
        assert!(update(&store, &body, &mut unavailable).is_err());
        assert_eq!(store.settings::<serde_json::Value>().unwrap(), saved);
        assert_eq!(
            token_vault::read_encrypted(&store).unwrap().as_deref(),
            Some("synthetic-opaque-credentials")
        );
    }

    #[test]
    fn settings_and_encrypted_tokens_commit_together_or_remain_unchanged() {
        for fault in ["none", "encryption", "storage"] {
            let root = tempfile::tempdir().unwrap();
            let connection =
                super::super::store::open(&root.path().join("synthetic.sqlite")).unwrap();
            let store = Store::new(&connection);
            let original = json!({"baseBranch":"original","taskTemplates":[{"name":"keep"}],"taskQueue":{"ids":["a"]},"future":{"value":1}});
            store.save_settings(&original).unwrap();
            token_vault::save_encrypted(&store, Some("opaque:{\"jira\":\"old\"}")).unwrap();
            if fault == "storage" {
                connection.execute_batch("CREATE TRIGGER refuse_settings BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
            }
            let mut protector = |value: &str, decrypt: bool| -> Result<String> {
                if decrypt {
                    Ok(value
                        .strip_prefix("opaque:")
                        .ok_or("bad fixture")?
                        .to_owned())
                } else if fault == "encryption" {
                    Err("synthetic failure".into())
                } else {
                    Ok(format!("opaque:{value}"))
                }
            };
            let body: Document = serde_json::from_str(r#"{"baseBranch":"changed","token":"synthetic-\u00e9","taskTemplates":[],"taskQueue":null}"#).unwrap();
            let response = update(&store, &body, &mut protector);
            if fault != "none" {
                assert!(response.is_err());
                assert_eq!(store.settings::<serde_json::Value>().unwrap(), original);
                assert_eq!(
                    token_vault::read_encrypted(&store).unwrap().as_deref(),
                    Some("opaque:{\"jira\":\"old\"}")
                );
            } else {
                let response = response.unwrap();
                assert_eq!(response.get::<bool>("tokenSet").unwrap(), Some(true));
                assert!(
                    !serde_json::to_string(&response)
                        .unwrap()
                        .contains("synthetic-")
                );
                assert_eq!(
                    store.settings::<serde_json::Value>().unwrap()["taskTemplates"],
                    original["taskTemplates"]
                );
                assert_eq!(
                    store.settings::<serde_json::Value>().unwrap()["taskQueue"],
                    original["taskQueue"]
                );
                let mut expected = String::from("synthetic-");
                expected.push('é');
                assert_eq!(
                    load(&store, &mut protector)
                        .unwrap()
                        .get::<String>("jira")
                        .unwrap(),
                    Some(expected)
                );
                let removed = remove_token(&store, "jira", &mut protector).unwrap();
                assert_eq!(removed.get::<bool>("tokenSet").unwrap(), Some(false));
                assert!(token_vault::read_encrypted(&store).unwrap().is_none());
            }
            let mut unavailable = |_: &str, _: bool| -> Result<String> {
                panic!("unknown services must not open the vault")
            };
            assert_eq!(
                remove_token(&store, "unknown", &mut unavailable)
                    .unwrap_err()
                    .to_string(),
                "message.unknown_service"
            );
        }
    }
}
