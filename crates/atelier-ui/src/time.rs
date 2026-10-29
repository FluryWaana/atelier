use crate::{field, i18n::I18n, live::BoardState, write};
use atelier_core::{
    document::Document,
    domain::time::{duration_text, parse_duration},
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
pub(super) fn clock() -> RwSignal<f64> {
    let now = RwSignal::new(js_sys::Date::now());
    let tick = Closure::<dyn FnMut()>::new(move || now.set(js_sys::Date::now()));
    let interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                1000,
            )
            .ok()
    });
    let timer = StoredValue::new_local((interval, tick));
    on_cleanup(move || {
        timer.with_value(|(interval, _)| {
            if let (Some(window), Some(interval)) = (web_sys::window(), *interval) {
                window.clear_interval_with_handle(interval);
            }
        })
    });
    now
}
fn seconds(entry: &Document, now: f64) -> u64 {
    let recorded = entry.get::<f64>("seconds").ok().flatten().unwrap_or(0.0);
    let started = entry
        .get::<String>("runningSince")
        .ok()
        .flatten()
        .map(|value| js_sys::Date::parse(&value));
    let elapsed = started
        .filter(|started| started.is_finite())
        .map(|started| ((now - started) / 1000.0).floor().max(0.0))
        .unwrap_or(0.0);
    (recorded + elapsed).clamp(0.0, 86400.0) as u64
}
#[derive(Clone, PartialEq)]
struct Draft {
    date: String,
    start: String,
    duration: String,
    description: String,
}
impl Draft {
    fn new() -> Self {
        let now = js_sys::Date::new_0();
        Self {
            date: format!(
                "{:04}-{:02}-{:02}",
                now.get_full_year(),
                now.get_month() + 1,
                now.get_date()
            ),
            start: format!(
                "{:02}:{:02}:{:02}",
                now.get_hours(),
                now.get_minutes(),
                now.get_seconds()
            ),
            duration: "00:30:00".into(),
            description: String::new(),
        }
    }
    fn from_entry(entry: &Document) -> Self {
        Self {
            date: field(entry, "date"),
            start: field(entry, "startTime"),
            duration: duration_text(seconds(entry, js_sys::Date::now())),
            description: field(entry, "description"),
        }
    }
}
#[component]
pub fn TimeTask(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    archived: bool,
    #[prop(default = None)] initial_entry: Option<Document>,
    #[prop(default = false)] editor_only: bool,
    #[prop(default = None)] on_saved: Option<Callback<()>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let now = clock();
    let original = RwSignal::new(
        initial_entry
            .as_ref()
            .map(Draft::from_entry)
            .unwrap_or_else(Draft::new),
    );
    let draft = RwSignal::new(original.get_untracked());
    let selected = RwSignal::new(initial_entry);
    let discard = RwSignal::new(false);
    let removing = RwSignal::new(None::<Document>);
    let page = RwSignal::new(0usize);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let own_dirty = Memo::new(move |_| draft.get() != original.get());
    let export_drafts = RwSignal::new(std::collections::BTreeSet::<String>::new());
    Effect::new(move |_| {
        dirty.set(own_dirty.get() || !export_drafts.with(std::collections::BTreeSet::is_empty))
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let entries = Signal::derive(move || {
        board.times.with(|entries| {
            entries
                .iter()
                .filter(|entry| field(entry, "cardId") == id.get_value())
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    Effect::new(move |_| {
        let last = entries.with(|entries| entries.len().saturating_sub(1) / 100);
        if page.get() > last {
            page.set(last);
        }
    });
    let execute = StoredValue::new_local(
        move |method: Method, path: String, body: serde_json::Value, reset: bool| {
            if pending.get_untracked() {
                return;
            }
            let credential = match token.get_value() {
                Ok(value) => value,
                Err(message) => {
                    error.set(message);
                    return;
                }
            };
            pending.set(true);
            error.set(String::new());
            notice.set(String::new());
            let alive = alive.clone();
            spawn_local(async move {
                let result = write(&credential, method, &path, &body).await;
                let completed = result.is_ok() && reset;
                if alive.get() {
                    match result {
                        Ok(_) => {
                            if reset {
                                let next = Draft::new();
                                original.set(next.clone());
                                draft.set(next);
                                selected.set(None);
                                page.set(0);
                            }
                            removing.set(None);
                            notice.set("message.time_entry_saved_locally".into());
                        }
                        Err(message) => error.set(message),
                    }
                }
                let _ = board.refresh(&credential).await;
                if alive.get() {
                    pending.set(false);
                    if completed && let Some(on_saved) = on_saved {
                        on_saved.run(());
                    }
                }
            });
        },
    );
    let save = StoredValue::new_local(move |start: bool| {
        if blocked.get_untracked()
            || pending.get_untracked()
            || !export_drafts.with_untracked(std::collections::BTreeSet::is_empty)
        {
            return;
        }
        let value = draft.get_untracked();
        let Some(seconds) = parse_duration(&value.duration) else {
            error.set("message.enter_a_duration_between_00_00_01_and_24_00_00".into());
            return;
        };
        let mut body = serde_json::json!({"cardId":id.get_value(),"seconds":seconds,"date":value.date,"startTime":if value.start.len()==5{format!("{}:00",value.start)}else{value.start},"description":value.description,"start":start});
        let (method, path) = if let Some(entry) = selected.get_untracked() {
            body["expected"] = serde_json::to_value(&entry).expect("Time snapshot serialization");
            (Method::PATCH, format!("/api/time/{}", field(&entry, "id")))
        } else {
            (Method::POST, "/api/time".into())
        };
        execute.with_value(|execute| execute(method, path, body, true));
    });
    let select = StoredValue::new_local(move |entry: Document| {
        if own_dirty.get_untracked()
            || !export_drafts.with_untracked(std::collections::BTreeSet::is_empty)
        {
            error.set("message.save_or_discard_the_entry_before_opening_another_one".into());
            return;
        }
        let value = Draft::from_entry(&entry);
        original.set(value.clone());
        draft.set(value);
        selected.set(Some(entry));
        error.set(String::new());
        notice.set(String::new());
    });
    let reset = move || {
        let next = Draft::new();
        original.set(next.clone());
        draft.set(next);
        selected.set(None);
        discard.set(false);
        error.set(String::new());
    };
    view! {<section class="task-files" aria-labelledby="time-task-heading" aria-busy=move||pending.get().to_string()><h3 id="time-task-heading">{move||i18n.text("message.human_time")}</h3>
        <p class="field-help">{move||i18n.text("message.local_entries_and_one_timer_for_the_entire_application_no_automa")}</p>
        <p>{move||format!("{} : {}",i18n.text("message.human_time"),duration_text(entries.with(|entries|entries.iter().fold(0u64,|sum,entry|sum.saturating_add(seconds(entry,now.get()))))))}</p>
        <p>{move||format!("{} : {}",i18n.text("message.measured_active_agent_time"),duration_text(board.runs.with(|runs|runs.iter().filter(|run|field(run,"cardId")==id.get_value()).filter_map(|run|run.get::<u64>("activeMilliseconds").ok().flatten()).fold(0u64,u64::saturating_add)/1000)))}</p>
        <form on:submit=move|event:leptos::ev::SubmitEvent|{event.prevent_default();save.with_value(|save|save(event.submitter().is_some_and(|button|button.id()=="start-human-timer")));}><fieldset disabled=move||pending.get()||blocked.get()||!export_drafts.with(std::collections::BTreeSet::is_empty)||(archived&&selected.with(|entry|entry.is_none()))>
            <label for="time-date">{move||i18n.text("message.date")}</label><input id="time-date" type="date" required prop:value=move||draft.get().date on:input=move|event|draft.update(|draft|draft.date=event_target_value(&event)) />
            <label for="time-start">{move||i18n.text("message.start_time")}</label><input id="time-start" type="time" step="1" required prop:value=move||draft.get().start on:input=move|event|draft.update(|draft|draft.start=event_target_value(&event)) />
            <label for="time-duration">{move||i18n.text("message.duration_hh_mm_ss")}</label><input id="time-duration" required maxlength="8" inputmode="text" pattern="[0-9]{2}:[0-9]{2}:[0-9]{2}" placeholder="00:30:00" prop:value=move||draft.get().duration on:input=move|event|draft.update(|draft|draft.duration=event_target_value(&event)) />
            <label for="time-description">{move||i18n.text("message.work_description")}</label><textarea id="time-description" required maxlength="4000" rows="3" prop:value=move||draft.get().description on:input=move|event|draft.update(|draft|draft.description=event_target_value(&event)) />
            <div class="editor-actions"><button id="save-time" type="submit">{move||i18n.text("message.save_time")}</button><button id="start-human-timer" type="submit" disabled=move||selected.with(|entry|entry.is_some())||board.times.with(|entries|entries.iter().any(|entry|entry.truthy("runningSince").unwrap_or(false)))>{move||i18n.text("message.start_timer")}</button></div>
        </fieldset></form><button id="reset-time-draft" type="button" hidden=editor_only disabled=move||pending.get() on:click=move|_|{if own_dirty.get_untracked(){discard.set(true);}else{reset();}}>{move||i18n.text("message.new_entry")}</button>
        <div class="confirmation" role="alert" hidden=move||!discard.get()><p>{move||i18n.text("message.discard_the_unsaved_time_entry")}</p><button id="discard-time-draft" type="button" on:click=move|_|reset()>{move||i18n.text("message.discard")}</button><button type="button" on:click=move|_|discard.set(false)>{move||i18n.text("message.keep_editing")}</button></div>
        <Show when=move||!editor_only><p hidden=move||!entries.with(|entries|entries.is_empty())>{move||i18n.text("message.no_time_entries_for_this_task")}</p>
        <ul><For each=move||{let mut ids=entries.with(|entries|entries.iter().skip(page.get()*100).take(100).map(|entry|field(entry,"id")).collect::<Vec<_>>());for id in export_drafts.get(){if !ids.contains(&id){ids.push(id);}}ids} key=|id|id.clone() children=move|entry_id|{let entry_id=StoredValue::new(entry_id);let entry=Signal::derive(move||board.times.with(|entries|entries.iter().find(|entry|field(entry,"id")==entry_id.get_value()).cloned().unwrap_or_default()));view!{<li data-time-id=entry_id.get_value()><strong>{move||format!("{} {} · {}",field(&entry.get(),"date"),field(&entry.get(),"startTime"),duration_text(seconds(&entry.get(),now.get())))}</strong><p>{move||field(&entry.get(),"description")}</p><p>{move||i18n.text(match field(&entry.get(),"exportState").as_str(){"draft"=>"message.not_submitted","exported"=>"message.submitted","sending"=>"message.submitting","uncertain"=>"message.uncertain_submission",_=>"message.unknown_submission_state"})}</p><p class="field-help" hidden=move||entry.get().raw("connectorExport").is_none()>{move||i18n.text("message.export_tracked_in_module_0_s_journal_open_its_settings_to_reread").replace("{0}",&field(&entry.get(),"exportConnector"))}</p><p hidden=move||!entry.with(|entry|entry.truthy("interrupted").unwrap_or(false))>{move||i18n.text("message.timer_interrupted_on_restart_duration_preserved")}</p><button type="button" hidden=move||!entry.with(|entry|entry.truthy("runningSince").unwrap_or(false)) disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute(Method::POST,format!("/api/time/{}/stop",entry_id.get_value()),serde_json::json!({}),false))>{move||i18n.text("message.stop_timer")}</button><crate::tempo_export::TempoExport entry card_id=id.get_value() board token pending blocked=Memo::new(move |_|blocked.get()||own_dirty.get()) drafts=export_drafts archived /><button class="edit-time" type="button" disabled=move||pending.get()||blocked.get()||entry.with(|entry|entry.truthy("runningSince").unwrap_or(false)||!entry.string_equals("exportState","draft").unwrap_or(false)) on:click=move|_|select.with_value(|select|select(entry.get_untracked()))>{move||i18n.text("message.edit_entry")}</button><button class="delete-time" type="button" disabled=move||pending.get()||blocked.get()||own_dirty.get()||!export_drafts.with(std::collections::BTreeSet::is_empty)||entry.with(|entry|entry.truthy("runningSince").unwrap_or(false)||!entry.string_equals("exportState","draft").unwrap_or(false)) on:click=move|_|removing.set(Some(entry.get_untracked()))>{move||i18n.text("message.delete_entry")}</button></li>}} /></ul>
        <div class="editor-actions" hidden=move||entries.with(|entries|entries.len()<=100)><button type="button" disabled=move||page.get()==0||!export_drafts.with(std::collections::BTreeSet::is_empty) on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("message.previous")}</button><button type="button" disabled=move||!export_drafts.with(std::collections::BTreeSet::is_empty)||entries.with(|entries|(page.get()+1)*100>=entries.len()) on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("message.next")}</button></div>
        <div class="confirmation" role="alert" hidden=move||removing.with(|entry|entry.is_none())><p>{move||i18n.text("message.permanently_delete_this_time_entry")}</p><button id="confirm-delete-time" type="button" disabled=move||pending.get() on:click=move|_|{if let Some(entry)=removing.get_untracked(){execute.with_value(|execute|execute(Method::DELETE,format!("/api/time/{}",field(&entry,"id")),serde_json::json!({"expected":entry}),true));}}>{move||i18n.text("message.delete_permanently")}</button><button type="button" disabled=move||pending.get() on:click=move|_|removing.set(None)>{move||i18n.text("message.cancel")}</button></div>
        </Show><p role="status">{move||if own_dirty.get(){i18n.text("message.unsaved_time_entry")}else{String::new()}}</p>
        <p id="time-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="time-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
#[component]
pub fn TimerBanner(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    #[prop(optional)] nested: bool,
    on_task: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let now = clock();
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let active = Signal::derive(move || {
        board.times.with(|entries| {
            entries
                .iter()
                .find(|entry| entry.truthy("runningSince").unwrap_or(false))
                .cloned()
        })
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let stop = move |_| {
        if pending.get_untracked() {
            return;
        }
        let Some(entry) = active.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        pending.set(true);
        error.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &format!("/api/time/{}/stop", field(&entry, "id")),
                &serde_json::json!({}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
                if let Err(message) = result {
                    error.set(message);
                }
            }
        });
    };
    view! {<aside id={if nested{"human-timer-in-task"}else{"human-timer-banner"}} class="task-files" hidden=move||active.with(|entry|entry.is_none()) aria-label=move||i18n.text("message.active_timer")><strong>{move||i18n.text("message.active_timer")}</strong><span>{move||active.get().map(|entry|duration_text(seconds(&entry,now.get()))).unwrap_or_default()}</span><button id={if nested{"open-timer-task-in-task"}else{"open-timer-task"}} type="button" on:click=move|_|{if let Some(entry)=active.get_untracked(){on_task.run(field(&entry,"cardId"));}}>{move||i18n.text("message.open_the_timer_task")}</button><button id={if nested{"stop-human-timer-in-task"}else{"stop-human-timer"}} type="button" disabled=move||pending.get() on:click=stop>{move||i18n.text("message.stop_timer")}</button><p class="error" role="alert">{move||i18n.text(&error.get())}</p></aside>}
}
