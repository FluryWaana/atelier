use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::application::automation::{Flow, State};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Automation(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let flow = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<Flow>("automation").ok().flatten())
    });
    let project = Signal::derive(move || {
        card.get().and_then(|card| {
            let repo = field(&card.fields, "repo");
            board.projects.with(|projects| {
                projects
                    .iter()
                    .find(|project| field(project, "repo") == repo)
                    .cloned()
            })
        })
    });
    let profile = Signal::derive(move || {
        card.get().and_then(|card| {
            atelier_core::domain::agent_options::providers(&board.settings.get())
                .ok()
                .and_then(|profiles| {
                    profiles
                        .into_iter()
                        .find(|profile| field(profile, "id") == field(&card.fields, "agent"))
                })
        })
    });
    let snapshot = Memo::new(move |_| {
        serde_json::to_string(&(card.get(), project.get(), profile.get())).unwrap_or_default()
    });
    let approved = RwSignal::new(None::<String>);
    let error = RwSignal::new(String::new());
    let active = Memo::new(move |_| flow.get().is_some_and(|flow| flow.state == State::Running));
    let action = Signal::derive(move || match flow.get().map(|flow| flow.state) {
        Some(State::Running) => None,
        Some(State::Waiting) => Some(("review", "Vérifier la publication et lancer la review")),
        Some(State::Interrupted | State::Error | State::Cancelled) => {
            Some(("resume", "Reprendre l’étape du parcours"))
        }
        _ => Some(("start", "Démarrer le parcours")),
    });
    let execute = StoredValue::new_local(move |stop: bool| {
        if pending.get_untracked()
            || (!stop
                && (blocked.get_untracked()
                    || approved.get_untracked().as_deref()
                        != Some(snapshot.get_untracked().as_str())))
        {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Ok(credential) = token.get_value() else {
            return;
        };
        let path = format!(
            "/api/cards/{}/automation{}",
            expected.id,
            if stop { "/stop" } else { "" }
        );
        let body = if stop {
            serde_json::json!({"id":flow.get_untracked().map(|flow|flow.id),"confirmed":true})
        } else {
            let Some((action, _)) = action.get_untracked() else {
                return;
            };
            serde_json::json!({"action":action,"expected":expected,"project":project.get_untracked(),"profile":profile.get_untracked(),"confirmed":true})
        };
        pending.set(true);
        approved.set(None);
        error.set(String::new());
        spawn_local(async move {
            let result = write(&credential, Method::POST, &path, &body).await;
            let _ = board.refresh(&credential).await;
            if pending.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    });
    view! {<section class="task-files" id="automation" aria-labelledby="automation-heading" hidden=move||project.get().is_none()>
        <h3 id="automation-heading">{move||i18n.text("Parcours du projet")}</h3>
        <p class="field-help">{move||i18n.text("Développement, vérifications et review utilisent le profil enregistré. Les corrections P0/P1 sont bornées à deux cycles. Commit, push et publication demandent un contrôle explicite ; aucune validation humaine des critères n’est automatisée.")}</p>
        <p>{move||project.get().map(|project|format!("{} · {}",field(&project,"testCommand"),field(&project,"prTarget"))).unwrap_or_default()}</p>
        <p>{move||profile.get().map(|profile|field(&profile,"name")).unwrap_or_default()}</p>
        <p role="status">{move||flow.get().map(|flow|format!("{} · {} · {}",i18n.text(flow.state.label()),i18n.text(flow.step.label()),flow.cycles)).unwrap_or_default()}</p>
        <p role="status">{move||flow.get().map(|flow|i18n.text(&flow.message)).unwrap_or_default()}</p>
        <p class="field-help" hidden=move||project.get().is_some_and(|project|project.get::<bool>("automationEnabled").ok().flatten()==Some(true))>{move||i18n.text("Active le parcours dans les réglages du projet pour démarrer une tâche.")}</p>
        <label hidden=move||active.get()><input id="approve-automation" type="checkbox" prop:checked=move||approved.get().as_deref()==Some(snapshot.get().as_str()) disabled=move||pending.get()||blocked.get() on:change=move|event|approved.set(event_target_checked(&event).then(||snapshot.get_untracked()))/>{move||i18n.text("Autoriser cette étape avec le besoin, le profil et les réglages affichés. Après interruption, j’ai vérifié le worktree et les effets distants avant reprise.")}</label>
        <button id="start-automation" type="button" hidden=move||active.get() disabled=move||pending.get()||blocked.get()||approved.get().as_deref()!=Some(snapshot.get().as_str()) on:click=move|_|execute.with_value(|execute|execute(false))>{move||i18n.text(action.get().map(|(_,label)|label).unwrap_or("Démarrer le parcours"))}</button>
        <button id="stop-automation" type="button" hidden=move||flow.get().is_none_or(|flow|!matches!(flow.state,State::Running|State::Waiting)) disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute(true))>{move||i18n.text("Arrêter le parcours")}</button>
        <p id="automation-error" class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
