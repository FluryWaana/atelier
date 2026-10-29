use super::{agent_options::find_provider, tasks::STATUSES, text_units, truthy};
use crate::document::{Document, JsonString};
use serde::Serialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, &'static str>;

pub fn same_repository(a: &str, b: &str, windows: bool) -> bool {
    same_repository_units(&JsonString::from(a), &JsonString::from(b), windows)
}

pub fn same_repository_units(a: &JsonString, b: &JsonString, windows: bool) -> bool {
    if windows {
        fn key(value: &JsonString) -> Vec<u16> {
            let mut result = Vec::new();
            let mut segment = String::new();
            for character in char::decode_utf16(value.0.iter().copied()) {
                match character {
                    Ok(character) => segment.push(if character == '\\' { '/' } else { character }),
                    Err(surrogate) => {
                        result.extend(segment.to_lowercase().encode_utf16());
                        segment.clear();
                        result.push(surrogate.unpaired_surrogate());
                    }
                }
            }
            result.extend(segment.to_lowercase().encode_utf16());
            result
        }
        key(a) == key(b)
    } else {
        a == b
    }
}

fn setting(body: &Document, key: &str, limit: usize) -> Result<JsonString> {
    let invalid = "message.invalid_project_setting";
    let value = body
        .raw(key)
        .filter(|value| value.get() != "null")
        .map(|value| serde_json::from_str::<JsonString>(value.get()))
        .transpose()
        .map_err(|_| invalid)?
        .unwrap_or_default();
    text_units(&value, limit).map_err(|_| invalid)
}

fn directory(path: JsonString) -> Result<JsonString> {
    let invalid = "message.the_service_folder_must_be_relative_and_stay_inside_the_worktree";
    // Replacing a surrogate cannot introduce ASCII traversal; keep the original units in the result.
    let view = String::from_utf16_lossy(&path.0);
    let bytes = view.as_bytes();
    if view.starts_with(['/', '\\'])
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        return Err(invalid);
    }
    let mut depth = 0usize;
    // Apply the Windows traversal rules on every OS, as the historical validator does.
    for part in view.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => depth = depth.checked_sub(1).ok_or(invalid)?,
            _ => depth += 1,
        }
    }
    Ok(path)
}

fn port(body: &Document, key: &str) -> Result<u16> {
    let invalid = "message.invalid_port_enter_a_number_from_1_to_65535_or_leave_empty_for_a";
    let Some(value) = body.raw(key) else {
        return Ok(0);
    };
    let value: Value = serde_json::from_str(value.get()).map_err(|_| invalid)?;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    pub project_rules: JsonString,
    pub agent: JsonString,
    pub base_branch: JsonString,
    pub repository_connector: JsonString,
    pub jira_mode: String,
    pub jira_status_map: Document,
    pub front_command: JsonString,
    pub back_command: JsonString,
    pub front_repo: JsonString,
    pub back_repo: JsonString,
    pub front_directory: JsonString,
    pub back_directory: JsonString,
    pub front_port: u16,
    pub back_port: u16,
    pub linked_project_id: JsonString,
    pub automation_enabled: bool,
    pub auto_fix: bool,
    pub pr_target: JsonString,
    pub test_command: JsonString,
}

pub fn project_settings(body: &Document, settings: &Document) -> Result<ProjectSettings> {
    let invalid = "message.invalid_project_setting";
    let base_branch = setting(body, "baseBranch", 180)?;
    let connector = setting(body, "repositoryConnector", 64)?;
    let connector_text = connector
        .utf8()
        .map_err(|_| "message.invalid_git_provider")?;
    if !connector.0.is_empty()
        && (!connector_text.as_bytes()[0].is_ascii_lowercase()
            || !connector_text
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    {
        return Err("message.invalid_git_provider");
    }
    let agent = if body.truthy("agent").map_err(|_| "message.invalid_agent")? {
        body.get::<JsonString>("agent")
            .map_err(|_| "message.invalid_agent")?
            .unwrap_or_default()
    } else {
        JsonString::default()
    };
    if !agent.0.is_empty() {
        find_provider(settings, &agent)
            .map_err(|_| "message.invalid_agent")?
            .ok_or("message.invalid_agent")?;
    }
    let jira_mode = if body.truthy("jiraMode").map_err(|_| invalid)? {
        body.get::<String>("jiraMode")
            .map_err(|_| invalid)?
            .unwrap_or_default()
    } else {
        "inherit".into()
    };
    if !["inherit", "off", "custom"].contains(&jira_mode.as_str())
        || base_branch.0.first() == Some(&u16::from(b'-'))
    {
        return Err("message.invalid_project_setting");
    }
    let mapping = body
        .raw("jiraStatusMap")
        .filter(|value| value.get() != "null")
        .map(|value| serde_json::from_str::<Document>(value.get()))
        .transpose()
        .map_err(|_| "message.invalid_jira_mapping")?
        .unwrap_or_default();
    if mapping.keys().any(|key| {
        !STATUSES
            .iter()
            .any(|status| key == &JsonString::from(*status))
    }) {
        return Err("message.invalid_jira_mapping");
    }
    let mut status_map = Document::default();
    let mut has_status = false;
    for status in STATUSES {
        let value = setting(&mapping, status, 100)?;
        has_status |= !value.0.is_empty();
        status_map.set(status, &value).map_err(|_| invalid)?;
    }
    if jira_mode == "custom" && !has_status {
        return Err("message.enter_at_least_one_jira_status");
    }
    for key in ["automationEnabled", "autoFix"] {
        if body.get::<bool>(key).is_err() {
            return Err("message.invalid_automatic_option");
        }
    }
    let pr_target = setting(body, "prTarget", 180)?;
    let test_command = setting(body, "testCommand", 2000)?;
    let placeholder = JsonString::from("{port}");
    for role in ["front", "back"] {
        if port(body, &format!("{role}Port"))? != 0
            && !setting(body, &format!("{role}Command"), 2000)?
                .0
                .windows(placeholder.0.len())
                .any(|units| units == placeholder.0)
        {
            return Err("message.add_port_to_the_command_to_apply_the_port_provided_by_atelier");
        }
    }
    let automation_enabled = body
        .get::<bool>("automationEnabled")
        .map_err(|_| "message.invalid_automatic_option")?
        .unwrap_or(false);
    if pr_target.0.first() == Some(&u16::from(b'-'))
        || (automation_enabled && (pr_target.0.is_empty() || test_command.0.is_empty()))
    {
        return Err("message.enter_the_target_branch_and_verification_command_before_enabling");
    }
    Ok(ProjectSettings {
        project_rules: setting(body, "projectRules", 12000)?,
        agent,
        base_branch,
        repository_connector: connector,
        jira_mode,
        jira_status_map: status_map,
        front_command: setting(body, "frontCommand", 2000)?,
        back_command: setting(body, "backCommand", 2000)?,
        front_repo: setting(body, "frontRepo", 2000)?,
        back_repo: setting(body, "backRepo", 2000)?,
        front_directory: directory(setting(body, "frontDirectory", 2000)?)?,
        back_directory: directory(setting(body, "backDirectory", 2000)?)?,
        front_port: port(body, "frontPort")?,
        back_port: port(body, "backPort")?,
        linked_project_id: setting(body, "linkedProjectId", 100)?,
        automation_enabled,
        auto_fix: body
            .get::<bool>("autoFix")
            .map_err(|_| "message.invalid_automatic_option")?
            .unwrap_or(true),
        pr_target,
        test_command,
    })
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
