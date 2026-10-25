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
    reply_drafts: RwSignal<std::collections::BTreeSet<String>>,
    blocked: Memo<bool>,
    on_open: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card = StoredValue::new(card_id);
    let brainstorm = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .is_some_and(|item| item.fields.truthy("brainstormSession").unwrap_or(false))
        })
    });
    let message = RwSignal::new(String::new());
    let message_id = StoredValue::new(format!("brainstorm-message:{}", card.get_value()));
    Effect::new(move |_| {
        reply_drafts.update(|drafts| {
            if !message.get().is_empty() {
                drafts.insert(message_id.get_value());
            } else {
                drafts.remove(&message_id.get_value());
            }
        })
    });
    on_cleanup(move || {
        let _ = reply_drafts.try_update(|drafts| {
            drafts.remove(&message_id.get_value());
        });
    });
    let agent = RwSignal::new(String::new());
    let mode = RwSignal::new("analyze".to_owned());
    let error = RwSignal::new(String::new());
    let selected_run = RwSignal::new(String::new());
    let profiles = Signal::derive(move || {
        board.settings.with(|settings| {
            atelier_core::domain::agent_options::providers(settings).unwrap_or_default()
        })
    });
    let history = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .filter(|run| field(run, "cardId") == card.get_value())
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let active_run = Signal::derive(move || history.get().into_iter().find(active));
    let latest = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .find(|run| {
                    field(run, "cardId") == card.get_value()
                        && (selected_run.get().is_empty() || field(run, "id") == selected_run.get())
                })
                .cloned()
        })
    });
    let latest_id = Memo::new(move |_| latest.get().map(|run| field(&run, "id")));
    let running = Memo::new(move |_| active_run.get().is_some());
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
    let cancelling = RwSignal::new(false);
    let act = move |stop: bool| {
        if cancelling.get_untracked() || (!stop && pending.get_untracked()) {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = if stop {
            let Some(run) = active_run.get_untracked() else {
                return;
            };
            format!("/api/runs/{}/stop", field(&run, "id"))
        } else {
            format!("/api/cards/{}/runs/start", card.get_value())
        };
        let body = if stop {
            serde_json::json!({})
        } else {
            let expected = board.cards.with_untracked(|cards| {
                cards
                    .iter()
                    .find(|item| item.id == card.get_value())
                    .cloned()
            });
            serde_json::json!({"agent":agent.get_untracked(),"mode":if brainstorm.get_untracked(){"analyze".to_owned()}else{mode.get_untracked()},"expected":expected,"message":message.get_untracked()})
        };
        if stop {
            cancelling.set(true);
        } else {
            pending.set(true);
        }
        error.set(String::new());
        let alive = acting.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body).await?;
                board.refresh(&credential).await
            }
            .await;
            if alive.get() {
                if result.is_ok() && !stop {
                    selected_run.set(String::new());
                    message.set(String::new());
                }
                if let Err(message) = result {
                    error.set(message);
                }
                if stop {
                    cancelling.set(false);
                } else {
                    pending.set(false);
                }
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
        <section class="task-files" aria-labelledby="runs-heading" aria-busy=move || (pending.get() || cancelling.get()).to_string()>
            <h3 id="runs-heading">{move || i18n.text("Exécutions")}</h3>
            <p class="field-help">{move || i18n.text("Choisis un profil enregistré dans Agents. Les demandes d’autorisation et les questions apparaissent pendant l’exécution.")}</p>
            <label for="run-agent">{move || i18n.text("Profil d’agent")}</label>
            <p class="field-help" hidden=move || !profiles.get().is_empty()>{move || i18n.text("Crée un profil de commande dans Agents pour démarrer une exécution.")}</p>
            <select id="run-agent" prop:value=move || agent.get() disabled=move || pending.get() || running.get() on:change=move |event| agent.set(event_target_value(&event))><option value="">{move || i18n.text("Choisir un profil")}</option><For each=move || profiles.get() key=|profile| field(profile,"id") children=move |profile| view! { <option value=field(&profile,"id")>{field(&profile,"name")}</option> } /></select>
            <label hidden=move||brainstorm.get() for="run-mode">{move || i18n.text("Mode")}</label><select hidden=move||brainstorm.get() id="run-mode" prop:value=move || mode.get() disabled=move || pending.get() || running.get() on:change=move |event| mode.set(event_target_value(&event))><option value="analyze">{move || i18n.text("Analyse")}</option><option value="edit">{move || i18n.text("Modification")}</option></select>
            <div hidden=move||!brainstorm.get()><label for="brainstorm-message">{move||i18n.text("Message pour le brainstorm")}</label><textarea id="brainstorm-message" maxlength="12000" rows="4" disabled=move||pending.get()||running.get() prop:value=move||message.get() on:input=move|event|message.set(event_target_value(&event)) /><p class="field-help">{move||i18n.text("Décris ta question ou demande un découpage en tâches. Chaque envoi démarre explicitement une exécution de l’agent choisi.")}</p></div>
            <div class="editor-actions"><button id="start-run" type="button" disabled=move || pending.get() || cancelling.get() || blocked.get() || running.get() || !ready.get() || agent.get().is_empty() || (brainstorm.get()&&message.get().trim().is_empty()) on:click=move |_| act.with_value(|act| act(false))>{move || i18n.text("Démarrer l’exécution")}</button><button id="stop-run" type="button" disabled=move || cancelling.get() || !running.get() on:click=move |_| act.with_value(|act| act(true))>{move || i18n.text("Arrêter l’exécution")}</button></div>
            <p class="field-help" hidden=move || ready.get()>{move || i18n.text("Prépare et vérifie le worktree, puis enregistre tes modifications avant de lancer une commande.")}</p>
            <p role="status">{move || i18n.text(latest.get().map(|run| match field(&run,"status").as_str() { "starting"=>"Démarrage…", "running"=>"Processus actif", "stopping"=>"Arrêt…", "completed"=>"Exécution terminée", "cancelled"=>"Exécution arrêtée", "interrupted"=>"Exécution interrompue : vérifie le dépôt avant de relancer.", _=>"Échec de l’exécution" }).unwrap_or("Aucune exécution pour cette tâche."))}</p>
            <p class="error" role="alert">{move || { let message = error.get(); i18n.text(&if message.is_empty() { latest.get().map(|run| field(&run,"attention")).unwrap_or_default() } else { message }) }}</p>
            <label for="run-history">{move || i18n.text("Historique des exécutions")}</label><select id="run-history" prop:value=move || selected_run.get() on:change=move |event| selected_run.set(event_target_value(&event))><option value="">{move || i18n.text("Dernière exécution")}</option><For each=move || history.get() key=|run| field(run,"id") children=move |run| view! { <option value=field(&run,"id")>{format!("{} · {}",field(&run,"startedAt"),field(&run,"agent"))}</option> } /></select>
            <p role="status" hidden=move || !active_run.get().is_some_and(|run| run.get::<bool>("waiting").ok().flatten().unwrap_or(false))>{move || i18n.text("En attente de votre réponse")}</p>
            <For each=move || active_run.get().map(|run| { let id=field(&run,"id"); run.get::<Vec<atelier_core::contracts::AgentInteraction>>("interactions").ok().flatten().unwrap_or_default().into_iter().map(|request| (id.clone(),request)).collect::<Vec<_>>() }).unwrap_or_default() key=|(id,request)| (id.clone(),request.id.clone(),serde_json::to_string(request).unwrap_or_default()) children=move |(run_id,request)| view! { <Interaction run_id request token board reply_drafts /> } />
            <pre class="service-log" tabindex="0" hidden=move || active_run.get().is_none_or(|run| field(&run,"liveText").is_empty())>{move || active_run.get().map(|run| field(&run,"liveText")).unwrap_or_default()}</pre>
            <Show when=move||brainstorm.get()><crate::brainstorm::Results card_id=card.get_value() board token pending blocked message run=latest on_open /></Show>
            <details hidden=move || latest.get().is_none_or(|run| field(&run,"summary").is_empty())><summary>{move || i18n.text("Résumé de l’exécution")}</summary><p>{move || latest.get().map(|run| field(&run,"summary")).unwrap_or_default()}</p></details>
            <p hidden=move || latest.get().is_none_or(|run| run.raw("usage").is_none())>{move || latest.get().and_then(|run| run.get::<serde_json::Value>("usage").ok().flatten()).map(|usage| format!("{} {} / {} / {}",i18n.text("Jetons entrée/cache/sortie :"),usage["inputTokens"],usage["cachedInputTokens"],usage["outputTokens"])).unwrap_or_default()}</p>
            <p hidden=move || latest.get().is_none_or(|run| run.raw("activeMilliseconds").is_none())>{move || latest.get().and_then(|run| run.get::<u64>("activeMilliseconds").ok().flatten()).map(|milliseconds| format!("{} {} s",i18n.text("Durée active :"),milliseconds/1000)).unwrap_or_default()}</p>
            <details><summary>{move || i18n.text("Journal de la dernière exécution")}</summary><p role="status" hidden=move || !log_busy.get() || !log.get().is_empty()>{move || i18n.text("Chargement du journal…")}</p><pre class="service-log" tabindex="0" aria-busy=move || log_busy.get().to_string()><code>{move || log.get()}</code></pre></details>
            <p class="field-help">{move || i18n.text("Affichage limité aux derniers 64 Kio. Les commandes utilisent leurs propres permissions ; le mode Analyse ne constitue pas une isolation du système.")}</p>
        </section>
    }
}

#[component]
fn Interaction(
    run_id: String,
    request: atelier_core::contracts::AgentInteraction,
    token: StoredValue<Result<String, String>>,
    board: BoardState,
    reply_drafts: RwSignal<std::collections::BTreeSet<String>>,
) -> impl IntoView {
    use std::collections::BTreeMap;
    let i18n = expect_context::<I18n>();
    let request = StoredValue::new(request);
    let run = StoredValue::new(run_id);
    let drafts = RwSignal::new(BTreeMap::<String, String>::new());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let identity = StoredValue::new(format!("{}:{}", run.get_value(), request.get_value().id));
    Effect::new(move |_| {
        let dirty =
            pending.get() || drafts.with(|drafts| drafts.values().any(|value| !value.is_empty()));
        reply_drafts.update(|all| {
            if dirty {
                all.insert(identity.get_value());
            } else {
                all.remove(&identity.get_value());
            }
        });
    });
    on_cleanup(move || {
        let _ = reply_drafts.try_update(|all| {
            all.remove(&identity.get_value());
        });
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let send = move |deny: bool| {
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let request = request.get_value();
        let answers = if deny {
            BTreeMap::new()
        } else {
            request
                .questions
                .iter()
                .map(|question| {
                    let value = drafts.with_untracked(|drafts| {
                        drafts.get(&question.id).cloned().unwrap_or_default()
                    });
                    (
                        question.id.clone(),
                        if question.multi_select {
                            value.lines().map(str::to_owned).collect()
                        } else {
                            vec![value]
                        },
                    )
                })
                .collect()
        };
        let body = match serde_json::to_value(atelier_core::contracts::AgentAnswer {
            request_id: request.id,
            decision: if deny {
                "deny"
            } else if request.questions.is_empty() {
                "allow"
            } else {
                "answer"
            }
            .into(),
            answers,
        }) {
            Ok(body) => body,
            Err(_) => {
                error.set("Réponse interactive invalide.".into());
                return;
            }
        };
        let path = format!("/api/runs/{}/reply", run.get_value());
        let alive = alive.clone();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body).await?;
                if alive.get() {
                    drafts.set(BTreeMap::new());
                }
                board.refresh(&credential).await
            }
            .await;
            if alive.get() {
                pending.set(false);
                if let Err(message) = result {
                    error.set(message);
                }
            }
        });
    };
    let send = StoredValue::new_local(send);
    view! {
        <form class="service-state agent-interaction" aria-label=move || i18n.text(&request.get_value().title) aria-busy=move || pending.get().to_string() on:submit=move |event| {event.prevent_default();send.with_value(|send| send(false));}>
            <h4>{move || i18n.text(&request.get_value().title)}</h4>
            <pre class="service-log" tabindex="0" hidden=move || request.get_value().detail.is_empty()>{move || request.get_value().detail}</pre>
            <For each=move || request.get_value().questions key=|question| question.id.clone() children=move |question| {
                let question=StoredValue::new(question);
                let input_id=StoredValue::new(format!("{}-{}-{}",run.get_value(),request.get_value().id,question.get_value().id));
                view! {
                    <label for=move || input_id.get_value()>{move || question.get_value().question}</label>
                    <ul><For each=move || question.get_value().options key=|option| option.clone() children=move |option| { let option=StoredValue::new(option); view! { <li><button type="button" disabled=move || pending.get() on:click=move |_| drafts.update(|drafts| { let value=drafts.entry(question.get_value().id).or_default(); if question.get_value().multi_select && !value.is_empty() {value.push('\n');value.push_str(&option.get_value());}else{*value=option.get_value();} })>{move || option.get_value()}</button></li> } } /></ul>
                    {if question.get_value().multi_select && !question.get_value().is_secret { view! {
                        <p class="field-help">{move || i18n.text("Une réponse par ligne ; tu peux aussi écrire un autre choix.")}</p><textarea id=move || input_id.get_value() required maxlength="32000" disabled=move || pending.get() prop:value=move || drafts.with(|drafts| drafts.get(&question.get_value().id).cloned().unwrap_or_default()) on:input=move |event| drafts.update(|drafts| {drafts.insert(question.get_value().id,event_target_value(&event));}) />
                    }.into_any() }else{view! {
                        <input class="agent-question" id=move || input_id.get_value() type={if question.get_value().is_secret{"password"}else{"text"}} autocomplete="off" required maxlength="4000" disabled=move || pending.get() prop:value=move || drafts.with(|drafts| drafts.get(&question.get_value().id).cloned().unwrap_or_default()) on:input=move |event| drafts.update(|drafts| {drafts.insert(question.get_value().id,event_target_value(&event));}) />
                    }.into_any()}}
                }
            } />
            <p class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div class="editor-actions"><button class="approve-interaction" type="submit" disabled=move || pending.get() || (!request.get_value().can_allow && request.get_value().questions.is_empty())>{move || i18n.text(if request.get_value().questions.is_empty(){"Autoriser cette demande"}else{"Transmettre la réponse"})}</button><button class="deny-interaction" type="button" disabled=move || pending.get() on:click=move |_| send.with_value(|send| send(true))>{move || i18n.text("Refuser cette demande")}</button></div>
        </form>
    }
}
