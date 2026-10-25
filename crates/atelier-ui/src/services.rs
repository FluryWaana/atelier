use crate::{i18n::I18n, live::BoardState, read, write};
use atelier_core::contracts::{ServiceSession, ServiceStatus, Worktree, WorktreeState};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Services(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let session = RwSignal::new(None::<ServiceSession>);
    let error = RwSignal::new(String::new());
    let standalone = RwSignal::new(false);
    let cancelling = RwSignal::new(false);
    let ready = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .and_then(|card| card.fields.get::<Worktree>("worktree").ok().flatten())
                .is_some_and(|worktree| worktree.state == WorktreeState::Ready)
        })
    });
    let running = Memo::new(move |_| {
        session.get().is_some_and(|session| {
            session.services.iter().any(|service| {
                service.pid.is_some()
                    || matches!(
                        service.status,
                        ServiceStatus::Starting | ServiceStatus::Running | ServiceStatus::Stopping
                    )
            })
        })
    });
    let alive = Rc::new(Cell::new(true));
    let loading = Rc::new(Cell::new(false));
    let revision = Rc::new(Cell::new(0u64));
    let refresh = {
        let alive = alive.clone();
        let loading = loading.clone();
        let revision = revision.clone();
        Rc::new(move || {
            if !alive.get()
                || pending.get_untracked()
                || !ready.get_untracked()
                || loading.replace(true)
            {
                return;
            }
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    loading.set(false);
                    error.set(message);
                    return;
                }
            };
            let card = id.get_value();
            let alive = alive.clone();
            let loading = loading.clone();
            let revision = revision.clone();
            let expected = revision.get();
            spawn_local(async move {
                let result = async {
                    read(&credential, &format!("/api/cards/{card}/services"))
                        .await?
                        .json::<Option<ServiceSession>>()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                if alive.get() && revision.get() == expected {
                    match result {
                        Ok(value) => session.set(value),
                        Err(message) => error.set(message),
                    }
                }
                loading.set(false);
            });
        })
    };
    {
        let refresh = refresh.clone();
        Effect::new(move |_| {
            let _ = ready.get();
            refresh();
        });
    }
    let tick = Closure::<dyn FnMut()>::new(move || refresh());
    let interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                1000,
            )
            .ok()
    });
    let timer = StoredValue::new_local((alive.clone(), interval, tick));
    on_cleanup(move || {
        timer.with_value(|(alive, interval, _)| {
            alive.set(false);
            if let (Some(window), Some(interval)) = (web_sys::window(), *interval) {
                window.clear_interval_with_handle(interval);
            }
        })
    });
    let act = move |action: &'static str, service: Option<String>| {
        if cancelling.get_untracked() || (action != "stop" && pending.get_untracked()) {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        if action == "stop" {
            cancelling.set(true);
        } else {
            pending.set(true);
        }
        error.set(String::new());
        revision.set(revision.get().wrapping_add(1));
        let card = id.get_value();
        let alive = alive.clone();
        let revision = revision.clone();
        let mut body = if action == "start" {
            serde_json::json!({"standalone":standalone.get_untracked()})
        } else {
            serde_json::json!({})
        };
        if let Some(service) = service {
            body["serviceId"] = serde_json::json!(service);
        }
        spawn_local(async move {
            let result = async {
                write(
                    &credential,
                    Method::POST,
                    &format!("/api/cards/{card}/services/{action}"),
                    &body,
                )
                .await?
                .json::<ServiceSession>()
                .await
                .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                revision.set(revision.get().wrapping_add(1));
                match result {
                    Ok(value) => session.set(Some(value)),
                    Err(message) => error.set(message),
                }
                if action == "stop" {
                    cancelling.set(false);
                } else {
                    pending.set(false);
                }
            }
        });
    };
    let act = StoredValue::new_local(act);
    view! {
        <section class="task-files" aria-labelledby="services-heading" aria-busy=move || (pending.get() || cancelling.get()).to_string()>
            <h3 id="services-heading">{move || i18n.text("Services Front/Back")}</h3>
            <p class="field-help">{move || i18n.text("Lance les commandes enregistrées dans les réglages du projet. Les services du projet lié utilisent son dépôt principal ; les services de cette tâche utilisent ses worktrees.")}</p>
            <label class="check-filter"><input id="services-standalone" type="checkbox" disabled=move || pending.get() || running.get() prop:checked=move || standalone.get() on:change=move |event| standalone.set(event_target_checked(&event)) />{move || i18n.text("Lancer seulement ce projet")}</label>
            <div class="editor-actions"><button id="start-services" type="button" disabled=move || pending.get() || cancelling.get() || blocked.get() || running.get() || !ready.get() on:click=move |_| act.with_value(|act| act("start",None))>{move || i18n.text("Lancer les services")}</button><button id="stop-services" type="button" disabled=move || cancelling.get() || !running.get() on:click=move |_| act.with_value(|act| act("stop",None))>{move || i18n.text("Arrêter les services")}</button></div>
            <p class="field-help" hidden=move || ready.get()>{move || i18n.text("Prépare et vérifie les worktrees avant de lancer les services.")}</p>
            <p id="services-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <For each={move || session.get().map(|session| session.services).unwrap_or_default()} key=|service| service.id.clone() children=move |initial| {
                let service_id = StoredValue::new(initial.id);
                let current = move || session.get().and_then(|session| session.services.into_iter().find(|service| service.id == service_id.get_value()));
                view! { <article class="service-state"><h4>{move || current().map(|service| service.name).unwrap_or_default()}</h4><p class="service-status" role="status">{move || current().map(|service| i18n.text(match service.status { ServiceStatus::Starting=>"Démarrage…",ServiceStatus::Running=>"Processus actif",ServiceStatus::Stopping=>"Arrêt…",ServiceStatus::Stopped=>"Arrêté",ServiceStatus::Failed=>"Échec du service" })).unwrap_or_default()}</p><p class="workspace-path">{move || current().map(|service| service.directory).unwrap_or_default()}</p><p>{move || current().and_then(|service| service.port).map(|port| format!("{} {port}", i18n.text("Port attribué :"))).unwrap_or_default()}</p><p class="field-help" role="status">{move || current().map(|service| i18n.text(&service.inspection_error)).unwrap_or_default()}</p><ul class="service-endpoints" aria-label=move || i18n.text("Écoutes TCP détectées")><For each=move || current().map(|service| service.endpoints).unwrap_or_default() key=|endpoint| (endpoint.pid,endpoint.address.clone(),endpoint.port,endpoint.url.clone()) children=move |endpoint| { if let Some(url) = endpoint.url { view! { <li><a href=url.clone() target="_blank" rel="noopener noreferrer">{url}</a></li> }.into_any() } else { view! { <li>{format!("{}:{} · PID {}",endpoint.address,endpoint.port,endpoint.pid)}</li> }.into_any() } } /></ul><p class="error">{move || current().map(|service| i18n.text(&service.error)).unwrap_or_default()}</p><details><summary>{move || i18n.text("Journaux du service")}</summary><pre class="service-log"><code>{move || current().map(|service| service.log).unwrap_or_default()}</code></pre></details><div class="editor-actions"><button class="restart-service" type="button" disabled=move || pending.get() || cancelling.get() || blocked.get() || !ready.get() on:click=move |_| act.with_value(|act| act("restart",Some(service_id.get_value())))>{move || i18n.text("Relancer ce service")}</button><button class="stop-service" type="button" disabled=move || cancelling.get() || current().is_none_or(|service| service.pid.is_none() && !matches!(service.status,ServiceStatus::Starting|ServiceStatus::Running|ServiceStatus::Stopping)) on:click=move |_| act.with_value(|act| act("stop",Some(service_id.get_value())))>{move || i18n.text("Arrêter ce service")}</button></div></article> }
            } />
            <p class="field-help">{move || i18n.text("Les journaux conservent les derniers 64 Kio par service. Un processus actif ne garantit pas encore que son serveur répond.")}</p>
        </section>
    }
}
