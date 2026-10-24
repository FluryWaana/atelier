use super::{text, truthy};
use crate::document::{Document, JsonString};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, &'static str>;

pub(crate) fn find_provider(
    settings: &Document,
    agent: &JsonString,
) -> serde_json::Result<Option<Document>> {
    Ok(providers(settings)?
        .into_iter()
        .find(|provider| provider.get::<JsonString>("id").ok().flatten().as_ref() == Some(agent)))
}

pub fn providers(settings: &Document) -> serde_json::Result<Vec<Document>> {
    if let Some(providers) = settings
        .raw("agentProviders")
        .filter(|raw| raw.get() != "null")
    {
        serde_json::from_str(providers.get())
    } else {
        let mut providers = Vec::new();
        for (id, name) in [("codex", "Codex"), ("claude", "Claude Code")] {
            let mut provider = Document::default();
            provider.set("id", &id)?;
            provider.set("name", &name)?;
            provider.set("type", &id)?;
            provider.set("auth", &"cli")?;
            for (key, suffix) in [("model", "Model"), ("effort", "Effort")] {
                let field = format!("{id}{suffix}");
                if settings.truthy(&field)? {
                    provider.set(key, &settings.raw(&field))?;
                } else {
                    provider.set(key, &"")?;
                }
            }
            providers.push(provider);
        }
        Ok(providers)
    }
}

pub fn agent_providers(settings: &Value) -> Result<Vec<Value>> {
    if let Some(providers) = settings
        .get("agentProviders")
        .filter(|value| !value.is_null())
    {
        return providers.as_array().cloned().ok_or("Agent invalide.");
    }
    Ok([("codex", "Codex"), ("claude", "Claude Code")]
        .into_iter()
        .map(|(id, name)| {
            let default = |suffix| settings.get(format!("{id}{suffix}")).filter(|value| truthy(value)).cloned().unwrap_or(json!(""));
            json!({"id": id, "name": name, "type": id, "auth": "cli", "model": default("Model"), "effort": default("Effort")})
        })
        .collect())
}

pub fn agent_provider(settings: &Value, id: &str) -> Result<Value> {
    agent_providers(settings)?
        .into_iter()
        .find(|provider| provider["id"] == id)
        .ok_or("Agent invalide.")
}

pub fn agent_options(agent: &str, body: &Value, settings: Option<&Value>) -> Result<Value> {
    let provider = settings
        .map(|settings| agent_provider(settings, agent))
        .transpose()?;
    let kind = match provider.as_ref() {
        Some(provider) => provider["type"].as_str().ok_or("Agent invalide.")?,
        None => agent,
    };
    let efforts: &[&str] = match kind {
        "codex" => &["none", "minimal", "low", "medium", "high", "xhigh"],
        "claude" => &["low", "medium", "high", "xhigh", "max"],
        "command" => &[],
        _ => return Err("Agent invalide."),
    };
    let model = body
        .get("model")
        .map_or(Some(""), Value::as_str)
        .ok_or("Modèle invalide.")?;
    let model = text(model, 180).map_err(|_| "Modèle invalide.")?;
    if !model.is_empty()
        && (!model.as_bytes()[0].is_ascii_alphanumeric()
            || !model
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(&c)))
    {
        return Err("Modèle invalide.");
    }
    let invalid_effort = "Niveau de réflexion invalide pour cet agent.";
    let effort = body
        .get("effort")
        .map_or(Some(""), Value::as_str)
        .ok_or(invalid_effort)?;
    if !effort.is_empty()
        && if kind == "command" {
            effort.len() > 40
                || !effort
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        } else {
            !efforts.contains(&effort)
        }
    {
        return Err(invalid_effort);
    }
    Ok(json!({"model": model, "effort": effort}))
}

pub fn resolve_agent_options(settings: &Value, card: &Value, body: &Value) -> Result<Value> {
    let agent = body["agent"].as_str().ok_or("Agent invalide.")?;
    let provider = agent_provider(settings, agent)?;
    let selected = |key| {
        body.get(key)
            .filter(|value| !value.is_null())
            .or_else(|| {
                (card["agent"] == agent)
                    .then(|| card.get(key))
                    .flatten()
                    .filter(|value| !value.is_null())
            })
            .cloned()
            .unwrap_or(json!(""))
    };
    let options = agent_options(
        agent,
        &json!({"model": selected("model"), "effort": selected("effort")}),
        Some(settings),
    )?;
    let fallback = |key| {
        options
            .get(key)
            .filter(|value| truthy(value))
            .or_else(|| provider.get(key).filter(|value| truthy(value)))
            .cloned()
            .unwrap_or(json!(""))
    };
    agent_options(
        agent,
        &json!({"model": fallback("model"), "effort": fallback("effort")}),
        Some(settings),
    )
}

fn provider_text(body: &Value, key: &str, limit: usize) -> Result<String> {
    let invalid = "Réglage du fournisseur invalide.";
    let value = body
        .get(key)
        .map_or(Some(""), Value::as_str)
        .ok_or(invalid)?;
    if value.chars().any(|c| c <= '\u{1f}') {
        return Err(invalid);
    }
    text(value, limit).map_err(|_| invalid)
}

fn absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && b"/\\".contains(&bytes[2]))
}

fn package_part(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
}

fn arguments(value: Option<&Value>) -> Result<Value> {
    let Some(value) = value else {
        return Ok(json!([]));
    };
    let list = value
        .as_array()
        .filter(|list| list.len() <= 100)
        .ok_or("Les arguments doivent être une liste JSON.")?;
    if list.iter().any(|value| {
        value
            .as_str()
            .is_none_or(|value| value.encode_utf16().count() > 4000 || value.contains('\0'))
    }) {
        return Err("Argument invalide.");
    }
    Ok(value.clone())
}

pub fn provider_settings(body: &Value) -> Result<Value> {
    if !body.is_object() {
        return Err("Fournisseur invalide.");
    }
    let id = provider_text(body, "id", 64)?;
    let name = provider_text(body, "name", 100)?;
    let kind = body["type"].as_str().unwrap_or_default();
    if !id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || ["constructor", "prototype", "__proto__"].contains(&id.as_str())
        || name.is_empty()
        || !["codex", "claude", "command"].contains(&kind)
    {
        return Err("Fournisseur invalide.");
    }
    let auth = body
        .get("auth")
        .filter(|value| !value.is_null())
        .map_or(Some("cli"), Value::as_str);
    let auth = auth
        .filter(|auth| ["cli", "api"].contains(auth) && (*auth != "api" || kind == "codex"))
        .ok_or("Authentification invalide.")?;
    let command = provider_text(body, "command", 2000)?;
    let config_dir = provider_text(body, "configDir", 2000)?;
    let account_label = provider_text(body, "accountLabel", 200)?;
    let npm_package = provider_text(body, "npmPackage", 200)?;
    if [&command, &config_dir]
        .iter()
        .any(|path| path.len() >= 2 && path.as_bytes()[..2].iter().all(|c| b"/\\".contains(c)))
    {
        return Err("Les chemins réseau et périphériques ne sont pas autorisés.");
    }
    if !config_dir.is_empty() && !absolute(&config_dir) {
        return Err("Le dossier de configuration doit être absolu.");
    }
    if command.contains(['/', '\\']) && !absolute(&command) {
        return Err("Le chemin de l’exécutable doit être absolu.");
    }
    if !npm_package.is_empty()
        && !match npm_package.strip_prefix('@') {
            Some(scoped) => scoped
                .split_once('/')
                .is_some_and(|(scope, name)| package_part(scope) && package_part(name)),
            None => package_part(&npm_package),
        }
    {
        return Err("Paquet npm invalide.");
    }
    if command.starts_with('-') || (kind == "command" && command.is_empty()) {
        return Err("Exécutable requis.");
    }
    let options = agent_options(kind, body, None)?;
    let mut provider = json!({"id": id, "name": name, "type": kind, "auth": auth, "command": command, "configDir": config_dir, "accountLabel": account_label, "model": options["model"], "effort": options["effort"]});
    if kind == "command" {
        for key in ["analyzeArgs", "editArgs"] {
            provider[key] = arguments(body.get(key))?;
        }
        provider["versionArgs"] = match body.get("versionArgs").filter(|value| !value.is_null()) {
            Some(value) => arguments(Some(value))?,
            None => json!(["--version"]),
        };
        provider["statusArgs"] = arguments(body.get("statusArgs"))?;
        provider["npmPackage"] = json!(npm_package);
    }
    Ok(provider)
}
