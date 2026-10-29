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
            Self::Conflict => "message.forge_connections_changed_reload_before_trying_again",
            Self::Invalid(message) => message,
            Self::OperationConflict(message) => message,
        })
    }
}
impl std::error::Error for ForgeFailure {}
pub fn same_pull_request(
    left: &crate::contracts::PullRequest,
    right: &crate::contracts::PullRequest,
) -> bool {
    left.kind == right.kind
        && left.base_url == right.base_url
        && left.repository == right.repository
        && left.number == right.number
        && left.source_branch == right.source_branch
        && left.target_branch == right.target_branch
        && left.source_commit == right.source_commit
        && left.target_commit == right.target_commit
        && left.status == right.status
        && left.draft == right.draft
        && left.fork == right.fork
        && left.title == right.title
        && left.body == right.body
}
pub fn valid_content(kind: ForgeKind, content: &crate::contracts::ForgeContent) -> bool {
    !content.title.trim().is_empty()
        && content.title.encode_utf16().count() <= 255
        && content.body.encode_utf16().count()
            <= if kind == ForgeKind::Azure {
                4_000
            } else {
                20_000
            }
        && !content.title.chars().any(char::is_control)
        && !content
            .body
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
}
pub fn unresolved(fields: &crate::document::Document) -> Result<bool, serde_json::Error> {
    use crate::contracts::{ForgeDraftIntent, ForgeWriteState};
    let pushing = fields
        .get::<crate::contracts::GitPushIntent>("gitPush")?
        .is_some_and(|intent| {
            matches!(
                intent.state,
                ForgeWriteState::Preparing | ForgeWriteState::Sending | ForgeWriteState::Uncertain
            )
        });
    let publishing = fields
        .get::<crate::contracts::ForgePublicationIntent>("forgePublication")?
        .is_some_and(|intent| {
            matches!(
                intent.state,
                ForgeWriteState::Preparing | ForgeWriteState::Sending | ForgeWriteState::Uncertain
            )
        });
    let committing = fields
        .get::<crate::contracts::LocalCommitIntent>("localCommit")?
        .is_some_and(|intent| {
            matches!(
                intent.state,
                ForgeWriteState::Preparing | ForgeWriteState::Sending | ForgeWriteState::Uncertain
            )
        });
    Ok(committing
        || publishing
        || pushing
        || fields
            .get::<ForgeDraftIntent>("forgeDraft")?
            .is_some_and(|intent| {
                matches!(
                    intent.state,
                    ForgeWriteState::Preparing
                        | ForgeWriteState::Sending
                        | ForgeWriteState::Uncertain
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
        return Err(ForgeFailure::Invalid("message.invalid_repository_or_pr_number"));
    }
    if kind != ForgeKind::Gitlab && parts.len() != 2 {
        return Err(ForgeFailure::Invalid(
            "message.use_owner_repository_or_project_repository_for_this_forge",
        ));
    }
    Ok(parts)
}
