use super::{Result, identifier, text};
use crate::contracts::Worktree;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryReference {
    pub instance: String,
    pub repository: String,
    pub origin: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModulePullRequest {
    pub reference: RepositoryReference,
    pub id: String,
    pub url: String,
    pub source_branch: String,
    pub target_branch: String,
    pub source_commit: String,
    pub target_commit: String,
    pub status: String,
    pub draft: bool,
    pub auto_merge: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryObservation {
    pub reference: RepositoryReference,
    pub target_branch: String,
    pub target_commit: String,
    pub remote_source: Option<String>,
    pub pull_request: Option<ModulePullRequest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModulePullRequestLink {
    pub module: String,
    pub revision: u64,
    pub pull_request: ModulePullRequest,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryReviewContext {
    pub module: String,
    pub revision: u64,
    pub card_id: String,
    pub requirements: Value,
    pub worktree: Worktree,
    pub origin: String,
    pub source: String,
    pub target: String,
    pub base: String,
    pub fingerprint: String,
    pub pull_request: Option<ModulePullRequest>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryProposal {
    #[serde(default)]
    pub automation_id: Option<String>,
    pub context: RepositoryReviewContext,
    pub card_revision: String,
    pub observation: RepositoryObservation,
    pub title: String,
    pub body: String,
    pub draft: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryPublication {
    pub reference: RepositoryReference,
    pub worktree: Worktree,
    pub source: String,
    pub target: String,
    pub target_branch: String,
    pub remote_source: Option<String>,
    pub title: String,
    pub body: String,
    pub draft: bool,
    pub pull_request: Option<ModulePullRequest>,
}
pub fn repository_publication(value: &Value) -> Result<RepositoryPublication> {
    let payload: RepositoryPublication = serde_json::from_value(value.clone())
        .map_err(|_| "message.invalid_module_publication_request")?;
    if !repository_commit(&payload.source)
        || !repository_commit(&payload.target)
        || !text(&payload.title, 255, false)
        || !text(&payload.body, 3500, true)
        || crate::application::worktrees::validate_reference(&payload.target_branch).is_err()
        || payload.target_branch == payload.worktree.branch
        || payload
            .remote_source
            .as_ref()
            .is_some_and(|value| !repository_commit(value))
    {
        return Err("message.invalid_module_publication_request");
    }
    Ok(payload)
}
pub fn repository_commit(value: &str) -> bool {
    [40, 64].contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
pub fn repository_observation(value: &Value) -> Result<RepositoryObservation> {
    let observation: RepositoryObservation = serde_json::from_value(value.clone())
        .map_err(|_| "message.invalid_module_repository_observation")?;
    let reference = &observation.reference;
    if !text(&reference.instance, 2000, false)
        || !text(&reference.repository, 1000, false)
        || !text(&reference.origin, 2000, false)
        || crate::application::worktrees::validate_reference(&observation.target_branch).is_err()
        || !repository_commit(&observation.target_commit)
        || observation
            .remote_source
            .as_ref()
            .is_some_and(|value| !repository_commit(value))
    {
        return Err("message.invalid_module_repository_identity_branch_or_commit");
    }
    if let Some(pr) = &observation.pull_request
        && (pr.reference != *reference
            || !text(&pr.id, 200, false)
            || !text(&pr.url, 2000, false)
            || crate::application::worktrees::validate_reference(&pr.source_branch).is_err()
            || pr.target_branch != observation.target_branch
            || pr.target_commit != observation.target_commit
            || !repository_commit(&pr.source_commit)
            || !["active", "completed", "abandoned"].contains(&pr.status.as_str()))
    {
        return Err("message.the_module_pr_mr_does_not_match_its_repository_and_commits");
    }
    Ok(observation)
}
pub fn repository_proposal(value: &Value) -> Result<RepositoryProposal> {
    let proposal: RepositoryProposal = serde_json::from_value(value.clone())
        .map_err(|_| "message.invalid_module_publication_preview")?;
    repository_observation(
        &serde_json::to_value(&proposal.observation).map_err(|_| "message.observation_invalid")?,
    )?;
    if !identifier(&proposal.context.module)
        || proposal.context.revision == 0
        || !text(&proposal.context.card_id, 100, false)
        || !text(&proposal.title, 255, false)
        || !text(&proposal.body, 3500, true)
        || proposal.card_revision.len() != 64
        || !proposal
            .card_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || [
            &proposal.context.source,
            &proposal.context.target,
            &proposal.context.base,
        ]
        .iter()
        .any(|value| !repository_commit(value))
        || proposal.context.target != proposal.observation.target_commit
        || proposal.context.origin != proposal.observation.reference.origin
        || proposal.context.pull_request != proposal.observation.pull_request
        || proposal.context.worktree.state != crate::contracts::WorktreeState::Ready
        || proposal.context.fingerprint.len() != 64
        || !proposal
            .context
            .fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || proposal.context.requirements["criteria"]
            .as_array()
            .is_none_or(|criteria| {
                criteria.len() > 20 || criteria.iter().any(|value| value.as_str().is_none())
            })
        || proposal
            .automation_id
            .as_ref()
            .is_some_and(|id| !workflow_id(id))
    {
        return Err("message.invalid_module_publication_preview");
    }
    Ok(proposal)
}
// Workflow IDs are host-generated UUIDs, not module identifiers. In particular,
// a UUID may start with a digit, which the module identifier grammar forbids.
fn workflow_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}
pub fn repository_source(card: &crate::contracts::Card) -> crate::contracts::Card {
    let mut card = card.clone();
    // A confirmed workflow changes its own column while rechecking the approved
    // sources. Preserve manual columns, but do not invalidate approval for this
    // explicitly owned transition from Waiting to Running (or back).
    if let Ok(Some(flow)) = card
        .fields
        .get::<crate::application::automation::Flow>("automation")
        && matches!(
            flow.state,
            crate::application::automation::State::Running
                | crate::application::automation::State::Waiting
        )
        && card
            .fields
            .string_equals("status", flow.state.card_status())
            .unwrap_or(false)
    {
        card.fields.remove("status");
    }
    for key in [
        "connectorRepositoryPreview",
        "connectorReviewContext",
        "connectorRepositoryObservation",
        "automation",
        "updatedAt",
    ] {
        card.fields.remove(key);
    }
    card
}
pub fn repository_prompt(context: &RepositoryReviewContext) -> Result<String> {
    crate::application::reviews::prompt_snapshot(
        &context.source,
        &context.target,
        &context.base,
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn approved_publication_accepts_numeric_workflow_uuid_and_rejects_invalid_ids() {
        let commit = "a".repeat(40);
        let origin = "https://synthetic.example.test/owner/repository.git";
        let mut proposal = json!({
            "automationId":"01234567-89ab-4cde-8fab-0123456789ab",
            "context":{
                "module":"synthetic-extension","revision":1,"cardId":"task",
                "requirements":{"criteria":["Synthetic criterion"]},
                "worktree":{"path":"synthetic-worktree","source":"synthetic-repository",
                    "branch":"task","base":"main","baseCommit":commit,"state":"ready"},
                "origin":origin,"source":commit,"target":commit,"base":commit,
                "fingerprint":"a".repeat(64),"pullRequest":null
            },
            "cardRevision":"b".repeat(64),
            "observation":{
                "reference":{"instance":"https://synthetic.example.test",
                    "repository":"owner/repository","origin":origin},
                "targetBranch":"main","targetCommit":commit,"remoteSource":commit,"pullRequest":null
            },
            "title":"Synthetic publication","body":"Approved body","draft":true
        });
        assert!(repository_proposal(&proposal).is_ok());
        for invalid in [
            "synthetic-workflow",
            "0123456789ab4cde8fab0123456789ab",
            "01234567-89ab-4cde-8fab-0123456789ag",
            "01234567-89ab-4cde-8fab-0123456789ab/",
        ] {
            proposal["automationId"] = json!(invalid);
            assert!(repository_proposal(&proposal).is_err(), "{invalid}");
        }
        proposal["automationId"] = Value::Null;
        assert!(repository_proposal(&proposal).is_ok());
    }
}
