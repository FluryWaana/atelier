use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    contracts::{Worktree, WorktreeState},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

fn active(run: &Document) -> bool {
    matches!(
        field(run, "status").as_str(),
        "starting" | "running" | "stopping"
    )
}

use atelier_core::contracts::RunLog as Log;

#[component]
pub fn Runs(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card = StoredValue::new(card_id);
    let agent = RwSignal::new(String::new());
    let mode = RwSignal::new("analyze".to_owned());
    let error = RwSignal::new(String::new());
    let profiles = Signal::derive(move || {
        board.settings.with(|settings| {
            atelier_core::domain::agent_options::providers(settings)
                .unwrap_or_default()
                .into_iter()
                .filter(|profile| field(profile, "type") == "command")
                .collect::<Vec<_>>()
        })
    });
    let latest = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .find(|run| field(run, "cardId") == card.get_value())
                .cloned()
        })
    });
    let latest_id = Memo::new(move |_| latest.get().map(|run| field(&run, "id")));
    let running = Memo::new(move |_| latest.get().is_some_and(|run| active(&run)));
    let ready = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .is_some_and(|item| {
                    !item.fields.truthy("archived").unwrap_or(true)
                        && item
                            .fields
                            .get::<Worktree>("worktree")
                            .ok()
                            .flatten()
                            .is_some_and(|worktree| worktree.state == WorktreeState::Ready)
                })
        })
    });
    let alive = Rc::new(Cell::new(true));
    let acting = alive.clone();
    let act = move |stop: bool| {
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = if stop {
            let Some(run) = latest.get_untracked() else {
                return;
            };
            format!("/api/runs/{}/stop", field(&run, "id"))
        } else {
            format!("/api/cards/{}/runs/start", card.get_value())
        };
        let body = if stop {
            serde_json::json!({})
        } else {
            serde_json::json!({"agent":agent.get_untracked(),"mode":mode.get_untracked()})
        };
        pending.set(true);
        error.set(String::new());
        let alive = acting.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body).await?;
                board.refresh(&credential).await
            }
            .await;
            if alive.get() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    };
    let act = StoredValue::new_local(act);
    let log = RwSignal::new(String::new());
    let cursor = RwSignal::new(0i64);
    let selected = RwSignal::new(String::new());
    let loading = Rc::new(Cell::new(false));
    let log_busy = RwSignal::new(false);
    let refresh = {
        let alive = alive.clone();
        let loading = loading.clone();
        Rc::new(move || {
            let Some(run) = latest.get_untracked() else {
                return;
            };
            let id = field(&run, "id");
            if !alive.get() || loading.replace(true) {
                return;
            }
            log_busy.set(true);
            if selected.get_untracked() != id {
                selected.set(id.clone());
                cursor.set(0);
                log.set(String::new());
            }
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    loading.set(false);
                    log_busy.set(false);
                    error.set(message);
                    return;
                }
            };
            let after = cursor.get_untracked();
            let alive = alive.clone();
            let loading = loading.clone();
            spawn_local(async move {
                let result = async {
                    read(&credential, &format!("/api/runs/{id}/logs?after={after}"))
                        .await?
                        .json::<Vec<Log>>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                if alive.get() && selected.get_untracked() == id {
                    match result {
                        Ok(entries) => {
                            for entry in entries {
                                cursor.set(entry.seq);
                                log.update(|text| {
                                    text.push_str(&format!("[{}] {}", entry.stream, entry.text));
                                    if !text.ends_with('\n') {
                                        text.push('\n');
                                    }
                                    if text.len() > 64 * 1024 {
                                        let mut start = text.len() - 64 * 1024;
                                        while !text.is_char_boundary(start) {
                                            start += 1;
                                        }
                                        text.drain(..start);
                                    }
                                });
                            }
                        }
                        Err(message) => error.set(message),
                    }
                }
                loading.set(false);
                if alive.get() {
                    log_busy.set(false);
                }
            });
        })
    };
    let initial = refresh.clone();
    Effect::new(move |_| {
        let current = latest_id.get().unwrap_or_default();
        selected.set(current);
        cursor.set(0);
        log.set(String::new());
        initial();
    });
    let tick = Closure::<dyn FnMut()>::new(move || refresh());
    let interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                1000,
            )
            .ok()
    });
    let timer = StoredValue::new_local((alive, interval, tick));
    on_cleanup(move || {
        timer.with_value(|(alive, interval, _)| {
            alive.set(false);
            if let (Some(window), Some(interval)) = (web_sys::window(), *interval) {
                window.clear_interval_with_handle(interval);
            }
        })
    });
    view! {
        <section class="task-files" aria-labelledby="runs-heading" aria-busy=move || pending.get().to_string()>
            <h3 id="runs-heading">{move || i18n.text("Exécutions")}</h3>
            <p class="field-help">{move || i18n.text("Lance explicitement une commande enregistrée dans Agents. Codex et Claude interactifs seront disponibles après leur raccordement.")}</p>
            <label for="run-agent">{move || i18n.text("Profil de commande")}</label>
            <p class="field-help" hidden=move || !profiles.get().is_empty()>{move || i18n.text("Crée un profil de commande dans Agents pour démarrer une exécution.")}</p>
            <select id="run-agent" prop:value=move || agent.get() disabled=move || pending.get() || running.get() on:change=move |event| agent.set(event_target_value(&event))><option value="">{move || i18n.text("Choisir un profil")}</option><For each=move || profiles.get() key=|profile| field(profile,"id") children=move |profile| view! { <option value=field(&profile,"id")>{field(&profile,"name")}</option> } /></select>
            <label for="run-mode">{move || i18n.text("Mode")}</label><select id="run-mode" prop:value=move || mode.get() disabled=move || pending.get() || running.get() on:change=move |event| mode.set(event_target_value(&event))><option value="analyze">{move || i18n.text("Analyse")}</option><option value="edit">{move || i18n.text("Modification")}</option></select>
            <div class="editor-actions"><button id="start-run" type="button" disabled=move || pending.get() || blocked.get() || running.get() || !ready.get() || agent.get().is_empty() on:click=move |_| act.with_value(|act| act(false))>{move || i18n.text("Démarrer l’exécution")}</button><button id="stop-run" type="button" disabled=move || pending.get() || !running.get() on:click=move |_| act.with_value(|act| act(true))>{move || i18n.text("Arrêter l’exécution")}</button></div>
            <p class="field-help" hidden=move || ready.get()>{move || i18n.text("Prépare et vérifie le worktree, puis enregistre tes modifications avant de lancer une commande.")}</p>
            <p role="status">{move || i18n.text(latest.get().map(|run| match field(&run,"status").as_str() { "starting"=>"Démarrage…", "running"=>"Processus actif", "stopping"=>"Arrêt…", "completed"=>"Exécution terminée", "cancelled"=>"Exécution arrêtée", "interrupted"=>"Exécution interrompue : vérifie le dépôt avant de relancer.", _=>"Échec de l’exécution" }).unwrap_or("Aucune exécution pour cette tâche."))}</p>
            <p class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <details><summary>{move || i18n.text("Journal de la dernière exécution")}</summary><p role="status" hidden=move || !log_busy.get() || !log.get().is_empty()>{move || i18n.text("Chargement du journal…")}</p><pre class="service-log" tabindex="0" aria-busy=move || log_busy.get().to_string()><code>{move || log.get()}</code></pre></details>
            <p class="field-help">{move || i18n.text("Affichage limité aux derniers 64 Kio. Les commandes utilisent leurs propres permissions ; le mode Analyse ne constitue pas une isolation du système.")}</p>
        </section>
    }
}
