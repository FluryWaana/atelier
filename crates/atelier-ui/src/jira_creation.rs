use crate::{field, i18n::I18n, live::BoardState, read, replace_card, write};
use atelier_core::{
    application::jira as domain,
    contracts::{
        Card, JiraConfiguration, JiraCreateRequest, JiraCreationIntent, JiraCreationMetadata,
        JiraCreationResolve, JiraIssueType, JiraWriteState as Phase,
    },
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, Copy)]
enum Action {
    Create,
    Link,
    Close,
}
#[component]
pub fn JiraCreation(
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
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<JiraCreationIntent>("jiraCreation")
                .ok()
                .flatten()
        })
    });
    let linked = Memo::new(move |_| {
        card.with(|card| {
            card.as_ref()
                .is_some_and(|card| card.fields.truthy("ticket").unwrap_or(false))
        })
    });
    let uncertain = Memo::new(move |_| {
        intent.with(|intent| {
            intent
                .as_ref()
                .is_some_and(|intent| intent.state == Phase::Uncertain)
        })
    });
    let writing = Memo::new(move |_| {
        intent.with(|intent| {
            intent
                .as_ref()
                .is_some_and(|intent| matches!(intent.state, Phase::Preparing | Phase::Sending))
        })
    });
    let project = RwSignal::new(String::new());
    let types = RwSignal::new(Vec::<JiraIssueType>::new());
    let selected = RwSignal::new(String::new());
    let prepared = RwSignal::new(None::<(Card, JiraCreationMetadata, u64)>);
    let sequence = RwSignal::new(0u64);
    let approved = RwSignal::new(None::<u64>);
    let key = RwSignal::new(String::new());
    let close_ack = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    Effect::new(move |_| dirty.set(!project.get().is_empty() || !key.get().is_empty()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || {
        mounted.with_value(|alive| alive.set(false));
        let _ = dirty.try_update(|value| *value = false);
    });
    let configuration = move || {
        board.settings.with(|settings| {
            settings
                .get::<JiraConfiguration>("jiraConnection")
                .ok()
                .flatten()
                .unwrap_or_default()
        })
    };
    let validation = Signal::derive(move || {
        prepared.with(|prepared| {
            prepared
                .as_ref()
                .and_then(|(card, metadata, _)| domain::validate_creation(metadata, card).err())
                .map(|error| error.to_string())
                .unwrap_or_default()
        })
    });
    let reading_alive = alive.clone();
    let load_types = move |_| {
        if pending.get_untracked()
            || loading.get_untracked()
            || blocked.get_untracked()
            || uncertain.get_untracked()
            || !domain::project_key(&project.get_untracked())
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
        let revision = configuration().revision;
        let requested = project.get_untracked();
        loading.set(true);
        types.set(Vec::new());
        selected.set(String::new());
        prepared.set(None);
        approved.set(None);
        error.set(String::new());
        notice.set(String::new());
        let alive = reading_alive.clone();
        spawn_local(async move {
            let result = async {
                let response = read(
                    &credential,
                    &format!("/api/jira/creation-metadata?revision={revision}&project={requested}"),
                )
                .await?
                .json::<Document>()
                .await
                .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                if response.get::<u64>("revision").ok().flatten() != Some(revision)
                    || field(&response, "projectKey") != requested
                {
                    return Err("Métadonnées de création Jira périmées.".to_owned());
                }
                response
                    .get::<Vec<JiraIssueType>>("types")
                    .map_err(|_| "Types de tickets Jira invalides.".to_owned())?
                    .ok_or("Types de tickets Jira absents.".to_owned())
            }
            .await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(value) => types.set(value),
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let preparing_alive = alive.clone();
    let prepare = move |_| {
        if pending.get_untracked()
            || loading.get_untracked()
            || blocked.get_untracked()
            || uncertain.get_untracked()
        {
            return;
        }
        let Some(source) = card.get_untracked() else {
            return;
        };
        let source = domain::card_snapshot(&source);
        let Some(kind) = types.with_untracked(|types| {
            types
                .iter()
                .find(|kind| kind.id == selected.get_untracked() && !kind.subtask)
                .cloned()
        }) else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let revision = configuration().revision;
        let requested = project.get_untracked();
        loading.set(true);
        prepared.set(None);
        approved.set(None);
        error.set(String::new());
        let alive = preparing_alive.clone();
        spawn_local(async move {
            let result=async{let metadata=read(&credential,&format!("/api/jira/creation-metadata?revision={revision}&project={requested}&issueType={}",kind.id)).await?.json::<JiraCreationMetadata>().await.map_err(|_|"Réponse du serveur invalide.".to_owned())?;
            if metadata.revision!=revision||metadata.project_key!=requested||metadata.issue_type!=kind{return Err("Métadonnées de création Jira périmées.".to_owned());}Ok(metadata)}.await;
            if alive.get() {
                loading.set(false);
                match result {
                    Ok(metadata) => {
                        sequence.update(|value| *value = value.wrapping_add(1));
                        prepared.set(Some((source, metadata, sequence.get_untracked())));
                    }
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let execute = StoredValue::new_local(move |action: Action, body: serde_json::Value| {
        if pending.get_untracked() || loading.get_untracked() || blocked.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!(
            "/api/cards/{}/jira/{}",
            id.get_value(),
            if matches!(action, Action::Create) {
                "create"
            } else {
                "creation-reconcile"
            }
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
                        let phase = card
                            .fields
                            .get::<JiraCreationIntent>("jiraCreation")
                            .ok()
                            .flatten()
                            .map(|intent| intent.state);
                        replace_card(board.cards, card);
                        project.set(String::new());
                        types.set(Vec::new());
                        selected.set(String::new());
                        prepared.set(None);
                        approved.set(None);
                        key.set(String::new());
                        close_ack.set(String::new());
                        notice.set(match(action,phase){(Action::Create,Some(Phase::Confirmed))=>"Ticket créé et associé à la carte. Son statut reste à relire.",(Action::Link,Some(Phase::Confirmed))=>"Ticket retrouvé et associé. Aucun nouveau ticket créé.",(Action::Close,Some(Phase::Dismissed))=>"Incertitude close après vérification. Aucun ticket distant supprimé.",_=>"Création ou association non confirmée. Vérifie Jira avant de réessayer."}.into());
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
    let create = move |_| {
        let Some((source, metadata, proposal)) = prepared.get_untracked() else {
            return;
        };
        if approved.get_untracked() != Some(proposal) || !validation.get_untracked().is_empty() {
            return;
        }
        let body = serde_json::to_value(JiraCreateRequest {
            revision: metadata.revision,
            expected: source,
            project_key: metadata.project_key,
            issue_type: metadata.issue_type,
            confirmed: true,
        })
        .expect("Jira creation serialization");
        execute.with_value(|execute| execute(Action::Create, body));
    };
    let reconcile = move |_| {
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let body = serde_json::to_value(JiraCreationResolve {
            revision: configuration().revision,
            expected: domain::card_snapshot(&expected),
            existing_key: (!key.get_untracked().trim().is_empty())
                .then(|| key.get_untracked().trim().to_ascii_uppercase()),
            close_uncertainty: false,
        })
        .expect("Jira creation reconciliation serialization");
        execute.with_value(|execute| execute(Action::Link, body));
    };
    let close_uncertainty = move |_| {
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let Some(current) = intent.get_untracked() else {
            return;
        };
        if close_ack.get_untracked() != current.id {
            return;
        }
        let body = serde_json::to_value(JiraCreationResolve {
            revision: configuration().revision,
            expected: domain::card_snapshot(&expected),
            existing_key: None,
            close_uncertainty: true,
        })
        .expect("Jira creation close serialization");
        execute.with_value(|execute| execute(Action::Close, body));
    };
    view! {<section class="task-files" hidden=move||linked.get()&&intent.with(|intent|intent.is_none()) aria-labelledby="jira-create-heading" aria-busy=move||(pending.get()||loading.get()).to_string()>
        <h3 id="jira-create-heading">{move||i18n.text("Création de ticket Jira")}</h3><p class="field-help">{move||i18n.text("Prépare un ticket depuis le titre, la description, les critères et les instructions enregistrés de cette carte. Aucun envoi sans confirmation.")}</p>
        <p role="status">{move||intent.get().map(|intent|i18n.text(match intent.state{Phase::Preparing=>"Préparation de la création Jira…",Phase::Sending=>"Demande de création Jira en cours…",Phase::Uncertain=>"Création Jira incertaine : aucun renvoi automatique.",Phase::Confirmed=>"Liaison au ticket Jira enregistrée.",Phase::Failed=>"Préparation Jira interrompue avant l’envoi.",Phase::Dismissed=>"Incertitude Jira close après vérification."})).unwrap_or_default()}</p>
        {move||intent.get().and_then(|intent|intent.result).map(|issue|view!{<p><a href=issue.url target="_blank" rel="noopener noreferrer">{issue.key}</a></p>})}
        <fieldset hidden=move||linked.get()||uncertain.get()||writing.get() disabled=move||pending.get()||loading.get()||blocked.get()||archived>
            <label for="jira-create-project">{move||i18n.text("Clé du projet Jira")}</label><input id="jira-create-project" maxlength="64" placeholder="SYN" prop:value=move||project.get() on:input=move|event|{project.set(event_target_value(&event).trim().to_ascii_uppercase());types.set(Vec::new());selected.set(String::new());prepared.set(None);approved.set(None);} />
            <button id="load-jira-types" type="button" disabled=move||!domain::project_key(&project.get()) on:click=load_types>{move||i18n.text("Charger les types de ticket")}</button>
            <label for="jira-create-type">{move||i18n.text("Type de ticket")}</label><select id="jira-create-type" prop:value=move||selected.get() on:change=move|event|{selected.set(event_target_value(&event));prepared.set(None);approved.set(None);} ><option value="">{move||i18n.text("Choisir un type de ticket")}</option><For each=move||types.get() key=|kind|kind.id.clone() children=move|kind|view!{<option value=kind.id disabled=kind.subtask>{kind.name}</option>} /></select>
            <p class="field-help">{move||i18n.text("Les sous-tâches et champs obligatoires supplémentaires se préparent dans Jira, puis s’associent à la carte.")}</p><button id="prepare-jira-create" type="button" disabled=move||selected.get().is_empty() on:click=prepare>{move||i18n.text("Préparer l’aperçu du ticket")}</button>
            {move||prepared.get().map(|(source,_,_)|view!{<div><h4>{String::from_utf16_lossy(&source.title.0)}</h4><pre class="service-log">{domain::creation_text(&source).unwrap_or_default()}</pre></div>})}
            <p class="error" role="alert">{move||i18n.text(&validation.get())}</p>
            <label hidden=move||prepared.with(|prepared|prepared.is_none())><input id="confirm-jira-create" type="checkbox" prop:checked=move||prepared.with(|prepared|prepared.as_ref().is_some_and(|(_,_,proposal)|approved.get()==Some(*proposal))) on:change=move|event|approved.set(if event_target_checked(&event){prepared.with(|prepared|prepared.as_ref().map(|(_,_,proposal)|*proposal))}else{None}) />{move||i18n.text("Créer ce ticket dans le projet Jira choisi et l’associer à cette carte.")}</label>
            <button id="send-jira-create" type="button" disabled=move||prepared.with(|prepared|prepared.as_ref().is_none_or(|(_,_,proposal)|approved.get()!=Some(*proposal)))||!validation.get().is_empty() on:click=create>{move||i18n.text("Confirmer la création dans Jira")}</button>
            <button id="cancel-jira-preparation" type="button" on:click=move|_|{project.set(String::new());types.set(Vec::new());selected.set(String::new());prepared.set(None);approved.set(None);}>{move||i18n.text("Annuler la préparation")}</button>
        </fieldset>
        <fieldset hidden=move||!uncertain.get() disabled=move||pending.get()||loading.get()||blocked.get()><p>{move||i18n.text("Vérifie le projet dans Jira. Si le ticket existe, indique sa clé ; si un identifiant a été reçu, tu peux le vérifier directement.")}</p><label for="jira-created-key">{move||i18n.text("Clé du ticket retrouvé")}</label><input id="jira-created-key" maxlength="100" placeholder="SYN-1" prop:value=move||key.get() on:input=move|event|key.set(event_target_value(&event).to_ascii_uppercase()) />
            <button id="resolve-jira-creation" type="button" disabled=move||key.get().trim().is_empty()&&intent.with(|intent|intent.as_ref().is_none_or(|intent|intent.result.is_none())) on:click=reconcile>{move||i18n.text("Vérifier et associer sans recréer")}</button>
            <p class="field-help">{move||i18n.text("Clore l’incertitude ne supprime aucun ticket distant. Une nouvelle création resterait une action explicite après vérification du risque de doublon.")}</p><label><input id="close-jira-creation-ack" type="checkbox" prop:checked=move||intent.with(|intent|intent.as_ref().is_some_and(|intent|intent.id==close_ack.get())) on:change=move|event|close_ack.set(if event_target_checked(&event){intent.with(|intent|intent.as_ref().map(|intent|intent.id.clone()).unwrap_or_default())}else{String::new()}) />{move||i18n.text("J’ai vérifié Jira et souhaite clore cette incertitude sans nouvel envoi.")}</label><button id="close-jira-creation" type="button" disabled=move||intent.with(|intent|intent.as_ref().is_none_or(|intent|intent.id!=close_ack.get())) on:click=close_uncertainty>{move||i18n.text("Clore l’incertitude après vérification")}</button>
        </fieldset>
        <p class="field-help" hidden=move||!loading.get()>{move||i18n.text("Lecture des métadonnées Jira…")}</p><p id="jira-create-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="jira-create-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
