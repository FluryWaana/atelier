use super::store::{self, Documents, Store};
use crate::Result;
use atelier_core::contracts::Card;
use rusqlite::Connection;
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
enum Command {
    Call(Job),
    Close(oneshot::Sender<()>),
}

pub struct Database {
    sender: Option<mpsc::Sender<Command>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.to_owned();
        let (sender, mut receiver) = mpsc::channel::<Command>(64);
        let (ready, initialized) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("atelier-sqlite".into())
            .spawn(move || {
                let opened = store::open(&path);
                match opened {
                    Ok(mut connection) => {
                        if ready.send(Ok(())).is_err() {
                            return;
                        }
                        while let Some(command) = receiver.blocking_recv() {
                            match command {
                                Command::Call(job) => job(&mut connection),
                                Command::Close(reply) => {
                                    drop(connection);
                                    let _ = reply.send(());
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            })?;
        let initialized = initialized
            .recv()
            .map_err(|error| -> crate::Error { error.into() })
            .and_then(|result| result);
        if let Err(error) = initialized {
            drop(sender);
            let _ = worker.join();
            return Err(error);
        }
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    pub async fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&Store<'_>) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (reply, result) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or("Stockage fermé.")?
            .send(Command::Call(Box::new(move |connection| {
                let _ = reply.send(operation(&Store::new(connection)));
            })))
            .await
            .map_err(|_| "Stockage fermé.")?;
        result.await.map_err(|_| "Opération SQLite interrompue.")?
    }

    pub async fn cards(&self) -> Result<Vec<Card>> {
        self.call(|store| {
            store
                .documents(Documents::Cards)?
                .into_iter()
                .map(|card| Ok(serde_json::from_value(card)?))
                .collect()
        })
        .await
    }

    pub async fn put_card(&self, card: Card) -> Result<Card> {
        self.call(move |store| {
            store.put(Documents::Cards, &serde_json::to_value(&card)?)?;
            Ok(card)
        })
        .await
    }

    pub async fn close(&self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or("Stockage fermé.")?
            .send(Command::Close(reply))
            .await
            .map_err(|_| "Stockage fermé.")?;
        result.await?;
        Ok(())
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        // Drain accepted work and close SQLite before the caller can release its directory lock.
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
