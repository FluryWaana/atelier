use super::{agent_options::agent_provider, tasks::STATUSES, text, truthy};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, &'static str>;

pub fn same_repository(a: &str, b: &str, windows: bool) -> bool {
    if windows {
        a.replace('\\', "/").to_lowercase() == b.replace('\\', "/").to_lowercase()
    } else {
        a == b
    }
}

fn setting(body: &Value, key: &str, limit: usize) -> Result<String> {
    let invalid = "Réglage de projet invalide.";
    let value = body
        .get(key)
        .filter(|value| !value.is_null())
        .map_or(Some(""), Value::as_str)
        .ok_or(invalid)?;
    text(value, limit).map_err(|_| invalid)
}

fn directory(path: String) -> Result<String> {
    let invalid = "Le dossier du service doit être relatif et rester dans le worktree.";
    let bytes = path.as_bytes();
    if path.starts_with(['/', '\\'])
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        return Err(invalid);
    }
    let mut depth = 0usize;
    // Apply the Windows traversal rules on every OS, as the historical validator does.
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => depth = depth.checked_sub(1).ok_or(invalid)?,
            _ => depth += 1,
        }
    }
    Ok(path)
}

fn port(value: Option<&Value>) -> Result<u16> {
    let invalid =
        "Port invalide : indique un nombre de 1 à 65535, ou laisse vide pour un port automatique.";
    let Some(value) = value else {
        return Ok(0);
    };
    if value == "" {
        return Ok(0);
    }
    let number = if let Some(value) = value.as_str() {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid);
        }
        value.parse::<f64>().map_err(|_| invalid)?
    } else {
        value.as_f64().ok_or(invalid)?
    };
    if !(0.0..=65535.0).contains(&number) || number.fract() != 0.0 {
        return Err(invalid);
    }
    Ok(number as u16)
}

pub fn project_settings(body: &Value, settings: &Value) -> Result<Value> {
    let agent = body
        .get("agent")
        .filter(|value| truthy(value))
        .cloned()
        .unwrap_or(json!(""));
    let base_branch = setting(body, "baseBranch", 180)?;
    let jira_mode = body
        .get("jiraMode")
        .filter(|value| truthy(value))
        .cloned()
        .unwrap_or(json!("inherit"));
    let connector = setting(body, "repositoryConnector", 64)?;
    if !connector.is_empty()
        && (!connector.as_bytes()[0].is_ascii_lowercase()
            || !connector
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'))
    {
        return Err("Fournisseur Git invalide.");
    }
    if truthy(&agent) {
        agent_provider(settings, agent.as_str().ok_or("Agent invalide.")?)?;
    }
    if ![json!("inherit"), json!("off"), json!("custom")].contains(&jira_mode)
        || base_branch.starts_with('-')
    {
        return Err("Réglage de projet invalide.");
    }
    let empty = json!({});
    let mapping = body
        .get("jiraStatusMap")
        .filter(|value| !value.is_null())
        .unwrap_or(&empty);
    if !mapping.is_object()
        || mapping
            .as_object()
            .is_some_and(|mapping| mapping.keys().any(|key| !STATUSES.contains(&key.as_str())))
    {
        return Err("Correspondance Jira invalide.");
    }
    let mut status_map = json!({});
    for status in STATUSES {
        status_map[status] = json!(setting(mapping, status, 100)?);
    }
    if jira_mode == "custom" && !STATUSES.iter().any(|status| truthy(&status_map[status])) {
        return Err("Renseigne au moins un statut Jira.");
    }
    for key in ["automationEnabled", "autoFix"] {
        if body.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err("Option automatique invalide.");
        }
    }
    let pr_target = setting(body, "prTarget", 180)?;
    let test_command = setting(body, "testCommand", 2000)?;
    for role in ["front", "back"] {
        if port(body.get(format!("{role}Port")))? != 0
            && !setting(body, &format!("{role}Command"), 2000)?.contains("{port}")
        {
            return Err(
                "Ajoute {port} dans la commande pour appliquer le port fourni par Atelier.",
            );
        }
    }
    if pr_target.starts_with('-')
        || (truthy(&body["automationEnabled"]) && (pr_target.is_empty() || test_command.is_empty()))
    {
        return Err(
            "Renseigne la branche cible et la commande de vérification avant d’activer le mode automatique.",
        );
    }
    let mut result = json!({"agent": agent, "baseBranch": base_branch, "repositoryConnector": connector, "jiraMode": jira_mode, "jiraStatusMap": status_map, "automationEnabled": body.get("automationEnabled").unwrap_or(&Value::Bool(false)), "autoFix": body.get("autoFix").unwrap_or(&Value::Bool(true)), "prTarget": pr_target, "testCommand": test_command});
    for (key, limit) in [
        ("projectRules", 12000),
        ("frontCommand", 2000),
        ("backCommand", 2000),
        ("frontRepo", 2000),
        ("backRepo", 2000),
    ] {
        result[key] = json!(setting(body, key, limit)?);
    }
    for role in ["front", "back"] {
        let key = format!("{role}Directory");
        result[&key] = json!(directory(setting(body, &key, 2000)?)?);
    }
    for role in ["front", "back"] {
        let key = format!("{role}Port");
        result[&key] = json!(port(body.get(&key))?);
    }
    result["linkedProjectId"] = json!(setting(body, "linkedProjectId", 100)?);
    Ok(result)
}

pub fn settings_for_project(
    settings: &Value,
    projects: &[Value],
    repo: &str,
    windows: bool,
) -> Value {
    let repository = if repo.is_empty() {
        settings["repo"].as_str().unwrap_or_default()
    } else {
        repo
    };
    let project = projects
        .iter()
        .find(|project| {
            same_repository(
                project["repo"].as_str().unwrap_or_default(),
                repository,
                windows,
            )
        })
        .unwrap_or(&Value::Null);
    let content = |key| {
        project
            .get(key)
            .filter(|value| truthy(value))
            .cloned()
            .unwrap_or(json!(""))
    };
    let mut result = settings.clone();
    result["projectRules"] = if repo.is_empty() {
        json!("")
    } else {
        content("projectRules")
    };
    for key in ["repositoryConnector", "testCommand", "prTarget"] {
        result[key] = content(key);
    }
    result["automationEnabled"] = json!(truthy(&project["automationEnabled"]));
    result["autoFix"] = project
        .get("autoFix")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or(json!(true));
    if truthy(&project["baseBranch"]) {
        result["baseBranch"] = project["baseBranch"].clone();
    }
    result["agent"] = project
        .get("agent")
        .filter(|value| truthy(value))
        .or_else(|| settings.get("agent").filter(|value| truthy(value)))
        .cloned()
        .unwrap_or(json!("codex"));
    match project["jiraMode"].as_str() {
        Some("off") => result["jiraAutoTransition"] = json!(false),
        Some("custom") => {
            result["jiraAutoTransition"] = json!(true);
            if let Some(mapping) = project.get("jiraStatusMap") {
                result["jiraStatusMap"] = mapping.clone();
            } else if let Some(object) = result.as_object_mut() {
                object.remove("jiraStatusMap");
            }
        }
        _ => {}
    }
    result
}
