use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::application::worktrees::ServiceWorktrees;
use atelier_core::contracts::{ForgeConfiguration, ForgeKind, Worktree, WorktreeState};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Workspace(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    on_change: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let current = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .and_then(|card| card.fields.get::<Worktree>("worktree").ok().flatten())
        })
    });
    let defaults = move || {
        let card = board
            .cards
            .get_untracked()
            .into_iter()
            .find(|card| card.id == id.get_value());
        let branch = card
            .as_ref()
            .map(|card| field(&card.fields, "branch"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("atelier/{}", id.get_value()));
        let base = card
            .as_ref()
            .map(|card| field(&card.fields, "baseBranch"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| field(&board.settings.get_untracked(), "baseBranch"));
        (branch, if base.is_empty() { "HEAD".into() } else { base })
    };
    let initial = defaults();
    let branch = RwSignal::new(initial.0);
    let base = RwSignal::new(initial.1);
    let fetch_origin = RwSignal::new(false);
    let fetch_forge = RwSignal::new("local".to_owned());
    let fetch_repository = RwSignal::new(String::new());
    let fetch_revision = RwSignal::new(0u64);
    let fetch_site = RwSignal::new(String::new());
    let forge_kind = move || match fetch_forge.get_untracked().as_str() {
        "github" => Some(ForgeKind::Github),
        "gitlab" => Some(ForgeKind::Gitlab),
        "azure" => Some(ForgeKind::Azure),
        _ => None,
    };
    let snapshot_forge = move || {
        let settings = board.settings.get_untracked();
        fetch_revision.set(
            settings
                .get::<u64>("forgeRevision")
                .ok()
                .flatten()
                .unwrap_or(0),
        );
        let connections = settings
            .get::<std::collections::BTreeMap<ForgeKind, ForgeConfiguration>>("forgeConnections")
            .ok()
            .flatten()
            .unwrap_or_default();
        fetch_site.set(
            forge_kind()
                .and_then(|kind| connections.get(&kind).cloned())
                .map(|configuration| configuration.base)
                .unwrap_or_default(),
        );
    };
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let confirming = RwSignal::new(false);
    let discard = RwSignal::new(false);
    let act = move |remove: bool| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!(
            "/api/cards/{}/worktree{}",
            id.get_value(),
            if remove { "/remove" } else { "" }
        );
        let body = if remove {
            serde_json::json!({"confirmed":true,"discardChanges":discard.get_untracked()})
        } else {
            {
                let remote = fetch_origin.get_untracked().then(forge_kind).flatten();
                serde_json::json!({"branch":branch.get_untracked(),"base":base.get_untracked(),"fetchOrigin":fetch_origin.get_untracked(),"fetchForge":remote,"fetchRepository":if remote.is_some(){fetch_repository.get_untracked()}else{String::new()},"forgeRevision":remote.map(|_|fetch_revision.get_untracked())})
            }
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        on_change.run(());
        spawn_local(async move {
            let result = write(&token, Method::POST, &path, &body).await;
            let _ = board.refresh(&token).await;
            if pending.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok(_) => {
                    dirty.set(false);
                    confirming.set(false);
                    discard.set(false);
                    notice.set(
                        if remove {
                            "message.worktree_removed_the_git_branch_is_preserved"
                        } else {
                            "message.worktree_ready"
                        }
                        .into(),
                    );
                    request_animation_frame(move || {
                        if let Some(element) = web_sys::window()
                            .and_then(|window| window.document())
                            .and_then(|document| {
                                document.get_element_by_id(if remove {
                                    "create-worktree"
                                } else {
                                    "verify-worktree"
                                })
                            })
                            && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
                        {
                            let _ = element.focus();
                        }
                    });
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    view! {
        <section class="task-files" aria-labelledby="workspace-heading" aria-busy=move || pending.get().to_string()>
            <h3 id="workspace-heading">{move || i18n.text("message.git_workspace")}</h3>
            <p class="field-help">{move || i18n.text("message.a_worktree_isolates_this_task_s_files_in_a_dedicated_folder_and")}</p>
            <p class="field-help">{move || i18n.text("message.preparation_and_verification_refresh_local_agent_context_git_tra")}</p>
            <p class="field-help">{move || i18n.text("message.ignored_node_modules_dependencies_and_nuget_packages_are_shared")}</p>
            <p class="field-help" hidden=move || !blocked.get()>{move || i18n.text("message.save_or_discard_the_task_edits_before_managing_its_worktree")}</p>
            <form hidden=move || current.get().is_some() on:submit=move |event| { event.prevent_default(); act(false); }>
                <fieldset disabled=move || pending.get() || blocked.get()>
                    <label for="worktree-branch">{move || i18n.text("message.task_branch")}</label><input id="worktree-branch" required maxlength="180" prop:value=move || branch.get() on:input=move |event| { branch.set(event_target_value(&event)); dirty.set(true); } />
                    <label for="worktree-base">{move || i18n.text("message.starting_reference")}</label><input id="worktree-base" required maxlength="180" prop:value=move || base.get() on:input=move |event| { base.set(event_target_value(&event)); dirty.set(true); } />
                    <label class="check-filter"><input id="worktree-fetch" type="checkbox" prop:checked=move || fetch_origin.get() on:change=move |event| { fetch_origin.set(event_target_checked(&event));snapshot_forge(); dirty.set(true); } />{move || i18n.text("message.refresh_the_reference_from_origin")}</label>
                    <p class="field-help">{move || i18n.text("message.without_a_refresh_the_reference_must_exist_locally_refreshing_fe")}</p>
                    <fieldset hidden=move||!fetch_origin.get()><label for="worktree-fetch-forge">{move||i18n.text("message.connection_for_refreshing_origin")}</label><select id="worktree-fetch-forge" prop:value=move||fetch_forge.get() on:change=move|event|{fetch_forge.set(event_target_value(&event));snapshot_forge();dirty.set(true);} ><option value="local">{move||i18n.text("message.authorized_local_repository")}</option><option value="github">"GitHub HTTPS"</option><option value="gitlab">"GitLab HTTPS"</option><option value="azure">"Azure DevOps HTTPS"</option></select>
                    <div hidden=move||fetch_forge.get()=="local"><p>{move||if fetch_site.get().is_empty(){i18n.text("message.configure_this_forge_in_settings_before_fetching")}else{fetch_site.get()}}</p><label for="worktree-fetch-repository">{move||i18n.text("message.allowed_remote_repository")}</label><input id="worktree-fetch-repository" maxlength="1000" placeholder=move||i18n.text("message.group_repository") prop:value=move||fetch_repository.get() on:input=move|event|{fetch_repository.set(event_target_value(&event));dirty.set(true);} /><p class="field-help">{move||i18n.text("message.the_site_repository_and_origin_https_url_must_match_the_vault_to")}</p><button id="reload-worktree-forge" type="button" on:click=move|_|{let Ok(credential)=token.get_value()else{return;};pending.set(true);spawn_local(async move{let result=board.refresh(&credential).await;if pending.try_get_untracked().is_some(){if let Err(message)=result{error.set(message);}else{snapshot_forge();}pending.set(false);}});}>{move||i18n.text("message.reload_selected_connection")}</button></div></fieldset>
                    <div class="editor-actions"><button id="create-worktree" type="submit">{move || i18n.text("message.prepare_worktree")}</button><button type="button" hidden=move || !dirty.get() on:click=move |_| { let initial = defaults(); branch.set(initial.0); base.set(initial.1); fetch_origin.set(false);fetch_forge.set("local".into());fetch_repository.set(String::new());fetch_site.set(String::new()); dirty.set(false); }>{move || i18n.text("message.reset")}</button></div>
                </fieldset>
            </form>
            <div hidden=move || current.get().is_none()>
                <p class="workspace-path">{move || current.get().map(|worktree| format!("{} · {}", worktree.branch, worktree.path)).unwrap_or_default()}</p>
                <p id="worktree-state" role="status">{move || i18n.text(match current.get().map(|worktree| worktree.state) { Some(WorktreeState::Ready) => "message.worktree_ready", Some(WorktreeState::Creating | WorktreeState::Removing) => "message.finish_or_verify_this_operation_files_are_preserved_until_remova", None => "" })}</p>
                {[("front", "Front"), ("back", "Back")].into_iter().map(move |(role, label)| {
                    let service = move || board.cards.with(|cards| cards.iter().find(|card| card.id == id.get_value()).and_then(|card| card.fields.get::<ServiceWorktrees>("serviceWorktrees").ok().flatten()).and_then(|mut worktrees| worktrees.remove(role)));
                    view! { <div class="service-worktree" data-role=role hidden=move || service().is_none()><strong>{label}</strong><p class="workspace-path">{move || service().map(|worktree| format!("{} · {}", worktree.branch, worktree.path)).unwrap_or_default()}</p><p role="status">{move || i18n.text(if service().is_some_and(|worktree| worktree.state == WorktreeState::Ready) { "message.worktree_ready" } else { "message.finish_or_verify_this_operation_files_are_preserved_until_remova" })}</p></div> }
                }).collect_view()}
                <div class="editor-actions"><button id="verify-worktree" type="button" disabled=move || pending.get() || blocked.get() on:click=move |_| act(false)>{move || i18n.text("message.verify_and_resume")}</button><button id="remove-worktree" type="button" class="danger" disabled=move || pending.get() || blocked.get() on:click=move |_| { discard.set(false); confirming.set(true); }>{move || i18n.text("message.remove_worktree")}</button></div>
            </div>
            <div class="confirmation" hidden=move || !confirming.get()>
                <p>{move || i18n.text("message.remove_the_main_worktree_and_its_associated_front_back_worktrees")}</p>
                <label class="check-filter"><input id="discard-worktree-files" type="checkbox" disabled=move || pending.get() prop:checked=move || discard.get() on:change=move |event| discard.set(event_target_checked(&event)) />{move || i18n.text("message.also_discard_modified_new_and_ignored_files_in_this_worktree_thi")}</label>
                <button id="confirm-remove-worktree" class="danger" type="button" disabled=move || pending.get() || blocked.get() on:click=move |_| act(true)>{move || i18n.text("message.confirm_removal")}</button>
                <button type="button" disabled=move || pending.get() on:click=move |_| confirming.set(false)>{move || i18n.text("message.cancel")}</button>
            </div>
            <p id="worktree-error" class="error" role="alert">{move || i18n.worktree_error(&error.get())}</p><p id="worktree-notice" role="status">{move || i18n.text(&notice.get())}</p>
        </section>
    }
}
