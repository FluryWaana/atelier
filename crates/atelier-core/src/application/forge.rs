use crate::contracts::ForgeKind;
#[derive(Debug)]
pub enum ForgeFailure {
    Conflict,
    OperationConflict(&'static str),
    Invalid(&'static str),
}
impl std::fmt::Display for ForgeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Conflict => "Les connexions aux forges ont changé. Recharge avant de réessayer.",
            Self::Invalid(message) => message,
            Self::OperationConflict(message) => message,
        })
    }
}
impl std::error::Error for ForgeFailure {}
pub fn unresolved(fields: &crate::document::Document) -> Result<bool, serde_json::Error> {
    use crate::contracts::{ForgeDraftIntent, ForgeWriteState};
    Ok(fields
        .get::<ForgeDraftIntent>("forgeDraft")?
        .is_some_and(|intent| {
            matches!(
                intent.state,
                ForgeWriteState::Preparing | ForgeWriteState::Sending | ForgeWriteState::Uncertain
            )
        }))
}
pub fn repository(kind: ForgeKind, name: &str, number: u64) -> Result<Vec<&str>, ForgeFailure> {
    let parts = name.split('/').collect::<Vec<_>>();
    if name.len() > 1000
        || parts.len() < 2
        || parts.len() > 20
        || parts.iter().any(|part| {
            part.is_empty()
                || [".", ".."].contains(part)
                || part.chars().any(|c| c.is_control() || c == '\\')
        })
        || number == 0
        || number > i32::MAX as u64
    {
        return Err(ForgeFailure::Invalid("Dépôt ou numéro de PR invalide."));
    }
    if kind != ForgeKind::Gitlab && parts.len() != 2 {
        return Err(ForgeFailure::Invalid(
            "Utilise propriétaire/dépôt ou projet/dépôt pour cette forge.",
        ));
    }
    Ok(parts)
}
