use crate::{i18n::I18n, live::BoardState, read, replace_card, write};
use atelier_core::contracts::{
    Card, ForgeConnections, ForgeDraftIntent, ForgeDraftRequest, ForgeKind, ForgeWriteState,
    PullRequest, PullRequestAttach, PullRequestDetach,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
type Draft = (ForgeKind, String, String);
#[component]
pub fn ForgeTask(
    card_id: String,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
    archived: bool,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let card = Signal::derive(move || {
        board
            .cards
            .with(|cards| cards.iter().find(|card| card.id == id.get_value()).cloned())
    });
    let linked = Signal::derive(move || {
        card.get()
            .and_then(|card| card.fields.get::<PullRequest>("pullRequest").ok().flatten())
    });
    let initial = linked
        .get_untracked()
        .map(|pr| (pr.kind, pr.repository, pr.number.to_string()))
        .unwrap_or((ForgeKind::Github, String::new(), String::new()));
    let original = RwSignal::new(initial.clone());
    let draft = RwSignal::new(initial);
    let configuration = RwSignal::new(None::<ForgeConnections>);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let approved = RwSignal::new(None::<(Draft, u64)>);
    let detach_ack = RwSignal::new(String::new());
    let draft_ack = RwSignal::new(String::new());
    let publication_dirty = RwSignal::new(false);
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<ForgeDraftIntent>("forgeDraft")
                .ok()
                .flatten()
        })
    });
    let uncertain = Memo::new(move |_| {
        intent
            .get()
            .is_some_and(|intent| intent.state == ForgeWriteState::Uncertain)
    });
    let unresolved = Memo::new(move |_| {
        card.get().is_some_and(|card| {
            atelier_core::application::forge::unresolved(&card.fields).unwrap_or(true)
        })
    });
    let draft_key = Signal::derive(move || {
        let card = card.get()?;
        let configuration = configuration.get()?;
        let linked = linked.get()?;
        Some(serde_json::json!({"card":atelier_core::application::jira::card_snapshot(&card),"revision":configuration.revision,"draft":if uncertain.get(){intent.get()?.draft}else{!linked.draft}}).to_string())
    });
    Effect::new(move |_| dirty.set(draft.get() != original.get() || publication_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let reading = alive.clone();
    let load = StoredValue::new_local(move || {
        if pending.get_untracked() || loading.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        loading.set(true);
        error.set(String::new());
        approved.set(None);
        let alive = reading.clone();
        spawn_local(async move {
            let result = async {
                read(&credential, "/api/forges")
                    .await?
                    .json::<ForgeConnections>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(value) => configuration.set(Some(value)),
                    Err(message) => error.set(message),
                }
            }
        });
    });
    Effect::new(move |_| load.with_value(|load| load()));
    let writing = alive.clone();
    let change_draft = move |reconcile: bool| {
        if pending.get_untracked()
            || publication_dirty.get_untracked()
            || loading.get_untracked()
            || blocked.get_untracked()
            || draft.get_untracked() != original.get_untracked()
        {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(configuration) = configuration.get_untracked() else {
            return;
        };
        let Some(pr) = linked.get_untracked() else {
            return;
        };
        if reconcile != uncertain.get_untracked()
            || (!reconcile
                && (archived
                    || unresolved.get_untracked()
                    || draft_key.get_untracked().as_deref()
                        != Some(draft_ack.get_untracked().as_str())))
        {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let target = if reconcile {
            let Some(intent) = intent.get_untracked() else {
                return;
            };
            intent.draft
        } else {
            !pr.draft
        };
        let body = serde_json::to_value(ForgeDraftRequest {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            revision: configuration.revision,
            draft: target,
            confirmed: true,
        })
        .expect("forge draft serialization");
        let path = format!(
            "/api/cards/{}/forge/{}",
            id.get_value(),
            if reconcile {
                "draft-reconcile"
            } else {
                "draft"
            }
        );
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        draft_ack.set(String::new());
        let alive = writing.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(card) => {
                        let still_uncertain =
                            atelier_core::application::forge::unresolved(&card.fields)
                                .unwrap_or(true);
                        replace_card(board.cards, card);
                        notice.set(if still_uncertain{"État demandé non observé. L’écriture reste incertaine ; aucune nouvelle tentative automatique."}else{"État brouillon observé sur la forge. Revalide les reviews avant de poursuivre."}.into());
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
    let change_draft = StoredValue::new_local(change_draft);
    let execute = StoredValue::new_local(move |remove: bool| {
        if pending.get_untracked() || blocked.get_untracked() || publication_dirty.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = if remove {
            let Some(linked) = linked.get_untracked() else {
                return;
            };
            if detach_ack.get_untracked() != serde_json::to_string(&linked).unwrap_or_default() {
                return;
            }
            serde_json::to_value(PullRequestDetach {
                expected: atelier_core::application::jira::card_snapshot(&expected),
                confirmed: true,
            })
            .expect("PR detach serialization")
        } else {
            if loading.get_untracked() || archived {
                return;
            }
            let Some(configuration) = configuration.get_untracked() else {
                return;
            };
            let draft = draft.get_untracked();
            if approved.get_untracked() != Some((draft.clone(), configuration.revision)) {
                return;
            }
            let Some(number) = draft
                .2
                .parse::<u64>()
                .ok()
                .filter(|number| *number > 0 && *number <= i32::MAX as u64)
            else {
                error.set("Numéro de PR/MR entier positif requis.".into());
                return;
            };
            serde_json::to_value(PullRequestAttach {
                expected: atelier_core::application::jira::card_snapshot(&expected),
                revision: configuration.revision,
                kind: draft.0,
                repository: draft.1,
                number,
                confirmed: true,
            })
            .expect("PR attach serialization")
        };
        let path = format!(
            "/api/cards/{}/forge/{}",
            id.get_value(),
            if remove { "detach" } else { "attach" }
        );
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Card>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                match result {
                    Ok(card) => {
                        let next = card
                            .fields
                            .get::<PullRequest>("pullRequest")
                            .ok()
                            .flatten()
                            .map(|pr| (pr.kind, pr.repository, pr.number.to_string()))
                            .unwrap_or((ForgeKind::Github, String::new(), String::new()));
                        original.set(next.clone());
                        draft.set(next);
                        replace_card(board.cards, card);
                        approved.set(None);
                        detach_ack.set(String::new());
                        notice.set(if remove{"Liaison retirée. Aucune PR/MR distante modifiée."}else{"PR/MR associée après vérification du dépôt et de la branche. Aucun effet distant."}.into());
                    }
                    Err(message) => error.set(message),
                }
            }
            let _ = board.refresh(&credential).await;
            if alive.get() {
                pending.set(false);
            }
        });
    });
    let brain = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref()
                .is_some_and(|card| card.fields.truthy("brainstormSession").unwrap_or(false))
        })
    });
    view! {<section class="task-files" hidden=move||brain.get() aria-labelledby="forge-task-heading" aria-busy=move||(pending.get()||loading.get()).to_string()><h3 id="forge-task-heading">{move||i18n.text("PR/MR de la tâche")}</h3>
        {move||linked.get().map(|pr|view!{<p>{let label=format!("{} #{} · {}",pr.repository,pr.number,pr.title);if pr.url.starts_with("https://")&&!pr.url.chars().any(char::is_control){view!{<a href=pr.url target="_blank" rel="noopener noreferrer">{label}</a>}.into_any()}else{view!{<span>{label}</span>}.into_any()}}</p><p>{pr.source_branch}" → "{pr.target_branch}</p><p>{move||i18n.text("Dernière lecture : {0}").replace("{0}",&pr.checked_at)}</p>})}
        <p class="field-help">{move||i18n.text("Associe une PR/MR existante dont le dépôt origin et la branche source correspondent au worktree vérifié. Relire une liaison met à jour ses commits et rend les anciennes reviews périmées si nécessaire. Aucune publication distante.")}</p>
        <fieldset disabled=move||pending.get()||loading.get()||blocked.get()||archived||unresolved.get()||publication_dirty.get()>
            <label for="task-forge-kind">{move||i18n.text("Forge")}</label><select id="task-forge-kind" prop:value=move||draft.get().0.name() on:change=move|event|{draft.update(|draft|draft.0=match event_target_value(&event).as_str(){"gitlab"=>ForgeKind::Gitlab,"azure"=>ForgeKind::Azure,_=>ForgeKind::Github});approved.set(None);} ><option value="github">"GitHub"</option><option value="gitlab">"GitLab"</option><option value="azure">"Azure DevOps Services"</option></select>
            <label for="task-forge-repository">{move||i18n.text("Dépôt de la PR/MR")}</label><input id="task-forge-repository" maxlength="1000" prop:value=move||draft.get().1 on:input=move|event|{draft.update(|draft|draft.1=event_target_value(&event));approved.set(None);} />
            <label for="task-forge-number">{move||i18n.text("Numéro de PR/MR")}</label><input id="task-forge-number" type="number" min="1" max="2147483647" step="1" prop:value=move||draft.get().2 on:input=move|event|{draft.update(|draft|draft.2=event_target_value(&event));approved.set(None);} />
            <label><input id="confirm-task-forge" type="checkbox" prop:checked=move||configuration.with(|configuration|configuration.as_ref().is_some_and(|configuration|approved.get()==Some((draft.get(),configuration.revision)))) on:change=move|event|approved.set(if event_target_checked(&event){configuration.with(|configuration|configuration.as_ref().map(|configuration|(draft.get_untracked(),configuration.revision)))}else{None}) />{move||i18n.text("Vérifier et associer cette PR/MR sans rien modifier sur la forge.")}</label>
            <button id="attach-task-forge" type="button" disabled=move||configuration.with(|configuration|configuration.as_ref().is_none_or(|configuration|approved.get()!=Some((draft.get(),configuration.revision)))) on:click=move|_|execute.with_value(|execute|execute(false))>{move||i18n.text("Vérifier et enregistrer la liaison")}</button>
            <button id="cancel-task-forge" type="button" on:click=move|_|{draft.set(original.get_untracked());approved.set(None);}>{move||i18n.text("Annuler la préparation")}</button><button id="reload-task-forges" type="button" on:click=move|_|load.with_value(|load|load())>{move||i18n.text("Recharger les connexions aux forges")}</button>
        </fieldset>
        <fieldset hidden=move||linked.with(|linked|linked.is_none()) disabled=move||pending.get()||blocked.get()||draft.get()!=original.get()||archived||unresolved.get()||publication_dirty.get()><label><input id="confirm-detach-forge" type="checkbox" prop:checked=move||linked.with(|linked|linked.as_ref().is_some_and(|linked|serde_json::to_string(linked).unwrap_or_default()==detach_ack.get())) on:change=move|event|detach_ack.set(if event_target_checked(&event){linked.with(|linked|linked.as_ref().map(|linked|serde_json::to_string(linked).unwrap_or_default()).unwrap_or_default())}else{String::new()}) />{move||i18n.text("Retirer uniquement la liaison locale ; conserver la PR/MR et le worktree.")}</label><button id="detach-task-forge" type="button" disabled=move||linked.with(|linked|linked.as_ref().is_none_or(|linked|serde_json::to_string(linked).unwrap_or_default()!=detach_ack.get())) on:click=move|_|execute.with_value(|execute|execute(true))>{move||i18n.text("Confirmer le retrait de la liaison")}</button></fieldset>
        <fieldset hidden=move||linked.with(|linked|linked.as_ref().is_none_or(|pr|pr.status!="active"))||unresolved.get() disabled=move||pending.get()||loading.get()||blocked.get()||archived||publication_dirty.get()||draft.get()!=original.get()||configuration.with(|value|value.is_none())>
            <p class="field-help">{move||i18n.text("Ce changement écrit sur la forge. Passer prête pour review peut notifier les reviewers ; aucune fusion ni CI n’est demandée par Atelier.")}</p>
            <label><input id="confirm-forge-draft" type="checkbox" prop:checked=move||draft_key.get().is_some_and(|key|key==draft_ack.get()) on:change=move|event|draft_ack.set(if event_target_checked(&event){draft_key.get_untracked().unwrap_or_default()}else{String::new()}) />{move||i18n.text(if linked.get().is_some_and(|pr|pr.draft){"Confirmer le passage de cette PR/MR prête pour review."}else{"Confirmer le passage de cette PR/MR en brouillon."})}</label>
            <button id="change-forge-draft" type="button" disabled=move||draft_key.get().is_none_or(|key|key!=draft_ack.get()) on:click=move|_|change_draft.with_value(|change|change(false))>{move||i18n.text(if linked.get().is_some_and(|pr|pr.draft){"Passer prête pour review"}else{"Passer en brouillon"})}</button>
        </fieldset>
        <div class="confirmation" hidden=move||!uncertain.get()><p>{move||i18n.text("Écriture de forge incertaine. Relis l’état distant ; Atelier ne renverra pas la demande.")}</p><p>{move||i18n.text(if intent.get().is_some_and(|intent|intent.draft){"État demandé : brouillon."}else{"État demandé : prête pour review."})}</p><button id="reconcile-forge-draft" type="button" disabled=move||pending.get()||loading.get()||blocked.get()||draft.get()!=original.get() on:click=move|_|change_draft.with_value(|change|change(true))>{move||i18n.text("Relire l’état brouillon distant")}</button><button type="button" disabled=move||pending.get()||loading.get() on:click=move|_|load.with_value(|load|load())>{move||i18n.text("Recharger les connexions aux forges")}</button></div>
        <p class="error" id="forge-task-error" role="alert">{move||i18n.text(&error.get())}</p><p id="forge-task-notice" role="status">{move||i18n.text(&notice.get())}</p>
        <button type="button" hidden=move||!unresolved.get() disabled=move||pending.get()||loading.get() on:click=move|_|load.with_value(|load|load())>{move||i18n.text("Recharger les connexions aux forges")}</button>
        <crate::git_push::GitPush card configuration=Signal::derive(move||configuration.get()) board token pending blocked=Memo::new(move|_|blocked.get()||draft.get()!=original.get()||publication_dirty.get()) archived />
        <crate::forge_publication::ForgePublication card configuration=Signal::derive(move||configuration.get()) board token pending blocked=Memo::new(move|_|blocked.get()||draft.get()!=original.get()) dirty=publication_dirty archived on_linked=Callback::new(move|pr:PullRequest|{let next=(pr.kind,pr.repository,pr.number.to_string());original.set(next.clone());draft.set(next);approved.set(None);detach_ack.set(String::new());}) />
    </section>}
}
