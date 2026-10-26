use super::{database::Database, files::Repository, process_tree::ContainedChild, runs};
use crate::Result;
use atelier_core::{
    application::agent_protocol::Protocol, contracts::AgentAnswer, document::Document,
};
use std::{
    collections::BTreeMap,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{broadcast, mpsc, oneshot, watch},
};

struct Entry {
    card: String,
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
    replies: Option<mpsc::Sender<Reply>>,
}

pub enum RunInput {
    Command(String),
    Agent(Box<Protocol>),
    Verification,
}

pub enum RunDirectory {
    Worktree(Repository),
    Conversation(tempfile::TempDir),
}
impl RunDirectory {
    pub fn conversation() -> Result<Self> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("atelier-brainstorm-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self::Conversation(builder.tempdir()?))
    }
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Worktree(repository) => &repository.path,
            Self::Conversation(directory) => directory.path(),
        }
    }
    fn close(self) -> Result<()> {
        match self {
            Self::Worktree(_) => Ok(()),
            Self::Conversation(directory) => directory.close().map_err(|_| {
                "Le dossier temporaire du brainstorm n’a pas pu être supprimé.".into()
            }),
        }
    }
}

pub(super) struct Reply {
    pub answer: AgentAnswer,
    pub result: oneshot::Sender<std::result::Result<(), &'static str>>,
}

pub struct CommandRuns {
    entries: Mutex<BTreeMap<String, Entry>>,
    changed: broadcast::Sender<()>,
}

fn now() -> Result<String> {
    Ok(time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?)
}

impl CommandRuns {
    pub fn new(changed: broadcast::Sender<()>) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(BTreeMap::new()),
            changed,
        })
    }
    fn entries(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub fn active_cards(&self) -> Vec<String> {
        self.entries()
            .values()
            .filter(|entry| !*entry.done.borrow())
            .map(|entry| entry.card.clone())
            .collect()
    }
    pub async fn start(
        self: &Arc<Self>,
        database: Arc<Database>,
        record: Document,
        mut command: Command,
        input: RunInput,
        directory: RunDirectory,
    ) -> Result<Document> {
        let id = record
            .get::<String>("id")?
            .ok_or("Identifiant d’exécution absent.")?;
        let card = record.get::<String>("cardId")?.ok_or("Carte absente.")?;
        let (stop, cancel) = watch::channel(false);
        let (finished, done) = watch::channel(false);
        let (replies, responses) = mpsc::channel(8);
        let interactive = matches!(&input, RunInput::Agent(_));
        let verification = matches!(&input, RunInput::Verification);
        #[cfg(unix)]
        {
            let opened = match &directory {
                RunDirectory::Worktree(repository) => repository.directory.try_clone()?,
                RunDirectory::Conversation(directory) => cap_std::fs::Dir::open_ambient_dir(
                    directory.path(),
                    cap_std::ambient_authority(),
                )?,
            };
            super::service_launch::anchor(&mut command, &opened)?;
        }
        {
            let mut entries = self.entries();
            entries.retain(|_, entry| !*entry.done.borrow());
            if entries.len() >= 4 {
                return Err("Quatre exécutions sont déjà actives.".into());
            }
            if entries.contains_key(&id) {
                return Err("Identifiant d’exécution déjà utilisé.".into());
            }
            if entries.values().any(|entry| entry.card == card) {
                return Err("Cette tâche a déjà une exécution active.".into());
            }
            entries.insert(
                id.clone(),
                Entry {
                    card,
                    stop,
                    done,
                    replies: interactive.then_some(replies),
                },
            );
        }
        let initial = record.clone();
        if let Err(error) = database
            .call(move |store| runs::reserve(store, &initial))
            .await
        {
            self.entries().remove(&id);
            return Err(error);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let manager = self.clone();
        tokio::spawn(async move {
            // Retain the directory owner until every child has stopped.
            let result = supervise(
                &database,
                &id,
                &mut command,
                input,
                cancel,
                responses,
                &manager.changed,
            )
            .await;
            let result = if verification && result.is_ok() {
                let fingerprint = match &directory {
                    RunDirectory::Worktree(repository) => repository.fingerprint().await.ok(),
                    _ => None,
                };
                let selected = id.clone();
                database
                    .call(move |store| {
                        super::verification::finish(store, &selected, fingerprint.as_deref())
                    })
                    .await
            } else {
                result
            };
            let cleanup = directory.close();
            let result = result.and(cleanup);
            if result.is_ok() {
                let _ = finished.send(true);
            } else {
                let _ = database.call(move |store| {
                    let mut run = store.document::<Document>(super::store::Documents::Runs, &id)?.ok_or("Exécution introuvable.")?;
                    run.set("attention", &"La fin de l’exécution n’est pas confirmée. Redémarre Atelier et vérifie le dépôt avant de relancer.")?;
                    store.put(super::store::Documents::Runs, &run)
                }).await;
            }
            // A storage/cleanup failure keeps the activity reservation until restart.
            let _ = manager.changed.send(());
        });
        Ok(record)
    }
    pub async fn reply(&self, id: &str, answer: AgentAnswer) -> Result<()> {
        let sender = {
            let entries = self.entries();
            let entry = entries
                .get(id)
                .filter(|entry| !*entry.done.borrow() && !*entry.stop.borrow())
                .ok_or("Exécution terminée ou arrêtée.")?;
            entry
                .replies
                .clone()
                .ok_or("Cette commande ne reçoit pas de réponse interactive.")?
        };
        let (result, received) = oneshot::channel();
        sender
            .try_send(Reply { answer, result })
            .map_err(|_| "Le canal de réponse est occupé ou fermé.")?;
        tokio::time::timeout(Duration::from_secs(10), received)
            .await
            .map_err(|_| "La transmission de la réponse n’est pas confirmée.")?
            .map_err(|_| "Le canal de réponse est fermé.")?
            .map_err(Into::into)
    }
    pub async fn stop(&self, id: &str) -> Result<()> {
        let mut done = {
            let entries = self.entries();
            let entry = entries.get(id).ok_or("Exécution active introuvable.")?;
            let _ = entry.stop.send(true);
            entry.done.clone()
        };
        tokio::time::timeout(Duration::from_secs(30), async {
            while !*done.borrow() {
                done.changed().await.map_err(
                    |_| "Arrêt non confirmé. Redémarre Atelier avant de modifier ce dossier.",
                )?;
            }
            Ok::<(), crate::Error>(())
        })
        .await
        .map_err(|_| "L’arrêt de l’exécution n’est pas confirmé.")?
    }
    pub async fn close(&self) -> Result<()> {
        let ids = {
            let entries = self.entries();
            for entry in entries.values() {
                let _ = entry.stop.send(true);
            }
            entries.keys().cloned().collect::<Vec<_>>()
        };
        for id in ids {
            self.stop(&id).await?;
        }
        Ok(())
    }
}

pub(super) async fn lines(
    reader: impl AsyncRead + Unpin,
    stream: &'static str,
    sender: mpsc::Sender<(&'static str, String)>,
) -> Result<()> {
    let mut reader = BufReader::new(reader);
    loop {
        // Bound allocation before looking for a newline, including hostile unterminated output.
        let mut bytes = Vec::new();
        let mut limited = (&mut reader).take(64 * 1024 + 1);
        let count = limited.read_until(b'\n', &mut bytes).await?;
        if count == 0 {
            return Ok(());
        }
        if count > 64 * 1024 {
            return Err("Ligne de sortie trop longue : exécution arrêtée.".into());
        }
        let text = String::from_utf8(bytes).map_err(|_| "Sortie non UTF-8 : exécution arrêtée.")?;
        sender
            .send((stream, text))
            .await
            .map_err(|_| "Journal fermé.")?;
    }
}

async fn supervise(
    database: &Database,
    id: &str,
    command: &mut Command,
    input: RunInput,
    mut cancel: watch::Receiver<bool>,
    replies: mpsc::Receiver<Reply>,
    changed: &broadcast::Sender<()>,
) -> Result<()> {
    let spawned = ContainedChild::spawn(command).await;
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => {
            let id = id.to_owned();
            let timestamp = now()?;
            database.call(move |store| {
                runs::append(store, &id, "system", "Impossible de démarrer la commande. Vérifie son chemin et ses permissions.")?;
                runs::transition(store, &id, "failed", &timestamp, None)?;
                Ok(())
            }).await?;
            return Ok(());
        }
    };
    let stdout = child.child.stdout.take().ok_or("Sortie absente.")?;
    let stderr = child
        .child
        .stderr
        .take()
        .ok_or("Sortie d’erreur absente.")?;
    let mut stdin = child.child.stdin.take().ok_or("Entrée absente.")?;
    let started = id.to_owned();
    let timestamp = now()?;
    if let Err(error) = database
        .call(move |store| runs::transition(store, &started, "running", &timestamp, None))
        .await
    {
        child.stop().await?;
        return Err(error);
    }
    let _ = changed.send(());
    if let RunInput::Agent(protocol) = input {
        return super::interactive_runs::drive(super::interactive_runs::Session {
            database,
            id,
            child,
            stdin,
            stdout,
            stderr,
            protocol: *protocol,
            cancel,
            replies,
            changed,
        })
        .await;
    }
    let (prompt, duration) = match input {
        RunInput::Command(prompt) => (prompt, 3600),
        RunInput::Verification => (String::new(), 600),
        RunInput::Agent(_) => unreachable!(),
    };
    let (sender, mut receiver) = mpsc::channel(16);
    let output = async {
        tokio::try_join!(
            lines(stdout, "stdout", sender.clone()),
            lines(stderr, "stderr", sender)
        )?;
        Ok::<(), crate::Error>(())
    };
    let logs = async {
        while let Some((stream, text)) = receiver.recv().await {
            let id = id.to_owned();
            database
                .call(move |store| runs::append(store, &id, stream, &text))
                .await?;
            let _ = changed.send(());
        }
        Ok::<(), crate::Error>(())
    };
    let operation = async {
        let input = async {
            if !prompt.is_empty() {
                stdin.write_all(prompt.as_bytes()).await?;
                stdin.shutdown().await?;
            }
            drop(stdin);
            Ok::<(), crate::Error>(())
        };
        let (_, (), (), ()) = tokio::try_join!(child.wait(), input, output, logs)?;
        Ok::<(), crate::Error>(())
    };
    // Preserve the actual root exit status separately from successful stream draining.
    let outcome = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(duration), operation) => match result { Ok(result) => result.map(|()| false), Err(_) => Err("Durée maximale de l’exécution atteinte.".into()) },
        _ = async { if !*cancel.borrow() { let _ = cancel.changed().await; } } => Ok(true),
    };
    let cancelled = matches!(outcome, Ok(true));
    if cancelled {
        let id = id.to_owned();
        let timestamp = now()?;
        database
            .call(move |store| runs::transition(store, &id, "stopping", &timestamp, None))
            .await?;
        let _ = changed.send(());
    }
    if cancelled || outcome.is_err() {
        child.stop().await?;
    }
    let code = child.child.try_wait()?.and_then(|status| status.code());
    let id = id.to_owned();
    let timestamp = now()?;
    database
        .call(move |store| {
            if outcome.is_err() {
                runs::append(
                    store,
                    &id,
                    "system",
                    "Exécution arrêtée après une erreur de flux, de stockage ou de délai.",
                )?;
            }
            let status = if cancelled {
                "cancelled"
            } else if outcome.is_ok() && code == Some(0) {
                "completed"
            } else {
                "failed"
            };
            runs::transition(store, &id, status, &timestamp, code)?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_free_conversations_have_unique_empty_directories_with_owned_lifetimes() {
        let first = RunDirectory::conversation().unwrap();
        let path = first.path().to_path_buf();
        let second = RunDirectory::conversation().unwrap();
        assert_ne!(path, second.path());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        drop(first);
        assert!(!path.exists());
        assert!(second.path().exists());
    }

    #[tokio::test]
    async fn output_accepts_split_unicode_and_final_lines_and_refuses_invalid_or_unbounded_lines() {
        let (sender, mut receiver) = mpsc::channel(16);
        let text = format!("{}\nlast", "é😀".repeat(2000));
        lines(text.as_bytes(), "stdout", sender).await.unwrap();
        let first = receiver.recv().await.unwrap();
        let last = receiver.recv().await.unwrap();
        assert_eq!(first.0, "stdout");
        assert_eq!(first.1 + &last.1, text);
        assert!(receiver.recv().await.is_none());
        for bytes in [vec![0xff], vec![b'x'; 64 * 1024 + 1]] {
            let (sender, mut receiver) = mpsc::channel(16);
            assert!(lines(bytes.as_slice(), "stderr", sender).await.is_err());
            assert!(receiver.recv().await.is_none());
        }
    }
}
