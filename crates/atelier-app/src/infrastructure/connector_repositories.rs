use super::{
    checked_file::CheckedFile,
    connectors,
    files::Repository,
    forge_association,
    store::{Documents, Store},
    worktrees,
};
use crate::Result;
use atelier_core::{
    application::{
        extensions::{
            self as protocol, Installed, Method, ModulePullRequestLink, RepositoryObservation,
            RepositoryProposal, RepositoryReviewContext,
        },
        reviews,
    },
    contracts::{Card, Worktree},
    document::Document,
};
use cap_std::fs::Dir;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub expected: Card,
    pub target: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}
#[derive(PartialEq, Eq)]
struct Disk {
    head: Vec<u8>,
    branch: Vec<u8>,
    source_branch: Option<Vec<u8>>,
    branches: Vec<u8>,
    index: Option<Vec<u8>>,
    fetch: Option<Vec<u8>>,
    source_head: Vec<u8>,
    source_index: Option<Vec<u8>>,
    source_fetch: Option<Vec<u8>>,
    origin: Vec<u8>,
    fingerprint: String,
}
pub struct Guard {
    pub selection: Selection,
    pub profile: Installed,
    repository: Repository,
    source: Repository,
    disk: Disk,
}
fn optional(directory: &Dir, path: &str) -> Result<Option<Vec<u8>>> {
    match directory.symlink_metadata(path) {
        Ok(_) => Ok(Some(
            CheckedFile::open_in(directory, Path::new(path))?.read(8 * 1024 * 1024)?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn url(value: &str) -> Result<Url> {
    let url = Url::parse(value)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(
            "message.a_repository_https_address_without_credentials_or_parameters_is".into(),
        );
    }
    Ok(url)
}
fn scope(profile: &Installed, observation: &RepositoryObservation) -> Result<()> {
    let instance = url(&observation.reference.instance)?;
    let origin = url(&observation.reference.origin)?;
    if instance.origin() != origin.origin()
        || !(origin.path() == instance.path().trim_end_matches('/')
            || origin
                .path()
                .starts_with(&format!("{}/", instance.path().trim_end_matches('/'))))
        || !profile.manifest.fields.iter().any(|field| {
            field.kind == protocol::FieldKind::Url
                && profile
                    .values
                    .get(&field.id)
                    .and_then(Value::as_str)
                    .is_some_and(|value| {
                        value.trim_end_matches('/')
                            == observation.reference.instance.trim_end_matches('/')
                    })
        })
    {
        return Err(
            "message.the_repository_instance_does_not_belong_to_the_selected_connecti".into(),
        );
    }
    if let Some(pr) = &observation.pull_request {
        let link = url(&pr.url)?;
        if link.origin() != instance.origin()
            || !link
                .path()
                .starts_with(&format!("{}/", instance.path().trim_end_matches('/')))
        {
            return Err("message.the_pr_mr_link_is_outside_the_module_instance".into());
        }
    }
    Ok(())
}
async fn disk(repository: &Repository, source: &Repository) -> Result<Disk> {
    let metadata = worktrees::metadata(source, repository)?;
    let principal = source.directory.open_dir(".git")?;
    Ok(Disk {
        head: repository
            .git(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?,
        branch: repository.git(&["symbolic-ref", "HEAD"]).await?,
        source_branch: source
            .git_optional(&["symbolic-ref", "--quiet", "HEAD"], true)
            .await?,
        branches: source
            .git(&[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)",
                "refs/heads/",
            ])
            .await?,
        index: optional(&metadata, "index")?,
        fetch: optional(&metadata, "FETCH_HEAD")?,
        source_head: source
            .git(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?,
        source_index: optional(&principal, "index")?,
        source_fetch: optional(&principal, "FETCH_HEAD")?,
        origin: repository
            .git(&[
                "config",
                "--local",
                "--no-includes",
                "--get",
                "remote.origin.url",
            ])
            .await?,
        fingerprint: repository.fingerprint().await?,
    })
}
pub fn selection(store: &Store<'_>, profile: &Installed, value: &Value) -> Result<Selection> {
    let mut selection: Selection = serde_json::from_str(&value.to_string())
        .map_err(|_| "message.invalid_module_repository_selection")?;
    selection.expected = forge_association::snapshot(store, &selection.expected)?;
    if selection.expected.fields.raw("pullRequest").is_some() {
        return Err(
            "message.this_task_already_uses_a_native_forge_detach_its_pr_mr_before_sw".into(),
        );
    }
    if let Some(link) = selection
        .expected
        .fields
        .get::<ModulePullRequestLink>("connectorPullRequest")?
        && link.module != profile.manifest.id
    {
        return Err("message.this_pr_mr_belongs_to_another_module".into());
    }
    atelier_core::application::worktrees::validate_reference(&selection.target)?;
    if selection.title.trim().is_empty()
        || selection.title.encode_utf16().count() > 255
        || selection.title.chars().any(char::is_control)
        || selection.body.encode_utf16().count() > 3500
        || selection
            .body
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err("message.publication_title_description_invalid".into());
    }
    Ok(selection)
}
impl Guard {
    pub async fn before(
        root: &Dir,
        roots: &[PathBuf],
        profile: Installed,
        selection: Selection,
    ) -> Result<Self> {
        let worktree = selection
            .expected
            .fields
            .get::<Worktree>("worktree")?
            .ok_or("message.prepare_the_worktree_before_this_repository_operation")?;
        if selection.target == worktree.branch {
            return Err("message.choose_distinct_target_branch".into());
        }
        let repository = worktrees::inspect(root, roots, &worktree).await?;
        let source = Repository::synthetic(root, roots, worktree.source.as_str())?;
        let disk = disk(&repository, &source).await?;
        url(String::from_utf8(disk.origin.clone())?.trim())?;
        if !repository
            .without_programs(&["status", "--porcelain=v1", "--untracked-files=all"])
            .await?
            .is_empty()
        {
            return Err(
                "message.prepare_a_local_commit_and_keep_the_worktree_clean_before_module".into(),
            );
        }
        Ok(Self {
            selection,
            profile,
            repository,
            source,
            disk,
        })
    }
    pub fn input(&self) -> Result<Value> {
        Ok(
            json!({"cardId":self.selection.expected.id,"worktree":self.selection.expected.fields.get::<Worktree>("worktree")?,"origin":String::from_utf8(self.disk.origin.clone())?.trim(),"source":String::from_utf8(self.disk.head.clone())?.trim(),"target":self.selection.target,"title":self.selection.title,"body":self.selection.body,"draft":self.selection.draft,"pullRequest":self.selection.expected.fields.get::<ModulePullRequestLink>("connectorPullRequest")?.map(|link|link.pull_request)}),
        )
    }
    pub async fn unchanged(&self) -> Result<()> {
        if disk(&self.repository, &self.source).await? != self.disk {
            return Err(
                "message.the_module_changed_the_branch_files_index_fetch_head_or_origin_i".into(),
            );
        }
        Ok(())
    }
    pub async fn observe(&self, value: &Value, review: bool) -> Result<RepositoryProposal> {
        self.unchanged().await?;
        let observation = protocol::repository_observation(value)?;
        scope(&self.profile, &observation)?;
        let origin = String::from_utf8(self.disk.origin.clone())?
            .trim()
            .to_owned();
        let source = String::from_utf8(self.disk.head.clone())?.trim().to_owned();
        if observation.reference.origin != origin
            || observation.target_branch != self.selection.target
        {
            return Err(
                "message.the_module_changed_the_requested_repository_or_target_branch".into(),
            );
        }
        let worktree = self
            .selection
            .expected
            .fields
            .get::<Worktree>("worktree")?
            .ok_or("message.worktree_missing")?;
        if let Some(link) = self
            .selection
            .expected
            .fields
            .get::<ModulePullRequestLink>("connectorPullRequest")?
            && observation.pull_request.as_ref().is_none_or(|pr| {
                pr.reference != link.pull_request.reference || pr.id != link.pull_request.id
            })
        {
            return Err("message.the_module_response_identifies_another_pr_mr".into());
        }
        if let Some(pr) = &observation.pull_request
            && (pr.source_branch != worktree.branch
                || (review && pr.source_commit != source)
                || pr.target_commit != observation.target_commit)
        {
            return Err("message.the_pr_mr_does_not_identify_this_worktree_s_commits".into());
        }
        if review
            && observation
                .pull_request
                .as_ref()
                .is_none_or(|pr| pr.status != "active")
        {
            return Err("message.active_worktree_pr_required".into());
        }
        self.repository
            .git(&[
                "cat-file",
                "-e",
                &format!("{}^{{commit}}", observation.target_commit),
            ])
            .await
            .map_err(|_| "message.target_commit_missing_refresh")?;
        let base = String::from_utf8(
            self.repository
                .git(&["merge-base", "--", &source, &observation.target_commit])
                .await?,
        )?
        .trim()
        .to_owned();
        if !protocol::repository_commit(&base) {
            return Err("message.the_actual_review_merge_base_is_unavailable".into());
        }
        self.unchanged().await?;
        let context = RepositoryReviewContext {
            module: self.profile.manifest.id.clone(),
            revision: self.profile.revision,
            card_id: self.selection.expected.id.clone(),
            requirements: reviews::requirements(&self.selection.expected)?,
            worktree,
            origin,
            source,
            target: observation.target_commit.clone(),
            base,
            fingerprint: self.disk.fingerprint.clone(),
            pull_request: observation.pull_request.clone(),
        };
        Ok(RepositoryProposal {
            automation_id: None,
            context,
            card_revision: revision(&self.selection.expected)?,
            observation,
            title: self.selection.title.clone(),
            body: self.selection.body.clone(),
            draft: self.selection.draft,
        })
    }
}
pub fn revision(card: &Card) -> Result<String> {
    Ok(crate::infrastructure::session::hexadecimal(
        &Sha256::digest(serde_json::to_vec(&serde_json::to_value(
            protocol::repository_source(card),
        )?)?),
    ))
}

pub fn publication_input(input: &Value) -> Value {
    json!({"reference":input["observation"]["reference"],"worktree":input["context"]["worktree"],"source":input["context"]["source"],"target":input["context"]["target"],"targetBranch":input["observation"]["targetBranch"],"remoteSource":input["observation"]["remoteSource"],"title":input["title"],"body":input["body"],"draft":input["draft"],"pullRequest":input["observation"]["pullRequest"]})
}
pub fn prepare_preview(store: &Store<'_>, profile: &Installed, value: &Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Selected {
        expected: Card,
        #[serde(default)]
        report_id: Option<String>,
        #[serde(default)]
        automation_id: Option<String>,
        #[serde(default)]
        approved: Option<RepositoryProposal>,
    }
    let selected: Selected = serde_json::from_str(&value.to_string())?;
    let card = forge_association::snapshot(store, &selected.expected)?;
    let proposal = if let Some(mut approved) = selected.approved {
        let id = selected
            .automation_id
            .ok_or("message.an_approved_workflow_is_required")?;
        let (_, _, plan) = super::automation::publication(store, &card.id, &id)?;
        if !matches!(plan,atelier_core::application::automation::PublicationPlan::Module(ref frozen) if serde_json::to_value(frozen)?==serde_json::to_value(&approved)?)
        {
            return Err("message.the_module_publication_plan_changed".into());
        }
        approved.automation_id = Some(id);
        approved
    } else {
        if selected.automation_id.is_some() {
            return Err("message.the_approved_workflow_preview_is_missing".into());
        }
        let report = profile
            .report
            .as_ref()
            .filter(|report| {
                Some(&report.id) == selected.report_id.as_ref()
                    && report.method == Method::RepositoriesPreview
            })
            .ok_or("message.read_the_repository_preview_again_before_publishing")?;
        let age = time::OffsetDateTime::now_utc()
            - time::OffsetDateTime::parse(
                &report.observed_at,
                &time::format_description::well_known::Rfc3339,
            )?;
        if age > time::Duration::minutes(10) || age < time::Duration::seconds(-60) {
            return Err(
                "message.the_repository_preview_expired_read_it_again_before_publishing".into(),
            );
        }
        protocol::repository_proposal(&report.output)?
    };
    matches(store, &card, &proposal.context)?;
    if proposal.card_revision != revision(&card)? {
        return Err("message.the_task_changed_since_the_publication_preview".into());
    }
    Ok(serde_json::to_value(proposal)?)
}
pub fn publication_owner(
    store: &Store<'_>,
    method: Method,
    input: &Value,
) -> Result<Option<(String, String)>> {
    if method != Method::RepositoriesPublish {
        return Ok(None);
    }
    let Some(operation) = input.get("automationId").and_then(Value::as_str) else {
        return Ok(None);
    };
    let expected: Card = serde_json::from_str(
        &input
            .get("expected")
            .ok_or("message.workflow_card_missing")?
            .to_string(),
    )?;
    let (_, _, plan) = super::automation::publication(store, &expected.id, operation)?;
    let approved = input
        .get("approved")
        .ok_or("message.connector_plan_missing")?;
    if !matches!(plan,atelier_core::application::automation::PublicationPlan::Module(ref frozen) if serde_json::to_value(frozen)?==*approved)
    {
        return Err("message.the_module_publication_plan_changed".into());
    }
    Ok(Some((expected.id, operation.into())))
}
pub fn matches(store: &Store<'_>, card: &Card, context: &RepositoryReviewContext) -> Result<()> {
    connectors::profile(store, &context.module, context.revision)?;
    forge_association::snapshot(store, card)?;
    if context.card_id != card.id
        || context.requirements != reviews::requirements(card)?
        || card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&context.worktree)
    {
        return Err("message.the_requirements_worktree_or_module_connection_changed".into());
    }
    if let Some(link) = card
        .fields
        .get::<ModulePullRequestLink>("connectorPullRequest")?
        && (link.module != context.module
            || link.revision != context.revision
            || Some(&link.pull_request) != context.pull_request.as_ref())
    {
        return Err("message.the_module_pr_mr_changed".into());
    }
    Ok(())
}
pub fn record(store: &Store<'_>, profile: &Installed, report: &protocol::Report) -> Result<()> {
    let proposal = protocol::repository_proposal(&report.output)?;
    if proposal.context.module != profile.manifest.id
        || proposal.context.revision != profile.revision
    {
        return Err("message.the_observation_belongs_to_another_repository_connection".into());
    }
    let mut card = store
        .document::<Card>(Documents::Cards, &proposal.context.card_id)?
        .ok_or("message.card_missing")?;
    forge_association::snapshot(store, &card)?;
    if revision(&card)? != proposal.card_revision {
        return Err("message.the_task_changed_during_the_repository_call".into());
    }
    if report.method == Method::RepositoriesRefresh {
        let pr = proposal
            .observation
            .pull_request
            .clone()
            .ok_or("message.the_module_refresh_is_missing_its_pr_mr")?;
        let previous = card
            .fields
            .get::<ModulePullRequestLink>("connectorPullRequest")?
            .ok_or("message.no_module_pr_mr_is_associated")?;
        if previous.module != profile.manifest.id
            || previous.pull_request.id != pr.id
            || previous.pull_request.reference != pr.reference
        {
            return Err("message.this_refresh_belongs_to_another_pr_mr".into());
        }
        let link = ModulePullRequestLink {
            module: profile.manifest.id.clone(),
            revision: profile.revision,
            pull_request: pr,
        };
        if previous != link {
            card.fields.set("connectorPullRequest", &link)?;
            forge_association::invalidate(
                store,
                &mut card,
                "message.the_module_pr_mr_or_its_commits_changed_run_the_review_again",
            )?;
            store.put(Documents::Cards, &card)?;
        }
    }
    if report.method == Method::RepositoriesPreview {
        card.fields.set("connectorRepositoryPreview", &proposal)?;
        store.put(Documents::Cards, &card)?;
    }
    Ok(())
}
pub fn review_matches(
    store: &Store<'_>,
    card: &Card,
    context: &RepositoryReviewContext,
) -> Result<()> {
    matches(store, card, context)?;
    let link = card
        .fields
        .get::<ModulePullRequestLink>("connectorPullRequest")?
        .ok_or("message.no_module_pr_mr_is_associated")?;
    if Some(&link.pull_request) != context.pull_request.as_ref() {
        return Err("message.the_module_pr_mr_changed".into());
    }
    Ok(())
}
pub fn validate_review(
    store: &Store<'_>,
    card: &Card,
    id: &str,
    context: &RepositoryReviewContext,
    timestamp: &str,
) -> Result<Document> {
    store.transaction(|store| {
        review_matches(store, card, context)?;
        let mut run = store
            .document::<Document>(Documents::Runs, id)?
            .ok_or("message.review_missing")?;
        if !run.string_equals("cardId", &card.id)?
            || !run.string_equals("kind", "pr-review")?
            || !run.string_equals("status", "completed")?
            || run
                .get::<RepositoryReviewContext>("connectorReviewContext")?
                .as_ref()
                != Some(context)
            || run.get::<reviews::Review>("review")?.is_none()
        {
            return Err("message.the_module_review_is_incomplete_or_stale".into());
        }
        run.remove("reviewStaleReason");
        run.set("reviewValidatedAt", &timestamp)?;
        store.put(Documents::Runs, &run)?;
        Ok(run)
    })
}
pub fn correction(
    store: &Store<'_>,
    card: &Card,
    id: &str,
    indexes: &[usize],
) -> Result<(RepositoryReviewContext, String)> {
    let run = store
        .document::<Document>(Documents::Runs, id)?
        .ok_or("message.review_missing")?;
    if !run.string_equals("cardId", &card.id)?
        || !run.string_equals("kind", "pr-review")?
        || !run.string_equals("status", "completed")?
        || run.has_text("reviewStaleReason")?
        || !run.has_text("reviewValidatedAt")?
    {
        return Err("message.select_a_completed_and_revalidated_module_review".into());
    }
    let context = run
        .get::<RepositoryReviewContext>("connectorReviewContext")?
        .ok_or("message.connector_review_context_missing")?;
    review_matches(store, card, &context)?;
    let review = run
        .get::<reviews::Review>("review")?
        .ok_or("message.review_report_missing")?;
    Ok((context, reviews::correction_prompt(&review, indexes)?))
}
pub fn before_send(store: &Store<'_>, operation: &protocol::RemoteOperation) -> Result<()> {
    if operation.method != Method::RepositoriesPublish {
        return Ok(());
    }
    let proposal = protocol::repository_proposal(&operation.input)?;
    if let Some(id) = &proposal.automation_id {
        let (_, _, plan) = super::automation::publication(store, &proposal.context.card_id, id)?;
        let mut frozen = proposal.clone();
        frozen.automation_id = None;
        if !matches!(plan,atelier_core::application::automation::PublicationPlan::Module(ref approved) if serde_json::to_value(approved)?==serde_json::to_value(&frozen)?)
        {
            return Err(
                "message.the_module_s_automatic_publication_was_stopped_or_replaced".into(),
            );
        }
    }
    let card = store
        .document::<Card>(Documents::Cards, &proposal.context.card_id)?
        .ok_or("message.card_missing")?;
    matches(store, &card, &proposal.context)?;
    if revision(&card)? != proposal.card_revision {
        return Err("message.the_task_changed_since_the_publication_preview".into());
    }
    Ok(())
}
pub fn invalidate_connections(
    store: &Store<'_>,
    previous: &Document,
    next: &Document,
) -> Result<()> {
    let old = connectors::profiles(previous)?;
    let new = connectors::profiles(next)?;
    let mut changed = std::collections::BTreeSet::new();
    for before in old {
        if new
            .iter()
            .find(|profile| profile.manifest.id == before.manifest.id)
            .is_none_or(|after| {
                after.revision != before.revision
                    || after.binary != before.binary
                    || after.values != before.values
            })
        {
            changed.insert(before.manifest.id);
        }
    }
    if changed.is_empty() {
        return Ok(());
    }
    for mut card in store.documents::<Card>(Documents::Cards)? {
        let linked = card
            .fields
            .get::<ModulePullRequestLink>("connectorPullRequest")?
            .is_some_and(|link| changed.contains(&link.module));
        let preview = card
            .fields
            .get::<RepositoryProposal>("connectorRepositoryPreview")?
            .is_some_and(|proposal| changed.contains(&proposal.context.module));
        if linked || preview {
            card.fields.remove("connectorRepositoryPreview");
            forge_association::invalidate(
                store,
                &mut card,
                "message.the_module_connection_or_credentials_changed_refresh_the_pr_mr_a",
            )?;
            store.put(Documents::Cards, &card)?;
        }
    }
    Ok(())
}
pub fn synchronize(store: &Store<'_>, operation: &protocol::RemoteOperation) -> Result<()> {
    if operation.method != Method::RepositoriesPublish
        || !matches!(
            operation.state,
            atelier_core::contracts::ForgeWriteState::Confirmed
                | atelier_core::contracts::ForgeWriteState::Reconciled
        )
    {
        return Ok(());
    }
    let proposal = protocol::repository_proposal(&operation.input)?;
    let reply = operation
        .reply
        .as_ref()
        .ok_or("message.remote_observation_missing")?;
    let observation = protocol::repository_observation(
        reply
            .output
            .get("repository")
            .ok_or("message.the_observation_is_missing_the_published_pr_mr")?,
    )?;
    let installed = Installed {
        manifest: operation.manifest.clone(),
        binary: operation.binary.clone(),
        revision: operation.revision,
        values: operation.values.clone(),
        report: None,
    };
    scope(&installed, &observation)?;
    let pr = observation
        .pull_request
        .ok_or("message.the_published_pr_mr_is_missing")?;
    if protocol::remote_receipt(&reply.output, &operation.id)?
        .is_none_or(|receipt| receipt.reference != pr.id)
    {
        return Err("message.the_module_observation_does_not_identify_the_reported_pr_mr".into());
    }
    if observation.reference != proposal.observation.reference
        || pr.source_branch != proposal.context.worktree.branch
        || pr.target_branch != proposal.observation.target_branch
        || pr.source_commit != proposal.context.source
        || pr.target_commit != proposal.context.target
        || pr.draft != proposal.draft
        || pr.auto_merge
        || observation.remote_source.as_deref() != Some(proposal.context.source.as_str())
    {
        return Err("message.the_observed_pr_mr_does_not_match_the_confirmed_publication".into());
    }
    let mut card = store
        .document::<Card>(Documents::Cards, &proposal.context.card_id)?
        .ok_or("message.card_missing")?;
    let link = ModulePullRequestLink {
        module: operation.module.clone(),
        revision: operation.revision,
        pull_request: pr,
    };
    if card
        .fields
        .get::<ModulePullRequestLink>("connectorPullRequest")?
        .as_ref()
        == Some(&link)
    {
        return Ok(());
    }
    if revision(&card)? != proposal.card_revision {
        return Err(
            "message.the_task_changed_reconcile_the_publication_without_overwriting_t".into(),
        );
    }
    card.fields.set("connectorPullRequest", &link)?;
    card.fields.remove("connectorRepositoryPreview");
    forge_association::invalidate(
        store,
        &mut card,
        "message.the_module_pr_mr_changed_review_its_current_commits_again",
    )?;
    store.put(Documents::Cards, &card)
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn revision_ignores_object_layout_but_detects_human_changes() {
        let first:Card=serde_json::from_str(r#"{"id":"synthetic","title":"Human title","description":"Keep","unknown":{"z":1,"a":2}}"#).unwrap();
        let same:Card=serde_json::from_str(r#"{"unknown":{"a":2,"z":1},"description":"Keep","title":"Human title","id":"synthetic"}"#).unwrap();
        assert_eq!(revision(&first).unwrap(), revision(&same).unwrap());
        let mut changed = same;
        changed.fields.set("description", &"Human change").unwrap();
        assert_ne!(revision(&first).unwrap(), revision(&changed).unwrap());
    }

    #[test]
    fn rechecking_an_approved_workflow_ignores_only_its_owned_column_transition() {
        let flow = |state: &str| json!({"id":"synthetic-workflow","state":state,"step":"publication","project":{},"requirements":{},"worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"},"agent":"synthetic","profile":{},"model":"","effort":"","instructions":"","runId":null,"reviewId":null,"verificationId":null,"publicationPlan":null,"commitPlan":null,"reviewRequested":false,"correctionIndexes":[],"cycles":0,"autoFix":false,"startedAt":"synthetic","updatedAt":"synthetic","message":""});
        let mut waiting:Card=serde_json::from_str(r#"{"id":"synthetic","title":"Human title","status":"standby","description":"Keep human text"}"#).unwrap();
        waiting.fields.set("automation", &flow("waiting")).unwrap();
        let mut running = waiting.clone();
        running.fields.set("automation", &flow("running")).unwrap();
        running.fields.set("status", &"doing").unwrap();
        assert_eq!(revision(&waiting).unwrap(), revision(&running).unwrap());
        running.fields.set("status", &"done").unwrap();
        assert_ne!(revision(&waiting).unwrap(), revision(&running).unwrap());
        running.fields.set("status", &"doing").unwrap();
        running.title = "Human edit after approval".into();
        assert_ne!(revision(&waiting).unwrap(), revision(&running).unwrap());
    }
}
