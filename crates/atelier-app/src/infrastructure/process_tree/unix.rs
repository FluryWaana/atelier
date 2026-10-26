use crate::Result;
use std::{io, process::ExitStatus, time::Duration};
use tokio::process::{Child, Command};

// Only bounded native tools use this adapter; process groups cannot contain agents that call setsid.
pub(super) struct ContainedChild {
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
    pub async fn wait(&mut self) -> Result<ExitStatus> {
        while !self.exited()? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.terminate()?;
        let status = self.child.wait().await?;
        self.group = None;
        Ok(status)
    }
    pub async fn stop(&mut self) -> Result<ExitStatus> {
        if self.group.is_none() {
            return Ok(self.child.wait().await?);
        }
        self.terminate()?;
        tokio::time::timeout(Duration::from_secs(10), self.wait())
            .await
            .map_err(|_| "L’arrêt du processus n’est pas confirmé.")?
    }
}
impl Drop for ContainedChild {
    fn drop(&mut self) {
        if self.group.is_some() {
            let _ = self.terminate();
        }
    }
}
