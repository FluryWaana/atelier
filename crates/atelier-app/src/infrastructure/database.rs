use crate::Result;
use atelier_core::contracts::Card;
use rusqlite::{Connection, params};
use std::path::Path;
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
enum Command {
    Call(Job),
    Close(oneshot::Sender<()>),
}

pub struct Database {
    sender: mpsc::Sender<Command>,
}

impl Database {
    // shortcut: M01 persists cards only; port the complete schema and recovery contract in M02.
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.to_owned();
        let (sender, mut receiver) = mpsc::channel::<Command>(64);
        let (ready, initialized) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new().name("atelier-sqlite".into()).spawn(move || {
            let opened = Connection::open(path).and_then(|connection| {
                connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS cards (id TEXT PRIMARY KEY, data TEXT NOT NULL);")?;
                Ok(connection)
            });
            match opened {
                Ok(mut connection) => {
                    if ready.send(Ok(())).is_err() { return; }
                    while let Some(command) = receiver.blocking_recv() {
                        match command {
                            Command::Call(job) => job(&mut connection),
                            Command::Close(reply) => { drop(connection); let _ = reply.send(()); return; }
                        }
                    }
                }
                Err(error) => { let _ = ready.send(Err(error)); }
            }
        })?;
        initialized.recv()??;
        Ok(Self { sender })
    }

    pub async fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Command::Call(Box::new(move |connection| {
                let _ = reply.send(operation(connection));
            })))
            .await
            .map_err(|_| "Stockage fermé.")?;
        result.await.map_err(|_| "Opération SQLite interrompue.")?
    }

    pub async fn cards(&self) -> Result<Vec<Card>> {
        self.call(|connection| {
            let mut statement = connection.prepare("SELECT data FROM cards ORDER BY rowid DESC")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
        })
        .await
    }

    pub async fn put_card(&self, card: Card) -> Result<Card> {
        self.call(move |connection| {
            connection.execute("INSERT INTO cards(id,data) VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![card.id, serde_json::to_string(&card)?])?;
            Ok(card)
        }).await
    }

    pub async fn close(&self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Command::Close(reply))
            .await
            .map_err(|_| "Stockage fermé.")?;
        result.await?;
        Ok(())
    }
}
