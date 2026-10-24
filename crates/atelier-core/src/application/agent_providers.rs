use super::{
    settings::{SettingsUpdate, config},
    task_creation::{Result, text},
};
use crate::{
    document::{Document, JsonString},
    domain::agent_options::{provider_settings, providers},
};

#[derive(Debug)]
pub enum ProviderFailure {
    Missing,
    Invalid(&'static str),
    Conflict(&'static str),
}
impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "Fournisseur introuvable.",
            Self::Invalid(message) | Self::Conflict(message) => message,
        })
    }
}
impl std::error::Error for ProviderFailure {}

fn check_expected(stored: &Document, tokens: &Document, body: &Document, id: &str) -> Result<()> {
    if let Some(expected) = body.raw("expected") {
        let expected: Option<serde_json::Value> = serde_json::from_str(expected.get())
            .map_err(|_| ProviderFailure::Invalid("Version du profil invalide."))?;
        let current = describe(stored, tokens)?
            .into_iter()
            .find(|profile| profile.string_equals("id", id).unwrap_or(false));
        let current = current.as_ref().map(serde_json::to_value).transpose()?;
        if expected != current {
            return Err(ProviderFailure::Conflict("Ce profil a changé ailleurs. Ton brouillon est conservé ; recharge le profil avant de réessayer.").into());
        }
    }
    Ok(())
}

pub fn prepare_save(
    stored: &Document,
    tokens: &Document,
    body: &Document,
    busy: bool,
) -> Result<SettingsUpdate> {
    let mut profile = provider_settings(&serde_json::value::to_raw_value(body)?)
        .map_err(ProviderFailure::Invalid)?;
    let mut settings = config(stored)?;
    let mut profiles = providers(&settings)?;
    if body.get::<JsonString>("apiKey").is_err() || body.get::<bool>("clearApiKey").is_err() {
        return Err(ProviderFailure::Invalid("Clé API invalide.").into());
    }
    let id: String = profile.get("id")?.ok_or("Identifiant de profil absent.")?;
    let kind: String = profile.get("type")?.ok_or("Type de profil absent.")?;
    let previous = profiles
        .iter()
        .position(|profile| profile.string_equals("id", &id).unwrap_or(false));
    if let Some(index) = previous
        && !profiles[index].string_equals("type", &kind)?
    {
        return Err(ProviderFailure::Invalid(
            "Le type d’un profil existant ne peut pas changer. Créez un autre profil.",
        )
        .into());
    }
    if busy {
        return Err(ProviderFailure::Conflict(
            "Attendez la fin des exécutions et mises à jour avant de modifier les fournisseurs.",
        )
        .into());
    }
    if previous.is_none() && profiles.len() >= 50 {
        return Err(ProviderFailure::Invalid("50 fournisseurs maximum.").into());
    }
    check_expected(stored, tokens, body, &id)?;
    let key = if body.truthy("apiKey")? {
        text(body.raw("apiKey"), 4000)?
    } else {
        JsonString::default()
    };
    let api = profile.string_equals("auth", "api")?;
    if !key.0.is_empty() && !api {
        return Err(
            ProviderFailure::Invalid("Sélectionnez l’authentification par clé API.").into(),
        );
    }
    if key.0.iter().any(|unit| [0, 10, 13].contains(unit)) {
        return Err(ProviderFailure::Invalid("Clé API invalide.").into());
    }
    let mut tokens = tokens.clone();
    let name = format!("agent:{id}:key");
    if body.truthy("clearApiKey")? || !api {
        tokens.set(&name, &"")?;
    } else if !key.0.is_empty() {
        tokens.set(&name, &key)?;
    }
    // A key rotation must invalidate a stale editor even when apiKeySet stays true.
    let revision = settings
        .get::<u64>("agentProvidersRevision")?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("Révision des profils épuisée.")?;
    settings.set("agentProvidersRevision", &revision)?;
    profile.set("revision", &revision)?;
    settings.set(&format!("{id}Model"), &profile.raw("model"))?;
    settings.set(&format!("{id}Effort"), &profile.raw("effort"))?;
    if previous.is_some() {
        for existing in &mut profiles {
            if existing.string_equals("id", &id)? {
                *existing = profile.clone();
            }
        }
    } else {
        profiles.push(profile);
    }
    settings.set("agentProviders", &profiles)?;
    Ok(SettingsUpdate { settings, tokens })
}

pub fn prepare_remove(
    stored: &Document,
    tokens: &Document,
    id: &str,
    body: &Document,
    cards: &[Document],
    projects: &[Document],
    busy: bool,
) -> Result<SettingsUpdate> {
    let mut settings = config(stored)?;
    let profiles = providers(&settings)?;
    if !profiles
        .iter()
        .any(|profile| profile.string_equals("id", id).unwrap_or(false))
    {
        return Err(ProviderFailure::Missing.into());
    }
    check_expected(stored, tokens, body, id)?;
    if busy
        || settings.string_equals("agent", id)?
        || cards
            .iter()
            .chain(projects)
            .any(|record| record.string_equals("agent", id).unwrap_or(false))
    {
        return Err(ProviderFailure::Conflict("Ce fournisseur est utilisé. Changez les tâches, les projets et le réglage par défaut avant de le retirer.").into());
    }
    let mut tokens = tokens.clone();
    tokens.set(&format!("agent:{id}:key"), &"")?;
    let kept: Vec<_> = profiles
        .into_iter()
        .filter(|profile| !profile.string_equals("id", id).unwrap_or(false))
        .collect();
    settings.set("agentProviders", &kept)?;
    Ok(SettingsUpdate { settings, tokens })
}

pub fn describe(stored: &Document, tokens: &Document) -> Result<Vec<Document>> {
    let mut profiles = providers(&config(stored)?)?;
    for profile in &mut profiles {
        let id: JsonString = profile.get("id")?.ok_or("Identifiant de profil absent.")?;
        let mut key = JsonString::from("agent:");
        key.0.extend(id.0);
        key.0.extend(":key".encode_utf16());
        let mut alias = JsonString::from("connector:");
        alias.0.extend(&key.0);
        alias.0.extend(":token".encode_utf16());
        let selected = if tokens
            .raw_key(&alias)
            .is_some_and(|value| value.get() != "null")
        {
            &alias
        } else {
            &key
        };
        profile.set("apiKeySet", &tokens.truthy_key(selected)?)?;
        // shortcut: CLI updates are unavailable until M04; connect the real reservation and progress then.
        profile.set("updating", &false)?;
        profile.set("updatePhase", &"")?;
    }
    Ok(profiles)
}

pub fn diagnostic_profile(
    stored: &Document,
    tokens: &Document,
    body: &Document,
    id: &str,
) -> Result<Document> {
    check_expected(stored, tokens, body, id)?;
    describe(stored, tokens)?
        .into_iter()
        .find(|profile| profile.string_equals("id", id).unwrap_or(false))
        .ok_or_else(|| ProviderFailure::Missing.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document(value: serde_json::Value) -> Document {
        serde_json::from_str(&value.to_string()).unwrap()
    }

    #[test]
    fn stale_profile_edits_and_removals_refuse_key_rotations_and_reused_ids() {
        let empty = Document::default();
        let mut body = document(
            json!({"id":"api","name":"Synthetic API","type":"codex","auth":"api","apiKey":"synthetic-one","expected":null}),
        );
        let first = prepare_save(&empty, &empty, &body, false).unwrap();
        assert!(prepare_save(&first.settings, &first.tokens, &body, false).is_err());
        let profile = describe(&first.settings, &first.tokens)
            .unwrap()
            .pop()
            .unwrap();
        body.set("expected", &profile).unwrap();
        body.set("apiKey", &"synthetic-two").unwrap();
        let second = prepare_save(&first.settings, &first.tokens, &body, false).unwrap();
        assert!(prepare_save(&second.settings, &second.tokens, &body, false).is_err());
        assert!(diagnostic_profile(&second.settings, &second.tokens, &body, "api").is_err());
        assert!(
            prepare_remove(
                &second.settings,
                &second.tokens,
                "api",
                &body,
                &[],
                &[],
                false
            )
            .is_err()
        );
        // Settings edits also change the public snapshot, even without a profile save.
        let mut external = first.settings.clone();
        let mut profiles = providers(&external).unwrap();
        profiles[2].set("model", &"changed-elsewhere").unwrap();
        external.set("agentProviders", &profiles).unwrap();
        assert!(prepare_save(&external, &first.tokens, &body, false).is_err());
        let current = describe(&second.settings, &second.tokens)
            .unwrap()
            .pop()
            .unwrap();
        body.set("expected", &current).unwrap();
        let removed = prepare_remove(
            &second.settings,
            &second.tokens,
            "api",
            &body,
            &[],
            &[],
            false,
        )
        .unwrap();
        assert!(prepare_save(&removed.settings, &removed.tokens, &body, false).is_err());
        body.set("expected", &serde_json::Value::Null).unwrap();
        let recreated = prepare_save(&removed.settings, &removed.tokens, &body, false).unwrap();
        let recreated = describe(&recreated.settings, &recreated.tokens)
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            recreated.get::<u64>("revision").unwrap() > current.get::<u64>("revision").unwrap()
        );
        assert!(
            !serde_json::to_string(&recreated)
                .unwrap()
                .contains("synthetic-two")
        );
    }

    #[test]
    fn profiles_preserve_keys_and_references_while_rejecting_busy_or_invalid_changes() {
        let stored = document(json!({"taskQueue":{"ids":["keep"]},"future":{"keep":true}}));
        let tokens = document(json!({"jira":"unrelated"}));
        let mut body: Document = serde_json::from_str(r#"{"id":"api","name":"\ud800","type":"codex","auth":"api","apiKey":" synthetic-\udc00 ","model":"custom/model","effort":"high"}"#).unwrap();
        let next = prepare_save(&stored, &tokens, &body, false).unwrap();
        assert!(next.tokens.string_equals("jira", "unrelated").unwrap());
        assert_eq!(
            next.settings.raw("future").unwrap().get(),
            stored.raw("future").unwrap().get()
        );
        assert!(
            next.settings
                .string_equals("apiModel", "custom/model")
                .unwrap()
        );
        assert_eq!(providers(&next.settings).unwrap().len(), 3);
        let catalog = describe(&next.settings, &next.tokens).unwrap();
        assert_eq!(catalog[2].get::<bool>("apiKeySet").unwrap(), Some(true));
        assert!(
            !serde_json::to_string(&catalog)
                .unwrap()
                .contains("synthetic-")
        );
        body.remove("apiKey");
        let updated = prepare_save(&next.settings, &next.tokens, &body, false).unwrap();
        assert_eq!(
            serde_json::to_string(&updated.tokens).unwrap(),
            serde_json::to_string(&next.tokens).unwrap()
        );
        let reference = document(json!({"agent":"api","archived":true}));
        for (cards, projects) in [(vec![reference.clone()], vec![]), (vec![], vec![reference])] {
            assert!(matches!(
                prepare_remove(
                    &next.settings,
                    &next.tokens,
                    "api",
                    &Document::default(),
                    &cards,
                    &projects,
                    false
                )
                .err()
                .unwrap()
                .downcast_ref::<ProviderFailure>(),
                Some(ProviderFailure::Conflict(_))
            ));
        }
        assert!(
            prepare_remove(
                &next.settings,
                &next.tokens,
                "codex",
                &Document::default(),
                &[],
                &[],
                false
            )
            .is_err()
        );
        assert!(prepare_save(&next.settings, &next.tokens, &body, true).is_err());
        body.set("type", &"claude").unwrap();
        body.set("auth", &"cli").unwrap();
        assert_eq!(
            prepare_save(&next.settings, &next.tokens, &body, true)
                .err()
                .unwrap()
                .to_string(),
            "Le type d’un profil existant ne peut pas changer. Créez un autre profil."
        );
        body.set("clearApiKey", &"false").unwrap();
        assert_eq!(
            prepare_save(&next.settings, &next.tokens, &body, true)
                .err()
                .unwrap()
                .to_string(),
            "Clé API invalide."
        );
        let removed = prepare_remove(
            &next.settings,
            &next.tokens,
            "api",
            &Document::default(),
            &[],
            &[],
            false,
        )
        .unwrap();
        assert_eq!(providers(&removed.settings).unwrap().len(), 2);
        assert!(removed.tokens.string_equals("agent:api:key", "").unwrap());
        assert!(
            removed
                .settings
                .string_equals("apiModel", "custom/model")
                .unwrap()
        );
        assert_eq!(
            prepare_remove(
                &removed.settings,
                &removed.tokens,
                "api",
                &Document::default(),
                &[],
                &[],
                true
            )
            .err()
            .unwrap()
            .to_string(),
            "Fournisseur introuvable."
        );
    }

    #[test]
    fn profile_limits_and_api_key_validation_keep_historical_error_order() {
        let profiles: Vec<_> = (0..50)
            .map(|index| json!({"id":format!("p{index}"),"type":"codex"}))
            .collect();
        let stored = document(json!({"agentProviders":profiles}));
        let mut body =
            document(json!({"id":"new","name":"New","type":"codex","auth":"api","apiKey":"a\nb"}));
        let tokens = Document::default();
        assert_eq!(
            prepare_save(&stored, &tokens, &body, false)
                .err()
                .unwrap()
                .to_string(),
            "50 fournisseurs maximum."
        );
        body.set("id", &"p0").unwrap();
        assert_eq!(
            prepare_save(&stored, &tokens, &body, false)
                .err()
                .unwrap()
                .to_string(),
            "Clé API invalide."
        );
        body.set("auth", &"cli").unwrap();
        assert_eq!(
            prepare_save(&stored, &tokens, &body, false)
                .err()
                .unwrap()
                .to_string(),
            "Sélectionnez l’authentification par clé API."
        );
        body.set("apiKey", &"").unwrap();
        let next = prepare_save(&stored, &tokens, &body, false).unwrap();
        assert_eq!(providers(&next.settings).unwrap().len(), 50);
        assert!(next.tokens.string_equals("agent:p0:key", "").unwrap());
        body.set("auth", &"api").unwrap();
        body.set("apiKey", &"\n key \n").unwrap();
        let next = prepare_save(&stored, &tokens, &body, false).unwrap();
        assert!(next.tokens.string_equals("agent:p0:key", "key").unwrap());
        body.set("clearApiKey", &true).unwrap();
        assert!(
            prepare_save(&stored, &next.tokens, &body, false)
                .unwrap()
                .tokens
                .string_equals("agent:p0:key", "")
                .unwrap()
        );
    }
}
