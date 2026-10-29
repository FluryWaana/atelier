use crate::files::{FileMode, Files};
use crate::i18n::I18n;
use crate::worktree::Workspace;
use crate::{STATUSES, field, live::BoardState, replace_card, write};
use atelier_core::contracts::Card;
use atelier_core::document::Document;
use atelier_core::domain::checklist::Criterion;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

const TASK_TABS: [(&str, &str); 4] = [
    ("details", "message.the_task"),
    ("activity", "message.agent_activity"),
    ("diff", "message.diff"),
    ("files", "message.files"),
];

#[derive(Clone, PartialEq)]
struct Draft {
    title: String,
    description: String,
    instructions: String,
    status: String,
    criteria: Vec<Criterion>,
    dependencies: Vec<String>,
    agent: String,
    model: String,
    effort: String,
}

impl Draft {
    fn from_card(card: &Card) -> Self {
        Self {
            title: String::from_utf16_lossy(&card.title.0),
            description: field(&card.fields, "description"),
            instructions: field(&card.fields, "instructions"),
            status: field(&card.fields, "status"),
            agent: field(&card.fields, "agent"),
            model: field(&card.fields, "model"),
            effort: field(&card.fields, "effort"),
            criteria: card
                .fields
                .get("checklist")
                .ok()
                .flatten()
                .unwrap_or_default(),
            dependencies: card
                .fields
                .get("blockedBy")
                .ok()
                .flatten()
                .unwrap_or_default(),
        }
    }

    fn fields(&self) -> serde_json::Value {
        serde_json::json!({"title":self.title, "description":self.description, "instructions":self.instructions, "status":self.status, "checklist":self.criteria, "blockedBy":self.dependencies,"agent":self.agent,"model":self.model,"effort":self.effort})
    }
    fn patch(&self, original: &Self) -> serde_json::Value {
        let current = self.fields();
        let previous = original.fields();
        let mut patch = serde_json::Map::new();
        let mut expected = serde_json::Map::new();
        for (key, value) in current.as_object().expect("draft fields are an object") {
            if value != &previous[key] {
                patch.insert(key.clone(), value.clone());
                expected.insert(key.clone(), previous[key].clone());
            }
        }
        patch.insert("expected".into(), expected.into());
        patch.into()
    }
}

#[derive(Clone, Copy)]
enum Action {
    Save,
    Archive,
    Duplicate,
    Delete,
    Template,
}

#[component]
pub fn TaskEditor(
    card: Card,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
    on_open: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let cards = board.cards;
    let templates = board.templates;
    let archived = card.fields.truthy("archived").unwrap_or(false);
    let no_repository = field(&card.fields, "repo").is_empty();

    let original = StoredValue::new(Draft::from_card(&card));
    let id = StoredValue::new(card.id);
    let status_label = move |status: &str| {
        let repo = board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .map(|card| field(&card.fields, "repo"))
                .unwrap_or_default()
        });
        board.projects.with(|projects| {
            crate::kanban_settings::project_column_name(i18n, projects, &repo, status)
        })
    };
    let ticket_info = Signal::derive(move || {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .and_then(|card| card.fields.get::<Document>("ticket").ok().flatten())
        })
    });
    let ticket = Memo::new(move |_| ticket_info.with(|ticket| ticket.is_some()));
    let draft = RwSignal::new(original.get_value());
    let dirty = Memo::new(move |_| draft.get() != original.get_value());
    let pending = RwSignal::new(false);
    let automation_active = Memo::new(move |_| {
        board.cards.with(|cards| {
            cards
                .iter()
                .find(|card| card.id == id.get_value())
                .is_some_and(|card| {
                    card.fields
                        .get::<atelier_core::application::automation::Flow>("automation")
                        .ok()
                        .flatten()
                        .is_some_and(|flow| {
                            flow.state == atelier_core::application::automation::State::Running
                        })
                })
        })
    });
    let workspace_dirty = RwSignal::new(false);
    let time_dirty = RwSignal::new(false);
    let creation_dirty = RwSignal::new(false);
    let forge_dirty = RwSignal::new(false);
    let commit_dirty = RwSignal::new(false);
    let connector_dirty = RwSignal::new(false);
    let reply_drafts = RwSignal::new(std::collections::BTreeSet::<String>::new());
    let initial_tab = if field(&card.fields, "status") == "review" && !no_repository {
        "diff"
    } else {
        "details"
    };
    let active_tab = RwSignal::new(initial_tab);
    let file_mode = RwSignal::new(if initial_tab == "diff" {
        FileMode::Changes
    } else {
        FileMode::Files
    });
    let files_revision = RwSignal::new(0u64);
    let select_tab = move |tab: &'static str| {
        if tab == "diff" {
            file_mode.set(FileMode::Changes);
        } else if tab == "files" {
            file_mode.set(FileMode::Files);
        }
        active_tab.set(tab);
    };
    let error = RwSignal::new(String::new());
    let confirmation = RwSignal::new("");
    let next_task = RwSignal::new(String::new());
    let show_template = RwSignal::new(false);
    let template_name = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let navigation = StoredValue::new(None::<Callback<()>>);
    let close_focus = StoredValue::new(String::new());
    let close = move || {
        let target = format!("card-open-{}", id.get_value());
        if let Some(next) = navigation.get_value() {
            navigation.set_value(None);
            next.run(());
            return;
        }
        on_close.run(());
        request_animation_frame(move || {
            if let Some(document) = web_sys::window().and_then(|window| window.document())
                && let Some(element) = document
                    .get_element_by_id(&target)
                    .or_else(|| document.get_element_by_id("task-title"))
                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = element.focus();
            }
        });
    };
    let navigate = Callback::new(move |target: String| {
        if target == id.get_value() {
            return;
        }
        if pending.get_untracked() {
            error.set("message.wait_for_the_operation_to_finish_before_switching_tasks".into());
            return;
        }
        navigation.set_value(None);
        if dirty.get_untracked()
            || workspace_dirty.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
            || !reply_drafts.with_untracked(std::collections::BTreeSet::is_empty)
            || !template_name.get_untracked().is_empty()
        {
            next_task.set(target);
            confirmation.set("switch");
        } else {
            close();
            request_animation_frame(move || on_open.run(target));
        }
    });
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked()
            || workspace_dirty.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
            || !reply_drafts.with_untracked(std::collections::BTreeSet::is_empty)
            || !template_name.get_untracked().is_empty()
        {
            confirmation.set("discard");
            close_focus.set_value(
                document()
                    .active_element()
                    .map(|element| element.id())
                    .unwrap_or_default(),
            );
            request_animation_frame(move || {
                if let Some(element) = document()
                    .get_element_by_id("confirm-editor")
                    .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    element.scroll_into_view();
                    let _ = element.focus();
                }
            });
        } else {
            close();
        }
    };
    let guard = expect_context::<crate::NavigationGuard>();
    guard.1.update(|generation| *generation += 1);
    let guard_generation = guard.1.get_untracked();
    guard.0.set(Some(Callback::new(move |next: Callback<()>| {
        if pending.get_untracked() {
            return;
        }
        navigation.set_value(Some(next));
        request_close();
    })));
    on_cleanup(move || {
        if guard.1.get_untracked() == guard_generation {
            guard.0.set(None);
        }
    });
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked()
            || workspace_dirty.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
            || !reply_drafts.with_untracked(std::collections::BTreeSet::is_empty)
            || pending.get_untracked()
            || !template_name.get_untracked().is_empty()
        {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let perform = move |action: Action| {
        if pending.get_untracked()
            || time_dirty.get_untracked()
            || creation_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || connector_dirty.get_untracked()
            || commit_dirty.get_untracked()
        {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let mut path = format!("/api/cards/{}", id.get_value());
        let value = draft.get_untracked();
        let (method, body) = match action {
            Action::Save => (Method::PATCH, value.patch(&original.get_value())),
            Action::Archive => {
                path.push_str("/archive");
                (Method::POST, serde_json::json!({"archived":!archived}))
            }
            Action::Duplicate => {
                path.push_str("/duplicate");
                (Method::POST, serde_json::json!({}))
            }
            Action::Delete => (Method::DELETE, serde_json::json!({})),
            Action::Template => {
                path = "/api/templates".into();
                (
                    Method::POST,
                    serde_json::json!({"cardId":id.get_value(), "name":template_name.get_untracked()}),
                )
            }
        };
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(&token, method, &path, &body).await?;
                if matches!(action, Action::Delete) {
                    cards.update(|cards| cards.retain(|card| card.id != id.get_value()));
                } else if matches!(action, Action::Template) {
                    let template: Document = response
                        .json()
                        .await
                        .map_err(|_| "message.invalid_server_response")?;
                    templates.update(|templates| {
                        if !templates
                            .iter()
                            .any(|item| field(item, "id") == field(&template, "id"))
                        {
                            templates.push(template);
                        }
                    });
                } else {
                    let card: Card = response
                        .json()
                        .await
                        .map_err(|_| "message.invalid_server_response")?;
                    if matches!(action, Action::Save) {
                        let saved = Draft::from_card(&card);
                        original.set_value(saved.clone());
                        draft.set(saved);
                    }
                    replace_card(cards, card);
                }
                Ok::<(), String>(())
            }
            .await;
            match result {
                Ok(()) => {
                    let _ = board.refresh(&token).await;
                    pending.set(false);
                    if matches!(action, Action::Template) {
                        template_name.set(String::new());
                        show_template.set(false);
                        notice.set("message.template_saved".into());
                    } else if matches!(action, Action::Save) {
                        notice.set("message.card_saved".into());
                    } else {
                        close();
                    }
                }
                Err(message) => {
                    let _ = board.refresh(&token).await;
                    error.set(message);
                    pending.set(false);
                }
            }
        });
    };
    let reload_draft = Callback::new(move |card: Card| {
        if !dirty.get_untracked() {
            let next = Draft::from_card(&card);
            original.set_value(next.clone());
            draft.set(next);
        }
    });
    // Hide panels without unmounting their forms, pending operations or draft guards.
    view! {
            <section id="task-page" class="task-workspace" aria-label=move ||i18n.text("message.task_details_a3f19152") on:keydown=move |event: web_sys::KeyboardEvent| {if event.key()=="Escape" && !event.default_prevented() && document().query_selector("dialog[open]").ok().flatten().is_none(){event.prevent_default();request_close();}}><div id="card-detail"><div id="task-editor" aria-labelledby="editor-heading">
                <header class="task-heading"><button id="close-editor" class="task-back" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("message.back_ef23a594")}</button><div class="task-heading-body"><div><div class="eyebrow">{move ||ticket_info.get().map(|ticket|field(&ticket,"key")).filter(|key|!key.is_empty()).unwrap_or_else(||i18n.text(if archived {"message.archive"} else {"message.local_task"}))}</div><h1 id="editor-heading" tabindex="-1">{move ||draft.get().title}</h1></div><span class="task-status">{move ||status_label(&draft.get().status)}</span></div></header>
                <nav class="task-tabs" role="tablist" aria-label=move || i18n.text("message.task_sections")>
                    {TASK_TABS.into_iter().enumerate().map(move |(index, (tab, label))| view! {
                        <button id=format!("task-tab-{tab}") type="button" role="tab" aria-controls=if matches!(tab,"diff"|"files") { "task-panel-repository".to_owned() } else { format!("task-panel-{tab}") } aria-selected=move || (active_tab.get()==tab).to_string() tabindex=move || if active_tab.get()==tab { 0 } else { -1 } on:click=move |_| select_tab(tab) on:keydown=move |event: leptos::ev::KeyboardEvent| {
                            let next = match event.key().as_str() {
                                "ArrowRight" => (index + 1) % TASK_TABS.len(),
                                "ArrowLeft" => (index + TASK_TABS.len() - 1) % TASK_TABS.len(),
                                "Home" => 0,
                                "End" => TASK_TABS.len() - 1,
                                _ => return,
                            };
                            event.prevent_default();
                            let tab = TASK_TABS[next].0;
                            select_tab(tab);
                            if let Some(document) = web_sys::window().and_then(|window| window.document())
                                && let Some(element) = document.get_element_by_id(&format!("task-tab-{tab}"))
                                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
                            {
                                let _ = element.focus();
                            }
                        }>{move || i18n.text(label)}</button>
                    }).collect_view()}
                </nav>
                <crate::time::TimerBanner board token nested=true on_task=navigate />
                <section id="task-panel-details" class="task-panel" role="tabpanel" aria-labelledby="task-tab-details" tabindex="0" hidden=move || active_tab.get() != "details">
                <div class="task-columns"><div class="task-main">
                {move ||ticket_info.get().map(|ticket| {let url=field(&ticket,"url");let key=field(&ticket,"key");let status=field(&ticket,"status");view!{<section class="task-files"><h3>{move || i18n.text("message.linked_issue")}</h3>{if url.starts_with("https://") && !url.chars().any(char::is_control) {view!{<a href=url target="_blank" rel="noopener noreferrer">{key}</a>}.into_any()}else{view!{<span>{key}</span>}.into_any()}}<p>{status}</p><p class="field-help">{move || i18n.text("message.the_title_and_description_of_this_card_are_preserved_locally_no")}</p></section>}})}
                {move||board.cards.with(|cards|cards.iter().find(|card|card.id==id.get_value()).map(|card|["brainstormQuestions","preparationQuestions"].into_iter().flat_map(|key|card.fields.get::<Vec<String>>(key).ok().flatten().unwrap_or_default()).collect::<Vec<_>>())).filter(|questions|!questions.is_empty()).map(|questions|view!{<section class="task-files"><h3>{move||i18n.text("message.questions_to_clarify")}</h3><ul>{questions.into_iter().map(|question|view!{<li>{question}</li>}).collect_view()}</ul></section>})}
                <form class="task-editor" on:submit=move |event| { event.prevent_default(); perform(Action::Save); } on:invalid:capture=move |event: web_sys::Event| {
                    select_tab("details");
                    if let Some(target) = event.target().and_then(|target| target.dyn_into::<web_sys::HtmlElement>().ok()) {
                        request_animation_frame(move || { let _ = target.focus(); });
                    }
                }>
                    <fieldset disabled=move || pending.get() || automation_active.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || archived || show_template.get()>
                        <label for="edit-title">{move || i18n.text("message.title")}</label><input id="edit-title" required maxlength="300" autofocus readonly=move ||ticket.get() prop:value=move || draft.get().title on:input=move |event| draft.update(|draft| draft.title = event_target_value(&event)) />
                        <label for="edit-description">{move || i18n.text("message.description")}</label><textarea id="edit-description" rows="5" maxlength="20000" readonly=move ||ticket.get() prop:value=move || draft.get().description on:input=move |event| draft.update(|draft| draft.description = event_target_value(&event))></textarea>
                        <details class="disclosure criteria" open><summary id="criteria-heading">{move || i18n.text("message.acceptance_criteria")}</summary><p class="hint">{move || i18n.text("message.check_criteria_after_verifying_them_up_to_20_criteria")}</p>
                            <For each={move || (0..draft.get().criteria.len()).collect::<Vec<_>>()} key=|index| *index children=move |index| view! {
                                <div class="criterion"><label><span class="sr-only">{move || i18n.text("message.criterion_0").replace("{0}", &(index + 1).to_string())}</span><input class="criterion-text" required maxlength="500" prop:value=move || draft.with(|draft| draft.criteria.get(index).map(|criterion| String::from_utf16_lossy(&criterion.text.0)).unwrap_or_default()) on:input=move |event| draft.update(|draft| { if let Some(criterion) = draft.criteria.get_mut(index) { criterion.text = event_target_value(&event).as_str().into(); } }) /></label>
                                <label class="check-filter"><input class="criterion-done" type="checkbox" prop:checked=move || draft.with(|draft| draft.criteria.get(index).is_some_and(|criterion| criterion.done)) on:change=move |event| draft.update(|draft| { if let Some(criterion) = draft.criteria.get_mut(index) { criterion.done = event_target_checked(&event); } }) />{move || i18n.text("message.verified")}</label>
                                <button type="button" aria-label=move || i18n.text("message.remove_criterion_0").replace("{0}", &(index + 1).to_string()) on:click=move |_| draft.update(|draft| { draft.criteria.remove(index); })>{move || i18n.text("message.remove")}</button></div>
                            } />
                            <button id="add-criterion" type="button" disabled={move || draft.get().criteria.len() >= 20} on:click=move |_| draft.update(|draft| draft.criteria.push(Criterion { text: "".into(), done: false }))>{move || i18n.text("message.add_a_criterion")}</button>
                        </details>
                        <div class="form-row"><div>                    <label for="edit-status">{move || i18n.text("message.local_progress")}</label><select id="edit-status" prop:value=move || draft.get().status on:change=move |event| draft.update(|draft| draft.status = event_target_value(&event))>{STATUSES.into_iter().map(move |(status, _)| view! { <option value=status>{move || status_label(status)}</option> }).collect_view()}</select>
    </div><div>                    <label for="edit-agent">{move||i18n.text("message.agent")}</label><select id="edit-agent" prop:value=move||draft.get().agent on:change=move|event|draft.update(|draft|{draft.agent=event_target_value(&event);draft.model.clear();draft.effort.clear();})><option value="" disabled>{move||i18n.text("message.choose_a_profile")}</option><For each=move||atelier_core::domain::agent_options::providers(&board.settings.get()).unwrap_or_default() key=|profile|field(profile,"id") children=move|profile|view!{<option value=field(&profile,"id")>{field(&profile,"name")}</option>} /></select>
    </div></div>
                        <details class="disclosure"><summary>{move ||i18n.text("message.model_and_reasoning")}</summary><div class="form-row"><div>                    <label for="edit-model">{move||i18n.text("message.task_model")}</label><input id="edit-model" maxlength="180" spellcheck="false" placeholder=move||i18n.text("message.inherit_from_the_profile_when_empty") prop:value=move||draft.get().model on:input=move|event|draft.update(|draft|draft.model=event_target_value(&event)) />
    </div><div>                    <label for="edit-effort">{move||i18n.text("message.task_effort")}</label><input id="edit-effort" maxlength="40" spellcheck="false" placeholder=move||i18n.text("message.inherit_from_the_profile_when_empty") prop:value=move||draft.get().effort on:input=move|event|draft.update(|draft|draft.effort=event_target_value(&event)) />
    </div></div></details>
                        <details class="disclosure dependencies"><summary id="dependencies-heading">{move || i18n.text("message.blocked_by")}</summary><p class="hint">{move || i18n.text("message.choose_prerequisite_tasks_completed_tasks_no_longer_block_the_wo")}</p>
                            <For each={move || cards.get().into_iter().filter(|card| card.id != id.get_value()).collect::<Vec<_>>()} key=|card| card.id.clone() children=move |card| {
                                let dependency = StoredValue::new(card.id);
                                view! { <label class="dependency-choice"><input type="checkbox" data-dependency=dependency.get_value() prop:checked=move || draft.with(|draft| draft.dependencies.contains(&dependency.get_value())) disabled=move || draft.with(|draft| draft.dependencies.len() >= 20 && !draft.dependencies.contains(&dependency.get_value())) on:change=move |event| draft.update(|draft| { let id = dependency.get_value(); if event_target_checked(&event) { if !draft.dependencies.contains(&id) { draft.dependencies.push(id); } } else { draft.dependencies.retain(|value| value != &id); } }) /><span>{String::from_utf16_lossy(&card.title.0)}</span></label> }
                            } />
                            <p class="field-help" hidden={move || cards.get().len() > 1}>{move || i18n.text("message.no_other_tasks_available")}</p>
                        </details>
                        <details class="disclosure"><summary>{move ||i18n.text("message.additional_instructions")}</summary>                    <label for="edit-instructions">{move || i18n.text("message.agent_instructions")}</label><textarea id="edit-instructions" rows="4" maxlength="20000" prop:value=move || draft.get().instructions on:input=move |event| draft.update(|draft| draft.instructions = event_target_value(&event))></textarea>
    </details>
                    </fieldset>
                    <div class="form-actions"><button id="save-task" class="primary" type="submit" hidden=archived disabled=move || pending.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || !dirty.get()>{move || i18n.text(if pending.get() { "message.saving" } else { "message.save" })}</button><span role="status">{move || i18n.text(if dirty.get() { "message.unsaved_changes" } else { "" })}</span></div>
                </form>
                </div><aside class="task-aside">
                <crate::commits::Commits card_id=id.get_value() board token pending dirty=commit_dirty blocked=Memo::new(move |_|dirty.get()||connector_dirty.get()||workspace_dirty.get()||time_dirty.get()||creation_dirty.get()||forge_dirty.get()||automation_active.get()||show_template.get()) archived />
                <crate::forge_task::ForgeTask card_id=id.get_value() board token pending dirty=forge_dirty blocked=Memo::new(move |_|dirty.get()||workspace_dirty.get()||time_dirty.get()||creation_dirty.get()||connector_dirty.get()||commit_dirty.get()||automation_active.get()||show_template.get()) archived />
                <crate::jira_creation::JiraCreation card_id=id.get_value() board token pending dirty=creation_dirty blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get()) archived />
                <crate::time::TimeTask card_id=id.get_value() board token pending dirty=time_dirty blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get()) archived />
                <Show when=move||!no_repository><Workspace card_id=id.get_value() board token pending dirty=workspace_dirty blocked=Memo::new(move |_| dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get()) on_change=Callback::new(move |()| files_revision.update(|revision| *revision += 1)) /></Show>
                <crate::connector_ticket::ConnectorTicket card_id=id.get_value() board token pending dirty=connector_dirty blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get() || archived) on_applied=reload_draft />
                <crate::jira_refresh::JiraRefresh card_id=id.get_value() board token pending blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get() || archived) on_applied=reload_draft />
                <crate::jira_task::JiraTask card_id=id.get_value() board token pending blocked=Memo::new(move |_|dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get()) />
                </aside></div></section>
                <section id="task-panel-activity" class="task-panel" role="tabpanel" aria-labelledby="task-tab-activity" tabindex="0" hidden=move || active_tab.get() != "activity">
                <crate::automation::Automation card_id=id.get_value() board token pending on_applied=reload_draft blocked=Memo::new(move |_|dirty.get()||workspace_dirty.get()||time_dirty.get()||creation_dirty.get()||forge_dirty.get()||connector_dirty.get()||commit_dirty.get()||show_template.get()||archived) />
                <crate::services::Services card_id=id.get_value() board token pending blocked=Memo::new(move |_| dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get()) />
                <crate::runs::Runs card_id=id.get_value() board token pending reply_drafts on_open=navigate on_applied=reload_draft blocked=Memo::new(move |_| dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get()) />
                </section>
                <section id="task-panel-repository" class="task-panel" role="tabpanel" aria-labelledby=move || if file_mode.get()==FileMode::Changes { "task-tab-diff" } else { "task-tab-files" } tabindex="0" hidden=move || !matches!(active_tab.get(),"diff"|"files")>
                    <Show when=move || !no_repository fallback=move || view! { <p class="field-help">{move || i18n.text("message.this_task_has_no_repository")}</p> }>
                        <Files card_id=id.get_value() token mode=Signal::derive(move || file_mode.get()) active=Signal::derive(move || matches!(active_tab.get(),"diff"|"files")) revision=Signal::derive(move || files_revision.get()) />
                    </Show>
                </section>
                <p id="editor-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
                <div class="editor-secondary"><button id="duplicate-task" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get() on:click=move |_| perform(Action::Duplicate)>{move || i18n.text("message.duplicate")}</button><button id="archive-task" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get() on:click=move |_| perform(Action::Archive)>{move || i18n.text(if archived { "message.restore" } else { "message.archive_8614cf00" })}</button><button id="delete-task" class="danger" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get() on:click=move |_| confirmation.set("delete")>{move || i18n.text("message.delete")}</button><button id="make-template" type="button" disabled=move || pending.get() || dirty.get() || workspace_dirty.get() || time_dirty.get() || creation_dirty.get() || forge_dirty.get() || connector_dirty.get() || commit_dirty.get() || automation_active.get() || show_template.get() on:click=move |_| { notice.set(String::new()); show_template.set(true); }>{move || i18n.text("message.create_template")}</button></div>
                <form class="template-form" hidden=move || !show_template.get() on:submit=move |event| { event.prevent_default(); perform(Action::Template); }><label for="template-name">{move || i18n.text("message.template_name")}</label><input id="template-name" required maxlength="100" disabled=move || pending.get() prop:value=move || template_name.get() on:input=move |event| template_name.set(event_target_value(&event)) /><p class="field-help">{move || i18n.text("message.the_template_includes_the_description_instructions_and_criteria")}</p><button id="save-template" class="primary" type="submit" disabled=move || pending.get()>{move || i18n.text("message.save_template")}</button><button type="button" disabled=move || pending.get() on:click=move |_| { template_name.set(String::new()); show_template.set(false); }>{move || i18n.text("message.cancel_template")}</button></form>
                <p id="editor-notice" role="status">{move || i18n.text(&notice.get())}</p>
                <div class="confirmation" role="alert" hidden=move || confirmation.get().is_empty()>
                    <p>{move || i18n.text(if confirmation.get() == "delete" { "message.permanently_delete_this_task_this_cannot_be_undone" } else { "message.close_without_saving_changes" })}</p>
                    <button id="confirm-editor" class="danger" type="button" disabled=move || pending.get() on:click=move |_| { if confirmation.get_untracked() == "delete" { perform(Action::Delete); } else if confirmation.get_untracked()=="switch" {let target=next_task.get_untracked();close();request_animation_frame(move ||on_open.run(target));} else { close(); } }>{move || i18n.text(if confirmation.get() == "delete" { "message.delete_permanently" } else { "message.discard_changes" })}</button>
                <button id="cancel-confirmation" type="button" disabled=move || pending.get() on:click=move |_| {navigation.set_value(None);confirmation.set("");if let Some(element)=document().get_element_by_id(&close_focus.get_value()).and_then(|element|element.dyn_into::<web_sys::HtmlElement>().ok()){let _=element.focus();}}>{move || i18n.text("message.keep_editing_746a82c6")}</button>
                </div>
            </div></div></section>
        }
}
