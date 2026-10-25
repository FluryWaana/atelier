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
                    title: task
                        .map(|card| String::from_utf16_lossy(&card.title.0))
                        .unwrap_or_default(),
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
                    running: entry.truthy("runningSince").unwrap_or(false),
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
                    title: task
                        .map(|card| String::from_utf16_lossy(&card.title.0))
                        .unwrap_or_default(),
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
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
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
                "Date",
                "Heure",
                "Ticket",
                "Tâche",
                "Secondes",
                "Description",
                "État Tempo",
                "ID Tempo",
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
                .map_err(|_| "Création du CSV impossible.".to_owned())?;
            let next = web_sys::Url::create_object_url_with_blob(&blob)
                .map_err(|_| "Création du CSV impossible.".to_owned())?;
            if let Some(previous) = url.get_untracked() {
                let _ = web_sys::Url::revoke_object_url(&previous);
            }
            url.set(Some(next.clone()));
            let document = web_sys::window()
                .and_then(|window| window.document())
                .ok_or("Navigateur indisponible.")?;
            let anchor = document
                .create_element("a")
                .map_err(|_| "Téléchargement indisponible.".to_owned())?
                .dyn_into::<web_sys::HtmlAnchorElement>()
                .map_err(|_| "Téléchargement indisponible.".to_owned())?;
            anchor.set_href(&next);
            anchor.set_download("atelier-time.csv");
            let body = document.body().ok_or("Navigateur indisponible.")?;
            body.append_child(&anchor)
                .map_err(|_| "Téléchargement indisponible.".to_owned())?;
            anchor.click();
            anchor.remove();
            Ok(())
        })();
        error.set(result.err().unwrap_or_default());
    };
    view! {<dialog id="time-report" class="editor" node_ref=dialog aria-labelledby="time-report-heading" aria-busy=move||loading.get().to_string() on:cancel=move|event:web_sys::Event|{event.prevent_default();close();}>
        <header class="editor-heading"><h2 id="time-report-heading">{move||i18n.text("Synthèse du temps")}</h2><button id="close-time-report" type="button" on:click=move|_|close()>{move||i18n.text("Fermer")}</button></header>
        <p role="status" hidden=move||!loading.get()>{move||i18n.text("Chargement du tableau…")}</p><p class="error" role="alert">{move||i18n.text(&load_error.get())}</p>
        <p class="field-help" role="status" hidden=move||board.connected.get()&&board.sync_error.get().is_empty()>{move||i18n.text(&if board.sync_error.get().is_empty(){"Actualisation interrompue. Reconnexion automatique…".to_owned()}else{board.sync_error.get()})}</p>
        <p class="field-help">{move||i18n.text("Les saisies humaines utilisent leur date locale. Les durées actives des agents sont rattachées à la date de début de la session en UTC ; elles ne sont pas réparties entre les jours.")}</p>
        <label for="report-from">{move||i18n.text("Du")}</label><input id="report-from" type="date" prop:value=move||from.get() on:input=move|event|{from.set(event_target_value(&event));page.set(0);} />
        <label for="report-to">{move||i18n.text("Au")}</label><input id="report-to" type="date" prop:value=move||to.get() on:input=move|event|{to.set(event_target_value(&event));page.set(0);} />
        <label for="report-project">{move||i18n.text("Projet")}</label><select id="report-project" prop:value=move||repo.get() on:change=move|event|{repo.set(event_target_value(&event));page.set(0);} ><option value="">{move||i18n.text("Tous les projets")}</option><For each=move||board.projects.get() key=|project|field(project,"id") children=move|project|view!{<option value=field(&project,"repo")>{field(&project,"name")}</option>} /></select>
        <label for="report-source">{move||i18n.text("Source du temps")}</label><select id="report-source" prop:value=move||source.get() on:change=move|event|{source.set(event_target_value(&event));page.set(0);} ><option value="all">{move||i18n.text("Toutes les sources")}</option><option value="human">{move||i18n.text("Temps humain")}</option><option value="agent">{move||i18n.text("Temps actif des agents mesuré")}</option></select>
        <label for="report-state">{move||i18n.text("État d’envoi humain")}</label><select id="report-state" prop:value=move||status.get() on:change=move|event|{status.set(event_target_value(&event));page.set(0);} ><option value="all">{move||i18n.text("Tous les états")}</option><option value="draft">{move||i18n.text("Non envoyé")}</option><option value="exported">{move||i18n.text("Envoyé")}</option><option value="uncertain">{move||i18n.text("Envoi incertain")}</option></select>
        <p role="alert" hidden=move||!invalid.get()>{move||i18n.text("La date de fin doit suivre la date de début.")}</p>
        <p id="report-human-total">{move||format!("{} : {}",i18n.text("Temps humain enregistré"),duration_text(rows.with(|rows|rows.iter().filter(|row|!row.agent).map(|row|row.seconds).fold(0u64,u64::saturating_add))))}</p>
        <p id="report-agent-total">{move||format!("{} : {}",i18n.text("Temps actif des agents mesuré"),duration_text(rows.with(|rows|rows.iter().filter(|row|row.agent).map(|row|row.seconds).fold(0u64,u64::saturating_add))))}</p>
        <crate::agent_usage::AgentUsage runs=usage_runs />
        <details><summary>{move||i18n.text("Synthèse humaine par jour")}</summary><dl><For each=move||daily.get() key=|(day,_)|day.clone() children=move|(day,seconds)|view!{<dt>{day}</dt><dd>{duration_text(seconds)}</dd>} /></dl></details>
        <p class="field-help">{move||i18n.text("Les chronomètres et agents actifs peuvent encore augmenter. Le CSV contient uniquement les saisies humaines arrêtées.")}</p><button id="download-time-csv" type="button" disabled=move||loading.get()||!load_error.get().is_empty()||invalid.get()||!rows.with(|rows|rows.iter().any(|row|!row.agent&&!row.running)) on:click=download>{move||i18n.text("Exporter les saisies arrêtées (CSV)")}</button>
        <p hidden=move||loading.get()||!load_error.get().is_empty()||!rows.with(|rows|rows.is_empty())>{move||i18n.text("Aucun temps dans cette sélection.")}</p><ul><For each=move||rows.with(|rows|rows.iter().skip(page.get()*100).take(100).cloned().collect::<Vec<_>>()) key=|row|row.clone() children=move|row|{let card=row.card.clone();view!{<li class="time-report-row"><p hidden=!row.running>{move||i18n.text("En cours")}</p><strong>{row.date}" "{row.start}" · "{duration_text(row.seconds)}</strong><p>{move||i18n.text(if row.agent{"Temps actif des agents mesuré"}else{"Temps humain"})}</p><button type="button" on:click=move|_|{if let Some(node)=dialog.get_untracked(){node.close();}on_task.run(card.clone());}>{row.ticket}" · "{row.title}</button><details><summary>{move||i18n.text("Description")}</summary><pre class="service-log">{row.description}</pre></details></li>}} /></ul>
        <div class="editor-actions" hidden=move||rows.with(|rows|rows.len()<=100)><button type="button" disabled=move||page.get()==0 on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled=move||rows.with(|rows|(page.get()+1)*100>=rows.len()) on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button></div>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </dialog>}
}
