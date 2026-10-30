use crate::{field, i18n::I18n, live::BoardState};
use atelier_core::{document::Document, domain::time::duration_text};
use leptos::prelude::*;
use std::collections::{HashMap, HashSet};
use wasm_bindgen::JsCast;
#[derive(Clone, PartialEq, Eq, Hash)]
struct Row {
    id: String,
    card: String,
    title: String,
    ticket: String,
    date: String,
    start: String,
    seconds: u64,
    description: String,
    state: String,
    agent: bool,
    running: bool,
}
#[component]
pub fn TimeReport(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    loading: Memo<bool>,
    load_error: Signal<String>,
    on_close: Callback<()>,
    on_task: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let from = RwSignal::new(String::new());
    let to = RwSignal::new(String::new());
    let repo = RwSignal::new(String::new());
    let source = RwSignal::new("all".to_owned());
    let status = RwSignal::new("all".to_owned());
    let page = RwSignal::new(0usize);
    let error = RwSignal::new(String::new());
    let editing = RwSignal::new(None::<(String, Option<Document>)>);
    let removing = RwSignal::new(None::<Document>);
    let pending = RwSignal::new(false);
    let action = Callback::new(
        move |(method, path, body): (gloo_net::http::Method, String, serde_json::Value)| {
            if pending.get_untracked() {
                return;
            }
            let Ok(credential) = token.get_value() else {
                return;
            };
            pending.set(true);
            error.set(String::new());
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::write(&credential, method, &path, &body).await;
                let _ = board.refresh(&credential).await;
                if let Err(message) = result
                    && error.try_get_untracked().is_some()
                {
                    error.set(message);
                }
                if pending.try_get_untracked().is_some() {
                    pending.set(false);
                    removing.set(None);
                }
            });
        },
    );
    let invalid =
        Memo::new(move |_| !from.get().is_empty() && !to.get().is_empty() && from.get() > to.get());
    let usage_runs = Signal::derive(move || {
        if invalid.get() || source.get() == "human" {
            return Vec::new();
        }
        let from = from.get();
        let to = to.get();
        let repo = repo.get();
        let cards = board.cards.get();
        let selected = cards
            .iter()
            .filter(|card| repo.is_empty() || field(&card.fields, "repo") == repo)
            .map(|card| card.id.as_str())
            .collect::<HashSet<_>>();
        board.runs.with(|runs| {
            runs.iter()
                .filter(|run| {
                    let start = field(run, "startedAt");
                    let date = start.get(..10).unwrap_or_default();
                    (repo.is_empty() || selected.contains(field(run, "cardId").as_str()))
                        && ((from.is_empty() && to.is_empty())
                            || (!date.is_empty()
                                && (from.is_empty() || date >= from.as_str())
                                && (to.is_empty() || date <= to.as_str())))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let rows = Memo::new(move |_| {
        if invalid.get() {
            return Vec::new();
        }
        let cards = board.cards.get();
        let by_id = cards
            .iter()
            .map(|card| (card.id.as_str(), card))
            .collect::<HashMap<_, _>>();
        let repo = repo.get();
        let from = from.get();
        let to = to.get();
        let source = source.get();
        let status = status.get();
        let mut rows = Vec::new();
        let included = |card: &str, date: &str| {
            (from.is_empty() || date >= from.as_str())
                && (to.is_empty() || date <= to.as_str())
                && (repo.is_empty()
                    || by_id
                        .get(card)
                        .is_some_and(|card| field(&card.fields, "repo") == repo))
        };
        if source != "agent" {
            for entry in board.times.get() {
                let id = field(&entry, "id");
                let card = field(&entry, "cardId");
                let date = field(&entry, "date");
                let state = field(&entry, "exportState");
                if !included(&card, &date) || (status != "all" && state != status) {
                    continue;
                }
                let task = by_id.get(card.as_str());
                rows.push(Row {
                    id: format!("human:{id}"),
                    card,
                    title: task.map(|card| card.title.clone()).unwrap_or_default(),
                    ticket: task
                        .and_then(|card| card.fields.get::<Document>("ticket").ok().flatten())
                        .map(|ticket| field(&ticket, "key"))
                        .unwrap_or_default(),
                    date,
                    start: field(&entry, "startTime"),
                    seconds: entry
                        .get::<f64>("seconds")
                        .ok()
                        .flatten()
                        .unwrap_or(0.0)
                        .clamp(0.0, 86400.0) as u64,
                    description: field(&entry, "description"),
                    state,
                    agent: false,
                    running: entry.has_text("runningSince").unwrap_or(false),
                });
            }
        }
        if source != "human" {
            for run in board.runs.get() {
                let Some(milliseconds) = run.get::<u64>("activeMilliseconds").ok().flatten() else {
                    continue;
                };
                let card = field(&run, "cardId");
                let started = field(&run, "startedAt");
                let Some(date) = started.get(..10) else {
                    continue;
                };
                if !included(&card, date) {
                    continue;
                }
                let task = by_id.get(card.as_str());
                rows.push(Row {
                    id: format!("agent:{}", field(&run, "id")),
                    card,
                    title: task.map(|card| card.title.clone()).unwrap_or_default(),
                    ticket: task
                        .and_then(|card| card.fields.get::<Document>("ticket").ok().flatten())
                        .map(|ticket| field(&ticket, "key"))
                        .unwrap_or_default(),
                    date: date.into(),
                    start: started.get(11..19).unwrap_or_default().into(),
                    seconds: milliseconds / 1000,
                    description: field(&run, "summary"),
                    state: field(&run, "status"),
                    agent: true,
                    running: matches!(
                        field(&run, "status").as_str(),
                        "starting" | "running" | "stopping"
                    ),
                });
            }
        }
        rows.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then_with(|| b.start.cmp(&a.start))
                .then_with(|| a.id.cmp(&b.id))
        });
        rows
    });
    Effect::new(move |_| {
        let last = rows.with(|rows| rows.len().saturating_sub(1) / 100);
        if page.get() > last {
            page.set(last);
        }
    });
    let daily = Memo::new(move |_| {
        let mut days = std::collections::BTreeMap::<String, u64>::new();
        rows.with(|rows| {
            for row in rows.iter().filter(|row| !row.agent) {
                let total = days.entry(row.date.clone()).or_default();
                *total = total.saturating_add(row.seconds);
            }
        });
        days.into_iter().collect::<Vec<_>>()
    });
    let url = RwSignal::new(None::<String>);
    on_cleanup(move || {
        if let Some(Some(url)) = url.try_get_untracked() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    });
    let close = move || {
        on_close.run(());
        request_animation_frame(move || {
            if let Some(element) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id("view-time-report"))
                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = element.focus();
            }
        });
    };
    let download = move |_| {
        let result = (|| -> Result<(), String> {
            let visible = rows.get_untracked();
            let ids = visible
                .iter()
                .filter(|row| !row.agent && !row.running)
                .map(|row| row.id.strip_prefix("human:").unwrap_or_default())
                .collect::<HashSet<_>>();
            let entries = board.times.with(|entries| {
                entries
                    .iter()
                    .filter(|entry| ids.contains(field(entry, "id").as_str()))
                    .cloned()
                    .collect::<Vec<_>>()
            });
            let headers = [
                "message.date",
                "message.time",
                "message.ticket",
                "message.task",
                "message.seconds",
                "message.description",
                "message.tempo_state",
                "message.tempo_id",
            ]
            .map(|key| i18n.text(key));
            let csv = atelier_core::application::time_reports::csv(
                &headers,
                &entries,
                &board.cards.get_untracked(),
            )
            .map_err(str::to_owned)?;
            let parts = js_sys::Array::new();
            parts.push(&wasm_bindgen::JsValue::from_str(&csv));
            let blob = web_sys::Blob::new_with_str_sequence(parts.as_ref())
                .map_err(|_| "message.unable_to_create_the_csv".to_owned())?;
            let next = web_sys::Url::create_object_url_with_blob(&blob)
                .map_err(|_| "message.unable_to_create_the_csv".to_owned())?;
            if let Some(previous) = url.get_untracked() {
                let _ = web_sys::Url::revoke_object_url(&previous);
            }
            url.set(Some(next.clone()));
            let document = web_sys::window()
                .and_then(|window| window.document())
                .ok_or("message.browser_unavailable")?;
            let anchor = document
                .create_element("a")
                .map_err(|_| "message.download_unavailable".to_owned())?
                .dyn_into::<web_sys::HtmlAnchorElement>()
                .map_err(|_| "message.download_unavailable".to_owned())?;
            anchor.set_href(&next);
            anchor.set_download("atelier-time.csv");
            let body = document.body().ok_or("message.browser_unavailable")?;
            body.append_child(&anchor)
                .map_err(|_| "message.download_unavailable".to_owned())?;
            anchor.click();
            anchor.remove();
            Ok(())
        })();
        error.set(result.err().unwrap_or_default());
    };
    view! {<section id="time-report" class="time-view" tabindex="-1" aria-labelledby="time-report-heading" aria-busy=move||loading.get().to_string()>
        <header class="panel-heading"><div><h2 id="time-report-heading">{move||i18n.text("message.entries_timers")}</h2><p class="hint">{move||i18n.text("message.local_entries_to_review_before_sending_to_the_connected_service")}</p></div><div class="row-actions"><button id="add-time" class="primary" type="button" disabled=move||loading.get()||!board.cards.with(|cards|cards.iter().any(|card|!card.fields.flag("archived").unwrap_or(false))) on:click=move|_|{if let Some(card)=board.cards.with_untracked(|cards|cards.iter().find(|card|!card.fields.flag("archived").unwrap_or(false)).cloned()){editing.set(Some((card.id,None)));}}>{move||i18n.text("message.log_time")}</button><button id="close-time-report" type="button" on:click=move|_|close()>{move||i18n.text("message.close")}</button></div></header>
        <p role="status" hidden=move||!loading.get()>{move||i18n.text("message.loading_the_board")}</p><p class="error" role="alert">{move||i18n.text(&load_error.get())}</p>
        <p class="field-help" role="status" hidden=move||board.connected.get()&&board.sync_error.get().is_empty()>{move||i18n.text(&if board.sync_error.get().is_empty(){"message.updates_interrupted_reconnecting".to_owned()}else{board.sync_error.get()})}</p>
        <p class="field-help">{move||i18n.text("message.human_entries_use_their_local_date_active_agent_durations_belong")}</p>
        <section class="time-filters" aria-label=move||i18n.text("message.filter_entries")>
        <label>{move||i18n.text("message.from")}<input id="report-from" type="date" prop:value=move||from.get() on:input=move|event|{from.set(event_target_value(&event));page.set(0);} /></label>
        <label>{move||i18n.text("message.to")}<input id="report-to" type="date" prop:value=move||to.get() on:input=move|event|{to.set(event_target_value(&event));page.set(0);} /></label>
        <label>{move||i18n.text("message.project")}<select id="report-project" prop:value=move||repo.get() on:change=move|event|{repo.set(event_target_value(&event));page.set(0);} ><option value="">{move||i18n.text("message.all_projects")}</option><For each=move||board.projects.get() key=|project|field(project,"id") children=move|project|view!{<option value=field(&project,"repo")>{field(&project,"name")}</option>} /></select></label>
        <label>{move||i18n.text("message.time_source")}<select id="report-source" prop:value=move||source.get() on:change=move|event|{source.set(event_target_value(&event));page.set(0);} ><option value="all">{move||i18n.text("message.all_sources")}</option><option value="human">{move||i18n.text("message.human_time")}</option><option value="agent">{move||i18n.text("message.measured_active_agent_time")}</option></select></label>
        <label>{move||i18n.text("message.human_submission_state")}<select id="report-state" prop:value=move||status.get() on:change=move|event|{status.set(event_target_value(&event));page.set(0);} ><option value="all">{move||i18n.text("message.all_states")}</option><option value="draft">{move||i18n.text("message.not_submitted")}</option><option value="exported">{move||i18n.text("message.submitted")}</option><option value="uncertain">{move||i18n.text("message.uncertain_submission")}</option></select></label>
        </section>
        <p role="alert" hidden=move||!invalid.get()>{move||i18n.text("message.the_end_date_must_follow_the_start_date")}</p>
        <div class="view-stats time-stats"><div><span>{move||i18n.text("message.human_time")}</span><strong id="report-human-total">{move||duration_text(rows.with(|rows|rows.iter().filter(|row|!row.agent).map(|row|row.seconds).fold(0u64,u64::saturating_add)))}</strong></div>
        <div><span>{move||i18n.text("message.agent_time")}</span><strong id="report-agent-total">{move||duration_text(rows.with(|rows|rows.iter().filter(|row|row.agent).map(|row|row.seconds).fold(0u64,u64::saturating_add)))}</strong></div>
        <div><span>{move||i18n.text("message.pending_approval")}</span><strong>{move||rows.with(|rows|rows.iter().filter(|row|!row.agent&&!row.running&&row.state!="exported").count())}</strong><small>{move||i18n.text("message.entries_to_send_or_check")}</small></div></div>
        <crate::agent_usage::AgentUsage runs=usage_runs />
        <details><summary>{move||i18n.text("message.daily_human_summary")}</summary><dl><For each=move||daily.get() key=|(day,_)|day.clone() children=move|(day,seconds)|view!{<dt>{day}</dt><dd>{duration_text(seconds)}</dd>} /></dl></details>
        <p class="field-help">{move||i18n.text("message.active_timers_and_agents_may_still_increase_the_csv_contains_onl")}</p><button id="download-time-csv" type="button" disabled=move||loading.get()||!load_error.get().is_empty()||invalid.get()||!rows.with(|rows|rows.iter().any(|row|!row.agent&&!row.running)) on:click=download>{move||i18n.text("message.export_stopped_entries_csv")}</button>
        <p hidden=move||loading.get()||!load_error.get().is_empty()||!rows.with(|rows|rows.is_empty())>{move||i18n.text("message.no_time_in_this_selection")}</p><div><For each=move||rows.with(|rows|rows.iter().skip(page.get()*100).take(100).cloned().collect::<Vec<_>>()) key=|row|row.clone() children=move|row|{let card=row.card.clone();let edit_card=card.clone();let entry_id=StoredValue::new(row.id.clone());view!{<article class="workspace-row time-entry time-report-row"><div><p hidden=!row.running>{move||i18n.text("message.in_progress")}</p><strong>{row.date}" "{row.start}" · "{duration_text(row.seconds)}</strong><p>{move||i18n.text(if row.agent{"message.measured_active_agent_time"}else{"message.human_time"})}</p><button type="button" on:click=move|_|{on_task.run(card.clone());}>{row.ticket}" · "{row.title}</button><p class="entry-description">{row.description}</p></div><div class="row-actions"><button type="button" hidden=row.agent||!row.running disabled=move||pending.get() on:click=move|_|action.run((gloo_net::http::Method::POST,format!("/api/time/{}/stop",entry_id.get_value()),serde_json::json!({})))>{move||i18n.text("message.stop")}</button><button class="edit-report-time" type="button" hidden=row.agent||row.running||row.state!="draft" disabled=move||pending.get() on:click=move|_|{if let Some(entry)=board.times.with_untracked(|times|times.iter().find(|entry|field(entry,"id")==entry_id.get_value()).cloned()){editing.set(Some((edit_card.clone(),Some(entry))));}}>{move||i18n.text("message.edit")}</button><button class="delete-report-time danger" type="button" hidden=row.agent||row.running||row.state!="draft" disabled=move||pending.get() on:click=move|_|removing.set(board.times.with_untracked(|times|times.iter().find(|entry|field(entry,"id")==entry_id.get_value()).cloned()))>{move||i18n.text("message.delete_5e5d0216")}</button></div></article>}} /></div>
        <div class="editor-actions" hidden=move||rows.with(|rows|rows.len()<=100)><button type="button" disabled=move||page.get()==0 on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("message.previous")}</button><button type="button" disabled=move||rows.with(|rows|(page.get()+1)*100>=rows.len()) on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("message.next")}</button></div>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
        <div class="confirmation" role="alert" hidden=move||removing.get().is_none()><p>{move||i18n.text("message.permanently_delete_this_time_entry")}</p><button id="confirm-report-delete-time" class="danger" type="button" disabled=move||pending.get() on:click=move|_|{if let Some(entry)=removing.get_untracked(){action.run((gloo_net::http::Method::DELETE,format!("/api/time/{}",field(&entry,"id")),serde_json::json!({"expected":entry})));}}>{move||i18n.text("message.delete_permanently")}</button><button type="button" disabled=move||pending.get() on:click=move|_|removing.set(None)>{move||i18n.text("message.cancel")}</button></div>
        {move||editing.get().map(|(card_id,entry)|view!{<TimeEditor card_id entry board token on_close=Callback::new(move|()|editing.set(None)) />})}
    </section>}
}

#[component]
fn TimeEditor(
    card_id: String,
    entry: Option<Document>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let card = RwSignal::new(card_id);
    let initial = StoredValue::new(entry);
    let pending = RwSignal::new(false);
    let dirty = RwSignal::new(false);
    let discard = RwSignal::new(false);
    let navigation = StoredValue::new(None::<Callback<()>>);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if let Some(next) = navigation.get_value() {
            navigation.set_value(None);
            next.run(());
        } else {
            on_close.run(());
            request_animation_frame(move || {
                if let Some(element) = document()
                    .get_element_by_id("add-time")
                    .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    let _ = element.focus();
                }
            });
        }
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
    let guard = expect_context::<crate::NavigationGuard>();
    guard.1.update(|generation| *generation += 1);
    let guard_generation = guard.1.get_untracked();
    guard.0.set(Some(Callback::new(move |next: Callback<()>| {
        if !pending.get_untracked() {
            navigation.set_value(Some(next));
            request_close();
        }
    })));
    on_cleanup(move || {
        if guard.1.get_untracked() == guard_generation {
            guard.0.set(None);
        }
    });
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {<dialog id="time-dialog" class="time-entry-editor" node_ref=dialog aria-labelledby="time-title" on:cancel=move|event:web_sys::Event|{event.prevent_default();request_close();}>
        <header class="dialog-heading"><div><div class="eyebrow">{move||i18n.text("message.human_time_a2731ccf")}</div><h2 id="time-title">{move||i18n.text(if initial.with_value(Option::is_some){"message.edit_time"}else{"message.log_time"})}</h2></div><button id="close-time-editor" type="button" disabled=move||pending.get() on:click=move|_|request_close()>{move||i18n.text("message.close")}</button></header>
        <label>{move||i18n.text("message.task")}<select id="time-card" prop:value=move||card.get() disabled=move||pending.get()||dirty.get()||initial.with_value(Option::is_some) on:change=move|event|card.set(event_target_value(&event))><For each={move||board.cards.get().into_iter().filter(|card|!card.fields.flag("archived").unwrap_or(false)||initial.with_value(Option::is_some)).collect::<Vec<_>>()} key=|card|card.id.clone() children=move|card|view!{<option value=card.id>{card.title.clone()}</option>}/></select></label>
        {move||{let card_id=card.get();let archived=board.cards.with_untracked(|cards|cards.iter().find(|card|card.id==card_id).is_none_or(|card|card.fields.flag("archived").unwrap_or(false)));view!{<crate::time::TimeTask card_id board token pending dirty blocked=Memo::new(move|_|false) archived initial_entry=initial.get_value() editor_only=true on_saved=Some(Callback::new(move|()|close()))/>}}}
        <div class="confirmation" role="alert" hidden=move||!discard.get()><p>{move||i18n.text("message.discard_the_unsaved_time_entry")}</p><button id="discard-time-editor" class="danger" type="button" disabled=move||pending.get() on:click=move|_|close()>{move||i18n.text("message.discard")}</button><button id="keep-time-editor" type="button" on:click=move|_|{discard.set(false);navigation.set_value(None);} >{move||i18n.text("message.keep_editing")}</button></div>
    </dialog>}
}
