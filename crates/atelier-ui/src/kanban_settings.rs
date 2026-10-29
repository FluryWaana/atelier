use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    application::kanban::{self, Configuration, JiraMapping},
    contracts::{JiraBoardConfiguration, JiraConfiguration},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

pub fn column_name(i18n: I18n, status: &str, name: &str) -> String {
    if !name.trim().is_empty() {
        return name.to_owned();
    }
    i18n.text(
        crate::STATUSES
            .into_iter()
            .find(|(id, _)| *id == status)
            .map(|(_, label)| label)
            .unwrap_or("message.to_do"),
    )
}
pub fn project_column_name(i18n: I18n, projects: &[Document], repo: &str, status: &str) -> String {
    let name = projects
        .iter()
        .find(|project| field(project, "repo") == repo)
        .and_then(|project| kanban::for_project(project).ok())
        .and_then(|configuration| {
            configuration
                .columns
                .into_iter()
                .find(|column| column.status == status)
        })
        .map(|column| column.name)
        .unwrap_or_default();
    column_name(i18n, status, &name)
}
#[component]
pub fn KanbanSettings(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    repository: RwSignal<String>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let project_id = RwSignal::new(
        board
            .projects
            .get_untracked()
            .iter()
            .find(|project| field(project, "repo") == repository.get_untracked())
            .map(|project| field(project, "id"))
            .unwrap_or_default(),
    );
    let original = RwSignal::new(None::<Configuration>);
    let draft = RwSignal::new(Configuration::default());
    let board_id = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let discard = RwSignal::new(false);
    let dirty = Memo::new(move |_| {
        draft.get() != original.get().unwrap_or_default()
            || board_id.get()
                != original
                    .get()
                    .and_then(|configuration| {
                        configuration.jira.map(|jira| jira.board_id.to_string())
                    })
                    .unwrap_or_default()
    });
    let load = move |id: String| {
        let saved = board
            .projects
            .get_untracked()
            .iter()
            .find(|project| field(project, "id") == id)
            .map(kanban::for_project)
            .transpose();
        match saved {
            Ok(saved) => {
                let stored = board
                    .projects
                    .get_untracked()
                    .iter()
                    .find(|project| field(project, "id") == id)
                    .and_then(|project| project.get::<Configuration>("kanban").ok().flatten());
                original.set(stored);
                let saved = saved.unwrap_or_default();
                board_id.set(
                    saved
                        .jira
                        .as_ref()
                        .map(|jira| jira.board_id.to_string())
                        .unwrap_or_default(),
                );
                draft.set(saved);
                project_id.set(id);
                error.set(String::new());
                notice.set(String::new());
                discard.set(false);
            }
            Err(_) => error.set("message.invalid_kanban_columns".into()),
        }
    };
    load(project_id.get_untracked());
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |dialog| {
        let _ = dialog.show_modal();
    });
    let close = move || {
        on_close.run(());
        request_animation_frame(move || {
            if let Some(element) = document()
                .get_element_by_id("configure-columns")
                .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = element.focus();
            }
        });
    };
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() {
            discard.set(true);
        } else {
            close();
        }
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let connection = move || {
        board
            .settings
            .get()
            .get::<JiraConfiguration>("jiraConnection")
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let reading = alive.clone();
    let fetch = move |_| {
        if pending.get_untracked() || project_id.get_untracked().is_empty() {
            return;
        }
        let id = match board_id.get_untracked().parse::<u64>() {
            Ok(id) if id > 0 => id,
            _ => {
                error.set("message.invalid_jira_board_configuration".into());
                return;
            }
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let scope = connection();
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = reading.clone();
        spawn_local(async move {
            let result = async {
                let result: JiraBoardConfiguration = read(
                    &credential,
                    &format!(
                        "/api/jira/boards/{id}/configuration?revision={}",
                        scope.revision
                    ),
                )
                .await?
                .json()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())?;
                if result.board_id != id || result.connection_revision != scope.revision {
                    return Err(
                        "message.the_jira_connection_changed_reload_settings_before_trying_again"
                            .to_owned(),
                    );
                }
                Ok(result)
            }
            .await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(result) => {
                    board_id.set(id.to_string());
                    draft.update(|draft| {
                        let mut targets = draft
                            .jira
                            .as_ref()
                            .filter(|mapping| {
                                mapping.instance == scope.url
                                    && mapping.account == scope.email
                                    && mapping.mode == scope.mode
                                    && mapping.board_id == id
                            })
                            .map(|mapping| mapping.targets.clone())
                            .unwrap_or_default();
                        targets.retain(|id, _| {
                            result
                                .columns
                                .iter()
                                .any(|column| column.status_ids.contains(id))
                        });
                        draft.jira = Some(JiraMapping {
                            instance: scope.url,
                            account: scope.email,
                            mode: scope.mode,
                            board_id: id,
                            name: result.name,
                            columns: result.columns,
                            targets,
                        });
                    });
                    notice.set("message.jira_columns_loaded_check_mapping".into());
                }
                Err(message) => error.set(message),
            }
            if alive.get() {
                pending.set(false);
            }
        });
    };
    let writing = alive.clone();
    let save = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if pending.get_untracked()
            || !dirty.get_untracked()
            || project_id.get_untracked().is_empty()
        {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let id = project_id.get_untracked();
        let configuration = draft.get_untracked();
        if let Err(message) = configuration.validate() {
            error.set(message.into());
            return;
        }
        if configuration
            .jira
            .as_ref()
            .map(|jira| jira.board_id.to_string())
            .unwrap_or_default()
            != board_id.get_untracked()
        {
            error.set("message.load_jira_board_before_saving".into());
            return;
        }
        let body = serde_json::json!({"configuration":configuration,"expected":original.get_untracked(),"jiraRevision":connection().revision});
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = writing.clone();
        spawn_local(async move {
            let result = async {
                let saved: Document = write(
                    &credential,
                    Method::PUT,
                    &format!("/api/projects/{id}/kanban"),
                    &body,
                )
                .await?
                .json()
                .await
                .map_err(|_| "message.invalid_server_response".to_owned())?;
                Ok::<_, String>(saved)
            }
            .await;
            if !alive.get() {
                return;
            }
            let saved = result.is_ok();
            match result {
                Ok(saved) => {
                    repository.set(field(&saved, "repo"));
                    board.projects.update(|projects| {
                        if let Some(project) = projects
                            .iter_mut()
                            .find(|project| field(project, "id") == id)
                        {
                            *project = saved;
                        }
                    });
                    original.set(Some(configuration));
                    notice.set("message.kanban_columns_saved".into());
                }
                Err(message) => error.set(message),
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
                if saved {
                    crate::focus_element("close-kanban-settings");
                }
            }
        });
    };
    view! {<dialog id="kanban-settings" aria-busy=move||pending.get().to_string() node_ref=dialog aria-labelledby="kanban-settings-title" on:cancel=move|event:web_sys::Event|{event.prevent_default();request_close();}>
        <header class="dialog-heading"><h2 id="kanban-settings-title">{move||i18n.text("message.configure_kanban_columns")}</h2><button id="close-kanban-settings" type="button" disabled=move||pending.get() on:click=move |_|request_close()>{move||i18n.text("message.close")}</button></header>
        <label for="kanban-project">{move||i18n.text("message.project")}</label><select id="kanban-project" prop:value=move||project_id.get() disabled=move||pending.get()||dirty.get() on:change=move|event|load(event_target_value(&event))><option value="">{move||i18n.text("message.choose_a_project")}</option><For each=move||board.projects.get() key=|project|field(project,"id") children=move|project|view!{<option value=field(&project,"id")>{field(&project,"name")}</option>}/></select>
        <form hidden=move||project_id.get().is_empty() on:submit=save><fieldset disabled=move||pending.get()>
            <h3>{move||i18n.text("message.kanban_local_columns")}</h3><p class="field-help">{move||i18n.text("message.kanban_columns_keep_workflow")}</p>
            <ol class="kanban-column-editor"><For each=move||draft.get().columns key=|column|column.status.clone() children=move|column|{
                let status=StoredValue::new(column.status);view!{<li><label for=format!("kanban-name-{}",status.get_value())>{move||column_name(i18n,&status.get_value(),"")}</label><input id=format!("kanban-name-{}",status.get_value()) maxlength="100" placeholder=move||column_name(i18n,&status.get_value(),"") prop:value=move||draft.get().columns.iter().find(|column|column.status==status.get_value()).map(|column|column.name.clone()).unwrap_or_default() on:input=move|event|draft.update(|draft|{if let Some(column)=draft.columns.iter_mut().find(|column|column.status==status.get_value()){column.name=event_target_value(&event);}})/><button type="button" disabled=move||draft.get().columns.first().is_some_and(|column|column.status==status.get_value()) on:click=move |_|draft.update(|draft|{if let Some(index)=draft.columns.iter().position(|column|column.status==status.get_value())&&index>0{draft.columns.swap(index,index-1);}}) aria-label=move||i18n.text("message.move_column_up")>{move||i18n.text("message.move_column_up")}</button><button type="button" disabled=move||draft.get().columns.last().is_some_and(|column|column.status==status.get_value()) on:click=move |_|draft.update(|draft|{if let Some(index)=draft.columns.iter().position(|column|column.status==status.get_value())&&index+1<draft.columns.len(){draft.columns.swap(index,index+1);}}) aria-label=move||i18n.text("message.move_column_down")>{move||i18n.text("message.move_column_down")}</button></li>}
            }/></ol>
            <h3>{move||i18n.text("message.jira_column_matching")}</h3><p class="field-help">{move||i18n.text("message.kanban_mapping_manual_only")}</p>
            <label for="kanban-jira-board">{move||i18n.text("message.jira_board_id")}</label><input id="kanban-jira-board" inputmode="numeric" maxlength="20" prop:value=move||board_id.get() on:input=move|event|board_id.set(event_target_value(&event))/><button id="load-jira-columns" type="button" disabled=move||connection().url.is_empty()||board_id.get().is_empty() on:click=fetch>{move||i18n.text("message.load_jira_columns")}</button>
            <p class="field-help">{move||i18n.text("message.jira_board_id_help")}</p>
            <div hidden=move||draft.get().jira.is_none()><h4>{move||draft.get().jira.map(|jira|jira.name).unwrap_or_default()}</h4><For each={move||draft.get().jira.map(|jira|jira.columns).unwrap_or_default().into_iter().enumerate().collect::<Vec<_>>()} key=|(index,column)|(*index,column.name.clone(),column.status_ids.clone()) children=move|(index,column)|{
                let ids=StoredValue::new(column.status_ids);
                let target=move||draft.get().jira.map(|jira|{let values=ids.get_value().iter().map(|id|jira.targets.get(id).cloned()).collect::<std::collections::BTreeSet<_>>();if values.len()>1{"mixed".into()}else{values.into_iter().next().flatten().unwrap_or_default()}}).unwrap_or_default();
                view!{<div class="kanban-match-row"><label for=format!("kanban-jira-column-{index}")>{column.name}</label><select id=format!("kanban-jira-column-{index}") disabled=ids.get_value().is_empty() prop:value=target on:change=move|event|{let target=event_target_value(&event);draft.update(|draft|{if let Some(jira)=&mut draft.jira{for id in ids.get_value(){if target.is_empty(){jira.targets.remove(&id);}else{jira.targets.insert(id,target.clone());}}}});}>{move||(target()=="mixed").then(||view!{<option value="mixed" disabled>{move||i18n.text("message.column_mapping_needs_review")}</option>})}<option value="">{move||i18n.text("message.no_column_mapping")}</option><For each=move||draft.get().columns key=|column|column.status.clone() children=move|column|{let status=StoredValue::new(column.status);view!{<option value=status.get_value()>{move||{let name=draft.get().columns.iter().find(|column|column.status==status.get_value()).map(|column|column.name.clone()).unwrap_or_default();column_name(i18n,&status.get_value(),&name)}}</option>}}/></select></div>}
            }/><button type="button" on:click=move |_|{draft.update(|draft|draft.jira=None);board_id.set(String::new());}>{move||i18n.text("message.remove_jira_column_mapping")}</button></div>
            <div class="form-actions"><button id="save-kanban-columns" class="primary" type="submit" disabled=move||!dirty.get()>{move||i18n.text("message.save")}</button><button id="reset-kanban-columns" type="button" disabled=move||!dirty.get() on:click=move |_|load(project_id.get_untracked())>{move||i18n.text("message.cancel")}</button></div>
        </fieldset></form><p role="status" hidden=move||!pending.get()>{move||i18n.text("message.kanban_operation_in_progress")}</p><p class="error" role="alert">{move||i18n.text(&error.get())}</p><p role="status">{move||i18n.text(&notice.get())}</p>
        <div class="confirmation" hidden=move||!discard.get()><p>{move||i18n.text("message.discard_unsaved_changes")}</p><button type="button" disabled=move||pending.get() on:click=move |_|close()>{move||i18n.text("message.discard")}</button><button type="button" disabled=move||pending.get() on:click=move |_|discard.set(false)>{move||i18n.text("message.keep_editing")}</button></div>
    </dialog>}
}
