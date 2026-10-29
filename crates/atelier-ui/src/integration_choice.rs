use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::application::integrations::{self, Kind, Provider};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn IntegrationChoice(
    kind: Kind,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    on_configure: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let current = move || {
        integrations::selected(&board.settings.get(), kind).unwrap_or_else(|_| "local".into())
    };
    let original = RwSignal::new(current());
    let selected = RwSignal::new(original.get_untracked());
    let changed = Memo::new(move |_| original.get() != selected.get());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(false);
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    Effect::new(move |_| dirty.set(changed.get()));
    Effect::new(move |_| {
        let saved = current();
        if !changed.get_untracked() {
            original.set(saved.clone());
            selected.set(saved);
        }
    });
    let providers = move || {
        board
            .settings
            .get()
            .get::<Vec<Provider>>("integrationCatalog")
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .filter(|provider| provider.kinds.contains(&kind))
            .collect::<Vec<_>>()
    };
    let id = kind.setting();
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if pending.get_untracked() || blocked.get_untracked() || !changed.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::json!({"kind":kind,"provider":selected.get_untracked(),"expected":original.get_untracked()});
        pending.set(true);
        error.set(String::new());
        notice.set(false);
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                let saved: serde_json::Value = write(
                    &credential,
                    Method::PUT,
                    "/api/integrations/defaults",
                    &body,
                )
                .await?
                .json()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())?;
                saved
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| "message.invalid_server_response".to_owned())
            }
            .await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(saved) => {
                    original.set(saved.clone());
                    selected.set(saved);
                    notice.set(true);
                }
                Err(message) => error.set(message),
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    view! {<section class="form-section">
        <h3>{move||i18n.text(if kind==Kind::Tickets {"message.ticket_source"}else{"message.time_destination"})}</h3>
        <p class="hint">{move||i18n.text("message.integration_defaults_future_only")}</p>
        <form on:submit=save><fieldset disabled=move||pending.get()||blocked.get()>
            <label for=id>{move||i18n.text("message.default_provider")}</label>
            <select id=id prop:value=move||selected.get() on:change=move|event|{selected.set(event_target_value(&event));notice.set(false);}>
                {move||(!providers().iter().any(|provider|provider.id==selected.get())).then(||view!{<option value=selected.get() disabled>{move||i18n.text("message.integration_unavailable")}</option>})}
                <For each=providers key=|provider|provider.id.clone() children=move|provider|{let name=provider.name;view!{<option value=provider.id>{move||i18n.text(&name)}</option>}}/>
            </select>
            <div class="form-actions"><button id=format!("save-{id}") class="primary" type="submit" disabled=move||!changed.get()>{move||i18n.text("message.save")}</button><button type="button" disabled=move||!changed.get() on:click=move |_|{let saved=current();original.set(saved.clone());selected.set(saved);error.set(String::new());}>{move||i18n.text("message.cancel")}</button></div>
        </fieldset></form>
        <p class="field-help" hidden=move||current()!="local">{move||i18n.text(if kind==Kind::Tickets{"message.local_tickets_explanation"}else{"message.local_time_explanation"})}</p>
        <button type="button" hidden=move||!current().starts_with("extension:") disabled=move||pending.get()||changed.get()||blocked.get() on:click=move |_|{on_configure.run(field(&board.settings.get_untracked(),id).trim_start_matches("extension:").into());}>{move||i18n.text("message.configure_selected_module")}</button>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p><p role="status">{move||if notice.get(){i18n.text("message.settings_saved")}else{String::new()}}</p>
    </section>}
}

// Built-in adapters own their configuration views; process modules use their manifest editor.
#[component]
pub fn IntegrationConfiguration(
    kind: Kind,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let source = Memo::new(move |_| {
        integrations::selected(&board.settings.get(), kind).unwrap_or_else(|_| "local".into())
    });
    move || match (kind, source.get().as_str()) {
        (Kind::Tickets, "jira") => {
            view! {<crate::jira::JiraSettings board token pending dirty/>}.into_any()
        }
        (Kind::Time, "tempo") => {
            view! {<crate::tempo::TempoSettings token pending dirty/>}.into_any()
        }
        _ => ().into_any(),
    }
}
