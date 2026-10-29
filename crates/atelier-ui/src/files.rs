use crate::{i18n::I18n, read};
use atelier_core::contracts::{ChangeListing, FileChange, FileDiff, FileListing, FilePreview};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

const PAGE_SIZE: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    Files,
    Changes,
}

#[component]
pub fn Files(
    card_id: String,
    token: StoredValue<Result<String, String>>,
    mode: Signal<FileMode>,
    active: Signal<bool>,
    revision: Signal<u64>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let files = RwSignal::new(Vec::<FileChange>::new());
    let filter = RwSignal::new(String::new());
    let page = RwSignal::new(0usize);
    let selected = RwSignal::new(None::<(String, bool)>);
    let content = RwSignal::new(String::new());
    let unavailable = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let loaded = RwSignal::new(None::<(FileMode, u64)>);
    let stale = Memo::new(move |_| loaded.get() != Some((mode.get(), revision.get())));
    let base = RwSignal::new(String::new());
    let matching = Memo::new(move |_| {
        let query = filter.get().to_lowercase();
        files
            .get()
            .into_iter()
            .filter(|entry| entry.path.to_lowercase().contains(&query))
            .collect::<Vec<_>>()
    });
    let load = move |selection: Option<FileChange>| {
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
        let card = id.get_value();
        let changes = mode.get_untracked() == FileMode::Changes;
        if selection.is_none() {
            loaded.set(Some((mode.get_untracked(), revision.get_untracked())));
            files.set(Vec::new());
            base.set(String::new());
            page.set(0);
        }
        selected.set(
            selection
                .as_ref()
                .map(|entry| (entry.path.clone(), entry.untracked)),
        );
        content.set(String::new());
        unavailable.set(String::new());
        error.set(String::new());
        pending.set(true);
        spawn_local(async move {
            let result = async {
                if let Some(entry) = selection {
                    let params = web_sys::UrlSearchParams::new()
                        .map_err(|_| "message.invalid_file_address")?;
                    params.set("path", &entry.path);
                    if changes {
                        params.set("untracked", &entry.untracked.to_string());
                    }
                    let action = if changes { "file-diff" } else { "file" };
                    let response = read(
                        &token,
                        &format!(
                            "/api/cards/{card}/{action}?{}",
                            String::from(params.to_string())
                        ),
                    )
                    .await?;
                    let (text, reason) = if changes {
                        let response: FileDiff = response
                            .json()
                            .await
                            .map_err(|_| "message.invalid_server_response")?;
                        (response.patch, response.unavailable)
                    } else {
                        let response: FilePreview = response
                            .json()
                            .await
                            .map_err(|_| "message.invalid_server_response")?;
                        (response.content, response.unavailable)
                    };
                    if pending.try_get_untracked().is_some() {
                        content.set(text.unwrap_or_default());
                        unavailable.set(reason.unwrap_or_default());
                    }
                } else if changes {
                    let response: ChangeListing =
                        read(&token, &format!("/api/cards/{card}/changes"))
                            .await?
                            .json()
                            .await
                            .map_err(|_| "message.invalid_server_response")?;
                    if pending.try_get_untracked().is_some() {
                        base.set(response.base);
                        files.set(response.files);
                    }
                } else {
                    let response: FileListing = read(&token, &format!("/api/cards/{card}/files"))
                        .await?
                        .json()
                        .await
                        .map_err(|_| "message.invalid_server_response")?;
                    if pending.try_get_untracked().is_some() {
                        files.set(
                            response
                                .files
                                .into_iter()
                                .map(|path| FileChange {
                                    path,
                                    status: String::new(),
                                    untracked: false,
                                })
                                .collect(),
                        );
                        page.set(0);
                    }
                }
                Ok::<(), String>(())
            }
            .await;
            if pending.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    };
    Effect::new(move |_| {
        // Finish an in-flight read before loading the requested mode or worktree.
        // The stale browser remains hidden until that new listing is loaded.
        if active.get() && stale.get() && !pending.get() {
            load(None);
        }
    });
    view! {
            <div class="file-explorer" aria-busy=move || (pending.get() || stale.get()).to_string()>
                <div class="file-toolbar"><div><h3>{move ||i18n.text(if mode.get()==FileMode::Changes {"message.task_changes"} else {"message.project_files"})}</h3><p class="hint file-context">{move || i18n.text("message.read_only_tracked_files_and_new_files_not_ignored_by_git")}</p></div><button id="refresh-files" class="refresh-files" type="button" disabled=move || pending.get() on:click=move |_| load(None)>{move || i18n.text("message.refresh")}</button></div>
                <p class="field-help" hidden=move || mode.get() != FileMode::Changes>{move || i18n.text("message.compared_with_the_worktree_s_starting_point_or_the_main_reposito")}</p>
                <p class="field-help" hidden=move || base.get().is_empty()>{move || i18n.text("message.base_commit_0").replace("{0}", &base.get())}</p>
                <p id="file-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
                <div class="file-browser" hidden=move || stale.get()><aside class="file-sidebar">
                    <div class="file-sidebar-heading">{move ||i18n.text(if mode.get()==FileMode::Changes {"message.changes"} else {"message.explorer"})}<span class="file-count">{move ||matching.with(Vec::len)}</span></div>
                    <label class="file-search" for="file-search"><span class="sr-only">{move || i18n.text("message.filter_files")}</span><input id="file-search" class="file-filter" type="search" placeholder=move ||i18n.text("message.filter_files") prop:value=move || filter.get() on:input=move |event| { filter.set(event_target_value(&event)); page.set(0); } /></label>
                    <p class="file-message" role="status">{move ||if pending.get(){i18n.text("message.loading_files")}else{String::new()}}</p>
                    <nav class="file-list" aria-label=move || i18n.text("message.project_files")><For each={move || matching.with(|files| files.iter().skip(page.get()*PAGE_SIZE).take(PAGE_SIZE).cloned().collect::<Vec<_>>())} key=|entry| (entry.path.clone(), entry.untracked, entry.status.clone()) children=move |entry| {
                        let name = StoredValue::new(entry);
                        view! { <button type="button" class="file-entry" title=name.get_value().path data-file=name.get_value().path aria-current=move || (selected.get() == Some((name.get_value().path, name.get_value().untracked))).to_string() disabled=move || pending.get() on:click=move |_| load(Some(name.get_value()))><svg class="file-icon file-code-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="M14 3H5v18h14V8zM14 3v5h5M8 12h8M8 16h6"/></svg><span class="file-name">{if name.get_value().status.is_empty() { String::new() } else { format!("{} · ", name.get_value().status) }}{name.get_value().path}</span></button> }
                    } /><p class="field-help" hidden=move || pending.get() || matching.with(|files| !files.is_empty())>{move || i18n.text("message.no_files_found")}</p></nav>
                    <div class="file-pages"><button id="previous-files" type="button" disabled=move || page.get()==0 on:click=move |_| page.update(|page| *page = page.saturating_sub(1))>{move || i18n.text("message.previous")}</button><span>{move || format!("{} / {}",page.get()+1,matching.with(Vec::len).div_ceil(PAGE_SIZE).max(1))}</span><button id="next-files" type="button" disabled={move || (page.get()+1)*PAGE_SIZE >= matching.with(Vec::len)} on:click=move |_| page.update(|page| *page += 1)>{move || i18n.text("message.next")}</button></div>
                </aside><section class="file-preview" aria-label=move || i18n.text("message.file_preview")><h4 class="file-title">{move || selected.get().map(|(path, _)| path).unwrap_or_default()}</h4><p class="hint" hidden=move || selected.get().is_some()>{move || i18n.text("message.select_a_file_to_read_it")}</p><p id="file-unavailable" role="status">{move || i18n.text(&unavailable.get())}</p><p class="hint" hidden=move || pending.get() || selected.get().is_none() || !content.get().is_empty() || !unavailable.get().is_empty() || !error.get().is_empty()>{move || i18n.text("message.no_text_content_to_display")}</p><div id="file-content" class="code-view" tabindex="0" hidden=move || selected.get().is_none() || !unavailable.get().is_empty()>{move || {
        let text = content.get();
        let changes = mode.get() == FileMode::Changes;
        let numbers = if changes { atelier_core::domain::files::diff_line_numbers(&text) } else { Vec::new() };
        text.lines().take(10_000).enumerate().map(|(index,line)| {
            if changes {
                let (old,new)=numbers[index];
                if old.is_none() && new.is_none() {
                    return view!{<div class=if line.starts_with("@@") {"diff-hunk"} else {"diff-meta"}>{line.to_owned()}</div>}.into_any();
                }
                let class=if line.starts_with('+'){"code-line diff-line line-added"}else if line.starts_with('-'){"code-line diff-line line-removed"}else{"code-line diff-line"};
                view!{<div class=class><span class="line-number">{old.map(|line|line.to_string()).unwrap_or_default()}</span><span class="line-number">{new.map(|line|line.to_string()).unwrap_or_default()}</span><code>{line.to_owned()}</code></div>}.into_any()
            } else {
                view!{<div class="code-line"><span class="line-number">{index+1}</span><code>{line.to_owned()}</code></div>}.into_any()
            }
        }).collect_view()
    }}</div></section></div>
            </div>
        }
}
