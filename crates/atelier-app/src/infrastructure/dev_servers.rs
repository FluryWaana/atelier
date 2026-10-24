use super::{
    launch_ports::PortLease,
    process_tree::ContainedChild,
    service_launch::{PreparedLaunch, WorkingDirectory},
    task_creation::path_string,
};
use crate::Result;
use atelier_core::{
    application::services::{LaunchPlan, compatible},
    contracts::{ServiceSession, ServiceState, ServiceStatus},
};
use std::{
    collections::BTreeMap,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{broadcast, watch},
};

const LOG_LIMIT: usize = 64 * 1024;
const SESSION_LIMIT: usize = 16;

struct Running {
    state: ServiceState,
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}
struct Session {
    plan: LaunchPlan,
    services: Vec<Running>,
}

struct PendingService {
    command: String,
    directory: WorkingDirectory,
    lease: Option<PortLease>,
    cancel: watch::Receiver<bool>,
    done: watch::Sender<bool>,
}
impl Session {
    fn active(&self) -> bool {
        self.services.iter().any(|service| !*service.done.borrow())
    }
    fn snapshot(&self) -> ServiceSession {
        ServiceSession {
            card_id: self.plan.card_id.clone(),
            project_ids: self.plan.project_ids.clone(),
            services: self
                .services
                .iter()
                .map(|service| service.state.clone())
                .collect(),
        }
    }
}

struct Inner {
    sessions: BTreeMap<String, Session>,
}
pub struct DevServers {
    inner: Mutex<Inner>,
    operations: tokio::sync::Mutex<()>,
    changed: broadcast::Sender<()>,
}

impl DevServers {
    pub fn new(changed: broadcast::Sender<()>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                sessions: BTreeMap::new(),
            }),
            operations: tokio::sync::Mutex::new(()),
            changed,
        })
    }
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn snapshot(&self, card: &str) -> Option<ServiceSession> {
        self.inner().sessions.get(card).map(Session::snapshot)
    }
    pub fn active_cards(&self) -> Vec<String> {
        self.inner()
            .sessions
            .values()
            .filter(|session| session.active())
            .map(|session| session.plan.card_id.clone())
            .collect()
    }
    pub fn active_projects(&self) -> Vec<String> {
        self.inner()
            .sessions
            .values()
            .filter(|session| session.active())
            .flat_map(|session| session.plan.project_ids.clone())
            .collect()
    }

    fn update(
        &self,
        card: &str,
        index: usize,
        force: bool,
        update: impl FnOnce(&mut ServiceState),
    ) {
        let mut inner = self.inner();
        if let Some(service) = inner
            .sessions
            .get_mut(card)
            .and_then(|session| session.services.get_mut(index))
        {
            update(&mut service.state);
        }
        if force {
            let _ = self.changed.send(());
        }
    }

    pub async fn start(self: &Arc<Self>, prepared: PreparedLaunch) -> Result<ServiceSession> {
        let _operation = self.operations.lock().await;
        if prepared.plan.services.len() != prepared.directories.len()
            || prepared.plan.services.is_empty()
            || prepared.plan.services.len() > 4
        {
            return Err("Plan de lancement invalide.".into());
        }
        let card = prepared.plan.card_id.clone();
        let (leases, controls) = {
            let mut inner = self.inner();
            if inner
                .sessions
                .values()
                .any(|session| session.active() && !compatible(&prepared.plan, &session.plan))
            {
                return Err("Arrête la pile déjà lancée pour ces projets, ou utilise {port} pour chaque service.".into());
            }
            if !inner.sessions.contains_key(&card) && inner.sessions.len() >= SESSION_LIMIT {
                let stale = inner
                    .sessions
                    .iter()
                    .find(|(_, session)| !session.active())
                    .map(|(id, _)| id.clone())
                    .ok_or("Trop de piles actives (16 maximum).")?;
                inner.sessions.remove(&stale);
            }
            let mut used = inner
                .sessions
                .values()
                .filter(|session| session.active())
                .flat_map(|session| {
                    session
                        .services
                        .iter()
                        .filter(|service| !*service.done.borrow())
                        .filter_map(|service| service.state.port)
                })
                .collect::<Vec<_>>();
            let mut leases = Vec::new();
            let mut controls = Vec::new();
            let mut services = Vec::new();
            for (launch, directory) in prepared.plan.services.iter().zip(&prepared.directories) {
                let lease = if launch.command.contains("{port}") {
                    let mut chosen = None;
                    for _ in 0..32 {
                        if launch.requested_port != 0 && used.contains(&launch.requested_port) {
                            return Err("Ce port est déjà attribué à un service Atelier.".into());
                        }
                        let lease = PortLease::reserve(launch.requested_port)?;
                        if !used.contains(&lease.port) {
                            chosen = Some(lease);
                            break;
                        }
                    }
                    Some(chosen.ok_or("Impossible de réserver un port local.")?)
                } else {
                    None
                };
                let port = lease.as_ref().map(|lease| lease.port);
                if let Some(port) = port {
                    used.push(port);
                }
                let (stop, cancel) = watch::channel(false);
                let (finished, done) = watch::channel(false);
                services.push(Running {
                    state: ServiceState {
                        id: launch.id.clone(),
                        name: launch.name.clone(),
                        directory: path_string(&directory.path).utf8()?,
                        status: ServiceStatus::Starting,
                        pid: None,
                        port,
                        exit_code: None,
                        log: String::new(),
                        error: String::new(),
                    },
                    stop,
                    done,
                });
                leases.push(lease);
                controls.push((cancel, finished));
            }
            inner.sessions.insert(
                card.clone(),
                Session {
                    plan: prepared.plan.clone(),
                    services,
                },
            );
            (leases, controls)
        };
        let _ = self.changed.send(());
        let mut failure = None;
        for (index, (((launch, directory), lease), (cancel, done))) in prepared
            .plan
            .services
            .into_iter()
            .zip(prepared.directories)
            .zip(leases)
            .zip(controls)
            .enumerate()
        {
            if failure.is_some() {
                self.update(&card, index, true, |state| {
                    state.status = ServiceStatus::Stopped
                });
                let _ = done.send(true);
                continue;
            }
            if let Err(error) = self
                .launch(
                    &card,
                    index,
                    PendingService {
                        command: launch.command,
                        directory,
                        lease,
                        cancel,
                        done,
                    },
                )
                .await
            {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            self.stop_inner(&card, None).await?;
            return Err(error);
        }
        self.inner()
            .sessions
            .get(&card)
            .map(Session::snapshot)
            .ok_or_else(|| "Session de services absente.".into())
    }

    async fn launch(
        self: &Arc<Self>,
        card: &str,
        index: usize,
        pending: PendingService,
    ) -> Result<()> {
        let PendingService {
            command,
            directory,
            lease,
            cancel,
            done,
        } = pending;
        let port = lease.as_ref().map(|lease| lease.port);
        let command = port
            .map(|port| command.replace("{port}", &port.to_string()))
            .unwrap_or(command);
        // shortcut: release immediately before spawn; the child must fail if another application wins the port race.
        drop(lease);
        let mut child = match spawn(&command, &directory, port).await {
            Ok(child) => child,
            Err(error) => {
                self.update(card, index, true, |state| {
                    state.status = ServiceStatus::Failed;
                    state.error = error.to_string();
                });
                let _ = done.send(true);
                return Err(error);
            }
        };
        let streams = child.child.stdout.take().zip(child.child.stderr.take());
        let Some((stdout, stderr)) = streams else {
            let pid = child.child.id();
            let terminated = child.stop().await.is_ok();
            self.update(card, index, true, |state| {
                state.status = ServiceStatus::Failed;
                state.pid = if terminated { None } else { pid };
                state.error = "Flux du service absents.".into();
            });
            let _ = done.send(terminated);
            return Err("Flux du service absents.".into());
        };
        self.update(card, index, true, |state| {
            state.status = ServiceStatus::Running;
            state.pid = child.child.id();
        });
        let manager = self.clone();
        let card = card.to_owned();
        tokio::spawn(async move {
            let mut cancel = cancel;
            let monitor = async {
                let result = tokio::select! { result = child.wait() => result, _ = cancel.changed() => child.stop().await };
                if result.is_err() {
                    let _ = child.stop().await;
                }
                result
            };
            let (exit, out, err) = tokio::join!(
                monitor,
                manager.read(&card, index, stdout),
                manager.read(&card, index, stderr)
            );
            let terminated = exit.is_ok();
            manager.update(&card, index, true, |state| {
                match exit {
                    Ok(status) => {
                        state.pid = None;
                        state.exit_code = status.code();
                        state.status = if status.success() || *cancel.borrow() {
                            ServiceStatus::Stopped
                        } else {
                            ServiceStatus::Failed
                        };
                    }
                    Err(error) => {
                        state.status = ServiceStatus::Failed;
                        state.error = error.to_string();
                    }
                }
                if out.is_err() || err.is_err() {
                    state.error = "Lecture des journaux interrompue.".into();
                }
            });
            drop(directory);
            let _ = done.send(terminated);
            let _ = manager.changed.send(());
        });
        Ok(())
    }

    pub async fn restart(self: &Arc<Self>, mut prepared: PreparedLaunch) -> Result<ServiceSession> {
        let _operation = self.operations.lock().await;
        if prepared.plan.services.len() != 1 || prepared.directories.len() != 1 {
            return Err("Sélectionne un seul service à relancer.".into());
        }
        let card = prepared.plan.card_id.clone();
        let launch = prepared.plan.services.remove(0);
        let directory = prepared.directories.remove(0);
        let (index, previous_port) = {
            let inner = self.inner();
            let session = inner
                .sessions
                .get(&card)
                .ok_or("Lance d’abord la pile depuis cette tâche.")?;
            let index = session
                .services
                .iter()
                .position(|service| service.state.id == launch.id)
                .ok_or("Service introuvable.")?;
            let mut prospective = session.plan.clone();
            if let Some(previous) = prospective
                .services
                .iter_mut()
                .find(|service| service.id == launch.id)
            {
                *previous = launch.clone();
            }
            prospective.isolated_ports = prospective
                .services
                .iter()
                .all(|service| service.command.contains("{port}"));
            if inner.sessions.values().any(|other| {
                other.plan.card_id != card
                    && other.active()
                    && !compatible(&prospective, &other.plan)
            }) {
                return Err("Arrête la pile déjà lancée pour ces projets, ou utilise {port} pour chaque service.".into());
            }
            (index, session.services[index].state.port)
        };
        self.stop_inner(&card, Some(&launch.id)).await?;
        let lease = if launch.command.contains("{port}") {
            let requested = if launch.requested_port == 0 {
                previous_port.unwrap_or(0)
            } else {
                launch.requested_port
            };
            if requested != 0
                && self
                    .inner()
                    .sessions
                    .values()
                    .flat_map(|session| &session.services)
                    .any(|service| !*service.done.borrow() && service.state.port == Some(requested))
            {
                return Err("Ce port est déjà attribué à un service Atelier.".into());
            }
            Some(PortLease::reserve(requested)?)
        } else {
            None
        };
        let (stop, cancel) = watch::channel(false);
        let (done, completion) = watch::channel(false);
        {
            let mut inner = self.inner();
            let session = inner
                .sessions
                .get_mut(&card)
                .ok_or("Session de services absente.")?;
            session.services[index] = Running {
                state: ServiceState {
                    id: launch.id.clone(),
                    name: launch.name.clone(),
                    directory: path_string(&directory.path).utf8()?,
                    status: ServiceStatus::Starting,
                    pid: None,
                    port: lease.as_ref().map(|lease| lease.port),
                    exit_code: None,
                    log: String::new(),
                    error: String::new(),
                },
                stop,
                done: completion,
            };
            if let Some(previous) = session
                .plan
                .services
                .iter_mut()
                .find(|service| service.id == launch.id)
            {
                *previous = launch.clone();
            }
            session.plan.isolated_ports = session
                .plan
                .services
                .iter()
                .all(|service| service.command.contains("{port}"));
        }
        self.launch(
            &card,
            index,
            PendingService {
                command: launch.command,
                directory,
                lease,
                cancel,
                done,
            },
        )
        .await?;
        self.snapshot(&card)
            .ok_or_else(|| "Session de services absente.".into())
    }

    async fn read(
        &self,
        card: &str,
        index: usize,
        mut reader: impl AsyncRead + Unpin,
    ) -> Result<()> {
        let mut buffer = [0u8; 4096];
        let mut decoder = Decoder::default();
        loop {
            let count = match reader.read(&mut buffer).await {
                Ok(count) => count,
                Err(error) => {
                    if let Some(service) = self
                        .inner()
                        .sessions
                        .get(card)
                        .and_then(|session| session.services.get(index))
                    {
                        let _ = service.stop.send(true);
                    }
                    return Err(error.into());
                }
            };
            let text = decoder.push(&buffer[..count], count == 0);
            self.update(card, index, false, |state| {
                append_log(&mut state.log, &text)
            });
            if count == 0 {
                return Ok(());
            }
        }
    }

    async fn stop_inner(&self, card: &str, selected: Option<&str>) -> Result<()> {
        let completions = {
            let mut inner = self.inner();
            let session = inner
                .sessions
                .get_mut(card)
                .ok_or("Session de services absente.")?;
            if selected.is_some_and(|id| {
                !session
                    .services
                    .iter()
                    .any(|service| service.state.id == id)
            }) {
                return Err("Service introuvable.".into());
            }
            let mut completions = Vec::new();
            for service in &mut session.services {
                if selected.is_none_or(|id| service.state.id == id) && !*service.done.borrow() {
                    service.state.status = ServiceStatus::Stopping;
                    let _ = service.stop.send(true);
                    completions.push(service.done.clone());
                }
            }
            completions
        };
        let _ = self.changed.send(());
        for mut done in completions {
            tokio::time::timeout(Duration::from_secs(15), async {
                while !*done.borrow() {
                    done.changed()
                        .await
                        .map_err(|_| "Le superviseur du service s’est arrêté sans confirmation.")?;
                }
                Ok::<_, crate::Error>(())
            })
            .await
            .map_err(|_| "L’arrêt du service n’est pas confirmé.")??;
        }
        Ok(())
    }
    pub async fn stop(&self, card: &str, selected: Option<&str>) -> Result<()> {
        let _operation = self.operations.lock().await;
        self.stop_inner(card, selected).await
    }
    pub async fn close(&self) -> Result<()> {
        let _operation = self.operations.lock().await;
        let cards = self.active_cards();
        let mut failure = None;
        for card in cards {
            if let Err(error) = self.stop_inner(&card, None).await {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
}

async fn spawn(
    command: &str,
    directory: &WorkingDirectory,
    port: Option<u16>,
) -> Result<ContainedChild> {
    use std::os::windows::process::CommandExt;
    let system = std::env::var_os("SystemRoot").ok_or("Dossier système Windows absent.")?;
    let mut child =
        tokio::process::Command::new(std::path::PathBuf::from(system).join("System32/cmd.exe"));
    child
        .as_std_mut()
        .raw_arg(format!("/d /s /c \"{command}\""));
    child
        .current_dir(&directory.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["JIRA_TOKEN", "AZURE_DEVOPS_PAT", "TEMPO_TOKEN"] {
        child.env_remove(key);
    }
    if let Some(port) = port {
        child.env("PORT", port.to_string()).env("HOST", "127.0.0.1");
    }
    ContainedChild::spawn(&mut child).await
}

fn append_log(log: &mut String, text: &str) {
    log.push_str(text);
    if log.len() > LOG_LIMIT {
        let mut cutoff = log.len() - LOG_LIMIT;
        while !log.is_char_boundary(cutoff) {
            cutoff += 1;
        }
        log.drain(..cutoff);
    }
}

#[derive(Default)]
struct Decoder(Vec<u8>);
impl Decoder {
    fn push(&mut self, bytes: &[u8], end: bool) -> String {
        self.0.extend_from_slice(bytes);
        let mut result = String::new();
        let mut offset = 0;
        while offset < self.0.len() {
            match std::str::from_utf8(&self.0[offset..]) {
                Ok(text) => {
                    result.push_str(text);
                    offset = self.0.len();
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    result.push_str(
                        std::str::from_utf8(&self.0[offset..offset + valid])
                            .expect("validated UTF-8 prefix"),
                    );
                    offset += valid;
                    if let Some(length) = error.error_len() {
                        result.push('\u{fffd}');
                        offset += length;
                    } else if end {
                        result.push('\u{fffd}');
                        offset = self.0.len();
                    } else {
                        break;
                    }
                }
            }
        }
        self.0.drain(..offset);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atelier_core::application::services::ServiceLaunch;
    use cap_std::{ambient_authority, fs::Dir};

    #[test]
    fn synthetic_service_fixture() {
        let Ok(port) = std::env::var("ATELIER_SERVICE_FIXTURE") else {
            return;
        };
        let port = port.parse::<u16>().unwrap();
        assert_ne!(port, 0);
        let _listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        println!("synthetic-service-ready é😀 <script>fixture()</script>");
        use std::io::Write;
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(120));
    }

    #[tokio::test]
    async fn managed_services_release_ports_stop_independently_and_close_all_children() {
        let fixture = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(fixture.path()).unwrap();
        let command = format!(
            "set \"ATELIER_SERVICE_FIXTURE={{port}}\" && \"{}\" --exact infrastructure::dev_servers::tests::synthetic_service_fixture --nocapture",
            std::env::current_exe().unwrap().display()
        );
        let plan = LaunchPlan {
            card_id: "synthetic-card".into(),
            project_ids: vec!["synthetic-project".into()],
            isolated_ports: true,
            services: ["front", "back"]
                .into_iter()
                .map(|role| ServiceLaunch {
                    id: role.into(),
                    project_id: "synthetic-project".into(),
                    name: role.into(),
                    repository: path.to_string_lossy().into(),
                    worktree: None,
                    directory: String::new(),
                    command: command.clone(),
                    requested_port: 0,
                })
                .collect(),
        };
        let prepared = || PreparedLaunch {
            plan: plan.clone(),
            directories: (0..2)
                .map(|_| WorkingDirectory {
                    path: path.clone(),
                    directory: Dir::open_ambient_dir(&path, ambient_authority()).unwrap(),
                })
                .collect(),
        };
        let manager = DevServers::new(broadcast::channel(16).0);
        let started = manager.start(prepared()).await.unwrap();
        assert_eq!(started.services.len(), 2);
        assert_ne!(started.services[0].port, started.services[1].port);
        assert!(manager.start(prepared()).await.is_err());
        tokio::time::timeout(Duration::from_secs(10), async {
            while !manager
                .snapshot("synthetic-card")
                .unwrap()
                .services
                .iter()
                .all(|service| service.log.contains("synthetic-service-ready é😀"))
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        manager.stop("synthetic-card", Some("front")).await.unwrap();
        let partial = manager.snapshot("synthetic-card").unwrap();
        assert_eq!(partial.services[0].status, ServiceStatus::Stopped);
        assert_eq!(partial.services[1].pid, started.services[1].pid);
        assert_eq!(partial.services[1].status, ServiceStatus::Running);
        let selected = || {
            let mut launch = prepared();
            launch.plan.services.retain(|service| service.id == "front");
            launch.directories.truncate(1);
            launch
        };
        let occupied = PortLease::reserve(started.services[0].port.unwrap()).unwrap();
        assert!(manager.restart(selected()).await.is_err());
        assert_eq!(
            manager.snapshot("synthetic-card").unwrap().services[1].pid,
            started.services[1].pid
        );
        drop(occupied);
        let restarted = manager.restart(selected()).await.unwrap();
        assert_ne!(restarted.services[0].pid, started.services[0].pid);
        assert_eq!(restarted.services[0].port, started.services[0].port);
        assert_eq!(restarted.services[1].pid, started.services[1].pid);
        let again = manager.restart(selected()).await.unwrap();
        assert_ne!(again.services[0].pid, restarted.services[0].pid);
        assert_eq!(again.services[1].pid, started.services[1].pid);
        assert!(
            manager
                .stop("synthetic-card", Some("unknown"))
                .await
                .is_err()
        );
        manager.close().await.unwrap();
        assert!(manager.active_cards().is_empty());
        assert!(manager.active_projects().is_empty());
        for service in started.services {
            let _lease = PortLease::reserve(service.port.unwrap()).unwrap();
        }
    }

    #[test]
    fn log_keeps_utf8_across_chunks_and_bounds_retained_memory() {
        let mut decoder = Decoder::default();
        let bytes = "é😀".as_bytes();
        let mut log = String::new();
        for byte in bytes {
            append_log(&mut log, &decoder.push(&[*byte], false));
        }
        assert_eq!(log, "é😀");
        assert_eq!(decoder.push(&[0xff, 0xe2], false), "�");
        assert_eq!(decoder.push(&[], true), "�");
        append_log(&mut log, &"😀".repeat(LOG_LIMIT));
        assert!(log.len() <= LOG_LIMIT);
        assert!(log.ends_with("😀"));
    }
}
