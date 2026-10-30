use crate::{NavigationGuard, field, i18n::I18n, live::BoardState, write};
use atelier_core::{
    application::brainstorm::Answer,
    contracts::{AgentInteraction, Card},
    document::Document,
    domain::agent_options::providers,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Draft {
    title: String,
    description: String,
    instructions: String,
    repo: String,
    agent: String,
    model: String,
    effort: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Saved {
    session: String,
    fields: Draft,
    reply: String,
}
fn saved(key: &str) -> Saved {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(key).ok().flatten())
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}
fn remember(key: &str, value: &Saved) {
    if let (Some(storage), Ok(text)) = (
        web_sys::window().and_then(|window| window.local_storage().ok().flatten()),
        serde_json::to_string(value),
    ) {
        let _ = storage.set_item(key, &text);
    }
}
fn active(run: &Document) -> bool {
    matches!(
        field(run, "status").as_str(),
        "starting" | "running" | "stopping"
    )
}
fn name(card: &Card) -> String {
    card.title.clone()
}
fn agent_name(board: BoardState, id: &str) -> String {
    providers(&board.settings.get_untracked())
        .unwrap_or_default()
        .iter()
        .find(|profile| field(profile, "id") == id)
        .map(|profile| field(profile, "name"))
        .unwrap_or_else(|| id.to_owned())
}
fn focus(id: &'static str) {
    request_animation_frame(move || {
        if let Some(element) = document()
            .get_element_by_id(id)
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = element.focus();
        }
    });
}

#[component]
pub fn BrainstormPage(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_open: Callback<String>,
    on_new: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let initial = saved("atelier-rust-brainstorm-page");
    let selected = RwSignal::new(initial.session);
    let draft = RwSignal::new(initial.fields);
    let reply = RwSignal::new(initial.reply);
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let tasks = RwSignal::new(false);
    let share = RwSignal::new(70u32);
    let drag = RwSignal::new(None::<i32>);
    let layout = NodeRef::<leptos::html::Div>::new();
    let reply_drafts = RwSignal::new(BTreeSet::<String>::new());
    let continuation = RwSignal::new(None::<Callback<()>>);
    let sessions = Signal::derive(move || {
        board
            .cards
            .get()
            .into_iter()
            .filter(|card| {
                !card.fields.flag("archived").unwrap_or(false)
                    && card.fields.flag("brainstormSession").unwrap_or(false)
            })
            .collect::<Vec<_>>()
    });
    let current = Signal::derive(move || {
        sessions.with(|sessions| {
            sessions
                .iter()
                .find(|card| card.id == selected.get())
                .cloned()
        })
    });
    let runs = Signal::derive(move || {
        board
            .runs
            .get()
            .into_iter()
            .filter(|run| !selected.get().is_empty() && field(run, "cardId") == selected.get())
            .collect::<Vec<_>>()
    });
    let latest = Signal::derive(move || runs.with(|runs| runs.first().cloned()));
    let running =
        Signal::derive(move || runs.with(|runs| runs.iter().find(|run| active(run)).cloned()));
    let answer = Signal::derive(move || {
        runs.with(|runs| {
            runs.iter()
                .find_map(|run| run.get::<Answer>("brainstormAnswer").ok().flatten())
        })
    });
    let proposals = Memo::new(move |_| {
        answer.with(|answer| {
            answer
                .as_ref()
                .is_some_and(|answer| !answer.tickets.is_empty())
        })
    });
    let dirty = Memo::new(move |_| {
        !reply.get().trim().is_empty()
            || !reply_drafts.with(BTreeSet::is_empty)
            || (selected.get().is_empty()
                && draft.with(|draft| {
                    !draft.title.is_empty()
                        || !draft.description.is_empty()
                        || !draft.instructions.is_empty()
                        || !draft.repo.is_empty()
                        || !draft.model.is_empty()
                        || !draft.effort.is_empty()
                        || (!draft.agent.is_empty()
                            && draft.agent != field(&board.settings.get_untracked(), "agent"))
                }))
    });
    Effect::new(move |_| {
        let value = Saved {
            session: selected.get(),
            fields: draft.get(),
            reply: reply.get(),
        };
        remember("atelier-rust-brainstorm-page", &value);
        remember(
            &format!("atelier-rust-brainstorm-draft:{}", value.session),
            &value,
        );
    });
    Effect::new(move |_| {
        if board.loaded.get() && current.get().is_none() && !selected.get().is_empty() {
            selected.set(String::new());
        }
    });
    let change = Callback::new(move |id: String| {
        let saved = saved(&format!("atelier-rust-brainstorm-draft:{id}"));
        let fields = board
            .cards
            .with_untracked(|cards| {
                cards.iter().find(|card| card.id == id).map(|card| Draft {
                    title: name(card),
                    description: field(&card.fields, "description"),
                    instructions: field(&card.fields, "instructions"),
                    repo: field(&card.fields, "repo"),
                    agent: field(&card.fields, "agent"),
                    model: field(&card.fields, "model"),
                    effort: field(&card.fields, "effort"),
                })
            })
            .unwrap_or(saved.fields);
        selected.set(id);
        draft.set(fields);
        reply.set(saved.reply);
        tasks.set(false);
        error.set(String::new());
        focus(if selected.get_untracked().is_empty() {
            "brainstorm-idea"
        } else {
            "brainstorm-reply"
        });
    });
    let navigation = expect_context::<NavigationGuard>();
    navigation.1.update(|owner| *owner = owner.wrapping_add(1));
    let owner = navigation.1.get_untracked();
    let request = Callback::new(move |next: Callback<()>| {
        if pending.get_untracked() {
            error.set("message.wait_for_the_current_operation_to_finish".into());
        } else if dirty.get_untracked() {
            continuation.set(Some(next));
        } else {
            next.run(());
        }
    });
    navigation.0.set(Some(request));
    on_cleanup(move || {
        if navigation.1.try_get_untracked() == Some(owner) {
            let _ = navigation.0.try_set(None);
        }
    });
    let choose = Callback::new(move |id: String| {
        request.run(Callback::new(move |()| change.run(id.clone())));
    });
    let send = StoredValue::new_local(move |local: bool, message: Option<String>| {
        if pending.get_untracked() || running.get_untracked().is_some() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            error.set("message.session_expired_or_missing".into());
            return;
        };
        let mut fields = draft.get_untracked();
        if fields.agent.is_empty() {
            fields.agent = field(&board.settings.get_untracked(), "agent");
        }
        if fields.title.trim().is_empty() {
            fields.title = fields
                .description
                .trim()
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(90)
                .collect();
        }
        let existing = current.get_untracked();
        if existing.is_none() && fields.title.trim().is_empty() {
            if local {
                on_new.run(());
            } else {
                error.set("message.describe_your_idea_or_give_the_conversation_a_title".into());
            }
            return;
        }
        let prompt = message.unwrap_or_else(|| {
            let reply = reply.get_untracked();
            if existing.is_some() {
                reply
            } else if !fields.description.trim().is_empty() {
                fields.description.clone()
            } else {
                fields.title.clone()
            }
        });
        if !local && prompt.trim().is_empty() {
            return;
        }
        pending.set(true);
        error.set(String::new());
        let sent_reply = reply.get_untracked();
        spawn_local(async move {
            let result=async{
                let card=if let Some(card)=existing{card}else{
                    let mut body=serde_json::to_value(&fields).map_err(|_|"message.invalid_conversation_data".to_owned())?;body["mode"]=serde_json::json!(if local{"edit"}else{"analyze"});
                    write(&credential,Method::POST,if local{"/api/cards"}else{"/api/brainstorms"},&body).await?.json::<Card>().await.map_err(|_|"message.invalid_server_response".to_owned())?
                };
                if local{return Ok::<_,String>((card,true));}
                crate::replace_card(board.cards,card.clone());selected.set(card.id.clone());draft.set(fields);
                write(&credential,Method::POST,&format!("/api/cards/{}/runs/start",card.id),&serde_json::json!({"agent":field(&card.fields,"agent"),"mode":"analyze","purpose":"work","expected":card,"message":prompt})).await?;
                Ok((card,false))
            }.await;
            let _ = board.refresh(&credential).await;
            if pending.try_get_untracked().is_some() {
                pending.set(false);
                match result {
                    Ok((card, true)) => {
                        draft.set(Draft::default());
                        reply.set(String::new());
                        on_open.run(card.id);
                    }
                    Ok((_, false)) => {
                        if reply.get_untracked() == sent_reply {
                            reply.set(String::new());
                        }
                        focus("brainstorm-reply");
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    });
    let stop = move || {
        let (Some(run), Ok(credential)) = (running.get_untracked(), token.get_value()) else {
            return;
        };
        pending.set(true);
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &format!("/api/runs/{}/stop", field(&run, "id")),
                &serde_json::json!({}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if pending.try_get_untracked().is_some() {
                pending.set(false);
                if let Err(message) = result {
                    error.set(message);
                }
            }
        });
    };
    let context = NodeRef::<leptos::html::Details>::new();
    Effect::new(move |_| {
        if let Some(context) = context.get() {
            context.set_open(selected.get().is_empty());
        }
    });
    view! {
            <section id="brainstorm-page" aria-label=move ||i18n.text("message.brainstorming_workshop")>
            <ol class="brainstorm-steps" aria-label=move ||i18n.text("message.brainstorming_steps")>{[("idea","message.describe_the_idea",1),("chat","message.explore_together",2),("tasks","message.choose_tasks",3)].into_iter().map(move |(step,label,number)|view!{<li id=format!("brainstorm-step-{step}") aria-current=move ||if (selected.get().is_empty()&&step=="idea")||(!selected.get().is_empty()&&if proposals.get(){step=="tasks"}else{step=="chat"}){"step"}else{"false"}><span aria-hidden="true">{number}</span>{i18n.text(label)}</li>}).collect_view()}</ol>
            <div class="brainstorm-toolbar" id="brainstorm-toolbar" hidden=move ||sessions.with(Vec::is_empty)><label>{i18n.text("message.your_conversations")}<select id="brainstorm-session" disabled=move ||pending.get() prop:value=move ||selected.get() on:change=move |event|choose.run(event_target_value(&event))><option value="">{i18n.text("message.new_conversation_e6a6a99b")}</option><For each={move ||sessions.get()} key=|card|card.id.clone() children=move |card|view!{<option value=card.id.clone()>{name(&card)}</option>} /></select></label><button type="button" id="brainstorm-toggle-tasks" aria-controls="brainstorm-results" hidden=move ||!proposals.get() on:click=move |_|tasks.update(|visible|*visible = !*visible)>{move ||i18n.text(if tasks.get(){"message.back_to_the_discussion"}else{"message.view_tasks"})}</button><button type="button" id="brainstorm-new" hidden=move ||selected.get().is_empty() disabled=move ||pending.get() on:click=move |_|choose.run(String::new())>{i18n.text("message.new_conversation")}</button></div>
            <div class="brainstorm-layout" id="brainstorm-layout" node_ref=layout data-panel=move ||if tasks.get(){"tasks"}else{"chat"} style=move ||format!("--brainstorm-chat-size:{}fr;--brainstorm-task-size:{}fr",share.get(),100-share.get())>
            <section class="brainstorm-editor"><div class="eyebrow" id="brainstorm-editor-kicker" hidden=move ||!selected.get().is_empty()>{i18n.text("message.start_with_an_idea")}</div><h2 id="brainstorm-editor-title">{move ||current.get().map(|card|name(&card)).unwrap_or_else(||i18n.text("message.what_would_you_like_to_improve"))}</h2><p class="hint" id="brainstorm-editor-intro" hidden=move ||!selected.get().is_empty()>{i18n.text("message.describe_your_idea_discuss_it_with_the_agent_then_turn_the_ideas")}</p><p class="hint" id="brainstorm-session-summary" hidden=move ||selected.get().is_empty()>{move ||current.get().map(|card|format!("{} · {}",{let repo=field(&card.fields,"repo");if repo.is_empty(){i18n.text("message.no_repository_general_brainstorming")}else{board.projects.with(|projects|projects.iter().find(|project|field(project,"repo")==repo).map(|project|field(project,"name")).unwrap_or(repo))}},agent_name(board,&field(&card.fields,"agent")))).unwrap_or_default()}</p>
            <details id="brainstorm-context" node_ref=context open><summary>{i18n.text("message.view_the_initial_idea_and_settings")}</summary><div id="brainstorm-session-context" hidden=move ||selected.get().is_empty()><p class="hint">{i18n.text("message.these_settings_are_fixed_for_this_discussion_to_change_the_proje")}</p><dl class="brainstorm-context-values">{move ||current.get().map(|card|[("message.project",field(&card.fields,"repo")),("message.agent",agent_name(board,&field(&card.fields,"agent"))),("message.template",field(&card.fields,"model")),("message.reasoning_effort",field(&card.fields,"effort")),("message.your_idea",field(&card.fields,"description")),("message.constraints_optional",field(&card.fields,"instructions"))].into_iter().filter(|(_,value)|!value.is_empty()).map(move |(label,value)|view!{<div><dt>{i18n.text(label)}</dt><dd>{value}</dd></div>}).collect_view())}</dl></div>
            <form id="brainstorm-form" hidden=move ||!selected.get().is_empty() on:submit=move |event|{event.prevent_default();send.with_value(|send|send(false,None));}><fieldset disabled=move ||pending.get()><label class="brainstorm-idea-label">{i18n.text("message.your_idea")}<textarea id="brainstorm-idea" name="description" rows="3" maxlength="20000" required=move ||draft.with(|draft|draft.title.trim().is_empty()) placeholder=move ||i18n.text("message.id_like_to_make_signing_up_easier_for_my_customers_but_im_not_su") prop:value=move ||draft.get().description on:input=move |event|draft.update(|draft|draft.description=event_target_value(&event)) /></label><div class="form-row"><label>{i18n.text("message.project_to_explore_optional")}<select name="repo" prop:value=move ||draft.get().repo on:change=move |event|draft.update(|draft|draft.repo=event_target_value(&event))><option value="">{i18n.text("message.no_repository_general_brainstorming")}</option><For each={move ||board.projects.get()} key=|project|field(project,"id") children=move |project|view!{<option value=field(&project,"repo")>{field(&project,"name")}</option>} /></select></label><label>{i18n.text("message.agent")}<select name="agent" prop:value=move ||{let agent=draft.get().agent;if agent.is_empty(){field(&board.settings.get(),"agent")}else{agent}} on:change=move |event|draft.update(|draft|{draft.agent=event_target_value(&event);draft.model.clear();draft.effort.clear();})><For each={move ||providers(&board.settings.get()).unwrap_or_default()} key=|profile|field(profile,"id") children=move |profile|view!{<option value=field(&profile,"id")>{field(&profile,"name")}</option>} /></select></label></div>
            <details class="brainstorm-settings"><summary>{i18n.text("message.customize_the_model_and_context")}<span class="hint">{i18n.text("message.optional")}</span></summary><div data-agent-fields=""><label>{i18n.text("message.template")}<input name="model" maxlength="180" prop:value=move ||draft.get().model on:input=move |event|draft.update(|draft|draft.model=event_target_value(&event)) placeholder=move ||i18n.text("message.cli_configuration") /></label><label>{i18n.text("message.reasoning_effort")}<input name="effort" maxlength="40" prop:value=move ||draft.get().effort on:input=move |event|draft.update(|draft|draft.effort=event_target_value(&event)) placeholder=move ||i18n.text("message.default") /></label></div><label>{i18n.text("message.conversation_name_optional")}<input name="title" maxlength="300" prop:value=move ||draft.get().title on:input=move |event|draft.update(|draft|draft.title=event_target_value(&event)) placeholder=move ||i18n.text("message.taken_from_your_idea_if_left_blank") /></label><label>{i18n.text("message.constraints_optional")}<textarea name="instructions" rows="2" maxlength="20000" prop:value=move ||draft.get().instructions on:input=move |event|draft.update(|draft|draft.instructions=event_target_value(&event)) placeholder=move ||i18n.text("message.budget_deadlines_things_to_preserve") /></label></details><p class="hint brainstorm-setup-note">{i18n.text("message.the_project_and_settings_are_fixed_when_you_start")}</p><div class="brainstorm-start-actions"><span class="hint">{i18n.text("message.no_files_are_changed")}<br/>{i18n.text("message.uses_your_agents_quota")}</span><button class="primary" id="brainstorm-start" type="submit">{move ||i18n.text(if pending.get(){"message.starting"}else{"message.start_the_discussion"})}</button></div><div class="brainstorm-direct"><button type="button" id="brainstorm-create-local" class="text-button" on:click=move |_|send.with_value(|send|send(true,None))>{i18n.text("message.create_a_task_without_a_discussion")}</button></div></fieldset></form></details>
            <div class="brainstorm-run-status"><p id="brainstorm-page-status" class="hint" role="status">{move ||running.get().map(|run|{let activity=field(&run,"activity");if activity.is_empty(){i18n.text("message.brainstorming_in_progress")}else{activity}}).or_else(||latest.get().filter(|run|field(run,"status")!="completed").map(|run|i18n.text(&field(&run,"attention")))).unwrap_or_default()}</p><button type="button" id="brainstorm-page-stop" hidden=move ||running.get().is_none() disabled=move ||pending.get() on:click=move |_|stop()>{i18n.text("message.stop")}</button><button type="button" id="brainstorm-retry" hidden=move ||selected.get().is_empty()||running.get().is_some()||latest.with(|run|run.as_ref().is_some_and(|run|field(run,"status")=="completed"&&field(run,"attention").is_empty())) disabled=move ||pending.get() on:click=move |_|send.with_value(|send|send(false,Some(latest.get_untracked().map(|run|field(&run,"userMessage")).filter(|message|!message.trim().is_empty()).unwrap_or_else(||i18n.text("message.resume_exploring_based_on_our_discussion")))))>{i18n.text("message.retry")}</button></div><p class="inline-error" role="alert">{move ||i18n.text(&error.get())}</p>
            <section id="brainstorm-chat" hidden=move ||selected.get().is_empty() aria-label=move ||i18n.text("message.discussion_with_the_agent")><div id="brainstorm-chat-history" class="brainstorm-chat-history" role="log" tabindex="0" aria-label=move ||i18n.text("message.discussion_history") aria-live="polite"><For each={move ||runs.get().into_iter().rev().collect::<Vec<_>>()} key=|run|serde_json::to_string(run).unwrap_or_default() children=move |run|{
                let answer=run.get::<Answer>("brainstormAnswer").ok().flatten();let user=field(&run,"userMessage");let text=answer.as_ref().map(|answer|answer.message.clone()).unwrap_or_else(||{let live=field(&run,"liveText");if !live.is_empty(){live}else if active(&run){let activity=field(&run,"activity");if activity.is_empty(){i18n.text("message.thinking")}else{activity}}else{let attention=field(&run,"attention");if attention.is_empty(){i18n.text("message.exploration_interrupted_or_failed_your_message_remains_in_the_di")}else{i18n.text(&attention)}}});
                view!{<article class="brainstorm-message user"><strong>{i18n.text("message.you")}</strong>{paragraphs(&user)}</article><article class="brainstorm-message assistant"><strong>{agent_name(board,&field(&run,"agent"))}</strong>{paragraphs(&text)}{answer.filter(|answer|!answer.tickets.is_empty()).map(move |answer|view!{<p class="hint"><button type="button" class="text-button" data-brainstorm-show-tasks="" on:click=move |_|{tasks.set(true);focus("brainstorm-results");}>{i18n.text("message.view_proposed_tasks_0").replace("{0}",&answer.tickets.len().to_string())}</button></p>})}</article>}
            } /><p class="hint" hidden=move ||!runs.with(Vec::is_empty)>{i18n.text("message.send_a_message_to_continue_exploring_your_idea")}</p></div>
            <For each={move ||running.get().map(|run|{let id=field(&run,"id");run.get::<Vec<AgentInteraction>>("interactions").ok().flatten().unwrap_or_default().into_iter().map(|request|(id.clone(),request)).collect::<Vec<_>>()}).unwrap_or_default()} key=|(id,request)|(id.clone(),serde_json::to_string(request).unwrap_or_default()) children=move |(run_id,request)|view!{<crate::runs::Interaction run_id request token board reply_drafts />} />
            <section id="brainstorm-question-context" class="brainstorm-question-context" aria-label=move ||i18n.text("message.agent_questions") hidden=move ||running.get().is_some()||answer.with(|answer|answer.as_ref().is_none_or(|answer|answer.questions.is_empty()))><For each={move ||answer.get().map(|answer|answer.questions).unwrap_or_default()} key=|question|question.question.clone() children=move |question|{let label=StoredValue::new(question.question.clone());view!{<div class="brainstorm-question"><strong>{question.question}</strong><div class="row-actions">{question.options.into_iter().map(move |option|{let text=option.clone();view!{<button type="button" disabled=move ||pending.get() on:click=move |_|{reply.update(|reply|{if !reply.is_empty(){reply.push('\n');}reply.push_str(&format!("{} : {}",label.get_value(),option));});focus("brainstorm-reply");}>{text}</button>}}).collect_view()}</div></div>}} /></section>
            <form id="brainstorm-reply-form" aria-label=move ||i18n.text("message.send_a_message") data-active=move ||running.get().is_some().to_string() on:submit=move |event|{event.prevent_default();send.with_value(|send|send(false,None));}><div class="brainstorm-composer"><textarea id="brainstorm-reply" required maxlength="12000" rows="1" aria-label=move ||i18n.text("message.message_to_the_agent") aria-describedby="brainstorm-composer-hint" placeholder=move ||i18n.text("message.your_reply_or_question") disabled=move ||pending.get() prop:value=move ||reply.get() on:input=move |event|reply.set(event_target_value(&event)) on:keydown=move |event:web_sys::KeyboardEvent|{if event.key()=="Enter"&&!event.shift_key()&&!event.is_composing(){event.prevent_default();send.with_value(|send|send(false,None));}} /><div class="brainstorm-composer-actions"><span id="brainstorm-composer-agent">{move ||current.get().map(|card|agent_name(board,&field(&card.fields,"agent"))).unwrap_or_else(||i18n.text("message.your_agent"))}</span><button type="submit" id="brainstorm-send" class="primary" disabled=move ||pending.get()||running.get().is_some()||reply.get().trim().is_empty()>{i18n.text("message.send")}</button></div></div><p class="hint brainstorm-composer-hint" id="brainstorm-composer-hint">{move ||i18n.text(if running.get().is_some(){"message.the_agent_is_working_you_can_draft_your_next_message_here"}else{"message.enter_to_send_shift_enter_for_a_new_line"})}</p></form>
            <div class="brainstorm-next-step"><div><strong>{i18n.text("message.ready_to_take_action")}</strong><p id="brainstorm-generate-hint" class="hint">{move ||i18n.text(if !reply.get().trim().is_empty(){"message.your_message_will_be_included"}else if proposals.get(){"message.tasks_already_added_will_remain_unchanged"}else{"message.when_your_idea_is_clear_enough"})}</p></div><button type="button" id="brainstorm-generate" aria-describedby="brainstorm-generate-hint" disabled=move ||pending.get()||running.get().is_some()||selected.get().is_empty() on:click=move |_|{let mut message=reply.get_untracked();if !message.trim().is_empty(){message.push_str("\n\n");}message.push_str(&i18n.text("message.suggest_a_task_breakdown"));send.with_value(|send|send(false,Some(message)));tasks.set(true);}>{move ||i18n.text(if !reply.get().trim().is_empty(){"message.send_and_prepare_tasks"}else if proposals.get(){"message.update_task_proposals"}else{"message.prepare_tasks"})}</button></div></section></section>
            <div id="brainstorm-splitter" role="separator" tabindex="0" aria-orientation="vertical" aria-controls="brainstorm-chat brainstorm-results" aria-label=move ||i18n.text("message.discussion_width") aria-valuemin="30" aria-valuemax="75" aria-valuenow=move ||share.get().to_string() aria-valuetext=move ||i18n.text("message.discussion_0_tasks_1").replace("{0}",&share.get().to_string()).replace("{1}",&(100-share.get()).to_string()) title=move ||i18n.text("message.drag_to_resize_you_can_also_use_the_left_and_right_arrow_keys") hidden=move ||!proposals.get() on:keydown=move |event:web_sys::KeyboardEvent|{let next=match event.key().as_str(){"ArrowLeft"=>share.get().saturating_sub(5),"ArrowRight"=>share.get().saturating_add(5),"Home"=>30,"End"=>75,_=>return};event.prevent_default();share.set(next.clamp(30,75));} on:pointerdown=move |event:web_sys::PointerEvent|{if event.button()!=0||!event.is_primary(){return;}event.prevent_default();if let Some(target)=event.target().and_then(|target|target.dyn_into::<web_sys::Element>().ok()){let _=target.set_pointer_capture(event.pointer_id());}drag.set(Some(event.pointer_id()));} on:pointermove=move |event:web_sys::PointerEvent|{if drag.get()!=Some(event.pointer_id()){return;}
    if let Some(layout)=layout.get(){let bounds=layout.get_bounding_client_rect();if bounds.width()>0.0{share.set((((f64::from(event.client_x())-bounds.left())/bounds.width())*100.0).round().clamp(30.0,75.0)as u32);}}} on:pointerup=move |_|drag.set(None) on:pointercancel=move |_|drag.set(None)><span aria-hidden="true">"⋮"</span></div>
            <section id="brainstorm-results" class="brainstorm-results" hidden=move ||!proposals.get() tabindex="-1" aria-label=move ||i18n.text("message.proposed_tasks")><h2>{i18n.text("message.proposed_tasks")}</h2><p class="hint">{i18n.text("message.review_and_add_the_useful_tasks_to_the_board")}</p><div id="brainstorm-page-proposals" tabindex="0" aria-label=move ||i18n.text("message.list_of_proposed_tasks") aria-live="polite"><For each={move ||current.get().map(|card|vec![card.id]).unwrap_or_default()} key=|id|id.clone() children=move |card_id|view!{<crate::brainstorm::Results card_id board token pending blocked=Memo::new(move |_|running.get().is_some()) message=reply run=latest on_open hide_history=true hide_questions=true />} /></div></section>
            </div></section>
            <Show when=move ||continuation.get().is_some()><DiscardDraft pending on_cancel=Callback::new(move |()|continuation.set(None)) on_confirm=Callback::new(move |()|{reply.set(String::new());if selected.get_untracked().is_empty(){draft.set(Draft::default());}
    if let Some(next)=continuation.get_untracked(){continuation.set(None);next.run(());}}) /></Show>
        }
}

fn paragraphs(text: &str) -> impl IntoView + use<> {
    text.split("\n\n")
        .map(|paragraph| view! {<p>{paragraph.to_owned()}</p>})
        .collect_view()
}

#[component]
fn DiscardDraft(
    pending: RwSignal<bool>,
    on_cancel: Callback<()>,
    on_confirm: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |dialog| {
        let _ = dialog.show_modal();
    });
    view! {<dialog class="confirm-dialog" role="alertdialog" aria-labelledby="brainstorm-discard-title" node_ref=dialog on:cancel=move |event:web_sys::Event|{event.prevent_default();on_cancel.run(());}><h2 id="brainstorm-discard-title">{i18n.text("message.discard_unsaved_changes")}</h2><div class="form-actions"><button autofocus type="button" disabled=move ||pending.get() on:click=move |_|on_cancel.run(())>{i18n.text("message.keep_editing")}</button><button type="button" class="confirm-action" disabled=move ||pending.get() on:click=move |_|on_confirm.run(())>{i18n.text("message.discard")}</button></div></dialog>}
}
