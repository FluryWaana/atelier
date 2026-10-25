use crate::{i18n::I18n, live::BoardState, read, replace_card, write};
use atelier_core::contracts::{
    Card, ChangeListing, FileChange, ForgeWriteState as Phase, LocalCommitIntent,
    LocalCommitPreview, LocalCommitRequest, LocalCommitResponse,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Commits(
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
    let selected = RwSignal::new(Vec::<String>::new());
    let files = RwSignal::new(Vec::<FileChange>::new());
    let loaded = RwSignal::new(false);
    let page = RwSignal::new(0usize);
    let name = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let message = RwSignal::new(String::new());
    let patch = RwSignal::new(String::new());
    let patch_key = RwSignal::new(String::new());
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let key = Signal::derive(move || {
        serde_json::json!({"card":card.get(),"paths":selected.get(),"name":name.get(),"email":email.get(),"message":message.get()}).to_string()
    });
    Effect::new(move |_| {
        let _ = key.get();
        approved.set(String::new());
    });
    Effect::new(move |_| {
        dirty.set(
            !selected.get().is_empty()
                || !name.get().is_empty()
                || !email.get().is_empty()
                || !message.get().is_empty(),
        )
    });
    let preview = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<LocalCommitPreview>("localCommitPreview")
                .ok()
                .flatten()
        })
    });
    let intent = Signal::derive(move || {
        card.get().and_then(|card| {
            card.fields
                .get::<LocalCommitIntent>("localCommit")
                .ok()
                .flatten()
        })
    });
    let uncertain = Memo::new(move |_| {
        intent
            .get()
            .is_some_and(|intent| intent.state == Phase::Uncertain)
    });
    let unresolved = Memo::new(move |_| {
        card.get().is_some_and(|card| {
            atelier_core::application::forge::unresolved(&card.fields).unwrap_or(true)
        })
    });
    let ready = Memo::new(move |_| {
        preview.get().is_some_and(|preview| {
            preview.snapshot.paths == selected.get()
                && preview.author_name == name.get()
                && preview.author_email == email.get()
                && preview.message == message.get()
        }) && patch_key.get() == key.get()
    });
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked()
            || (action != "reconcile"
                && (blocked.get_untracked() || archived || unresolved.get_untracked()))
        {
            return;
        }
        if action == "create"
            && (!ready.get_untracked() || approved.get_untracked() != key.get_untracked())
        {
            return;
        }
        if action == "reconcile" && !uncertain.get_untracked() {
            return;
        }
        let Some(expected) = card.get_untracked() else {
            return;
        };
        let credential = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let request = LocalCommitRequest {
            expected: expected.clone(),
            paths: selected.get_untracked(),
            author_name: name.get_untracked(),
            author_email: email.get_untracked(),
            message: message.get_untracked(),
            confirmed: action != "preview",
        };
        let path = format!("/api/cards/{}/commit/{action}", expected.id);
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        approved.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                if action == "changes" {
                    let listing: ChangeListing = read(&credential, &path)
                        .await?
                        .json()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                    Ok::<_, String>((None, None, Some(listing.files)))
                } else {
                    let response = write(
                        &credential,
                        Method::POST,
                        &path,
                        &serde_json::to_value(request).expect("commit request serialization"),
                    )
                    .await?;
                    if action == "preview" {
                        let response: LocalCommitResponse = response
                            .json()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        Ok((Some(response.card), Some(response.patch), None))
                    } else {
                        let response: Card = response
                            .json()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        Ok((Some(response), None, None))
                    }
                }
            }
            .await;
            if alive.get() {
                match result {
                    Ok((updated, diff, listing)) => {
                        if let Some(listing) = listing {
                            files.set(listing);
                            loaded.set(true);
                            page.set(0);
                        }
                        if let Some(updated) = updated {
                            let state = updated
                                .fields
                                .get::<LocalCommitIntent>("localCommit")
                                .ok()
                                .flatten();
                            replace_card(board.cards, updated);
                            if let Some(diff) = diff {
                                // Inputs are disabled while pending; the returned card adds only this preview.
                                patch.set(diff);
                                patch_key.set(key.get_untracked());
                                notice.set("Aperçu préparé. Aucun commit créé.".into());
                            } else if state.is_some_and(|intent| intent.state == Phase::Confirmed) {
                                if action == "create" {
                                    selected.set(Vec::new());
                                    message.set(String::new());
                                    name.set(String::new());
                                    email.set(String::new());
                                    patch.set(String::new());
                                    patch_key.set(String::new());
                                    loaded.set(false);
                                }
                                notice.set("Commit observé sur la branche locale. L’index et les fichiers humains sont conservés.".into());
                            } else {
                                notice.set("La branche a été relue. Consulte le commit observé ; un résultat incertain ne sera pas rejoué.".into());
                            }
                        }
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
    view! {<section class="task-files" aria-labelledby="local-commit-heading" hidden=move||card.get().is_none_or(|card|card.fields.raw("worktree").is_none()) aria-busy=move||pending.get().to_string()>
        <h4 id="local-commit-heading">{move||i18n.text("Créer un commit local")}</h4>
        <p class="field-help">{move||i18n.text("Choisis les fichiers du disque à inclure. L’index existant reste inchangé, y compris les sélections partielles ; inspecte-le avant un prochain commit externe. Aucun push automatique.")}</p>
        <fieldset disabled=move||pending.get()||blocked.get()||archived||unresolved.get()>
            <button id="load-commit-files" type="button" on:click=move|_|execute.with_value(|execute|execute("changes"))>{move||i18n.text("Actualiser les fichiers à commiter")}</button>
            <p hidden=move||!loaded.get()||!files.get().is_empty()>{move||i18n.text("Aucun fichier modifié depuis le dernier commit.")}</p>
            <div class="file-list" role="group" aria-label=move||i18n.text("Fichiers du commit")>{move||files.get().into_iter().skip(page.get()*100).take(100).map(|file|{
                let path=StoredValue::new(file.path);
                view!{<label><input type="checkbox" prop:checked=move||selected.with(|selected|selected.contains(&path.get_value())) disabled=move||selected.with(|selected|selected.len()>=100&&!selected.contains(&path.get_value())) on:change=move|event|selected.update(|selected|{let value=path.get_value(); if event_target_checked(&event){if !selected.contains(&value){selected.push(value);selected.sort();}}else{selected.retain(|path|path!=&value);}}) /><code>{path.get_value()}</code></label>}
            }).collect_view()}</div>
            <div class="editor-actions"><button type="button" disabled=move||page.get()==0 on:click=move|_|page.update(|page|*page=page.saturating_sub(1))>{move||i18n.text("Précédent")}</button><button type="button" disabled=move||(page.get()+1)*100>=files.with(Vec::len) on:click=move|_|page.update(|page|*page+=1)>{move||i18n.text("Suivant")}</button></div>
            <p>{move||i18n.text("Sélection du commit :")}" "{move||selected.get().join(", ")}</p>
            <label for="commit-author-name">{move||i18n.text("Auteur du commit")}</label><input id="commit-author-name" maxlength="255" prop:value=move||name.get() on:input=move|event|name.set(event_target_value(&event)) />
            <label for="commit-author-email">{move||i18n.text("E-mail de l’auteur")}</label><input id="commit-author-email" type="email" maxlength="254" prop:value=move||email.get() on:input=move|event|email.set(event_target_value(&event)) />
            <label for="commit-message">{move||i18n.text("Message du commit")}</label><textarea id="commit-message" maxlength="2000" prop:value=move||message.get() on:input=move|event|message.set(event_target_value(&event)) />
            <button id="preview-local-commit" type="button" disabled=move||selected.with(Vec::is_empty)||name.get().trim().is_empty()||email.get().trim().is_empty()||message.get().trim().is_empty() on:click=move|_|execute.with_value(|execute|execute("preview"))>{move||i18n.text("Préparer l’aperçu du commit")}</button>
            <button type="button" on:click=move|_|{selected.set(Vec::new());name.set(String::new());email.set(String::new());message.set(String::new());patch.set(String::new());patch_key.set(String::new());}>{move||i18n.text("Effacer le brouillon du commit")}</button>
            <Show when=move||ready.get()><pre class="file-preview" tabindex="0" aria-label=move||i18n.text("Différence du commit")>{move||patch.get()}</pre><label><input id="confirm-local-commit" type="checkbox" prop:checked=move||approved.get()==key.get() on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked()}else{String::new()}) />{move||i18n.text("Confirmer cet auteur, ce message et ces changements.")}</label><button id="create-local-commit" type="button" disabled=move||approved.get()!=key.get() on:click=move|_|execute.with_value(|execute|execute("create"))>{move||i18n.text("Créer le commit confirmé")}</button></Show>
        </fieldset>
        <div class="confirmation" hidden=move||!uncertain.get()><p>{move||i18n.text("Commit incertain. Relis la branche locale avant toute nouvelle tentative.")}</p><button id="reconcile-local-commit" type="button" disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute("reconcile"))>{move||i18n.text("Relire la branche locale")}</button></div>
        {move||intent.get().and_then(|intent|intent.observed).map(|observed|view!{<p>{move||i18n.text("Dernier commit local observé :")}" "<code>{observed}</code></p>})}
        <p id="local-commit-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="local-commit-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
