use crate::i18n::I18n;
use crate::{field, live::BoardState, read, write};
use atelier_core::document::Document;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

async fn list(token: &str) -> Result<Vec<Document>, String> {
    let response: Document = read(token, "/api/backups")
        .await?
        .json()
        .await
        .map_err(|_| "message.invalid_server_response")?;
    response
        .get("backups")
        .map_err(|_| "message.the_backup_list_is_invalid")?
        .ok_or_else(|| "message.the_backup_list_is_missing".into())
}

#[component]
pub fn Backups(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let items = RwSignal::new(Vec::<Document>::new());
    let selected = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(None::<(&'static str, String)>);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let close = move || {
        if pending.get_untracked() {
            return;
        }
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
        request_animation_frame(move || {
            if let Some(element) = document()
                .get_element_by_id("manage-backups")
                .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = element.focus();
            }
        });
    };
    let refresh = move || {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            match list(&token).await {
                Ok(backups) => items.set(backups),
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    refresh();
    let execute = move |restore: bool| {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let name = selected.get_untracked();
        if restore && name.is_empty() {
            return;
        }
        pending.set(true);
        error.set(String::new());
        notice.set(None);
        spawn_local(async move {
            let result = async {
                let (path, body) = if restore {
                    (
                        "/api/backups/restore",
                        serde_json::json!({"name":name,"confirmed":true}),
                    )
                } else {
                    ("/api/backups", serde_json::json!({}))
                };
                let response: Document = write(&token, Method::POST, path, &body)
                    .await?
                    .json()
                    .await
                    .map_err(|_| "message.invalid_server_response")?;
                notice.set(Some(if restore {
                    (
                        "message.restore_complete_the_previous_state_was_saved_in_0_connections_m",
                        field(&response, "rescue"),
                    )
                } else {
                    ("message.backup_created", String::new())
                }));
                selected.set(String::new());
                if restore {
                    let _ = board.refresh(&token).await;
                }
                items.set(list(&token).await?);
                Ok::<(), String>(())
            }
            .await;
            if let Err(message) = result {
                error.set(message);
            }
            pending.set(false);
        });
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <crate::modal::Modal
            id="backups-dialog"
            class="editor"
            labelledby="backups-title"
            node_ref={dialog}
            busy={Signal::derive(move || pending.get())}
            on_close={Callback::new(move |()| close())}
        >
            <crate::modal::ModalHeader class="inset" title_id="backups-title" title=Signal::derive(move||i18n.text("message.backups_and_restore")) close_id="close-backups" busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|close())/>
            <p class="field-help">
                {move || i18n.text("message.backups_preserve_application_data_repository_and_worktree_files")}
            </p>
            <div class="editor-actions">
                <crate::button::Button
                    variant={crate::button::ButtonVariant::Primary}
                    disabled={Signal::derive(move || pending.get())}
                    class="primary"
                    {..}
                    id="create-backup"
                    type="button"
                    autofocus=true
                    on:click={move |_| execute(false)}
                >
                    {move || i18n.text("message.create_backup")}
                </crate::button::Button>
                <crate::button::Button
                    disabled={Signal::derive(move || pending.get())}
                    {..}
                    id="refresh-backups"
                    type="button"
                    on:click={move |_| refresh()}
                >
                    {move || i18n.text("message.refresh")}
                </crate::button::Button>
            </div>
            <p id="backups-notice" class="field-help" role="status">
                {move || {
                    if pending.get() {
                        i18n.text("message.working")
                    } else {
                        notice
                            .get()
                            .map(|(key, value)| i18n.text(key).replace("{0}", &value))
                            .unwrap_or_default()
                    }
                }}
            </p>
            <p id="backups-error" class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <div
                class="backup-list"
                aria-label={move || i18n.text("message.local_backups")}
                aria-busy={move || pending.get().to_string()}
            >
                <For
                    each={move || items.get()}
                    key={|backup| field(backup, "name")}
                    children={move |backup| {
                        let name = StoredValue::new(field(&backup, "name"));
                        let date = field(&backup, "date").replace('T', " ").replace('Z', " UTC");
                        let size = backup.get::<u64>("bytes").ok().flatten().unwrap_or(0).div_ceil(1024);
                        view! {
                            <crate::button::Button
                                disabled={Signal::derive(move || pending.get())}
                                class="ui-card-choice project-choice"
                                {..}
                                type="button"
                                data-backup={name.get_value()}
                                aria-pressed={move || (selected.get() == name.get_value()).to_string()}
                                on:click={move |_| selected.set(name.get_value())}
                            >
                                <strong>
                                    {move || {
                                        format!(
                                            "{date} · {size} {}",
                                            if i18n.code() == "en-US" { "KiB" } else { "Kio" },
                                        )
                                    }}
                                </strong>
                                <small>{name.get_value()}</small>
                            </crate::button::Button>
                        }
                    }}
                />
                <p class="field-help" hidden={move || pending.get() || !items.get().is_empty()}>
                    {move || i18n.text("message.no_backups_available")}
                </p>
            </div>
            <crate::modal::Confirmation error=Signal::derive(move||i18n.text(&error.get())) open=Signal::derive(move||!(selected.get().is_empty())) busy=Signal::derive(move||pending.get()) on_close=Callback::new(move|()|selected.set(String::new()))>
                <p>
                    {move || {
                        i18n.text("message.restoring_this_backup_will_replace_current_tasks_projects_settin")
                    }}
                </p>
                <p class="backup-name">{move || selected.get()}</p>
                <crate::modal::ModalActions><crate::button::Button
                    disabled={Signal::derive(move || pending.get())}
                    {..}
                    data-confirmation-cancel=""
                    autofocus=true
                    id="cancel-restore"
                    type="button"
                    on:click={move |_| selected.set(String::new())}
                >
                    {move || i18n.text("message.cancel")}
                </crate::button::Button><crate::button::Button
                    variant={crate::button::ButtonVariant::Danger}
                    disabled={Signal::derive(move || pending.get())}
                    class="danger"
                    {..}
                    id="restore-backup"
                    type="button"
                    on:click={move |_| execute(true)}
                >
                    {move || i18n.text("message.restore_this_backup")}
                </crate::button::Button></crate::modal::ModalActions>
            </crate::modal::Confirmation>
        </crate::modal::Modal>
    }
}
