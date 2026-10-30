use super::{card_lifecycle::CardFailure, worktrees::ServiceWorktrees};
use crate::Result;
use crate::{
    contracts::{Card, Worktree, WorktreeState},
    document::Document,
    domain::{files::validate_path, projects::same_repository},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLaunch {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub repository: String,
    pub worktree: Option<Worktree>,
    pub directory: String,
    pub command: String,
    pub requested_port: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub card_id: String,
    pub project_ids: Vec<String>,
    pub services: Vec<ServiceLaunch>,
    pub isolated_ports: bool,
}

pub fn directory(value: &str) -> Result<String> {
    let invalid = "message.the_service_folder_must_be_relative_and_stay_inside_the_worktree";
    if value.len() > 2000
        || value.starts_with(['/', '\\'])
        || value.contains([':', '\0', '\r', '\n'])
    {
        return Err(invalid.into());
    }
    let mut parts = Vec::new();
    for part in value.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or(invalid)?;
            }
            _ => {
                validate_path(part, true).map_err(|_| invalid)?;
                parts.push(part);
            }
        }
    }
    Ok(parts.join("/"))
}

pub fn plan(
    card: &Card,
    projects: &[Document],
    standalone: bool,
    windows: bool,
) -> Result<LaunchPlan> {
    if card.fields.flag("archived")? {
        return Err(CardFailure::Conflict("message.restore_this_task_before_resuming_work").into());
    }
    let primary = card
        .fields
        .get::<Worktree>("worktree")?
        .ok_or("message.prepare_worktree_before_project")?;
    let repo = card.fields.get::<String>("repo")?.unwrap_or_default();
    if primary.state != WorktreeState::Ready || !same_repository(&primary.source, &repo, windows) {
        return Err("message.verify_primary_worktree_before_services".into());
    }
    let owned = card
        .fields
        .get::<ServiceWorktrees>("serviceWorktrees")?
        .unwrap_or_default();
    let project = projects
        .iter()
        .find(|project| {
            project
                .get::<String>("repo")
                .ok()
                .flatten()
                .is_some_and(|source| same_repository(&source, &repo, windows))
        })
        .ok_or("message.save_project_commands_before_launch")?;
    let id = project
        .get::<String>("id")?
        .ok_or("message.project_id_missing")?;
    let mut selected = vec![project];
    let linked = project
        .get::<String>("linkedProjectId")?
        .unwrap_or_default();
    if !standalone && !linked.is_empty() {
        if linked == id {
            return Err("message.project_self_link_forbidden".into());
        }
        selected.push(
            projects
                .iter()
                .find(|project| project.string_equals("id", &linked).unwrap_or(false))
                .ok_or("message.linked_project_missing")?,
        );
    }
    let mut result = LaunchPlan {
        card_id: card.id.clone(),
        project_ids: Vec::new(),
        services: Vec::new(),
        isolated_ports: true,
    };
    for project in selected {
        let project_id = project
            .get::<String>("id")?
            .ok_or("message.project_id_missing")?;
        let name = project
            .get::<String>("name")?
            .unwrap_or_else(|| project_id.clone());
        let main = project_id == id;
        let source = project
            .get::<String>("repo")?
            .ok_or("message.missing_project_repository")?;
        let before = result.services.len();
        for (role, label) in [("front", "Front"), ("back", "Back")] {
            let command = project
                .get::<String>(&format!("{role}Command"))?
                .unwrap_or_default();
            if command.trim().is_empty() {
                continue;
            }
            if command.len() > 8000 || command.contains('\0') {
                return Err("message.launch_command_invalid".into());
            }
            let dedicated = project
                .get::<String>(&format!("{role}Repo"))?
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| source.clone());
            let worktree = if main {
                if same_repository(&dedicated, &primary.source, windows) {
                    Some(primary.clone())
                } else {
                    let service = owned.get(role).ok_or("message.prepare_service_worktrees")?;
                    if service.state != WorktreeState::Ready
                        || service.branch != primary.branch
                        || !same_repository(&service.source, &dedicated, windows)
                    {
                        return Err("message.verify_service_worktrees".into());
                    }
                    Some(service.clone())
                }
            } else {
                if !same_repository(&dedicated, &source, windows) {
                    return Err("message.linked_project_repositories_separate".into());
                }
                None
            };
            let port = project.get::<u16>(&format!("{role}Port"))?.unwrap_or(0);
            let isolated = command.contains("{port}");
            if port != 0 && !isolated {
                return Err(
                    "message.add_port_to_the_command_to_apply_the_port_provided_by_atelier".into(),
                );
            }
            result.isolated_ports &= isolated;
            result.services.push(ServiceLaunch {
                id: format!("{project_id}:{role}"),
                project_id: project_id.clone(),
                name: format!("{name} · {label}"),
                repository: dedicated,
                worktree,
                directory: directory(
                    &project
                        .get::<String>(&format!("{role}Directory"))?
                        .unwrap_or_default(),
                )?,
                command,
                requested_port: port,
            });
        }
        if before == result.services.len() {
            return Err(format!("Renseigne les commandes Front/Back du projet {name}.").into());
        }
        result.project_ids.push(project_id);
    }
    Ok(result)
}

pub fn compatible(next: &LaunchPlan, active: &LaunchPlan) -> bool {
    next.card_id != active.card_id
        && (!next
            .project_ids
            .iter()
            .any(|id| active.project_ids.contains(id))
            || (next.isolated_ports && active.isolated_ports))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn launch_plan_is_complete_before_any_process_and_checks_owned_service_roots() {
        let primary = Worktree {
            path: "/work/main".into(),
            source: "/repo".into(),
            branch: "task".into(),
            base: "HEAD".into(),
            base_commit: "a".repeat(40),
            state: WorktreeState::Ready,
        };
        let service = Worktree {
            path: "/work/service".into(),
            source: "/service".into(),
            ..primary.clone()
        };
        let mut card: Card = serde_json::from_str(&json!({"id":"task", "title":"Synthetic", "repo":"/repo", "worktree":primary,"serviceWorktrees":{"front":service}}).to_string()).unwrap();
        let projects: Vec<Document> = serde_json::from_str(&json!([
            {"id":"main","name":"Main","repo":"/repo","frontRepo":"/service","frontCommand":"synthetic --port {port}","frontDirectory":"client/../web","linkedProjectId":"linked"},
            {"id":"linked","name":"Linked","repo":"/linked","backCommand":"synthetic --port {port}"}
        ]).to_string()).unwrap();
        let all = plan(&card, &projects, false, false).unwrap();
        assert_eq!(all.project_ids, ["main", "linked"]);
        assert_eq!(all.services.len(), 2);
        assert_eq!(
            all.services[0].worktree.as_ref().unwrap().path,
            "/work/service"
        );
        assert_eq!(all.services[0].directory, "web");
        assert!(all.services[1].worktree.is_none());
        assert_eq!(all.services[1].repository, "/linked");
        assert_eq!(
            plan(&card, &projects, true, false).unwrap().services.len(),
            1
        );
        assert!(plan(&card, &projects[..1], false, false).is_err());
        assert!(!compatible(&all, &all));
        let mut another = all.clone();
        another.card_id = "another".into();
        assert!(compatible(&another, &all));
        another.isolated_ports = false;
        assert!(!compatible(&another, &all));
        card.fields.remove("serviceWorktrees");
        assert!(plan(&card, &projects, false, false).is_err());
        for path in [
            "../outside",
            "a/../../outside",
            "/absolute",
            "C:\\path",
            ".git",
            "a:stream",
            "NUL",
        ] {
            assert!(directory(path).is_err(), "{path}");
        }
        assert_eq!(directory(".\\client\\..\\web").unwrap(), "web");
    }
}
