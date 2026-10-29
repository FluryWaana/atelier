use super::{
    files::Repository,
    forge, forge_association,
    git_transport::HttpsAuthentication,
    store::{Documents, Store},
    worktrees,
};
use crate::Result;
use atelier_core::{
    application::reviews::{self, Context, Review},
    contracts::{Card, ForgeConfiguration, PullRequest},
    document::{Document, JsonString},
};
use cap_std::fs::Dir;
use std::path::PathBuf;

pub const UNCHECKED: &str =
    "message.review_has_not_been_revalidated_after_execution_check_current_co";

fn commit(value: Option<&str>) -> Result<&str> {
    value
        .filter(|value| {
            [40, 64].contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or_else(|| "message.pr_mr_commits_are_unavailable_refresh_the_pr_mr_before_reviewing".into())
}
async fn clean(repository: &Repository, source: &str) -> Result<()> {
    if !super::local_commit::disk_matches_commit(repository, source).await? {
        return Err("message.publish_changes_and_keep_the_worktree_clean_at_the_pr_mr_commit".into());
    }
    Ok(())
}

/// The caller owns the exclusive mutation reservation throughout network and Git operations.
pub async fn prepare(
    root: &Dir,
    roots: &[PathBuf],
    card: &Card,
    configuration: ForgeConfiguration,
    token: String,
) -> Result<Context> {
    let linked = card
        .fields
        .get::<PullRequest>("pullRequest")?
        .ok_or("message.a_pr_mr_and_its_worktree_are_required_for_review")?;
    let forge = forge::Forge::new(configuration.clone(), token.clone(), linked.revision)?;
    let authentication =
        HttpsAuthentication::new(configuration.clone(), linked.repository.clone(), token)?;
    prepare_with_adapters(root, roots, card, configuration, &forge, &authentication).await
}

/// Explicit adapter composition, retaining the same worktree, remote and commit checks.
/// The caller owns the exclusive mutation reservation throughout this operation.
pub async fn prepare_with_adapters(
    root: &Dir,
    roots: &[PathBuf],
    card: &Card,
    configuration: ForgeConfiguration,
    forge: &forge::Forge,
    authentication: &HttpsAuthentication,
) -> Result<Context> {
    let linked = card
        .fields
        .get::<PullRequest>("pullRequest")?
        .ok_or("message.a_pr_mr_and_its_worktree_are_required_for_review")?;
    if configuration.kind != linked.kind || configuration.base != linked.base_url {
        return Err("message.the_pr_mr_connection_changed_refresh_its_link_before_reviewing".into());
    }
    let repository =
        forge_association::verify(root, roots, card, &configuration, &linked.repository).await?;
    let remote = forge
        .pull_request(&linked.repository, linked.number)
        .await?;
    if remote.kind != configuration.kind
        || remote.base_url != configuration.base
        || remote.revision != linked.revision
        || remote.repository != linked.repository
        || remote.number != linked.number
    {
        return Err("message.review_connection_scope_changed".into());
    }
    if remote.fork
        || remote.status != "active"
        || remote.source_branch
            != card
                .fields
                .get::<atelier_core::contracts::Worktree>("worktree")?
                .ok_or("message.pr_mr_worktree_missing")?
                .branch
    {
        return Err("message.review_requires_an_open_pr_mr_from_this_branch_without_a_fork".into());
    }
    let source = commit(remote.source_commit.as_deref())?.to_owned();
    let target = commit(remote.target_commit.as_deref())?.to_owned();
    clean(&repository, &source).await?;
    let worktree = card
        .fields
        .get::<atelier_core::contracts::Worktree>("worktree")?
        .ok_or("message.pr_mr_worktree_missing")?;
    let principal =
        Repository::synthetic(root, roots, &JsonString::from(worktree.source.as_str()))?;
    principal.verify_root().await?;
    // The target is fetched by branch and must equal the forge's advertised commit.
    let fetched = worktrees::fetch_origin_commit(
        root,
        roots,
        &principal,
        &format!("refs/heads/{}", remote.target_branch),
        Some(authentication),
    )
    .await?;
    if fetched != target {
        return Err(
            "message.the_pr_mr_target_changed_refresh_the_pr_mr_before_reviewing_agai".into(),
        );
    }
    let base = String::from_utf8(
        repository
            .git(&["merge-base", "--", &source, &target])
            .await?,
    )?
    .trim()
    .to_owned();
    commit(Some(&base))?;
    let checked =
        forge_association::verify(root, roots, card, &configuration, &linked.repository).await?;
    clean(&checked, &source).await?;
    Ok(Context {
        requirements: reviews::requirements(card)?,
        worktree,
        kind: linked.kind,
        revision: linked.revision,
        repository: linked.repository,
        number: linked.number,
        source,
        target,
        base,
    })
}

pub fn matches(store: &Store<'_>, card: &Card, context: &Context) -> Result<()> {
    forge::require_revision(store, context.revision)?;
    forge_association::snapshot(store, card)?;
    let linked = card
        .fields
        .get::<PullRequest>("pullRequest")?
        .ok_or("message.pr_mr_link_removed")?;
    if reviews::requirements(card)? != context.requirements
        || card
            .fields
            .get::<atelier_core::contracts::Worktree>("worktree")?
            .as_ref()
            != Some(&context.worktree)
        || linked.kind != context.kind
        || linked.repository != context.repository
        || linked.number != context.number
        || linked.revision != context.revision
    {
        return Err("message.the_reviewed_requirements_worktree_or_pr_mr_changed".into());
    }
    Ok(())
}

pub fn validate(
    store: &Store<'_>,
    card: &Card,
    id: &str,
    context: &Context,
    timestamp: &str,
) -> Result<Document> {
    store.transaction(|store| {
        matches(store, card, context)?;
        let mut run = store
            .document::<Document>(Documents::Runs, id)?
            .ok_or("message.review_not_found")?;
        if !run.string_equals("cardId", &card.id)?
            || !run.string_equals("kind", "pr-review")?
            || !run.string_equals("status", "completed")?
            || run.get::<Context>("reviewContext")?.as_ref() != Some(context)
            || run.get::<Review>("review")?.is_none()
        {
            return Err("message.review_is_incomplete_or_stale_run_it_again".into());
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
) -> Result<(Context, String)> {
    let run = store
        .document::<Document>(Documents::Runs, id)?
        .ok_or("message.review_not_found")?;
    if !run.string_equals("cardId", &card.id)?
        || !run.string_equals("kind", "pr-review")?
        || !run.string_equals("status", "completed")?
        || run.truthy("reviewStaleReason")?
        || !run.truthy("reviewValidatedAt")?
    {
        return Err(
            "message.choose_a_completed_and_revalidated_review_before_correcting_its".into(),
        );
    }
    let context = run
        .get::<Context>("reviewContext")?
        .ok_or("message.missing_review_context")?;
    matches(store, card, &context)?;
    let review = run
        .get::<Review>("review")?
        .ok_or("message.review_is_incomplete_or_stale_run_it_again")?;
    Ok((context, reviews::correction_prompt(&review, indexes)?))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    async fn git(path: &std::path::Path, arguments: &[&str]) -> Vec<u8> {
        let mut command = tokio::process::Command::new("git");
        command
            .current_dir(path)
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .args([
                "-c",
                "core.hooksPath=NUL",
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
            ])
            .args(arguments);
        let output = super::super::process_tree::capture(
            &mut command,
            &[],
            std::time::Duration::from_secs(15),
            1024 * 1024,
        )
        .await
        .unwrap();
        assert!(output.status.success());
        output.stdout
    }
    #[tokio::test]
    async fn review_compares_disk_to_commit_and_preserves_a_different_human_index() {
        let fixture = tempfile::tempdir().unwrap();
        let root_path = fixture.path().canonicalize().unwrap();
        let path = root_path.join("repositories/review");
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-b", "main"]).await;
        std::fs::write(path.join("tracked"), "original").unwrap();
        git(&path, &["add", "."]).await;
        git(&path, &["commit", "-qm", "Synthetic baseline"]).await;
        let head = String::from_utf8(git(&path, &["rev-parse", "HEAD"]).await)
            .unwrap()
            .trim()
            .to_owned();
        let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
        let repository = Repository::synthetic(
            &root,
            &[root_path],
            &JsonString::from(path.to_str().unwrap()),
        )
        .unwrap();
        clean(&repository, &head).await.unwrap();
        assert!(clean(&repository, &"f".repeat(40)).await.is_err());
        std::fs::write(path.join("tracked"), "human staged").unwrap();
        git(&path, &["add", "tracked"]).await;
        let index = std::fs::read(path.join(".git/index")).unwrap();
        assert!(clean(&repository, &head).await.is_err());
        std::fs::write(path.join("tracked"), "human unstaged").unwrap();
        assert!(clean(&repository, &head).await.is_err());
        assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(path.join("tracked")).unwrap(),
            "human unstaged"
        );
        std::fs::write(path.join("tracked"), "original").unwrap();
        clean(&repository, &head).await.unwrap();
        assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
        git(&path, &["add", "tracked"]).await;
        clean(&repository, &head).await.unwrap();
        std::fs::write(path.join("untracked"), "human untracked").unwrap();
        assert!(clean(&repository, &head).await.is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("untracked")).unwrap(),
            "human untracked"
        );
    }
}
