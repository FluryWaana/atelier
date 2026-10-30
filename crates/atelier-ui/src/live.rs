use crate::load;
use atelier_core::{
    contracts::{Board, Card},
    document::Document,
};
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Copy)]
pub struct BoardState {
    pub cards: RwSignal<Vec<Card>>,
    pub projects: RwSignal<Vec<Document>>,
    pub templates: RwSignal<Vec<Document>>,
    pub settings: RwSignal<Document>,
    pub runs: RwSignal<Vec<Document>>,
    pub times: RwSignal<Vec<Document>>,
    pub connected: RwSignal<bool>,
    pub loaded: RwSignal<bool>,
    pub sync_error: RwSignal<String>,
    revision: RwSignal<u64>,
}

impl Default for BoardState {
    fn default() -> Self {
        Self {
            cards: RwSignal::new(Vec::new()),
            projects: RwSignal::new(Vec::new()),
            templates: RwSignal::new(Vec::new()),
            settings: RwSignal::new(Document::default()),
            runs: RwSignal::new(Vec::new()),
            times: RwSignal::new(Vec::new()),
            connected: RwSignal::new(false),
            loaded: RwSignal::new(false),
            sync_error: RwSignal::new(String::new()),
            revision: RwSignal::new(0),
        }
    }
}

impl BoardState {
    pub fn apply(self, board: Board) {
        let Some(current) = self.revision.try_get_untracked() else {
            return;
        };
        if board.revision < current {
            return;
        }
        self.revision.set(board.revision);
        self.loaded.set(true);
        let mut cards = board.cards;
        cards.extend(board.brainstorms);
        self.cards.set(cards);
        self.projects.set(board.projects);
        self.runs.set(board.runs);
        self.times.set(board.times);
        self.templates.set(
            board
                .settings
                .get("taskTemplates")
                .ok()
                .flatten()
                .unwrap_or_default(),
        );
        self.settings.set(board.settings);
        self.sync_error.set(String::new());
    }

    pub async fn refresh(self, token: &str) -> Result<(), String> {
        match load(token).await {
            Ok(board) => {
                self.apply(board);
                Ok(())
            }
            Err(message) => {
                self.sync_error.set(message.clone());
                Err(message)
            }
        }
    }

    pub fn listen(self, token: String) -> Result<(), String> {
        let owner = Owner::current().ok_or("message.automatic_refresh_is_unavailable")?;
        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cleanup = alive.clone();
        on_cleanup(move || cleanup.store(false, std::sync::atomic::Ordering::Relaxed));
        spawn_local(async move {
            let result = match gloo_net::http::Request::get("/api/events/session")
                .header("Authorization", &format!("Bearer {token}"))
                .send()
                .await
            {
                Ok(response) => crate::checked_response(response).await.map(|_| ()),
                Err(_) => Err("message.automatic_refresh_is_unavailable".into()),
            };
            if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            let result = result.and_then(|()| owner.with(|| self.connect(token)));
            if let Err(message) = result {
                self.sync_error.set(message);
            }
        });
        Ok(())
    }

    fn connect(self, token: String) -> Result<(), String> {
        let source = web_sys::EventSource::new("/api/events")
            .map_err(|_| "message.automatic_refresh_is_unavailable")?;
        let pending = Rc::new(Cell::new(false));
        let again = Rc::new(Cell::new(false));
        let alive = Rc::new(Cell::new(true));
        let listening = alive.clone();
        let message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |_| {
            self.connected.set(true);
            again.set(true);
            if pending.replace(true) {
                return;
            }
            let (pending, again, alive, token) = (
                pending.clone(),
                again.clone(),
                listening.clone(),
                token.clone(),
            );
            spawn_local(async move {
                // Coalesce notifications into one active request and one pending refresh.
                while alive.get() && again.replace(false) {
                    let result = load(&token).await;
                    if !alive.get() {
                        return;
                    }
                    match result {
                        Ok(board) => self.apply(board),
                        Err(message) => self.sync_error.set(message),
                    }
                }
                pending.set(false);
            });
        });
        let connection = source.clone();
        let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            self.connected.set(false);
            if connection.ready_state() == web_sys::EventSource::CLOSED {
                self.sync_error
                    .set("message.connection_closed_reopen_atelier_using_its_launcher".into());
            }
        });
        source.set_onmessage(Some(message.as_ref().unchecked_ref()));
        source.set_onerror(Some(error.as_ref().unchecked_ref()));
        let resource = StoredValue::new_local((source, message, error, alive));
        on_cleanup(move || {
            resource.with_value(|(source, _, _, alive)| {
                alive.set(false);
                source.set_onmessage(None);
                source.set_onerror(None);
                source.close();
            });
        });
        Ok(())
    }
}
