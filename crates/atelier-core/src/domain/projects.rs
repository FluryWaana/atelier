use super::{agent_options::find_provider, tasks::STATUSES, text};
use crate::document::Document;
use serde::Serialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, &'static str>;

pub fn same_repository(a: &str, b: &str, windows: bool) -> bool {
    if windows {
        a.replace('\\', "/").to_lowercase() == b.replace('\\', "/").to_lowercase()
    } else {
        a == b
    }
}

fn setting(body: &Document, key: &str, limit: usize) -> Result<String> {
    let invalid = "message.invalid_project_setting";
    let value = body
        .raw(key)
        .filter(|value| value.get() != "null")
        .map(|value| serde_json::from_str::<String>(value.get()))
        .transpose()
        .map_err(|_| invalid)?
        .unwrap_or_default();
    text(&value, limit).map_err(|_| invalid)
}

fn directory(path: String) -> Result<String> {
    super::files::relative_directory(&path)?;
    Ok(path)
}

#[cfg(test)]
mod directory_tests {
    use super::*;

    #[test]
    fn saved_service_directories_follow_launch_rules_without_rewriting_the_input() {
        for key in ["frontDirectory", "backDirectory"] {
            for path in ["a:stream", "NUL", ".git", "a\0b", "a\nb", "../outside"] {
                let mut body = Document::default();
                body.set(key, path).unwrap();
                assert!(
                    project_settings(&body, &Document::default()).is_err(),
                    "{key}: {path:?}"
                );
            }
        }
        assert_eq!(
            directory(".\\client\\..\\web".into()).unwrap(),
            ".\\client\\..\\web"
        );
    }
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
    pub project_rules: String,
    pub agent: String,
    pub base_branch: String,
    pub repository_connector: String,
    pub jira_mode: String,
    pub jira_status_map: Document,
    pub front_command: String,
    pub back_command: String,
    pub front_repo: String,
    pub back_repo: String,
    pub front_directory: String,
    pub back_directory: String,
    pub front_port: u16,
    pub back_port: u16,
    pub linked_project_id: String,
    pub automation_enabled: bool,
    pub auto_fix: bool,
    pub pr_target: String,
    pub test_command: String,
}

pub fn project_settings(body: &Document, settings: &Document) -> Result<ProjectSettings> {
    let invalid = "message.invalid_project_setting";
    let base_branch = setting(body, "baseBranch", 180)?;
    let connector = setting(body, "repositoryConnector", 64)?;
    let connector_text = connector.clone();
    if !connector.is_empty()
        && (!connector_text.as_bytes()[0].is_ascii_lowercase()
            || !connector_text
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    {
        return Err("message.invalid_git_provider");
    }
    let agent = if body
        .has_text("agent")
        .map_err(|_| "message.invalid_agent")?
    {
        body.get::<String>("agent")
            .map_err(|_| "message.invalid_agent")?
            .unwrap_or_default()
    } else {
        String::default()
    };
    if !agent.is_empty() {
        find_provider(settings, &agent)
            .map_err(|_| "message.invalid_agent")?
            .ok_or("message.invalid_agent")?;
    }
    let jira_mode = if body.has_text("jiraMode").map_err(|_| invalid)? {
        body.get::<String>("jiraMode")
            .map_err(|_| invalid)?
            .unwrap_or_default()
    } else {
        "inherit".into()
    };
    if !["inherit", "off", "custom"].contains(&jira_mode.as_str()) || base_branch.starts_with('-') {
        return Err("message.invalid_project_setting");
    }
    let mapping = body
        .raw("jiraStatusMap")
        .filter(|value| value.get() != "null")
        .map(|value| serde_json::from_str::<Document>(value.get()))
        .transpose()
        .map_err(|_| "message.invalid_jira_mapping")?
        .unwrap_or_default();
    if mapping.keys().any(|key| !STATUSES.contains(&key)) {
        return Err("message.invalid_jira_mapping");
    }
    let mut status_map = Document::default();
    let mut has_status = false;
    for status in STATUSES {
        let value = setting(&mapping, status, 100)?;
        has_status |= !value.is_empty();
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
    let placeholder = String::from("{port}");
    for role in ["front", "back"] {
        if port(body, &format!("{role}Port"))? != 0
            && !setting(body, &format!("{role}Command"), 2000)?.contains(&placeholder)
        {
            return Err("message.add_port_to_the_command_to_apply_the_port_provided_by_atelier");
        }
    }
    let automation_enabled = body
        .get::<bool>("automationEnabled")
        .map_err(|_| "message.invalid_automatic_option")?
        .unwrap_or(false);
    if pr_target.starts_with('-')
        || (automation_enabled && (pr_target.is_empty() || test_command.is_empty()))
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
            .filter(|value| value.as_str().is_some_and(|value| !value.is_empty()))
            .cloned()
            .unwrap_or(json!(""))
    };
    let mut result = settings.clone();
    result["kanban"] = project["kanban"].clone();
    result["projectRules"] = if repo.is_empty() {
        json!("")
    } else {
        content("projectRules")
    };
    for key in ["repositoryConnector", "testCommand", "prTarget"] {
        result[key] = content(key);
    }
    result["automationEnabled"] = json!(project["automationEnabled"].as_bool().unwrap_or(false));
    result["autoFix"] = project
        .get("autoFix")
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or(json!(true));
    if project["baseBranch"]
        .as_str()
        .is_some_and(|value| !value.is_empty())
    {
        result["baseBranch"] = project["baseBranch"].clone();
    }
    result["agent"] = project
        .get("agent")
        .filter(|value| value.as_str().is_some_and(|value| !value.is_empty()))
        .or_else(|| {
            settings
                .get("agent")
                .filter(|value| value.as_str().is_some_and(|value| !value.is_empty()))
        })
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
