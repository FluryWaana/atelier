use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Available,
    Current,
    Unknown,
    Managed,
    Custom,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInfo {
    pub package: String,
    pub latest_version: Option<String>,
    pub managed_root: Option<String>,
    pub observed_at: String,
    pub availability: Availability,
    pub installed_version: Option<String>,
    pub image: Option<super::extensions::Binary>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Running,
    Observed,
    Failed,
    Uncertain,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub id: String,
    pub provider: String,
    pub info: UpdateInfo,
    pub phase: Phase,
    pub started_at: String,
    pub updated_at: String,
    pub actual_version: Option<String>,
    pub target_reached: Option<bool>,
    pub message: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRequest {
    pub expected: crate::document::Document,
    pub info: UpdateInfo,
    pub confirmed: bool,
}
pub fn operation(settings: &crate::document::Document) -> Result<Option<Operation>, &'static str> {
    if settings
        .raw("agentCliUpdate")
        .is_some_and(|raw| raw.get().len() > 65_536)
    {
        return Err("Journal de mise à jour CLI trop volumineux.");
    }
    let operation = settings
        .get::<Operation>("agentCliUpdate")
        .map_err(|_| "Journal de mise à jour CLI invalide.")?;
    if let Some(operation) = &operation
        && (!super::extensions::identifier(&operation.id)
            || operation.provider.is_empty()
            || operation.provider.len() > 100
            || operation.provider.chars().any(char::is_control)
            || !matches!(
                operation.info.package.as_str(),
                "@openai/codex" | "@anthropic-ai/claude-code"
            ))
    {
        return Err("Journal de mise à jour CLI invalide.");
    }
    Ok(operation)
}
pub fn busy(settings: &crate::document::Document) -> Result<bool, &'static str> {
    Ok(operation(settings)?
        .is_some_and(|operation| matches!(operation.phase, Phase::Preparing | Phase::Running)))
}
pub fn recover(settings: &mut crate::document::Document, now: &str) -> Result<bool, &'static str> {
    let Some(mut operation) = operation(settings)? else {
        return Ok(false);
    };
    operation.phase = match operation.phase {
        Phase::Preparing => Phase::Failed,
        Phase::Running => Phase::Uncertain,
        _ => return Ok(false),
    };
    operation.updated_at = now.into();
    operation.message = if operation.phase == Phase::Failed {
        "Préparation CLI interrompue avant envoi."
    } else {
        "Mise à jour CLI interrompue. Relis la version installée ; aucune commande rejouée."
    }
    .into();
    settings
        .set("agentCliUpdate", &operation)
        .map_err(|_| "Journal de mise à jour CLI invalide.")?;
    Ok(true)
}
