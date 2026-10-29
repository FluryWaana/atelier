#[cfg(any(windows, test))]
use super::task_creation::path_string;
use super::{checked_file::require_plain, launch_detection, worktrees};
use crate::Result;
use atelier_core::{application::services::LaunchPlan, document::JsonString};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::path::PathBuf;

pub struct PreparedLaunch {
    pub plan: LaunchPlan,
    pub directories: Vec<WorkingDirectory>,
}

pub struct WorkingDirectory {
    pub path: PathBuf,
    // Keep the anchored directory alive until its process stops, preventing Windows relocation.
    pub directory: Dir,
}

#[cfg(windows)]
pub(crate) async fn shell(command: &str) -> Result<tokio::process::Command> {
    use std::os::windows::process::CommandExt;
    if command.trim().is_empty() || command.len() > 8000 || command.contains('\0') {
        return Err("message.invalid_verification_command".into());
    }
    let system = std::env::var_os("SystemRoot").ok_or("Dossier système Windows absent.")?;
    let mut profile = atelier_core::document::Document::default();
    profile.set("type", &"command")?;
    profile.set(
        "command",
        &path_string(&PathBuf::from(system).join("System32/cmd.exe")).utf8()?,
    )?;
    let mut child = super::agent_diagnostics::command(&profile).await?;
    child
        .as_std_mut()
        .raw_arg(format!("/d /s /c \"{command}\""));
    Ok(child)
}

#[cfg(unix)]
pub(crate) async fn shell(command: &str) -> Result<tokio::process::Command> {
    if command.trim().is_empty() || command.len() > 8000 || command.contains('\0') {
        return Err("message.invalid_verification_command".into());
    }
    let mut profile = atelier_core::document::Document::default();
    profile.set("type", &"command")?;
    profile.set("command", &"/bin/sh")?;
    let mut child = super::agent_diagnostics::command(&profile).await?;
    child.args(["-c", command]);
    Ok(child)
}

#[cfg(unix)]
pub(crate) fn anchor(command: &mut tokio::process::Command, directory: &Dir) -> Result<()> {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    let opened = directory.try_clone()?;
    command.current_dir("/");
    // Only async-signal-safe fchdir runs after fork; the closure retains the approved directory FD.
    unsafe {
        command.as_std_mut().pre_exec(move || {
            if libc::fchdir(opened.as_raw_fd()) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    Ok(())
}

pub async fn prepare(root: &Dir, roots: &[PathBuf], plan: LaunchPlan) -> Result<PreparedLaunch> {
    let mut directories = Vec::new();
    for service in &plan.services {
        let (path, directory) = if let Some(worktree) = &service.worktree {
            let repository = worktrees::inspect(root, roots, worktree).await?;
            (repository.path, repository.directory)
        } else {
            let selected = launch_detection::synthetic_repository(
                root,
                roots,
                &serde_json::value::to_raw_value(&JsonString::from(service.repository.as_str()))?,
            )?;
            let path = PathBuf::from(selected.utf8()?);
            let directory = launch_detection::open_repository(root, roots, &selected)?;
            (path, directory)
        };
        let relative = atelier_core::application::services::directory(&service.directory)?;
        let mut opened = directory;
        let mut path = path;
        for part in relative.split('/').filter(|part| !part.is_empty()) {
            require_plain(&opened.symlink_metadata(part)?)?;
            opened = opened.open_dir_nofollow(part)?;
            path.push(part);
        }
        directories.push(WorkingDirectory {
            path,
            directory: opened,
        });
    }
    Ok(PreparedLaunch { plan, directories })
}

#[cfg(test)]
mod tests {
    use super::*;
    use atelier_core::application::services::ServiceLaunch;

    #[tokio::test]
    async fn preparation_anchors_all_companion_directories_before_returning() {
        let fixture = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(fixture.path()).unwrap();
        std::fs::create_dir_all(path.join("repositories/linked/api")).unwrap();
        let root = Dir::open_ambient_dir(&path, cap_std::ambient_authority()).unwrap();
        let plan = LaunchPlan {
            card_id: "synthetic".into(),
            project_ids: vec!["linked".into()],
            isolated_ports: false,
            services: vec![ServiceLaunch {
                id: "linked:back".into(),
                project_id: "linked".into(),
                name: "Linked · Back".into(),
                repository: path_string(&path.join("repositories/linked"))
                    .utf8()
                    .unwrap(),
                worktree: None,
                directory: "api".into(),
                command: "never execute this fixture".into(),
                requested_port: 0,
            }],
        };
        let ready = prepare(&root, std::slice::from_ref(&path), plan.clone())
            .await
            .unwrap();
        assert_eq!(
            std::fs::canonicalize(&ready.directories[0].path).unwrap(),
            path.join("repositories/linked/api")
        );
        drop(ready);
        let mut invalid = plan.clone();
        invalid.services.push(ServiceLaunch {
            directory: "missing".into(),
            ..plan.services[0].clone()
        });
        assert!(
            prepare(&root, std::slice::from_ref(&path), invalid)
                .await
                .is_err()
        );
        let mut outside = plan;
        outside.services[0].directory = "../../../outside".into();
        assert!(
            prepare(&root, std::slice::from_ref(&path), outside)
                .await
                .is_err()
        );
    }
}
