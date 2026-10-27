use atelier_core::application::extensions::{self as protocol, Method, Reply, Request};
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf};
fn root(request: &Request) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let root = std::fs::canonicalize(
        request
            .settings
            .get("fixtureRoot")
            .and_then(Value::as_str)
            .ok_or("Synthetic repository root missing")?,
    )?;
    if std::fs::read_to_string(root.join(".extension-fixture"))? != "atelier-r04-synthetic-only" {
        return Err("Synthetic repository marker missing".into());
    }
    Ok(root)
}
pub fn observation(request: &Request) -> Result<Value, Box<dyn std::error::Error>> {
    let root = root(request)?;
    let observation: Value =
        serde_json::from_slice(&std::fs::read(root.join("repository-observation.json"))?)?;
    protocol::repository_observation(&observation)?;
    if request.input["origin"] != observation["reference"]["origin"]
        || request.input["target"] != observation["targetBranch"]
    {
        return Err("Synthetic repository input scope changed".into());
    }
    let path = std::fs::canonicalize(
        request.input["worktree"]["path"]
            .as_str()
            .ok_or("Synthetic worktree absent")?,
    )?;
    if !path.starts_with(&root)
        || std::fs::read_to_string(path.join(".review-fixture"))? != "atelier-review-synthetic-only"
    {
        return Err("Synthetic worktree marker missing".into());
    }
    if request.settings.get("fault").and_then(Value::as_str) == Some("repository-dirty") {
        std::fs::write(
            path.join("src/lib.rs"),
            "Synthetic forbidden module mutation\n",
        )?;
    }
    Ok(observation)
}
pub fn effect(request: &Request, reply: &mut Reply) -> Result<bool, Box<dyn std::error::Error>> {
    if request.method != Method::RepositoriesPublish
        && !(request.method == Method::Reconcile
            && request.input["method"] == "repositoriesPublish")
    {
        return Ok(false);
    }
    let root = root(request)?;
    let operation = request
        .operation_id
        .as_deref()
        .filter(|id| protocol::identifier(id))
        .ok_or("Synthetic publication identity missing")?;
    let input = if request.method == Method::RepositoriesPublish {
        &request.input
    } else {
        &request.input["input"]
    };
    let proposal = protocol::repository_publication(input)?;
    let marker = root.join(format!("{operation}.repository-effect"));
    if request.method == Method::RepositoriesPublish {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&marker)?;
        file.write_all(b"one synthetic publication")?;
        file.sync_all()?;
        let pr = protocol::ModulePullRequest {
            reference: proposal.reference.clone(),
            id: "7".into(),
            url: format!(
                "{}/owner/repo/pull/7",
                proposal.reference.instance.trim_end_matches('/')
            ),
            source_branch: proposal.worktree.branch.clone(),
            target_branch: proposal.target_branch.clone(),
            source_commit: proposal.source.clone(),
            target_commit: proposal.target.clone(),
            status: "active".into(),
            draft: proposal.draft,
            auto_merge: false,
        };
        let observed = protocol::RepositoryObservation {
            reference: proposal.reference.clone(),
            target_branch: proposal.target_branch.clone(),
            target_commit: proposal.target.clone(),
            remote_source: Some(proposal.source.clone()),
            pull_request: Some(pr),
        };
        std::fs::write(
            root.join("repository-observation.json"),
            serde_json::to_vec(&observed)?,
        )?;
        if request.settings.get("fault").and_then(Value::as_str) == Some("repository-lost") {
            std::io::stdout().write_all(b"synthetic lost publication reply")?;
            return Ok(true);
        }
    } else if !marker.is_file() {
        return Err("Synthetic publication effect absent".into());
    }
    let observation: Value =
        serde_json::from_slice(&std::fs::read(root.join("repository-observation.json"))?)?;
    reply.output = json!({"repository":observation,"receipt":{"operationId":operation,"outcome":"applied","reference":"7","observedAt":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?}});
    Ok(false)
}
