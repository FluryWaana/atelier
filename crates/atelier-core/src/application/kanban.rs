use crate::{
    contracts::{JiraBoardColumn, JiraConfiguration, JiraMode},
    document::Document,
    domain::tasks::STATUSES,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub status: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JiraMapping {
    pub instance: String,
    pub account: String,
    pub mode: JiraMode,
    pub board_id: u64,
    pub name: String,
    pub columns: Vec<JiraBoardColumn>,
    pub targets: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub columns: Vec<Column>,
    pub jira: Option<JiraMapping>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            columns: STATUSES
                .iter()
                .map(|status| Column {
                    status: (*status).into(),
                    name: String::new(),
                })
                .collect(),
            jira: None,
        }
    }
}
impl Configuration {
    pub fn validate(&self) -> Result<(), &'static str> {
        let invalid = "message.invalid_kanban_columns";
        // shortcut: preserve the five workflow stages, add separate column identities before supporting extra columns.
        if self.columns.len() != STATUSES.len() {
            return Err(invalid);
        }
        let mut seen = BTreeSet::new();
        for column in &self.columns {
            if !STATUSES.contains(&column.status.as_str())
                || !seen.insert(&column.status)
                || column.name.chars().count() > 100
                || column.name.chars().any(char::is_control)
            {
                return Err(invalid);
            }
        }
        if let Some(mapping) = &self.jira {
            if mapping.instance.len() > 2000 || mapping.account.len() > 300 || mapping.board_id == 0
            {
                return Err(invalid);
            }
            let value = serde_json::json!({"id":mapping.board_id,"name":mapping.name,"columnConfig":{"columns":mapping.columns.iter().map(|column|serde_json::json!({"name":column.name,"statuses":column.status_ids.iter().map(|id|serde_json::json!({"id":id})).collect::<Vec<_>>()})).collect::<Vec<_>>()}});
            super::jira::board_configuration(&value, mapping.board_id, 0).map_err(|_| invalid)?;
            for column in &mapping.columns {
                let targets = column
                    .status_ids
                    .iter()
                    .map(|id| mapping.targets.get(id))
                    .collect::<BTreeSet<_>>();
                if targets.len() > 1 {
                    return Err("message.column_mapping_needs_review");
                }
            }
            let ids = mapping
                .columns
                .iter()
                .flat_map(|column| column.status_ids.iter())
                .collect::<BTreeSet<_>>();
            if mapping
                .targets
                .iter()
                .any(|(id, status)| !ids.contains(id) || !STATUSES.contains(&status.as_str()))
            {
                return Err(invalid);
            }
        }
        Ok(())
    }
    pub fn mapped_status(&self, connection: &JiraConfiguration, id: &str) -> Option<String> {
        let mapping = self.jira.as_ref()?;
        if mapping.instance != connection.url
            || mapping.account != connection.email
            || mapping.mode != connection.mode
        {
            return None;
        }
        mapping
            .targets
            .get(id)
            .filter(|status| STATUSES.contains(&status.as_str()))
            .cloned()
    }
}
pub fn for_project(project: &Document) -> serde_json::Result<Configuration> {
    let configuration: Configuration = project.get("kanban")?.unwrap_or_default();
    configuration
        .validate()
        .map_err(<serde_json::Error as serde::de::Error>::custom)?;
    Ok(configuration)
}
