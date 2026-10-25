use crate::{i18n::I18n, live::BoardState, replace_card, write};
use atelier_core::contracts::{
    Card, ForgeConnections, ForgeKind, ForgePublicationIntent, ForgePublicationPreview,
    ForgePublicationRequest, ForgeWriteState, PullRequest,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, PartialEq)]
struct Draft {
    kind: ForgeKind,
    repository: String,
    target: String,
    title: String,
    body: String,
    draft: bool,
}
impl From<ForgePublicationPreview> for Draft {
    fn from(value: ForgePublicationPreview) -> Self {
        Self {
            kind: value.configuration.kind,
            repository: value.repository,
            target: value.target,
            title: value.title,
            body: value.body,
            draft: value.draft,
        }
    }
}
#[component]
pub fn ForgePublication(
    card: Signal<Option<Card>>,
    configuration: Signal<Option<ForgeConnections>>,
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    dirty: RwSignal<bool>,
    archived: bool,
    on_linked: Callback<PullRequest>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<ForgePublicationPreview>("forgePublicationPreview")
                .ok()
                .flatten()
        })
    });
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<ForgePublicationIntent>("forgePublication")
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
    let linked = Memo::new(move |_| {
        card.get()
            .is_some_and(|card| card.fields.raw("pullRequest").is_some())
    });
    let initial = preview
        .get_untracked()
        .or_else(|| intent.get_untracked().map(|intent| intent.preview))
        .map(Draft::from)
        .unwrap_or_else(|| Draft {
            kind: ForgeKind::Github,
            repository: String::new(),
            target: "main".into(),
            title: card
                .get_untracked()
                .and_then(|card| card.title.utf8().ok())
                .unwrap_or_default(),
            body: card
                .get_untracked()
                .and_then(|card| card.fields.get::<String>("description").ok().flatten())
                .unwrap_or_default(),
            draft: true,
        });
    let original = RwSignal::new(initial.clone());
    let draft = RwSignal::new(initial);
    let own_dirty = Memo::new(move |_| draft.get() != original.get());
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let matches = Memo::new(move |_| {
        preview.get().is_some_and(|preview| {
            configuration
                .get()
                .is_some_and(|configuration| configuration.revision == preview.revision)
                && Draft::from(preview) == draft.get()
        })
    });
    let key = Signal::derive(move || {
        let fields = draft.get();
        Some(serde_json::json!({"card":atelier_core::application::jira::card_snapshot(&card.get()?),"revision":configuration.get()?.revision,"kind":fields.kind,"repository":fields.repository,"target":fields.target,"title":fields.title,"body":fields.body,"draft":fields.draft}).to_string())
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|dirty| *dirty = false);
    });
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked() || blocked.get_untracked() {
            return;
        }
        if action == "publication-reconcile" {
            if !uncertain.get_untracked() {
                return;
            }
        } else if archived || linked.get_untracked() || unresolved.get_untracked() {
            return;
        }
        if action == "publication"
            && (!matches.get_untracked()
                || key.get_untracked().as_deref() != Some(approved.get_untracked().as_str()))
        {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(configuration) = configuration.get_untracked() else {
            return;
        };
        let values = draft.get_untracked();
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!("/api/cards/{}/forge/{action}", expected.id);
        let body = serde_json::to_value(ForgePublicationRequest {
            expected: atelier_core::application::jira::card_snapshot(&expected),
            revision: configuration.revision,
            kind: values.kind,
            repository: values.repository,
            target: values.target,
            title: values.title,
            body: values.body,
            draft: values.draft,
            confirmed: action != "publication-preview",
        })
        .expect("publication request serialization");
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(String::new());
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
                        if let Ok(Some(preview)) = card
                            .fields
                            .get::<ForgePublicationPreview>("forgePublicationPreview")
                        {
                            draft.set(Draft::from(preview));
                        }
                        let unresolved = atelier_core::application::forge::unresolved(&card.fields)
                            .unwrap_or(true);
                        if let Ok(Some(pr)) = card.fields.get::<PullRequest>("pullRequest") {
                            original.set(draft.get_untracked());
                            on_linked.run(pr);
                        }
                        replace_card(board.cards, card);
                        notice.set(if action=="publication-preview"{"Aperçu préparé. Aucune PR/MR créée."}else if unresolved{"Publication retrouvée mais association impossible. L’incertitude est conservée."}else{"PR/MR retrouvée, vérifiée et associée. Aucun renvoi de création."}.into());
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
    view! {<section class="task-files" hidden=move||linked.get()&&!own_dirty.get()&&!uncertain.get() aria-labelledby="forge-publication-heading" aria-busy=move||pending.get().to_string()><h4 id="forge-publication-heading">{move||i18n.text("Créer une PR/MR")}</h4>
        <p class="field-help">{move||i18n.text("La branche de tâche et son commit doivent déjà être présents sur la forge. Prépare puis confirme le contenu ; aucun commit local ni fusion ne sera effectué.")}</p>
        <form on:submit=move|event:leptos::ev::SubmitEvent|{event.prevent_default();execute.with_value(|execute|execute("publication-preview"));}><fieldset disabled=move||pending.get()||blocked.get()||archived||linked.get()||unresolved.get()||configuration.with(|value|value.is_none())>
            <label for="publication-kind">{move||i18n.text("Forge")}</label><select id="publication-kind" prop:value=move||draft.get().kind.name() on:change=move|event|draft.update(|draft|draft.kind=match event_target_value(&event).as_str(){"gitlab"=>ForgeKind::Gitlab,"azure"=>ForgeKind::Azure,_=>ForgeKind::Github})><option value="github">"GitHub"</option><option value="gitlab">"GitLab"</option><option value="azure">"Azure DevOps Services"</option></select>
            <label for="publication-repository">{move||i18n.text("Dépôt de la PR/MR")}</label><input id="publication-repository" required maxlength="1000" prop:value=move||draft.get().repository on:input=move|event|draft.update(|draft|draft.repository=event_target_value(&event)) />
            <label for="publication-target">{move||i18n.text("Branche cible")}</label><input id="publication-target" required maxlength="160" prop:value=move||draft.get().target on:input=move|event|draft.update(|draft|draft.target=event_target_value(&event)) />
            <label for="publication-title">{move||i18n.text("Titre de la PR/MR")}</label><input id="publication-title" required maxlength="255" prop:value=move||draft.get().title on:input=move|event|draft.update(|draft|draft.title=event_target_value(&event)) />
            <label for="publication-body">{move||i18n.text("Description de la PR/MR")}</label><textarea id="publication-body" maxlength="3500" prop:value=move||draft.get().body on:input=move|event|draft.update(|draft|draft.body=event_target_value(&event)) />
            <label><input id="publication-draft" type="checkbox" prop:checked=move||draft.get().draft on:change=move|event|draft.update(|draft|draft.draft=event_target_checked(&event)) />{move||i18n.text("Créer en brouillon")}</label>
            <button id="preview-publication" type="submit">{move||i18n.text("Préparer l’aperçu de publication")}</button>
        </fieldset></form>
        {move||preview.get().map(|preview|view!{<p>{preview.worktree.branch}" → "{preview.target}</p><p><code>{preview.source_commit}</code>" / "<code>{preview.target_commit}</code></p>})}
        <fieldset hidden=move||!matches.get()||unresolved.get()||linked.get() disabled=move||pending.get()||blocked.get()||archived><p>{move||i18n.text("La création écrit sur la forge et peut déclencher ses notifications ou règles CI.")}</p><label><input id="confirm-publication" type="checkbox" prop:checked=move||key.get().is_some_and(|key|key==approved.get()) on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()}) />{move||i18n.text("Confirmer la création de cette PR/MR avec ces branches et ce contenu.")}</label><button id="create-publication" type="button" disabled=move||key.get().is_none_or(|key|key!=approved.get()) on:click=move|_|execute.with_value(|execute|execute("publication"))>{move||i18n.text("Créer la PR/MR confirmée")}</button></fieldset>
        <div class="confirmation" hidden=move||!uncertain.get()><p>{move||i18n.text("Publication incertaine. Recherche la PR/MR existante ; aucune nouvelle création automatique.")}</p><button id="reconcile-publication" type="button" disabled=move||pending.get()||blocked.get() on:click=move|_|execute.with_value(|execute|execute("publication-reconcile"))>{move||i18n.text("Rechercher la publication existante")}</button></div>
        <button id="cancel-publication" type="button" hidden=move||!own_dirty.get() disabled=move||pending.get()||unresolved.get() on:click=move|_|{draft.set(original.get_untracked());approved.set(String::new());}>{move||i18n.text("Annuler la préparation")}</button>
        <p id="publication-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="publication-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
