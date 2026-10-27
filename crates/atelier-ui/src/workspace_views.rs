use crate::{NavigationGuard, WorkspaceView, field, i18n::I18n, live::BoardState, read, write};
use atelier_core::{
    contracts::{Card, ServiceSession, ServiceState, ServiceStatus, Worktree},
    document::Document,
    domain::{agent_options::providers, projects::same_repository, time::duration_text},
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;

fn title(card: &Card) -> String {
    String::from_utf16_lossy(&card.title.0)
}
fn nested(document: &Document, key: &str) -> Document {
    document.get(key).ok().flatten().unwrap_or_default()
}
fn active(run: &Document) -> bool {
    matches!(
        field(run, "status").as_str(),
        "starting" | "running" | "stopping"
    )
}
fn needs_attention(card: &Card, runs: &[Document], cards: &[Card]) -> bool {
    if field(&card.fields, "status") == "done" {
        return false;
    }
    let blockers = serde_json::to_value(card).ok().is_some_and(|card| {
        let cards = cards
            .iter()
            .filter_map(|card| serde_json::to_value(card).ok())
            .collect::<Vec<_>>();
        !atelier_core::domain::tasks::blockers(&card, &cards).is_empty()
    });
    blockers
        || field(&nested(&card.fields, "ticketSync"), "state") == "error"
        || matches!(
            field(&nested(&card.fields, "automation"), "state").as_str(),
            "error" | "interrupted" | "cancelled" | "blocked"
        )
        || runs
            .iter()
            .find(|run| field(run, "cardId") == card.id)
            .is_some_and(|run| {
                !field(run, "attention").is_empty()
                    || run
                        .get::<Vec<serde_json::Value>>("interactions")
                        .ok()
                        .flatten()
                        .is_some_and(|items| !items.is_empty())
                    || matches!(
                        field(run, "status").as_str(),
                        "failed" | "interrupted" | "cancelled"
                    )
            })
}
fn agent_name(settings: &Document, run: &Document) -> String {
    let id = field(run, "agent");
    providers(settings)
        .unwrap_or_default()
        .iter()
        .find(|profile| field(profile, "id") == id)
        .map(|profile| field(profile, "name"))
        .filter(|name| !name.is_empty())
        .unwrap_or(id)
}
fn agent_state(run: &Document, now: f64) -> &'static str {
    if active(run)
        && run
            .get::<Vec<serde_json::Value>>("interactions")
            .ok()
            .flatten()
            .is_some_and(|items| !items.is_empty())
    {
        return "En attente de votre réponse";
    }
    if !field(run, "attention").is_empty() {
        return if active(run) {
            "Attention requise"
        } else {
            "Bloqué · consulter le journal"
        };
    }
    match field(run, "status").as_str() {
        "starting" => "Démarrage…",
        "stopping" => "Arrêt en cours",
        "completed" => "Terminée",
        "cancelled" => "Arrêtée",
        "interrupted" => "Interrompue",
        "failed" => "Échec",
        "running" => {
            let event = field(run, "lastEventAt");
            let started = if event.is_empty() {
                field(run, "startedAt")
            } else {
                event
            };
            if now - js_sys::Date::parse(&started) > 60_000.0 {
                "Sans signal · état à vérifier"
            } else {
                "Travaille"
            }
        }
        _ => "À examiner",
    }
}
fn elapsed(run: &Document, now: f64) -> String {
    let start = js_sys::Date::parse(&field(run, "startedAt"));
    let end = if active(run) {
        now
    } else {
        js_sys::Date::parse(&field(run, "finishedAt"))
    };
    if start.is_finite() && end.is_finite() {
        duration_text(((end - start).max(0.0) / 1000.0) as u64)
    } else {
        String::new()
    }
}
fn service_active(service: &ServiceState) -> bool {
    matches!(
        service.status,
        ServiceStatus::Starting | ServiceStatus::Running | ServiceStatus::Stopping
    )
}
fn session_active(session: &ServiceSession) -> bool {
    session.services.iter().any(service_active)
}
fn api_cost(run: &Document) -> Option<f64> {
    let usage = run.get::<serde_json::Value>("usage").ok().flatten()?;
    if usage["costSource"] != "openai-standard-api"
        || usage["costScope"] != "requested-model-thread"
    {
        return None;
    }
    usage["estimatedCostUsd"]
        .as_f64()
        .filter(|amount| amount.is_finite() && (0.0..=1_000_000.0).contains(amount))
}
fn usage_text(run: &Document, i18n: I18n) -> String {
    let Some(usage) = run.get::<serde_json::Value>("usage").ok().flatten() else {
        return String::new();
    };
    let counter = |key: &str| {
        usage[key]
            .as_u64()
            .map(|count| count.to_string())
            .unwrap_or_else(|| "—".into())
    };
    format!(
        "{} {} / {} / {}",
        i18n.text("Jetons entrée/cache/sortie :"),
        counter("inputTokens"),
        counter("cachedInputTokens"),
        counter("outputTokens")
    )
}
fn safe_link(url: &str) -> Option<String> {
    (url.starts_with("https://") || url.starts_with("http://")).then(|| url.to_owned())
}

/// Historical workspace panels from 7f48996, composed with the Rust application contracts.
#[component]
pub fn WorkspaceViews(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    view: Signal<WorkspaceView>,
    project: Signal<String>,
    on_open: Callback<String>,
    on_view: Callback<WorkspaceView>,
    on_focus: Callback<String>,
    on_projects: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let now = crate::time::clock();
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let removing = RwSignal::new(None::<String>);
    let cards = Signal::derive(move || {
        let repo = project.get();
        let windows = board
            .settings
            .with(|settings| field(settings, "platform") == "win32");
        board.cards.with(|cards| {
            cards
                .iter()
                .filter(|card| {
                    !card.fields.truthy("brainstormSession").unwrap_or(false)
                        && (view.get() == WorkspaceView::Processes
                            || repo.is_empty()
                            || same_repository(&field(&card.fields, "repo"), &repo, windows))
                        && (view.get() != WorkspaceView::Overview
                            || !card.fields.truthy("archived").unwrap_or(false))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let runs = Signal::derive(move || {
        board.runs.with(|runs| {
            runs.iter()
                .filter(|run| {
                    cards.with(|cards| cards.iter().any(|card| card.id == field(run, "cardId")))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let navigation = expect_context::<NavigationGuard>();
    navigation.1.update(|owner| *owner = owner.wrapping_add(1));
    let owner = navigation.1.get_untracked();
    let guard = navigation.0;
    guard.set(Some(Callback::new(
        move |continue_navigation: Callback<()>| {
            if busy.get_untracked() {
                error.set("Attends la fin de l’opération en cours.".into());
            } else if removing.get_untracked().is_some() {
                error.set(
                    "Confirme ou annule le retrait du worktree avant de quitter cette vue.".into(),
                );
            } else {
                continue_navigation.run(());
            }
        },
    )));
    on_cleanup(move || {
        if navigation.1.try_get_untracked() == Some(owner) {
            let _ = guard.try_set(None);
        }
    });
    let stop_run = Callback::new(move |id: String| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &format!("/api/runs/{id}/stop"),
                &serde_json::json!({}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                busy.set(false);
            }
        });
    });
    view! {
        <p class="inline-error" role="alert">{move || i18n.text(&error.get())}</p>
        <Show when=move || view.get() == WorkspaceView::Overview>
            <Overview board cards runs now project on_open on_view on_focus />
        </Show>
        <Show when=move || view.get() == WorkspaceView::Agents>
            <crate::queue::Queue board token on_open />
            <Agents board cards runs now busy on_open on_stop=stop_run />
        </Show>
        <Show when=move || view.get() == WorkspaceView::Processes>
            <Processes board token cards project busy error on_open on_projects />
        </Show>
        <Show when=move || view.get() == WorkspaceView::Worktrees>
            <Worktrees board token cards runs busy error removing on_open />
        </Show>
    }
}

#[component]
fn Overview(
    board: BoardState,
    cards: Signal<Vec<Card>>,
    runs: Signal<Vec<Document>>,
    now: RwSignal<f64>,
    project: Signal<String>,
    on_open: Callback<String>,
    on_view: Callback<WorkspaceView>,
    on_focus: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let attention = Signal::derive(move || {
        cards.with(|cards| {
            cards
                .iter()
                .filter(|card| needs_attention(card, &runs.get(), cards))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let priority = Signal::derive(move || {
        let mut items = attention.get();
        for status in ["review", "todo"] {
            for card in cards
                .get()
                .into_iter()
                .filter(|card| field(&card.fields, "status") == status)
            {
                if !items.iter().any(|item| item.id == card.id) {
                    items.push(card);
                }
            }
        }
        items.truncate(6);
        items
    });
    view! {
        <For each={move || cards.get().into_iter().filter(|card| field(&card.fields,"status")!="done" && matches!(field(&nested(&card.fields,"automation"),"state").as_str(),"ready"|"blocked"|"error"|"interrupted"|"cancelled")).collect::<Vec<_>>()} key=|card| serde_json::to_string(card).unwrap_or_default() children=move |card| {
            let id=card.id.clone(); let message=field(&nested(&card.fields,"automation"),"message");
            view!{<article class="worktree-box"><strong>{title(&card)}</strong><p>{i18n.text(&message)}</p><button data-open-card=id.clone() on:click=move |_|on_open.run(id.clone())>{i18n.text("Consulter le résultat")}</button><PullRequest card /></article>}
        } />
        <div class="overview-metrics">
            <button data-go-view="board" data-focus-target="review" on:click=move |_|on_focus.run("review".into())><span>{i18n.text("À vérifier")}</span><strong>{move ||cards.with(|cards|cards.iter().filter(|card|field(&card.fields,"status")=="review").count())}</strong><small>{i18n.text("Relire le travail terminé →")}</small></button>
            <button data-go-view="board" data-focus-target="attention" on:click=move |_|on_focus.run("attention".into())><span>{i18n.text("À reprendre")}</span><strong>{move ||attention.with(Vec::len)}</strong><small>{i18n.text("Examiner les interruptions →")}</small></button>
            <button data-go-view="agents" on:click=move |_|on_view.run(WorkspaceView::Agents)><span>{i18n.text("Agents actifs")}</span><strong>{move ||runs.with(|runs|runs.iter().filter(|run|active(run)).count())}</strong><small>{i18n.text("Suivre les exécutions →")}</small></button>
            <button data-go-view="time" on:click=move |_|on_view.run(WorkspaceView::Time)><span>{i18n.text("Temps à valider")}</span><strong>{move ||board.times.with(|entries|entries.iter().filter(|entry|board.cards.with(|cards|cards.iter().any(|card|card.id==field(entry,"cardId")&&(project.get().is_empty()||same_repository(&field(&card.fields,"repo"),&project.get(),board.settings.with(|settings|field(settings,"platform")=="win32")))))&&!entry.truthy("runningSince").unwrap_or(false)&&field(entry,"exportState")!="exported").count())}</strong><small>{i18n.text("Relire avant l’envoi Tempo →")}</small></button>
        </div>
        <div class="overview-columns"><section class="overview-section"><div class="panel-heading"><h2>{i18n.text("Votre prochain pas")}</h2></div>
            <For each={move ||priority.get()} key=|card|serde_json::to_string(card).unwrap_or_default() children=move |card| {
                let id=card.id.clone(); let label=if attention.with(|items|items.iter().any(|item|item.id==card.id)){"À reprendre"}else if field(&card.fields,"status")=="review"{"À vérifier"}else{"À faire"};
                let ticket=field(&nested(&card.fields,"ticket"),"key");let criteria=card.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist").ok().flatten().unwrap_or_default();
                view!{<button class="next-task" data-open-card=id.clone() on:click=move |_|on_open.run(id.clone())><span class="state-badge">{i18n.text(label)}</span><strong>{title(&card)}</strong><small>{if ticket.is_empty(){i18n.text("Tâche locale")}else{ticket}}{if criteria.is_empty(){String::new()}else{i18n.text(" · {0}/{1} critères validés").replace("{0}",&criteria.iter().filter(|item|item.done).count().to_string()).replace("{1}",&criteria.len().to_string())}}</small><span class="next-arrow">"→"</span></button>}
            } />
            <p class="empty" hidden=move ||!priority.with(Vec::is_empty)>{i18n.text("Rien à reprendre ou à relire. Vous pouvez préparer la suite.")}</p>
        </section><section class="overview-section"><div class="panel-heading"><h2>{i18n.text("Dernières exécutions")}</h2></div>
            <For each={move ||runs.get().into_iter().take(5).collect::<Vec<_>>()} key=|run|serde_json::to_string(run).unwrap_or_default() children=move |run| {
                let id=field(&run,"cardId");let name=cards.with(|cards|cards.iter().find(|card|card.id==id).map(title).unwrap_or_default());let current=StoredValue::new(run);
                view!{<button class="recent-run" data-open-card=id.clone() on:click=move |_|on_open.run(id.clone())><span class=format!("agent-pill {}",field(&current.get_value(),"agent"))>{agent_name(&board.settings.get_untracked(),&current.get_value())}</span><strong>{name}</strong><small data-agent-state=field(&current.get_value(),"id")>{move ||i18n.text(agent_state(&current.get_value(),now.get()))}</small><small data-clock=field(&current.get_value(),"id")>{move ||elapsed(&current.get_value(),now.get())}</small></button>}
            } />
            <p class="empty" hidden=move ||!runs.with(Vec::is_empty)>{i18n.text("Vos agents apparaîtront ici dès leur première tâche.")}</p>
            <button class="text-button" data-go-view="worktrees" on:click=move |_|on_view.run(WorkspaceView::Worktrees)>{i18n.text("Voir les branches, worktrees et PR →")}</button>
        </section></div>
    }
}

#[component]
fn Agents(
    board: BoardState,
    cards: Signal<Vec<Card>>,
    runs: Signal<Vec<Document>>,
    now: RwSignal<f64>,
    busy: RwSignal<bool>,
    on_open: Callback<String>,
    on_stop: Callback<String>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {
        <div class="view-stats">{[("En cours","Agents au travail",0),("Terminées","Exécutions à relire",1),("À examiner","Erreurs ou interruptions",2)].into_iter().map(move |(label,help,kind)|view!{<div><span>{i18n.text(label)}</span><strong>{move ||runs.with(|runs|runs.iter().filter(|run|match kind{0=>active(run),1=>field(run,"status")=="completed",_=>matches!(field(run,"status").as_str(),"failed"|"cancelled"|"interrupted")}).count())}</strong><small>{i18n.text(help)}</small></div>}).collect_view()}</div>
        <div class="panel-heading"><div><h2>{i18n.text("Vos exécutions")}</h2><p class="hint">{i18n.text("Suivez le travail en cours ou ouvrez une tâche pour consulter son résultat.")}</p></div></div>
        <details class="view-help"><summary>{i18n.text("Un agent ne répond plus ?")}</summary><p class="hint">{i18n.text("Le silence ne prouve pas un blocage. Après un refus de permission, adaptez les instructions puis relancez depuis la tâche.")}</p></details>
        <section class="worktree-box"><h3>{i18n.text("Coût API approximatif et temps agent")}</h3><p><strong>{move ||{let costs=runs.with(|runs|runs.iter().filter_map(api_cost).collect::<Vec<_>>());if costs.is_empty(){i18n.text("Indisponible")}else{format!("{:.4} USD",costs.iter().sum::<f64>())}}}</strong>" "{move ||i18n.text("sur {0} exécution(s) avec coût · Temps cumulé :").replace("{0}",&runs.with(|runs|runs.iter().filter(|run|api_cost(run).is_some()).count()).to_string())}" "<strong>{move ||duration_text(runs.with(|runs|runs.iter().filter_map(|run|run.get::<u64>("activeMilliseconds").ok().flatten()).fold(0u64,u64::saturating_add))/1000)}</strong>" "{move ||i18n.text("sur {0} exécution(s).").replace("{0}",&runs.with(Vec::len).to_string())}</p><p class="hint">{move ||i18n.text("{0} exécution(s) sans coût disponible. Périmètre affiché, toutes dates. Estimation API indicative ; la facturation de votre abonnement peut différer.").replace("{0}",&runs.with(|runs|runs.iter().filter(|run|api_cost(run).is_none()).count()).to_string())}</p><crate::agent_usage::AgentUsage runs /></section>
        <For each={move ||runs.get()} key=|run|serde_json::to_string(run).unwrap_or_default() children=move |run| {
            let card_id=field(&run,"cardId");let run_id=field(&run,"id");let data=StoredValue::new(run);let open=card_id.clone();let stop=run_id.clone();
            view!{<article class="workspace-row agent-run" data-status=field(&data.get_value(),"status")><div><span class=format!("agent-pill {}",field(&data.get_value(),"agent"))>{agent_name(&board.settings.get_untracked(),&data.get_value())}</span><h3>{cards.with(|cards|cards.iter().find(|card|card.id==card_id).map(title).unwrap_or_default())}</h3><p><span class="state-badge" data-agent-state=run_id.clone()>{move ||i18n.text(agent_state(&data.get_value(),now.get()))}</span>" "<span data-clock=run_id>{move ||elapsed(&data.get_value(),now.get())}</span></p><p class="hint">{let attention=field(&data.get_value(),"attention");if attention.is_empty(){field(&data.get_value(),"activity")}else{attention}}</p><p class="hint">{usage_text(&data.get_value(),i18n)}</p><code>{let branch=field(&data.get_value(),"branch");if branch.is_empty(){field(&data.get_value(),"repo")}else{branch}}</code></div><div class="row-actions"><button data-open-card=open.clone() on:click=move |_|on_open.run(open.clone())>{i18n.text("Ouvrir la tâche ↗")}</button><button class="danger" hidden=!active(&data.get_value()) disabled=move ||busy.get() data-stop-agent=stop.clone() on:click=move |_|on_stop.run(stop.clone())>{i18n.text("Arrêter")}</button></div></article>}
        } />
        <p class="empty" hidden=move ||!runs.with(Vec::is_empty)>{i18n.text("Aucun agent lancé pour ce projet.")}</p>
    }
}

#[component]
fn PullRequest(card: Card) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let pr = nested(&card.fields, "pullRequest");
    let pr = if pr.keys().next().is_none() {
        nested(&nested(&card.fields, "connectorPullRequest"), "pullRequest")
    } else {
        pr
    };
    let url = safe_link(&field(&pr, "url"));
    url.map(move |url|view!{<p><a href=url target="_blank" rel="noopener noreferrer">{i18n.text("Ouvrir la PR ↗")}</a></p>})
}

#[component]
fn Processes(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    cards: Signal<Vec<Card>>,
    project: Signal<String>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    on_open: Callback<String>,
    on_projects: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let sessions = RwSignal::new(Vec::<ServiceSession>::new());
    let selected = RwSignal::new(BTreeMap::<String, String>::new());
    let loading = Rc::new(Cell::new(false));
    let alive = Rc::new(Cell::new(true));
    let visible_projects = Signal::derive(move || {
        board
            .projects
            .get()
            .into_iter()
            .filter(|item| project.get().is_empty() || field(item, "repo") == project.get())
            .collect::<Vec<_>>()
    });
    let visible_sessions = Signal::derive(move || {
        let repo = project.get();
        if repo.is_empty() {
            return sessions.get();
        }
        let projects = board.projects.get();
        let windows = board
            .settings
            .with(|settings| field(settings, "platform") == "win32");
        let selected = projects
            .iter()
            .find(|item| same_repository(&field(item, "repo"), &repo, windows))
            .map(|item| field(item, "id"));
        sessions
            .get()
            .into_iter()
            .filter(|session| {
                selected
                    .as_ref()
                    .is_some_and(|id| session.project_ids.contains(id))
                    || cards.with(|cards| {
                        cards
                            .iter()
                            .find(|card| card.id == session.card_id)
                            .is_some_and(|card| {
                                same_repository(&field(&card.fields, "repo"), &repo, windows)
                                    || selected.as_ref().is_some_and(|id| {
                                        projects.iter().any(|item| {
                                            same_repository(
                                                &field(item, "repo"),
                                                &field(&card.fields, "repo"),
                                                windows,
                                            ) && field(item, "linkedProjectId") == *id
                                        })
                                    })
                            })
                    })
            })
            .collect::<Vec<_>>()
    });
    Effect::new(move |_| {
        let cards = cards.get();
        let sessions = sessions.get();
        let projects = visible_projects.get();
        let mut next = selected.get_untracked();
        let windows = board
            .settings
            .with_untracked(|settings| field(settings, "platform") == "win32");
        for item in projects {
            let id = field(&item, "id");
            let repo = field(&item, "repo");
            let valid = |candidate: &str| {
                cards.iter().any(|card| {
                    card.id == candidate
                        && card.fields.raw("worktree").is_some()
                        && !card.fields.truthy("archived").unwrap_or(false)
                        && same_repository(&field(&card.fields, "repo"), &repo, windows)
                })
            };
            if next.get(&id).is_some_and(|candidate| !valid(candidate)) {
                next.remove(&id);
            }
            if !next.contains_key(&id)
                && let Some(session) = sessions.iter().find(|session| {
                    session.project_ids.contains(&id)
                        && session_active(session)
                        && valid(&session.card_id)
                })
            {
                next.insert(id, session.card_id.clone());
            }
        }
        if next != selected.get_untracked() {
            selected.set(next);
        }
    });
    let refresh = {
        let loading = loading.clone();
        let alive = alive.clone();
        Rc::new(move || {
            if !alive.get() || loading.replace(true) {
                return;
            }
            let Ok(credential) = token.get_value() else {
                loading.set(false);
                return;
            };
            let targets = cards
                .get_untracked()
                .into_iter()
                .map(|card| card.id)
                .collect::<Vec<_>>();
            let loading = loading.clone();
            let alive = alive.clone();
            spawn_local(async move {
                let mut next = Vec::new();
                let mut failures = Vec::new();
                for id in targets {
                    let result = async {
                        read(&credential, &format!("/api/cards/{id}/services"))
                            .await?
                            .json::<Option<ServiceSession>>()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.".to_owned())
                    }
                    .await;
                    match result {
                        Ok(Some(session)) => next.push(session),
                        Ok(None) => {}
                        Err(message) => failures.push(message),
                    }
                }
                if alive.get() {
                    sessions.set(next);
                    if !failures.is_empty() {
                        error.set(failures.join("\n"));
                    }
                }
                loading.set(false);
            });
        })
    };
    {
        let refresh = refresh.clone();
        Effect::new(move |_| {
            let _ = cards.get();
            refresh();
        });
    }
    let timer_refresh = refresh.clone();
    let tick = Closure::<dyn FnMut()>::new(move || timer_refresh());
    let interval = web_sys::window().and_then(|window| {
        window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                3000,
            )
            .ok()
    });
    let timer = StoredValue::new_local((interval, tick));
    let cleanup_alive = StoredValue::new_local(alive);
    on_cleanup(move || {
        cleanup_alive.with_value(|alive| alive.set(false));
        timer.with_value(|(interval, _)| {
            if let (Some(window), Some(id)) = (web_sys::window(), *interval) {
                window.clear_interval_with_handle(id);
            }
        });
    });
    let action = {
        let refresh = StoredValue::new_local(refresh.clone());
        Callback::new(
            move |operations: Vec<(String, &'static str, Option<String>)>| {
                if busy.get_untracked() || operations.is_empty() {
                    return;
                }
                let Ok(credential) = token.get_value() else {
                    return;
                };
                busy.set(true);
                error.set(String::new());
                let refresh = refresh.get_value();
                spawn_local(async move {
                    let mut failures = Vec::new();
                    for (id, operation, service) in operations {
                        let mut body = serde_json::json!({"standalone":true});
                        if let Some(service) = service {
                            body["serviceId"] = serde_json::json!(service);
                        }
                        if let Err(message) = write(
                            &credential,
                            Method::POST,
                            &format!("/api/cards/{id}/services/{operation}"),
                            &body,
                        )
                        .await
                        {
                            failures.push(message);
                        }
                    }
                    let _ = board.refresh(&credential).await;
                    if busy.try_get_untracked().is_some() {
                        busy.set(false);
                        error.set(failures.join("\n"));
                        refresh();
                    }
                });
            },
        )
    };
    let start_selected = move || {
        selected
            .get_untracked()
            .into_iter()
            .filter(|(project_id, id)| {
                visible_projects.with_untracked(|projects| {
                    projects
                        .iter()
                        .any(|project| field(project, "id") == *project_id)
                }) && !sessions.with(|sessions| {
                    sessions
                        .iter()
                        .any(|session| session.card_id == *id && session_active(session))
                })
            })
            .map(|(_, id)| (id, "start", None))
            .collect::<Vec<_>>()
    };
    let manual_refresh = refresh.clone();
    view! {
        <div class="panel-heading"><div><h2>{i18n.text("Lancer les projets")}</h2><p class="hint">{i18n.text("Choisissez un worktree par projet. Ses commandes Front et Back démarrent ensemble, dans ce worktree. Pour deux dépôts, sélectionnez une ligne pour chacun.")}</p></div><div class="row-actions"><button id="start-selected-projects" class="primary" disabled=move ||busy.get()||start_selected().is_empty() on:click=move |_|action.run(start_selected())>{i18n.text("Lancer les sélections")}</button><button id="refresh-processes" disabled=move ||busy.get() on:click=move |_|manual_refresh()>{i18n.text("Actualiser")}</button><button id="stop-visible-processes" class="danger" disabled=move ||busy.get()||!visible_sessions.with(|sessions|sessions.iter().any(session_active)) on:click=move |_|action.run(visible_sessions.get_untracked().iter().filter(|session|session_active(session)).map(|session|(session.card_id.clone(),"stop",None)).collect())>{i18n.text("Tout arrêter dans cette vue")}</button></div></div>
        <For each={move ||visible_projects.get()} key=|item|serde_json::to_string(item).unwrap_or_default() children=move |item|{
            let id=StoredValue::new(field(&item,"id"));let repo=field(&item,"repo");
            let choices=Signal::derive(move ||cards.get().into_iter().filter(|card|card.fields.raw("worktree").is_some()&&!card.fields.truthy("archived").unwrap_or(false)&&same_repository(&field(&card.fields,"repo"),&repo,board.settings.with(|settings|field(settings,"platform")=="win32"))).collect::<Vec<_>>());
            let current=Signal::derive(move ||selected.with(|selected|selected.get(&id.get_value()).cloned().unwrap_or_default()));
            let configured=!field(&item,"frontCommand").is_empty()||!field(&item,"backCommand").is_empty();
            let managed=[field(&item,"frontCommand"),field(&item,"backCommand")].iter().filter(|command|!command.is_empty()).all(|command|command.contains("{port}"));
            let occupied=move ||sessions.with(|sessions|sessions.iter().any(|session|session.project_ids.contains(&id.get_value())&&session_active(session)&&(!managed||session.services.iter().any(|service|service.port.is_none()))));
            view!{<article class="workspace-row"><div><h3>{field(&item,"name")}</h3><p class="hint">{if configured{[(!field(&item,"frontCommand").is_empty()).then(||i18n.text("Front")),(!field(&item,"backCommand").is_empty()).then(||i18n.text("Back"))].into_iter().flatten().collect::<Vec<_>>().join(" + ")}else{i18n.text("Renseignez les commandes dans les réglages.")}}</p><label>{i18n.text("Worktree")}<select data-launch-project=id.get_value() disabled=move ||busy.get()||occupied()||!configured prop:value=move ||current.get() on:change=move |event|selected.update(|selected|{let value=event_target_value(&event);if value.is_empty(){selected.remove(&id.get_value());}else{selected.insert(id.get_value(),value);}})><option value="">{move ||if choices.with(Vec::is_empty){i18n.text("Préparez un worktree depuis une carte")}else{i18n.text("Ne pas lancer")}}</option><For each={move ||choices.get()} key=|card|serde_json::to_string(card).unwrap_or_default() children=move |card|{let branch=card.fields.get::<Worktree>("worktree").ok().flatten().map(|worktree|worktree.branch).unwrap_or_else(||title(&card));view!{<option value=card.id.clone()>{format!("{branch} · {}",title(&card))}</option>}} /></select></label><p class="path-text">{move ||choices.with(|cards|cards.iter().find(|card|card.id==current.get()).and_then(|card|card.fields.get::<Worktree>("worktree").ok().flatten()).map(|worktree|worktree.path).unwrap_or_default())}</p></div><div class="row-actions"><button data-launch-one=id.get_value() disabled=move ||busy.get()||occupied()||!configured||current.get().is_empty() on:click=move |_|action.run(vec![(current.get_untracked(),"start",None)])>{i18n.text("Lancer")}</button><button data-launch-settings=id.get_value() disabled=move ||busy.get() on:click=move |_|on_projects.run(())>{i18n.text("Configurer le lancement")}</button></div></article>}
        } />
        <h3>{move ||i18n.text("Services et journaux · {0} en cours").replace("{0}",&visible_sessions.with(|sessions|sessions.iter().flat_map(|session|&session.services).filter(|service|matches!(service.status,ServiceStatus::Running)).count()).to_string())}</h3>
        <For each={move ||cards.get().into_iter().filter(|card|visible_sessions.with(|sessions|sessions.iter().any(|session|session.card_id==card.id))).collect::<Vec<_>>()} key=|card|serde_json::to_string(card).unwrap_or_default() children=move |card|{
            let id=StoredValue::new(card.id.clone());let current=Signal::derive(move ||sessions.with(|sessions|sessions.iter().find(|session|session.card_id==id.get_value()).cloned()));
            let owner=board.projects.with_untracked(|projects|projects.iter().find(|project|same_repository(&field(project,"repo"),&field(&card.fields,"repo"),board.settings.with_untracked(|settings|field(settings,"platform")=="win32"))).cloned());
            let ready=card.fields.raw("worktree").is_some()&&!card.fields.truthy("archived").unwrap_or(false)&&owner.as_ref().is_some_and(|project|!field(project,"frontCommand").is_empty()||!field(project,"backCommand").is_empty());
            let branch=card.fields.get::<Worktree>("worktree").ok().flatten().map(|worktree|worktree.branch).unwrap_or_else(||i18n.text("Worktree de la carte"));
            view!{<article class="process-stack"><div class="panel-heading"><div><span class="eyebrow">{owner.map(|owner|field(&owner,"name")).unwrap_or_else(||i18n.text("Projet retiré"))}</span><h3>{title(&card)}</h3><p class="hint">{branch}{if card.fields.truthy("archived").unwrap_or(false){i18n.text(" · Carte archivée")}else{String::new()}}</p></div><div class="row-actions"><button data-open-card=id.get_value() disabled=move ||busy.get() on:click=move |_|on_open.run(id.get_value())>{i18n.text("Voir la carte")}</button><button data-process-action="start" data-process-card=id.get_value() disabled=move ||busy.get()||!ready||current.with(|session|session.as_ref().is_some_and(session_active)) on:click=move |_|action.run(vec![(id.get_value(),"start",None)])>{i18n.text("Démarrer l’ensemble")}</button><button class="danger" data-process-action="stop" data-process-card=id.get_value() disabled=move ||busy.get()||current.with(|session|session.as_ref().is_none_or(|session|!session_active(session))) on:click=move |_|action.run(vec![(id.get_value(),"stop",None)])>{i18n.text("Arrêter l’ensemble")}</button></div></div><div class="process-services"><For each={move ||current.get().map(|session|session.services).unwrap_or_default()} key=|service|service.id.clone() children=move |service|{
                let service_id=StoredValue::new(service.id.clone());let service=Signal::derive(move ||current.get().and_then(|session|session.services.into_iter().find(|service|service.id==service_id.get_value())));
                view!{<ProcessService service card_id=id busy ready on_action=action />}
            } /></div><p class="hint" hidden=ready>{i18n.text("Worktree absent : préparez-le depuis la carte avant de relancer.")}</p><details class="process-journal" data-process-journal=id.get_value()><summary>{i18n.text("Journal de l’ensemble")}</summary><pre class="log" data-process-log=id.get_value()>{move ||current.get().map(|session|session.services.into_iter().map(|service|format!("{}\n{}",service.name,service.log)).collect::<Vec<_>>().join("\n")).unwrap_or_default()}</pre></details></article>}
        } />
        <p class="empty" hidden=move ||!visible_sessions.with(Vec::is_empty)>{i18n.text("Aucun service lancé. Préparez le worktree d’une carte puis sélectionnez-le ci-dessus.")}</p>
    }
}

#[component]
fn ProcessService(
    service: Signal<Option<ServiceState>>,
    card_id: StoredValue<String>,
    busy: RwSignal<bool>,
    ready: bool,
    on_action: Callback<Vec<(String, &'static str, Option<String>)>>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    view! {<section class="process-service"><div class="panel-heading"><div><h4>{move ||service.get().map(|service|service.name).unwrap_or_default()}</h4><span class="state-badge" data-status=move ||service.get().and_then(|service|serde_json::to_value(service.status).ok()).and_then(|value|value.as_str().map(str::to_owned)).unwrap_or_default()>{move ||service.get().map(|service|match service.status{ServiceStatus::Starting|ServiceStatus::Running=>i18n.text("En cours"),ServiceStatus::Stopping=>i18n.text("Arrêt en cours"),_=>i18n.text("Arrêté{0}").replace("{0}",&service.exit_code.map(|code|i18n.text(" · code {0}").replace("{0}",&code.to_string())).unwrap_or_default())}).unwrap_or_default()}</span></div><div class="row-actions">{[("start","Démarrer"),("restart","Redémarrer"),("stop","Arrêter")].into_iter().map(move |(operation,label)|view!{<button class=if operation=="stop"{"danger"}else{""} data-process-action=operation data-process-card=card_id.get_value() data-service-id=move ||service.get().map(|service|service.id).unwrap_or_default() disabled=move ||busy.get()||service.with(|service|service.as_ref().is_none_or(|service|if operation=="stop"{!service_active(service)}else if operation=="restart"{!ready||!matches!(service.status,ServiceStatus::Running)}else{!ready||service_active(service)})) on:click=move |_|{if let Some(service)=service.get_untracked(){on_action.run(vec![(card_id.get_value(),if operation=="start"{"restart"}else{operation},Some(service.id))]);}}>{i18n.text(label)}</button>}).collect_view()}</div></div>
        <p class="path-text">{move ||service.get().map(|service|service.directory).unwrap_or_default()}</p><p class="hint">{i18n.text("Port attribué par Atelier :")}" "<code>{move ||service.get().and_then(|service|service.port).map(|port|port.to_string()).unwrap_or_else(||"—".into())}</code>" "{i18n.text("· PID lanceur :")}" "<code>{move ||service.get().and_then(|service|service.pid).map(|pid|pid.to_string()).unwrap_or_else(||"—".into())}</code>{move ||service.get().map(|service|if service.process_ids.len()>1{i18n.text(" · Processus enfants : {0}").replace("{0}",&service.process_ids.iter().filter(|pid|Some(**pid)!=service.pid).map(u32::to_string).collect::<Vec<_>>().join(", "))}else{String::new()}).unwrap_or_default()}</p>
        <p class="inline-error">{move ||service.get().map(|service|[service.error,service.inspection_error].into_iter().filter(|error|!error.is_empty()).collect::<Vec<_>>().join("\n")).unwrap_or_default()}</p>
        <div class="process-ports" hidden=move ||service.with(|service|service.as_ref().is_none_or(|service|service.endpoints.is_empty()))><table><caption class="hint">{i18n.text("Adresses TCP en écoute")}</caption><thead><tr><th>{i18n.text("Adresse IP")}</th><th>{i18n.text("Port")}</th><th>"PID"</th><th>{i18n.text("Application")}</th></tr></thead><tbody><For each={move ||service.get().map(|service|service.endpoints).unwrap_or_default()} key=|endpoint|(endpoint.pid,endpoint.address.clone(),endpoint.port) children=move |endpoint|view!{<tr><td><code>{endpoint.address.clone()}</code>{if ["0.0.0.0","::"].contains(&endpoint.address.as_str()){i18n.text("(toutes interfaces)")}else{String::new()}}</td><td>{endpoint.port}</td><td>{endpoint.pid}</td><td>{endpoint.url.and_then(|url|safe_link(&url)).map(move |url|{let protocol=if url.starts_with("https:"){ "HTTPS" }else{"HTTP"};view!{<a href=url target="_blank" rel="noopener noreferrer">{i18n.text("Ouvrir {0} ↗").replace("{0}",protocol)}</a>}})}</td></tr>} /></tbody></table></div>
        <p class="hint" hidden=move ||service.with(|service|service.as_ref().is_some_and(|service|!service.endpoints.is_empty()))>{move ||if service.with(|service|service.as_ref().is_some_and(service_active)){i18n.text("Aucun port TCP en écoute détecté pour l’instant.")}else{i18n.text("Aucun port en écoute.")}}</p>
    </section>}
}

fn cleanup_candidate(card: &Card) -> bool {
    card.fields.raw("worktree").is_some()
        && (field(&card.fields, "status") == "done"
            || card.fields.truthy("archived").unwrap_or(false)
            || field(&nested(&card.fields, "pullRequest"), "status") == "completed")
}

#[component]
fn Worktrees(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    cards: Signal<Vec<Card>>,
    runs: Signal<Vec<Document>>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    removing: RwSignal<Option<String>>,
    on_open: Callback<String>,
) -> impl IntoView {
    use atelier_core::contracts::{
        ForgeConnections, PullRequest, PullRequestAttach, WorktreeInspection,
    };
    let i18n = expect_context::<I18n>();
    let checks = RwSignal::new(BTreeMap::<String, Result<WorktreeInspection, String>>::new());
    let force = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    let inspect = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let selected = cards
            .get_untracked()
            .into_iter()
            .filter(|card| card.fields.raw("worktree").is_some())
            .collect::<Vec<_>>();
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let mut next = BTreeMap::new();
            for card in selected {
                let result = async {
                    read(
                        &credential,
                        &format!("/api/cards/{}/worktree/inspection", card.id),
                    )
                    .await?
                    .json::<WorktreeInspection>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
                }
                .await;
                next.insert(card.id, result);
            }
            if busy.try_get_untracked().is_some() {
                checks.set(next);
                busy.set(false);
            }
        });
    });
    let refresh_prs = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        let Ok(credential) = token.get_value() else {
            return;
        };
        let selected = cards
            .get_untracked()
            .into_iter()
            .filter(|card| {
                card.fields.raw("pullRequest").is_some()
                    || card.fields.raw("connectorPullRequest").is_some()
            })
            .collect::<Vec<_>>();
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            let mut failures = Vec::new();
            let configurations = async {
                read(&credential, "/api/forges")
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            for card in selected {
                let result=async{
                    if let Some(pr)=card.fields.get::<PullRequest>("pullRequest").map_err(|_|"Liaison de PR/MR invalide.".to_owned())?{
                        let configuration=configurations.as_ref().map_err(Clone::clone)?;
                        if !configuration.connections.iter().any(|connection|connection.configuration.kind==pr.kind&&connection.configuration.base==pr.base_url){return Err("Reconnecte le site de forge de cette PR/MR avant de l’actualiser.".into());}
                        let body=serde_json::to_value(PullRequestAttach{expected:atelier_core::application::jira::card_snapshot(&card),revision:configuration.revision,kind:pr.kind,repository:pr.repository,number:pr.number,confirmed:true}).map_err(|_|"Liaison de PR/MR invalide.".to_owned())?;
                        write(&credential,Method::POST,&format!("/api/cards/{}/forge/attach",card.id),&body).await?;
                    }else if let Some(link)=card.fields.get::<atelier_core::application::extensions::ModulePullRequestLink>("connectorPullRequest").map_err(|_|"Liaison de PR/MR invalide.".to_owned())?{
                        let input=serde_json::json!({"expectedRevision":link.revision,"method":"repositoriesRefresh","action":null,"confirmed":true,"input":{"expected":card,"target":link.pull_request.target_branch}});
                        write(&credential,Method::POST,&format!("/api/extensions/{}/invoke",link.module),&input).await?;
                    }
                    Ok::<(),String>(())
                }.await;
                if let Err(message) = result {
                    failures.push(format!("{} : {message}", title(&card)));
                }
            }
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                busy.set(false);
                if failures.is_empty() {
                    notice.set("États des PR actualisés.".into());
                } else {
                    error.set(failures.join("\n"));
                }
            }
        });
    });
    let remove = move || {
        if busy.get_untracked() {
            return;
        }
        let (Some(id), Ok(credential)) = (removing.get_untracked(), token.get_value()) else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        let discard = force.get_untracked();
        spawn_local(async move {
            let result = write(
                &credential,
                Method::POST,
                &format!("/api/cards/{id}/worktree/remove"),
                &serde_json::json!({"confirmed":true,"discardChanges":discard}),
            )
            .await;
            let _ = board.refresh(&credential).await;
            if busy.try_get_untracked().is_some() {
                busy.set(false);
                match result {
                    Ok(_) => {
                        removing.set(None);
                        checks.update(|checks| {
                            checks.remove(&id);
                        });
                        notice.set("Worktrees retirés, branches conservées.".into());
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let check_label = move |id: &str, cleanup: bool| {
        checks.with(|checks| match checks.get(id) {
            Some(Ok(check)) => i18n.text(if check.clean {
                if cleanup {
                    "Dossier principal propre"
                } else {
                    "Dossier propre"
                }
            } else if cleanup {
                "Modifications locales à conserver"
            } else {
                "Modifications locales"
            }),
            Some(Err(message)) => i18n.text(message),
            None => i18n.text("État Git non vérifié"),
        })
    };
    view! {
        <div class="panel-heading"><div><h2>{i18n.text("Dossiers de travail")}</h2><p class="hint">{i18n.text("Vos branches, leurs dossiers de travail et les pull requests associées.")}</p></div><div class="row-actions"><button id="refresh-prs" disabled=move ||busy.get() on:click=move |_|refresh_prs.run(())>{i18n.text("Actualiser les PR")}</button><button id="inspect-worktrees" disabled=move ||busy.get() on:click=move |_|inspect.run(())>{i18n.text("Actualiser l’état Git")}</button></div></div>
        <p role="status">{move ||i18n.text(&notice.get())}</p>
        <details class="disclosure"><summary>{i18n.text("Nettoyer après livraison")}" "<span class="count">{move ||cards.with(|cards|cards.iter().filter(|card|cleanup_candidate(card)).count())}</span></summary><p class="hint">{i18n.text("Tâches terminées, archivées ou PR fusionnées. Actualise l’état Git avant de choisir. Le retrait conserve les branches et refuse les modifications locales ; les worktrees associés sont également contrôlés.")}</p><For each={move ||cards.get().into_iter().filter(cleanup_candidate).collect::<Vec<_>>()} key=|card|serde_json::to_string(card).unwrap_or_default() children=move |card|{
            let id=StoredValue::new(card.id.clone());
            view!{<article class="workspace-row"><div><h3>{title(&card)}</h3><p class="hint">{move ||check_label(&id.get_value(),true)}</p></div><button class="danger" data-clean-candidate=id.get_value() disabled=move ||busy.get()||runs.with(|runs|runs.iter().any(|run|field(run,"cardId")==id.get_value()&&active(run))) on:click=move |_|{force.set(false);removing.set(Some(id.get_value()));}>{i18n.text("Retirer sans forcer")}</button></article>}
        } /><p class="hint" hidden=move ||cards.with(|cards|cards.iter().any(cleanup_candidate))>{i18n.text("Aucun dossier candidat au nettoyage.")}</p></details>
        <For each={move ||cards.get().into_iter().filter(|card|card.fields.raw("worktree").is_none()&&(card.fields.raw("pullRequest").is_some()||card.fields.raw("connectorPullRequest").is_some())).collect::<Vec<_>>()} key=|card|serde_json::to_string(card).unwrap_or_default() children=move |card|{let id=card.id.clone();view!{<article class="workspace-row"><div><h3>{title(&card)}</h3><PullRequest card /></div><div class="row-actions"><button data-open-card=id.clone() on:click=move |_|on_open.run(id.clone())>{i18n.text("Ouvrir la tâche ↗")}</button></div></article>}} />
        <For each={move ||cards.get().into_iter().filter(|card|card.fields.raw("worktree").is_some()).collect::<Vec<_>>()} key=|card|serde_json::to_string(card).unwrap_or_default() children=move |card|{
            let id=StoredValue::new(card.id.clone());let worktree=card.fields.get::<Worktree>("worktree").ok().flatten();
            let key=field(&nested(&card.fields,"ticket"),"key");let services=card.fields.get::<atelier_core::application::worktrees::ServiceWorktrees>("serviceWorktrees").ok().flatten().unwrap_or_default();
            view!{<article class="workspace-row worktree-row"><div><span class="section-kicker">{if key.is_empty(){i18n.text("Branche locale")}else{key}}</span><h3>{title(&card)}</h3><code class="branch-label">{worktree.as_ref().map(|worktree|worktree.branch.clone()).unwrap_or_default()}</code><p class="path-text resource-location">{worktree.map(|worktree|worktree.path).unwrap_or_default()}</p>{services.into_iter().map(move |(role,worktree)|view!{<p class="path-text resource-location">{i18n.text("Worktree {0} : {1} · {2}").replace("{0}",&role).replace("{1}",&worktree.path).replace("{2}",&worktree.branch)}</p>}).collect_view()}<p class="hint">{move ||check_label(&id.get_value(),false)}</p><PullRequest card /></div><div class="row-actions"><button data-open-card=id.get_value() disabled=move ||busy.get() on:click=move |_|on_open.run(id.get_value())>{i18n.text("Ouvrir la tâche ↗")}</button><button class="danger" data-clean-worktree=id.get_value() disabled=move ||busy.get()||runs.with(|runs|runs.iter().any(|run|field(run,"cardId")==id.get_value()&&active(run))) on:click=move |_|{force.set(true);removing.set(Some(id.get_value()));}>{i18n.text("Retirer le worktree")}</button></div></article>}
        } />
        <p class="empty" hidden=move ||cards.with(|cards|cards.iter().any(|card|card.fields.raw("worktree").is_some()))>{i18n.text("Aucun worktree. Le premier sera créé au lancement d’une tâche.")}</p>
        <Show when=move ||removing.get().is_some()><RemoveConfirmation busy force removing on_confirm=Callback::new(move |()|remove()) /></Show>
    }
}

#[component]
fn RemoveConfirmation(
    busy: RwSignal<bool>,
    force: RwSignal<bool>,
    removing: RwSignal<Option<String>>,
    on_confirm: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |dialog| {
        let _ = dialog.show_modal();
    });
    let cancel = move || {
        if !busy.get_untracked() {
            removing.set(None);
        }
    };
    view! {<dialog class="confirm-dialog" role="alertdialog" aria-labelledby="worktree-confirm-title" aria-describedby="worktree-confirm-message" node_ref=dialog on:cancel=move |event:web_sys::Event|{event.prevent_default();cancel();}>
        <div class="dialog-heading"><div class="confirm-icon" aria-hidden="true"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10.3 4.6 2.5 18a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 4.6a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4m0 4h.01"/></svg></div><button type="button" disabled=move ||busy.get() aria-label=move ||i18n.text("Fermer") on:click=move |_|cancel()>"×"</button></div>
        <h2 id="worktree-confirm-title">{move ||i18n.text(if force.get(){"Retirer le worktree ?"}else{"Nettoyer ce dossier ?"})}</h2><p id="worktree-confirm-message">{move ||i18n.text(if force.get(){"Retirer le worktree principal et les worktrees Front / Back associés, et supprimer leurs fichiers locaux, y compris les modifications non commitées et les fichiers non suivis ? Cette perte est irréversible. Les branches et leurs commits seront conservés. Aucun ticket Jira ni autre service ne sera modifié."}else{"Retirer le worktree principal et ses worktrees associés sans forcer ? Les branches sont conservées. Des modifications locales feront refuser le retrait."})}</p>
        <div class="form-actions"><button type="button" autofocus disabled=move ||busy.get() on:click=move |_|cancel()>{i18n.text("Annuler")}</button><button class="confirm-action" type="button" disabled=move ||busy.get() on:click=move |_|on_confirm.run(())>{move ||i18n.text(if force.get(){"Retirer et supprimer les fichiers"}else{"Retirer sans forcer"})}</button></div>
    </dialog>}
}
