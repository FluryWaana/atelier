use crate::Result;
use std::{io, process::ExitStatus, time::Duration};
use tokio::process::{Child, Command};

// shortcut: process groups do not contain descendants using setsid/setpgid; use OS supervision if daemon isolation is required.
pub struct ContainedChild {
    pub child: Child,
    group: Option<libc::pid_t>,
}
impl ContainedChild {
    pub async fn spawn(command: &mut Command) -> Result<Self> {
        command.process_group(0).kill_on_drop(true);
        let child = command.spawn()?;
        let group = libc::pid_t::try_from(child.id().ok_or("Processus de démarrage absent.")?)?;
        Ok(Self {
            child,
            group: Some(group),
        })
    }
    pub async fn process_ids(&self) -> Result<Vec<u32>> {
        let group = self
            .group
            .ok_or("Le propriétaire du groupe de processus est absent.")?;
        members(group).await
    }
    #[cfg(target_os = "macos")]
    fn exited(&mut self) -> Result<bool> {
        let group = self
            .group
            .ok_or("Le propriétaire du groupe de processus est absent.")?;
        // Darwin's waitid(WNOHANG) can fail its MAC check when no child is ready.
        // Query only our unreaped leader; arg=1 includes zombies, without consuming its exit status.
        let mut information = std::mem::MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdshortinfo>();
        // The kernel writes this POD buffer, checked for its complete ABI size before reading it.
        let written = unsafe {
            libc::proc_pidinfo(
                group,
                libc::PROC_PIDT_SHORTBSDINFO,
                1,
                information.as_mut_ptr().cast(),
                i32::try_from(size)?,
            )
        };
        if written <= 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                self.group = None;
            }
            return Err(error.into());
        }
        if usize::try_from(written)? != size {
            return Err("État du processus propriétaire incomplet.".into());
        }
        // A full successful proc_pidinfo call initialized the fields used below.
        let information = unsafe { information.assume_init() };
        let pid = u32::try_from(group)?;
        if information.pbsi_pid != pid
            || information.pbsi_ppid != std::process::id()
            || information.pbsi_pgid != pid
        {
            return Err("L’identité du groupe propriétaire a changé.".into());
        }
        Ok(information.pbsi_status == libc::SZOMB)
    }
    #[cfg(not(target_os = "macos"))]
    fn exited(&mut self) -> Result<bool> {
        let group = self
            .group
            .ok_or("Le propriétaire du groupe de processus est absent.")?;
        // waitid initializes this POD; si_pid is zero when the owned child has not exited.
        let mut status = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        // WNOWAIT keeps the leader unreaped, preventing its PID/group ID from being reused before cleanup.
        if unsafe {
            libc::waitid(
                libc::P_PID,
                group as libc::id_t,
                &mut status,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                self.group = None;
            }
            return Err(error.into());
        }
        // waitid wrote a valid SIGCHLD siginfo structure on success.
        Ok(unsafe { status.si_pid() } == group)
    }
    fn terminate(&mut self) -> Result<()> {
        let _ = self.exited()?;
        let group = self
            .group
            .ok_or("Le propriétaire du groupe de processus est absent.")?;
        // This unreaped leader was created by us with process_group(0); no external group is selected.
        if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
        }
        Ok(())
    }
    async fn terminate_and_confirm(&mut self) -> Result<()> {
        let result = self.terminate();
        #[cfg(target_os = "macos")]
        if let Err(error) = &result
            && error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.raw_os_error() == Some(libc::EPERM))
        {
            // Darwin can reject the signal during exit before the leader becomes observable as
            // a zombie. Observe that transition, never resubmit the signal or accept a live group.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                if self.exited()? && self.process_ids().await?.is_empty() {
                    return Ok(());
                }
                if tokio::time::Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        result
    }
    pub async fn wait(&mut self) -> Result<ExitStatus> {
        if self.group.is_none() {
            return Ok(self.child.wait().await?);
        }
        while !self.exited()? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.terminate_and_confirm().await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !self.process_ids().await?.is_empty() {
            if tokio::time::Instant::now() >= deadline {
                return Err(
                    "L’arrêt des membres du groupe de processus n’est pas confirmé.".into(),
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let status = self.child.wait().await?;
        self.group = None;
        Ok(status)
    }
    pub async fn stop(&mut self) -> Result<ExitStatus> {
        if self.group.is_none() {
            return Ok(self.child.wait().await?);
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            self.terminate_and_confirm().await?;
            self.wait().await
        })
        .await
        .map_err(|_| "L’arrêt du processus n’est pas confirmé.")?
    }
}

async fn members(group: libc::pid_t) -> Result<Vec<u32>> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    let mut command = Command::new("/bin/ps");
    command
        .args(["-axo", "pid=,pgid=,stat="])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let output = child.stdout.take().ok_or("Sortie d’inspection absente.")?;
    let mut output = output.take(4 * 1024 * 1024 + 1);
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let (_, status) = tokio::try_join!(output.read_to_end(&mut bytes), child.wait())?;
        if !status.success() || bytes.len() > 4 * 1024 * 1024 {
            return Err("Inspection des processus indisponible ou excessive.".into());
        }
        parse_members(&bytes, group)
    })
    .await;
    match result {
        Ok(Ok(ids)) => Ok(ids),
        result => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            match result {
                Ok(Err(error)) => Err(error),
                _ => Err("Délai d’inspection des processus dépassé.".into()),
            }
        }
    }
}

fn invalid_process_number(field: &str, value: &str) -> crate::Error {
    if value.len() <= 20
        && !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        format!("Inspection des processus : {field} invalide ou hors limites ({value}).").into()
    } else {
        format!("Inspection des processus : {field} non numérique ou trop long.").into()
    }
}

fn parse_members(bytes: &[u8], group: libc::pid_t) -> Result<Vec<u32>> {
    let text = std::str::from_utf8(bytes)?;
    let mut members = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index >= 100_000 {
            return Err("Trop de processus pour une inspection complète.".into());
        }
        let mut fields = line.split_whitespace();
        let pid = fields.next().ok_or("PID absent.")?;
        let pid: u32 = pid
            .parse()
            .map_err(|_| invalid_process_number("PID", pid))?;
        let observed = fields.next().ok_or("Groupe absent.")?;
        let observed: libc::pid_t = observed
            .parse()
            .map_err(|_| invalid_process_number("PGID", observed))?;
        let state = fields.next().ok_or("État du processus absent.")?;
        if fields.next().is_some() || pid == 0 || state.is_empty() {
            return Err("Réponse d’inspection des processus invalide.".into());
        }
        if observed == group && !state.starts_with('Z') {
            // A reused PID or a member that moved groups cannot be attributed to this owner.
            if unsafe { libc::getpgid(libc::pid_t::try_from(pid)?) } == group {
                members.push(pid);
            }
        }
    }
    members.sort_unstable();
    members.dedup();
    if members.len() > 4096 {
        return Err("Trop de processus dans le service (4 096 maximum).".into());
    }
    Ok(members)
}
impl Drop for ContainedChild {
    fn drop(&mut self) {
        if self.group.is_some() {
            let _ = self.terminate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }

    #[tokio::test]
    async fn normal_descendants_are_stopped_before_the_leader_is_reaped() {
        let mut command = shell("sleep 60 & printf '%s\\n' \"$!\"; exit 0");
        let mut child = ContainedChild::spawn(&mut command).await.unwrap();
        let group = child.group.unwrap();
        let mut stdout = child.child.stdout.take().unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await.unwrap();
        let pid = std::str::from_utf8(&bytes)
            .unwrap()
            .trim()
            .parse::<libc::pid_t>()
            .unwrap();
        assert_ne!(unsafe { libc::getpgid(pid) }, unsafe { libc::getpgrp() });
        assert!(members(group).await.unwrap().is_empty());
        assert!(child.group.is_none());
        assert!(child.wait().await.unwrap().success());
    }

    #[tokio::test]
    async fn explicit_stop_cleans_the_owned_group_and_does_not_signal_the_parent() {
        let parent = unsafe { libc::getpgrp() };
        let mut command = shell("sleep 60 & wait");
        let mut child = ContainedChild::spawn(&mut command).await.unwrap();
        let group = child.group.unwrap();
        assert_ne!(group, parent);
        assert!(!child.exited().unwrap());
        let ids = child.process_ids().await.unwrap();
        assert!(ids.contains(&u32::try_from(group).unwrap()));
        child.stop().await.unwrap();
        assert!(child.group.is_none());
        assert_eq!(unsafe { libc::getpgrp() }, parent);
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_macos_spontaneous_exit_racing_explicit_stop_still_confirms_cleanup() {
        for _ in 0..16 {
            let mut command = shell("exit 0");
            let mut child = ContainedChild::spawn(&mut command).await.unwrap();
            tokio::task::yield_now().await;
            child.stop().await.unwrap();
            assert!(child.group.is_none());
        }
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_macos_exited_leader_stays_owned_and_unreaped_until_group_cleanup() {
        let mut command = shell("exit 0");
        let mut child = ContainedChild::spawn(&mut command).await.unwrap();
        let group = child.group.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !child.exited().unwrap() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(child.child.id(), Some(u32::try_from(group).unwrap()));
        assert!(child.exited().unwrap());
        assert!(child.wait().await.unwrap().success());
        assert!(child.group.is_none());
    }

    #[test]
    fn malformed_or_overflowed_process_snapshots_are_never_partial_successes() {
        assert!(parse_members(b"1 2 S unexpected\n", 2).is_err());
        assert!(parse_members(b"4294967296 2 S\n", 2).is_err());
        assert!(parse_members(b"1 2\n", 2).is_err());
        assert!(
            parse_members(b"42 4294967295 S\n", 2)
                .unwrap_err()
                .to_string()
                .contains("PGID invalide ou hors limites (4294967295)")
        );
        assert!(
            !invalid_process_number("PID", "synthetic-private-argument")
                .to_string()
                .contains("synthetic-private-argument")
        );
    }
}
