use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::{
    application::brainstorm::{Answer, Convert},
    contracts::Card,
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Results(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    message: RwSignal<String>,
    run: Signal<Option<Document>>,
    on_open: Callback<String>,
    #[prop(optional)] hide_history: bool,
    #[prop(optional)] hide_questions: bool,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card = StoredValue::new(card_id);
    let answer = Signal::derive(move || {
        run.get()
            .and_then(|run| run.get::<Answer>("brainstormAnswer").ok().flatten())
    });
    let identity = Memo::new(move |_| {
        run.with(|run| run.as_ref().map(|run| field(run, "id")).unwrap_or_default())
    });
    let question = RwSignal::new(0usize);
    let selected = RwSignal::new(BTreeSet::<usize>::new());
    let project = RwSignal::new(None::<Document>);
    let approved = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let converted = Signal::derive(move || {
        run.with(|run| {
            run.as_ref()
                .and_then(|run| {
                    run.get::<BTreeMap<usize, String>>("convertedProposals")
                        .ok()
                        .flatten()
                })
                .unwrap_or_default()
        })
    });
    let mut previous = String::new();
    Effect::new(move |_| {
        let id = identity.get();
        if id != previous {
            previous = id;
            selected.set(BTreeSet::new());
            approved.set(false);
            question.set(0);
            error.set(String::new());
            notice.set(String::new());
        }
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let choose = Callback::new(move |text: String| {
        let current = message.get_untracked();
        let combined = if current.trim().is_empty() {
            text
        } else {
            format!("{current}\n{text}")
        };
        if combined.encode_utf16().count() > 12_000 {
            error.set(
                "message.message_too_long_send_the_answers_already_prepared_before_contin".into(),
            );
            return;
        }
        message.set(combined);
        question.update(|index| *index += 1);
    });
    let convert = move |_| {
        if pending.get_untracked()
            || blocked.get_untracked()
            || !approved.get_untracked()
            || selected.with_untracked(BTreeSet::is_empty)
        {
            return;
        }
        let Some(expected) = board.cards.with_untracked(|cards| {
            cards
                .iter()
                .find(|item| item.id == card.get_value())
                .cloned()
        }) else {
            return;
        };
        let Some(project) = project.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = serde_json::to_value(Convert {
            expected,
            run_id: identity.get_untracked(),
            indices: selected.with_untracked(|indices| indices.iter().copied().collect()),
            project,
            confirmed: true,
        })
        .expect("Brainstorm conversion serialization");
        let path = format!("/api/cards/{}/brainstorm/convert", card.get_value());
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Vec<Card>>()
                    .await
                    .map_err(|_| "message.invalid_server_response".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(_) => {
                        selected.set(BTreeSet::new());
                        approved.set(false);
                        notice.set("message.proposals_converted_previously_created_tasks_are_preserved_witho".into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    };
    view! {
        <section class="task-files" aria-labelledby="brainstorm-answer-heading">
            <h4 id="brainstorm-answer-heading" hidden={hide_history}>
                {move || i18n.text("message.conversation_and_proposals")}
            </h4>
            <p class="error" role="alert">
                {move || {
                    run.with(|run| {
                        run.as_ref().map(|run| i18n.text(&field(run, "brainstormError"))).unwrap_or_default()
                    })
                }}
            </p>
            <p hidden={hide_history}>
                {move || {
                    run.with(|run| run.as_ref().map(|run| field(run, "userMessage")).unwrap_or_default())
                }}
            </p>
            <pre class="service-log" hidden={hide_history}>
                {move || {
                    answer
                        .with(|answer| {
                            answer.as_ref().map(|answer| answer.message.clone()).unwrap_or_default()
                        })
                }}
            </pre>
            <div hidden={move || {
                hide_questions || pending.get()
                    || run
                        .with(|run| {
                            run.as_ref()
                                .is_some_and(|run| {
                                    matches!(
                                        field(run, "status").as_str(),
                                        "starting" | "running" | "stopping"
                                    )
                                })
                        })
            }}>
                {move || {
                    answer
                        .get()
                        .and_then(|answer| answer.questions.get(question.get()).cloned())
                        .map(|item| {
                            let label = StoredValue::new(item.question.clone());
                            view! {
                                <div class="task-files">
                                    <p>{item.question}</p>
                                    <For
                                        each={move || item.options.clone()}
                                        key={|option| option.clone()}
                                        children={move |option| {
                                            let text = option.clone();
                                            view! {
                                                <crate::button::Button
                                                    disabled={Signal::derive(move || blocked.get())}
                                                    class="brainstorm-option"
                                                    {..}
                                                    type="button"
                                                    on:click={move |_| {
                                                        choose.run(format!("{} : {}", label.get_value(), option))
                                                    }}
                                                >
                                                    {text}
                                                </crate::button::Button>
                                            }
                                        }}
                                    />
                                    <p class="field-help">
                                        {move || {
                                            i18n.text(
                                                "message.you_can_also_enter_a_free_form_answer_in_the_message",
                                            )
                                        }}
                                    </p>
                                    <crate::button::Button
                                        {..}
                                        type="button"
                                        on:click={move |_| question.update(|index| *index += 1)}
                                    >
                                        {move || i18n.text("message.next_question")}
                                    </crate::button::Button>
                                </div>
                            }
                        })
                }}
                <crate::button::Button
                    {..}
                    type="button"
                    hidden={move || question.get() == 0}
                    on:click={move |_| question.set(0)}
                >
                    {move || i18n.text("message.review_questions")}
                </crate::button::Button>
            </div>
            <fieldset
                disabled={move || pending.get() || blocked.get()}
                hidden={move || {
                    answer.with(|answer| answer.as_ref().is_none_or(|answer| answer.tickets.is_empty()))
                }}
            >
                <legend>{move || i18n.text("message.proposals_to_convert")}</legend>
                <For
                    each={move || {
                        answer
                            .get()
                            .map(|answer| answer.tickets.into_iter().enumerate().collect::<Vec<_>>())
                            .unwrap_or_default()
                    }}
                    key={move |(index, _)| (identity.get_untracked(), *index)}
                    children={move |(index, proposal)| {
                        view! {
                            <crate::card::Card class="task-files" {..} >
                                <label>
                                    <crate::form::Checkbox
                                        {..}
                                        class="brainstorm-proposal"
                                        prop:checked={move || {
                                            selected.with(|selected| selected.contains(&index))
                                        }}
                                        disabled={move || {
                                            converted.with(|converted| converted.contains_key(&index))
                                        }}
                                        on:change={move |event| {
                                            selected
                                                .update(|selected| {
                                                    if event_target_checked(&event) {
                                                        selected.insert(index);
                                                    } else {
                                                        selected.remove(&index);
                                                    }
                                                });
                                            approved.set(false);
                                        }}
                                    />
                                    {proposal.title}
                                </label>
                                <p>{proposal.description}</p>
                                <ul>
                                    {proposal
                                        .criteria
                                        .into_iter()
                                        .map(|criterion| view! { <li>{criterion}</li> })
                                        .collect_view()}
                                </ul>
                                <ul>
                                    {proposal
                                        .questions
                                        .into_iter()
                                        .map(|question| view! { <li>{question}</li> })
                                        .collect_view()}
                                </ul>
                                {move || {
                                    converted
                                        .with(|converted| converted.get(&index).cloned())
                                        .map(|id| {
                                            view! {
                                                <crate::button::Button
                                                    {..}
                                                    type="button"
                                                    on:click={move |_| on_open.run(id.clone())}
                                                >
                                                    {move || i18n.text("message.open_the_created_task")}
                                                </crate::button::Button>
                                            }
                                        })
                                }}
                            </crate::card::Card>
                        }
                    }}
                />
                <crate::form::Field
                    id="brainstorm-project"
                    label={Signal::derive(move || i18n.text("message.project_for_new_tasks"))}
                >
                    <crate::form::Select
                        {..}
                        id="brainstorm-project"
                        prop:value={move || {
                            project
                                .with(|project| {
                                    project.as_ref().map(|project| field(project, "id")).unwrap_or_default()
                                })
                        }}
                        on:change={move |event| {
                            let id = event_target_value(&event);
                            project
                                .set(
                                    board
                                        .projects
                                        .with(|projects| {
                                            projects
                                                .iter()
                                                .find(|project| field(project, "id") == id)
                                                .cloned()
                                        }),
                                );
                            approved.set(false);
                        }}
                    >
                        <option value="">{move || i18n.text("message.choose_a_project")}</option>
                        <For
                            each={move || board.projects.get()}
                            key={|project| (field(project, "id"), field(project, "repo"))}
                            children={move |project| {
                                let id = field(&project, "id");
                                let name = move || board.projects.with(|items| items.iter().find(|project| field(project, "id") == id).map(|project| field(project, "name")).unwrap_or_default());
                                view! {
                                    <option value={field(&project, "id")}>{name}</option>
                                }

                            }}
                        />
                    </crate::form::Select>
                </crate::form::Field>
                <label>
                    <crate::form::Checkbox
                        {..}
                        id="confirm-brainstorm-convert"
                        prop:checked={move || approved.get()}
                        on:change={move |event| approved.set(event_target_checked(&event))}
                    />
                    {move || {
                        i18n.text("message.create_the_selected_tasks_with_criteria_requiring_human_validati")
                    }}
                </label>
                <crate::button::Button
                    disabled={Signal::derive(move || {
                        !approved.get() || project.with(|project| project.is_none())
                            || selected.with(BTreeSet::is_empty)
                    })}
                    {..}
                    id="convert-brainstorm"
                    type="button"
                    on:click={convert}
                >
                    {move || i18n.text("message.confirm_conversion_into_tasks")}
                </crate::button::Button>
            </fieldset>
            <p class="error" role="alert">
                {move || i18n.text(&error.get())}
            </p>
            <p role="status">{move || i18n.text(&notice.get())}</p>
        </section>
    }
}
