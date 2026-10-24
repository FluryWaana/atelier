use super::{text, text_units, truthy};
use crate::document::{Document, JsonString};
use serde_json::{Value, json, value::RawValue};

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

pub fn options_document(kind: &str, body: &Document) -> Result<Value> {
    let mut options = serde_json::Map::new();
    for key in ["model", "effort"] {
        if let Some(raw) = body.raw(key) {
            // Both fields are ASCII-only; malformed UTF-16 keeps the normal validation priority.
            options.insert(
                key.into(),
                serde_json::from_str(raw.get()).unwrap_or(Value::Null),
            );
        }
    }
    agent_options(kind, &Value::Object(options), None)
}

fn provider_text(body: &Document, key: &str, limit: usize) -> Result<JsonString> {
    let invalid = "Réglage du fournisseur invalide.";
    let value = body
        .get::<JsonString>(key)
        .map_err(|_| invalid)?
        .unwrap_or_default();
    if value.0.iter().any(|unit| *unit <= 0x1f) {
        return Err(invalid);
    }
    text_units(&value, limit).map_err(|_| invalid)
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

fn arguments(value: Option<&RawValue>) -> Result<Vec<JsonString>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list: Vec<Box<RawValue>> = serde_json::from_str(value.get())
        .map_err(|_| "Les arguments doivent être une liste JSON.")?;
    if list.len() > 100 {
        return Err("Les arguments doivent être une liste JSON.");
    }
    list.iter()
        .map(|raw| {
            let value: JsonString =
                serde_json::from_str(raw.get()).map_err(|_| "Argument invalide.")?;
            if value.0.len() > 4000 || value.0.contains(&0) {
                return Err("Argument invalide.");
            }
            Ok(value)
        })
        .collect()
}

pub fn provider_settings(raw: &RawValue) -> Result<Document> {
    let body: Document = serde_json::from_str(raw.get()).map_err(|_| "Fournisseur invalide.")?;
    let id = provider_text(&body, "id", 64)?;
    let name = provider_text(&body, "name", 100)?;
    let id = id.utf8().map_err(|_| "Fournisseur invalide.")?;
    let kind = body
        .get::<String>("type")
        .ok()
        .flatten()
        .unwrap_or_default();
    let kind = kind.as_str();
    if !id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || ["constructor", "prototype", "__proto__"].contains(&id.as_str())
        || name.0.is_empty()
        || !["codex", "claude", "command"].contains(&kind)
    {
        return Err("Fournisseur invalide.");
    }
    let auth = body
        .raw("auth")
        .filter(|value| value.get() != "null")
        .map(|raw| serde_json::from_str::<String>(raw.get()))
        .transpose()
        .map_err(|_| "Authentification invalide.")?
        .unwrap_or_else(|| "cli".into());
    let auth = Some(auth.as_str())
        .filter(|auth| ["cli", "api"].contains(auth) && (*auth != "api" || kind == "codex"))
        .ok_or("Authentification invalide.")?;
    let command = provider_text(&body, "command", 2000)?;
    let config_dir = provider_text(&body, "configDir", 2000)?;
    let account_label = provider_text(&body, "accountLabel", 200)?;
    let npm_package = provider_text(&body, "npmPackage", 200)?;
    // Classification uses ASCII delimiters; preserve original UTF-16 units in the saved profile.
    let command_view = String::from_utf16_lossy(&command.0);
    let config_view = String::from_utf16_lossy(&config_dir.0);
    if [&command_view, &config_view]
        .iter()
        .any(|path| path.len() >= 2 && path.as_bytes()[..2].iter().all(|c| b"/\\".contains(c)))
    {
        return Err("Les chemins réseau et périphériques ne sont pas autorisés.");
    }
    if !config_view.is_empty() && !absolute(&config_view) {
        return Err("Le dossier de configuration doit être absolu.");
    }
    if command_view.contains(['/', '\\']) && !absolute(&command_view) {
        return Err("Le chemin de l’exécutable doit être absolu.");
    }
    let npm_package = npm_package.utf8().map_err(|_| "Paquet npm invalide.")?;
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
    if command_view.starts_with('-') || (kind == "command" && command_view.is_empty()) {
        return Err("Exécutable requis.");
    }
    let options = options_document(kind, &body)?;
    let mut provider = Document::default();
    for (key, value) in [
        ("id", JsonString::from(id.as_str())),
        ("name", name),
        ("type", JsonString::from(kind)),
        ("auth", JsonString::from(auth)),
        ("command", command),
        ("configDir", config_dir),
        ("accountLabel", account_label),
    ] {
        provider
            .set(key, &value)
            .map_err(|_| "Fournisseur invalide.")?;
    }
    for key in ["model", "effort"] {
        provider
            .set(key, &options[key])
            .map_err(|_| "Fournisseur invalide.")?;
    }
    if kind == "command" {
        for key in ["analyzeArgs", "editArgs"] {
            provider
                .set(key, &arguments(body.raw(key))?)
                .map_err(|_| "Fournisseur invalide.")?;
        }
        let version = match body
            .raw("versionArgs")
            .filter(|value| value.get() != "null")
        {
            Some(value) => arguments(Some(value))?,
            None => vec![JsonString::from("--version")],
        };
        provider
            .set("versionArgs", &version)
            .map_err(|_| "Fournisseur invalide.")?;
        provider
            .set("statusArgs", &arguments(body.raw("statusArgs"))?)
            .map_err(|_| "Fournisseur invalide.")?;
        provider
            .set("npmPackage", &npm_package)
            .map_err(|_| "Fournisseur invalide.")?;
    }
    Ok(provider)
}
