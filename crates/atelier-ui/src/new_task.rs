use crate::{field, focus_element, i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::Card;
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn NewTask(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    repository: RwSignal<String>,
    loading: RwSignal<bool>,
    pending: RwSignal<bool>,
    open_request: RwSignal<u64>,
    on_created: Callback<(String, String)>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let projects = board.projects;
    let templates = board.templates;
    let template = RwSignal::new(String::new());
    let remove_template = RwSignal::new(false);
    let template_pending = RwSignal::new(false);
    let title = RwSignal::new(String::new());
    let title_input = NodeRef::<leptos::html::Input>::new();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let description = RwSignal::new(String::new());
    let instructions = RwSignal::new(String::new());
    let repo = RwSignal::new(String::new());
    let agent = RwSignal::new(String::new());
    let initialized = RwSignal::new(false);
    let mode = RwSignal::new("edit".to_owned());
    let model = RwSignal::new(String::new());
    let effort = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let template_candidate = RwSignal::new(None::<String>);
    let busy = Memo::new(move |_| pending.get() || template_pending.get());
    let submit_blocked =
        Memo::new(move |_| busy.get() || loading.get() || template_candidate.get().is_some());
    let apply_template = Callback::new(move |id: String| {
        if let Some(item) = templates
            .with_untracked(|items| items.iter().find(|item| field(item, "id") == id).cloned())
        {
            description.set(field(&item, "description"));
            instructions.set(field(&item, "instructions"));
            mode.set(field(&item, "mode"));
            template.set(id);
        }
        template_candidate.set(None);
    });
    Effect::new(move |_| {
        if open_request.get() == 0 {
            return;
        }
        if !initialized.get_untracked() {
            let selected = repository.get_untracked();
            let selected = if selected.is_empty() {
                projects.with_untracked(|items| {
                    if items.len() == 1 {
                        field(&items[0], "repo")
                    } else {
                        board
                            .settings
                            .with_untracked(|settings| field(settings, "repo"))
                    }
                })
            } else {
                selected
            };
            let selected_agent = projects.with_untracked(|items| {
                items
                    .iter()
                    .find(|project| field(project, "repo") == selected)
                    .map(|project| field(project, "agent"))
                    .filter(|agent| !agent.is_empty())
                    .unwrap_or_else(|| {
                        board
                            .settings
                            .with_untracked(|settings| field(settings, "agent"))
                    })
            });
            repo.set(selected);
            agent.set(selected_agent);
            initialized.set(true);
        }
        if let Some(dialog) = dialog.get_untracked() {
            let _ = dialog.show_modal();
        }
        focus_element("task-title");
    });
    let close = move || {
        if !busy.get_untracked() {
            if let Some(dialog) = dialog.get_untracked() {
                dialog.close();
            }
            on_close.run(());
        }
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if busy.get_untracked()
            || !title.get_untracked().is_empty()
            || !description.get_untracked().is_empty()
            || !instructions.get_untracked().is_empty()
        {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if submit_blocked.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let selected_repo = repo.get_untracked();
        let mut body = serde_json::json!({
            "title": title.get_untracked(), "mode": mode.get_untracked(),
            "model": model.get_untracked(), "effort": effort.get_untracked()
        });
        for (key, value) in [
            ("description", description.get_untracked()),
            ("instructions", instructions.get_untracked()),
            ("agent", agent.get_untracked()),
            ("repo", selected_repo.clone()),
            ("templateId", template.get_untracked()),
        ] {
            if !value.is_empty() {
                body[key] = value.into();
            }
        }
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                let response = write(&credential, Method::POST, "/api/cards", &body).await?;
                let card: Card = response
                    .json()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())?;
                let id = card.id.clone();
                replace_card(board.cards, card);
                initialized.set(false);
                for signal in [title, description, instructions, template, model, effort] {
                    signal.set(String::new());
                }
                mode.set("edit".into());
                if let Some(dialog) = dialog.get_untracked() {
                    dialog.close();
                }
                let _ = board.refresh(&credential).await;
                on_created.run((id, selected_repo));
                Ok::<(), String>(())
            }
            .await;
            if let Err(message) = result {
                error.set(message);
            }
            pending.set(false);
        });
    };
    let apply_builtin = Callback::new(move |kind: &'static str| {
        let previous = description.get_untracked();
        let is_builtin = ["feature", "bug", "analysis"]
            .into_iter()
            .any(|kind| i18n.text(task_template(kind)) == previous);
        if !previous.trim().is_empty() && !is_builtin {
            error.set(
                "message.the_context_already_entered_is_kept_clear_it_to_apply_a_template".into(),
            );
            return;
        }
        template.set(String::new());
        description.set(i18n.text(task_template(kind)));
        mode.set(
            if kind == "analysis" {
                "analyze"
            } else {
                "edit"
            }
            .into(),
        );
        focus_element("task-title");
    });
    let select_template = move |event| {
        let id = event_target_value(&event);
        remove_template.set(false);
        if id.is_empty() {
            template.set(id);
        } else if !description.get_untracked().trim().is_empty()
            || !instructions.get_untracked().trim().is_empty()
        {
            template_candidate.set(Some(id));
        } else {
            apply_template.run(id);
        }
    };
    let delete_template = move |_| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let id = template.get_untracked();
        template_pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            match write(
                &credential,
                Method::DELETE,
                &format!("/api/templates/{id}"),
                &serde_json::json!({}),
            )
            .await
            {
                Ok(_) => {
                    templates.update(|items| items.retain(|item| field(item, "id") != id));
                    template.set(String::new());
                    remove_template.set(false);
                    let _ = board.refresh(&credential).await;
                }
                Err(message) => error.set(message),
            }
            template_pending.set(false);
        });
    };
    view! {
        <dialog
            id="new-dialog"
            node_ref={dialog}
            aria-labelledby="new-task-title"
            on:cancel={move |event: web_sys::Event| {
                event.prevent_default();
                close();
            }}
        >
            <form id="new-form" on:submit={submit}>
                <div class="dialog-heading">
                    <div>
                        <h2 id="new-task-title">{move || i18n.text("message.new_task")}</h2>
                    </div>
                    <button
                        type="button"
                        aria-label={move || i18n.text("message.close")}
                        disabled={move || busy.get()}
                        on:click={move |_| close()}
                    >
                        "×"
                    </button>
                </div>
                <details class="disclosure new-task-templates">
                    <summary>{move || i18n.text("message.task_templates")}</summary>
                    <div class="template-options" aria-label={move || i18n.text("message.task_templates")}>
                        <span>{move || i18n.text("message.start_from")}</span>
                        {[
                            ("feature", "message.feature"),
                            ("bug", "message.correction"),
                            ("analysis", "message.analysis"),
                        ]
                            .into_iter()
                            .map(move |(kind, label)| {
                                view! {
                                    <button
                                        type="button"
                                        data-template={kind}
                                        disabled={move || busy.get()}
                                        on:click={move |_| apply_builtin.run(kind)}
                                    >
                                        {move || i18n.text(label)}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                    <details class="disclosure" hidden={move || templates.get().is_empty()}>
                        <summary>{move || i18n.text("message.use_a_personal_template")}</summary>
                        <div class="template-picker">
                            <label for="task-template">{move || i18n.text("message.template")}</label>
                            <select

                                id="task-template"
                                disabled={move || busy.get()}
                                prop:value={move || template.get()}
                                on:change={select_template}
                            >
                                <option value="">{move || i18n.text("message.no_template")}</option>
                                <For
                                    each={move || templates.get()}
                                    key={|item| field(item, "id")}
                                    children={|item| {
                                        view! {
                                            <option value={field(&item, "id")}>{field(&item, "name")}</option>
                                        }
                                    }}
                                />
                            </select>

                            <button
                                id="remove-template"
                                type="button"
                                class="text-action"
                                hidden={move || template.get().is_empty()}
                                disabled={move || busy.get()}
                                on:click={move |_| remove_template.set(true)}
                            >
                                {move || i18n.text("message.remove_this_template")}
                            </button>
                        </div>
                    </details>
                </details>
                <section class="form-section">
                    <label for="task-title">
                        {move || i18n.text("message.title_required_3d8bf263")}
                        <input

                            node_ref={title_input}
                            id="task-title"
                            name="title"
                            required=true
                            maxlength="300"
                            disabled={move || busy.get()}
                            placeholder={move || i18n.text("message.e_g_add_a_csv_export_for_tasks")}
                            prop:value={move || title.get()}
                            on:input={move |event| title.set(event_target_value(&event))}
                        />
                    </label>
                    <label>
                        {move || i18n.text("message.description")}
                        <textarea

                            id="new-description"
                            name="description"
                            rows="5"
                            maxlength="20000"
                            disabled={move || busy.get()}
                            placeholder={move || i18n.text("message.context_expected_outcome_constraints")}
                            prop:value={move || description.get()}
                            on:input={move |event| description.set(event_target_value(&event))}
                        />
                    </label>
                </section>
                <section class="form-section execution-options">
                    <label>
                        {move || i18n.text("message.saved_project")}
                        <select

                            id="new-project"
                            disabled={move || busy.get()}
                            prop:value={move || repo.get()}
                            on:change={move |event| {
                                let selected = event_target_value(&event);
                                if let Some(selected_agent) = projects
                                    .with_untracked(|items| {
                                        items
                                            .iter()
                                            .find(|project| field(project, "repo") == selected)
                                            .map(|project| field(project, "agent"))
                                    })
                                    .filter(|agent| !agent.is_empty())
                                {
                                    agent.set(selected_agent);
                                }
                                repo.set(selected);
                            }}
                        >
                            <option value="">{move || i18n.text("message.choose_a_project_c85ff122")}</option>
                            <For
                                each={move || projects.get()}
                                key={|project| field(project, "id")}
                                children={move |project| {
                                    view! {
                                        <option value={field(
                                            &project,
                                            "repo",
                                        )}>{field(&project, "name")}</option>
                                    }
                                }}
                            />
                        </select>
                    </label>
                    <details class="disclosure">
                        <summary>{move || i18n.text("message.choose_another_directory")}</summary>
                        <label>
                            {move || i18n.text("message.source_git_repository")}
                            <input

                                name="repo"
                                disabled={move || busy.get()}
                                prop:value={move || repo.get()}
                                on:input={move |event| repo.set(event_target_value(&event))}
                                placeholder={move || i18n.text("message.absolute_project_path")}
                            />
                        </label>
                    </details>
                    <details class="disclosure new-task-agent">
                        <summary>{move || i18n.text("message.project_agent")}</summary>
                        <div class="form-row">
                            <label>
                                {move || i18n.text("message.agent")}
                                <select

                                    name="agent"
                                    disabled={move || busy.get()}
                                    prop:value={move || agent.get()}
                                    on:change={move |event| {
                                        agent.set(event_target_value(&event));
                                        model.set(String::new());
                                        effort.set(String::new());
                                    }}
                                >
                                    <option value="">
                                        {move || i18n.text("message.use_project_settings")}
                                    </option>
                                    <For
                                        each={move || {
                                            atelier_core::domain::agent_options::providers(
                                                    &board.settings.get(),
                                                )
                                                .unwrap_or_default()
                                        }}
                                        key={|profile| field(profile, "id")}
                                        children={move |profile| {
                                            view! {
                                                <option value={field(
                                                    &profile,
                                                    "id",
                                                )}>{field(&profile, "name")}</option>
                                            }
                                        }}
                                    />
                                </select>
                            </label>
                            <label>
                                {move || i18n.text("message.requested_work")}
                                <select

                                    name="mode"
                                    disabled={move || busy.get()}
                                    prop:value={move || mode.get()}
                                    on:change={move |event| mode.set(event_target_value(&event))}
                                >
                                    <option value="edit">{move || i18n.text("message.edit_code")}</option>
                                    <option value="analyze">
                                        {move || i18n.text("message.analyze_only")}
                                    </option>
                                </select>
                            </label>
                        </div>
                        <details class="disclosure">
                            <summary>{move || i18n.text("message.add_extra_instructions")}</summary>
                            <label>
                                {move || i18n.text("message.agent_instructions")}
                                <textarea

                                    name="instructions"
                                    rows="2"
                                    maxlength="20000"
                                    disabled={move || busy.get()}
                                    prop:value={move || instructions.get()}
                                    on:input={move |event| instructions.set(event_target_value(&event))}
                                />
                            </label>
                        </details>
                    </details>
                </section>
                <details class="disclosure model-options">
                    <summary>{move || i18n.text("message.model_reasoning_optional")}</summary>
                    <label>
                        {move || i18n.text("message.template")}
                        <input

                            maxlength="180"
                            disabled={move || busy.get()}
                            prop:value={move || model.get()}
                            on:input={move |event| model.set(event_target_value(&event))}
                        />
                    </label>
                    <label>
                        {move || i18n.text("message.reasoning_effort_4387e5d3")}
                        <input

                            maxlength="40"
                            disabled={move || busy.get()}
                            prop:value={move || effort.get()}
                            on:input={move |event| effort.set(event_target_value(&event))}
                        />
                    </label>
                </details>
                <p class="hint notice-line">
                    {move || {
                        i18n.text("message.creating_the_task_does_not_start_the_agent_you_can_review_its_in")
                    }}
                </p>
                <p class="error" role="alert">
                    {move || i18n.text(&error.get())}
                </p>
                <div class="form-actions">
                    <button type="button" disabled={move || busy.get()} on:click={move |_| close()}>
                        {move || i18n.text("message.cancel")}
                    </button>
                    <button
                        id="create-task"
                        class="primary"
                        type="submit"
                        disabled={move || submit_blocked.get()}
                    >
                        {move || {
                            i18n.text(if pending.get() { "message.creating" } else { "message.create_task" })
                        }}
                    </button>
                </div>
            </form>
            <div class="confirmation" hidden={move || template_candidate.get().is_none()}>
                <p>
                    {move || {
                        i18n.text("message.replace_the_entered_description_instructions_and_mode_with_this")
                    }}
                </p>
                <button
                    type="button"
                    disabled={move || busy.get()}
                    on:click={move |_| {
                        if let Some(id) = template_candidate.get_untracked() {
                            apply_template.run(id);
                        }
                    }}
                >
                    {move || i18n.text("message.apply")}
                </button>
                <button
                    type="button"
                    disabled={move || busy.get()}
                    on:click={move |_| {
                        template_candidate.set(None);
                        template.set(String::new());
                    }}
                >
                    {move || i18n.text("message.cancel")}
                </button>
            </div>
            <div class="confirmation" hidden={move || !remove_template.get()}>
                <p>{move || i18n.text("message.remove_this_template_existing_tasks_will_be_kept")}</p>
                <button
                    id="confirm-remove-template"
                    type="button"
                    disabled={move || busy.get()}
                    on:click={delete_template}
                >
                    {move || i18n.text("message.remove_template")}
                </button>
                <button
                    type="button"
                    disabled={move || busy.get()}
                    on:click={move |_| remove_template.set(false)}
                >
                    {move || i18n.text("message.cancel")}
                </button>
            </div>
        </dialog>
    }
}

fn task_template(kind: &str) -> &'static str {
    match kind {
        "bug" => "message.observed_problem_steps_to_reproduce_1_expected_result_fix_verifi",
        "analysis" => "message.question_to_investigate_context_expected_deliverable_an_analysis",
        _ => "message.goal_expected_behavior_acceptance_criteria_constraints",
    }
}
