use crate::{i18n::I18n, read, write};
use atelier_core::contracts::{TempoConfigurationUpdate, TempoConnection};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn TempoSettings(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let original = RwSignal::new(None::<TempoConnection>);
    let base = RwSignal::new("https://api.tempo.io/4".to_owned());
    let key = RwSignal::new(String::new());
    let clear = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let reload = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let own_dirty = Memo::new(move |_| {
        original
            .get()
            .is_some_and(|connection| connection.configuration.base != base.get())
            || !key.get().is_empty()
            || clear.get()
    });
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let execute = StoredValue::new_local(move |save: bool| {
        if pending.get_untracked() || loading.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = if save {
            let Some(connection) = original.get_untracked() else {
                return;
            };
            Some(
                serde_json::to_value(TempoConfigurationUpdate {
                    expected_revision: connection.configuration.revision,
                    expected_jira_revision: connection.jira_revision,
                    base: base.get_untracked(),
                    token: (!key.get_untracked().is_empty()).then(|| key.get_untracked()),
                    clear_token: clear.get_untracked(),
                })
                .expect("Tempo configuration serialization"),
            )
        } else {
            None
        };
        if save {
            pending.set(true);
        } else {
            loading.set(true);
        }
        error.set(String::new());
        notice.set(String::new());
        reload.set(false);
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                let response = if let Some(body) = body {
                    write(&credential, Method::PUT, "/api/tempo/config", &body).await?
                } else {
                    read(&credential, "/api/tempo/config").await?
                };
                response
                    .json::<TempoConnection>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(connection) => {
                        base.set(connection.configuration.base.clone());
                        original.set(Some(connection));
                        key.set(String::new());
                        clear.set(false);
                        if save {
                            notice.set("message.tempo_connection_saved_remote_access_and_export_have_not_been_ve".into());
                        }
                    }
                    Err(message) => error.set(message),
                }
                loading.set(false);
                if save {
                    pending.set(false);
                }
            }
        });
    });
    Effect::new(move |_| execute.with_value(|execute| execute(false)));
    view! {
        <section
            id="tempo-settings"
            aria-labelledby="tempo-settings-heading"
            aria-busy={move || loading.get().to_string()}
        >
            <h3 id="tempo-settings-heading">"Tempo Cloud"</h3>
            <p>
                {move || {
                    i18n.text("message.the_tempo_token_is_stored_in_the_vault_choose_its_region_changin")
                }}
            </p>
            <p>
                {move || i18n.text("message.associated_jira_site")}" "
                {move || {
                    original
                        .get()
                        .map(|connection| connection.jira_url)
                        .filter(|url| !url.is_empty())
                        .unwrap_or_else(|| i18n.text("message.no_jira_site_configured"))
                }}
            </p>
            <fieldset disabled={move || pending.get() || loading.get() || original.get().is_none()}>
                <crate::form::Field
                    id="tempo-base"
                    label={Signal::derive(move || i18n.text("message.tempo_region"))}
                >
                    <crate::form::Select
                        {..}
                        id="tempo-base"
                        prop:value={move || base.get()}
                        on:change={move |event| base.set(event_target_value(&event))}
                    >
                        <option value="https://api.tempo.io/4">
                            {move || i18n.text("message.automatic")}
                        </option>
                        <option value="https://api.eu.tempo.io/4">
                            {move || i18n.text("message.europe")}
                        </option>
                        <option value="https://api.us.tempo.io/4">
                            {move || i18n.text("message.americas")}
                        </option>
                    </crate::form::Select>
                </crate::form::Field>
                <p>
                    {move || {
                        i18n.text(
                            if original.get().is_some_and(|connection| connection.token_set) {
                                "message.tempo_token_stored"
                            } else {
                                "message.no_tempo_token_stored"
                            },
                        )
                    }}
                </p>
                <crate::form::Field
                    id="tempo-token"
                    label={Signal::derive(move || i18n.text("message.new_tempo_token"))}
                >
                    <crate::form::Input
                        input_type="password"
                        reveal=true
                        disabled={Signal::derive(move || clear.get())}
                        {..}
                        id="tempo-token"
                        autocomplete="off"
                        maxlength="4000"
                        prop:value={move || key.get()}
                        on:input={move |event| key.set(event_target_value(&event))}
                    />
                </crate::form::Field>
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="tempo-clear-token"
                        prop:checked={move || clear.get()}
                        on:change={move |event| {
                            clear.set(event_target_checked(&event));
                            if clear.get_untracked() {
                                key.set(String::new());
                            }
                        }}
                    />
                    {move || i18n.text("message.remove_tempo_token")}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || !own_dirty.get())}
                    {..}
                    id="save-tempo"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute(true))}
                >
                    {move || i18n.text("message.save_tempo")}
                </crate::button::Button>
            </fieldset>
            <crate::button::Button
                disabled={Signal::derive(move || pending.get() || loading.get())}
                {..}
                id="reload-tempo"
                type="button"
                on:click={move |_| {
                    if own_dirty.get_untracked() {
                        reload.set(true);
                    } else {
                        execute.with_value(|execute| execute(false));
                    }
                }}
            >
                {move || i18n.text("message.reload_tempo")}
            </crate::button::Button>
            <crate::modal::Confirmation error=Signal::derive(move||i18n.text(&error.get())) open=Signal::derive(move||!(!reload.get())) busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|reload.set(false))>
                <p>{move || i18n.text("message.discard_the_tempo_draft_and_read_the_connection_again")}</p>
                <crate::modal::ModalActions><crate::button::Button
                    {..}
                    data-confirmation-cancel=""
                    autofocus=true
                    type="button"
                    on:click={move |_| reload.set(false)}
                >
                    {move || i18n.text("message.back")}
                </crate::button::Button><crate::button::Button
                    {..}
                    id="confirm-reload-tempo"
                    type="button"
                    on:click={move |_| execute.with_value(|execute| execute(false))}
                >
                    {move || i18n.text("message.discard_and_reload")}
                </crate::button::Button></crate::modal::ModalActions>
            </crate::modal::Confirmation>
            <p id="tempo-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p id="tempo-notice" role="status">
                {move || i18n.text(&notice.get())}
            </p>
        </section>
    }
}
