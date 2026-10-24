use crate::{i18n::I18n, read};
use atelier_core::contracts::{ChangeListing, FileChange, FileDiff, FileListing, FilePreview};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

const PAGE_SIZE: usize = 200;

#[component]
pub fn Files(card_id: String, token: StoredValue<Result<String, String>>) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let files = RwSignal::new(Vec::<FileChange>::new());
    let filter = RwSignal::new(String::new());
    let page = RwSignal::new(0usize);
    let selected = RwSignal::new(None::<(String, bool)>);
    let content = RwSignal::new(String::new());
    let unavailable = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let changes = RwSignal::new(false);
    let base = RwSignal::new(String::new());
    let matching = Memo::new(move |_| {
        let query = filter.get().to_lowercase();
        files
            .get()
            .into_iter()
            .filter(|entry| entry.path.to_lowercase().contains(&query))
            .collect::<Vec<_>>()
    });
    let load = move |selection: Option<FileChange>| {
        if pending.get_untracked() {
            return;
        }
        let token = match token.get_value() {
            Ok(token) => token,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let card = id.get_value();
        let changes = changes.get_untracked();
        if selection.is_none() {
            files.set(Vec::new());
            base.set(String::new());
            page.set(0);
        }
        selected.set(
            selection
                .as_ref()
                .map(|entry| (entry.path.clone(), entry.untracked)),
        );
        content.set(String::new());
        unavailable.set(String::new());
        error.set(String::new());
        pending.set(true);
        spawn_local(async move {
            let result = async {
                if let Some(entry) = selection {
                    let params = web_sys::UrlSearchParams::new()
                        .map_err(|_| "Adresse de fichier invalide.")?;
                    params.set("path", &entry.path);
                    if changes {
                        params.set("untracked", &entry.untracked.to_string());
                    }
                    let action = if changes { "file-diff" } else { "file" };
                    let response = read(
                        &token,
                        &format!(
                            "/api/cards/{card}/{action}?{}",
                            String::from(params.to_string())
                        ),
                    )
                    .await?;
                    let (text, reason) = if changes {
                        let response: FileDiff = response
                            .json()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.")?;
                        (response.patch, response.unavailable)
                    } else {
                        let response: FilePreview = response
                            .json()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.")?;
                        (response.content, response.unavailable)
                    };
                    if pending.try_get_untracked().is_some() {
                        content.set(text.unwrap_or_default());
                        unavailable.set(reason.unwrap_or_default());
                    }
                } else if changes {
                    let response: ChangeListing =
                        read(&token, &format!("/api/cards/{card}/changes"))
                            .await?
                            .json()
                            .await
                            .map_err(|_| "Réponse du serveur invalide.")?;
                    if pending.try_get_untracked().is_some() {
                        base.set(response.base);
                        files.set(response.files);
                    }
                } else {
                    let response: FileListing = read(&token, &format!("/api/cards/{card}/files"))
                        .await?
                        .json()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.")?;
                    if pending.try_get_untracked().is_some() {
                        files.set(
                            response
                                .files
                                .into_iter()
                                .map(|path| FileChange {
                                    path,
                                    status: String::new(),
                                    untracked: false,
                                })
                                .collect(),
                        );
                        page.set(0);
                    }
                }
                Ok::<(), String>(())
            }
            .await;
            if pending.try_get_untracked().is_some() {
                if let Err(message) = result {
                    error.set(message);
                }
                pending.set(false);
            }
        });
    };
    load(None);
    view! {
        <div class="file-explorer" aria-busy=move || pending.get().to_string()>
            <div class="editor-actions" role="group" aria-label=move || i18n.text("Vue du dépôt")>
                <button id="show-all-files" type="button" aria-pressed=move || (!changes.get()).to_string() disabled=move || pending.get() on:click=move |_| { changes.set(false); load(None); }>{move || i18n.text("Tous les fichiers")}</button>
                <button id="show-changes" type="button" aria-pressed=move || changes.get().to_string() disabled=move || pending.get() on:click=move |_| { changes.set(true); load(None); }>{move || i18n.text("Changements")}</button>
            </div>
            <p class="field-help">{move || i18n.text("Lecture seule : fichiers suivis et nouveaux fichiers non ignorés par Git.")}</p>
            <p class="field-help" hidden=move || !changes.get()>{move || i18n.text("Comparaison avec le dernier commit : ajouts (A), modifications (M), suppressions (D). Les renommages apparaissent comme une suppression et un ajout.")}</p>
            <p class="field-help" hidden=move || base.get().is_empty()>{move || i18n.text("Commit de référence : {0}").replace("{0}", &base.get())}</p>
            <div class="editor-actions"><button id="refresh-files" type="button" disabled=move || pending.get() on:click=move |_| load(None)>{move || i18n.text("Actualiser")}</button><span role="status">{move || if pending.get() { i18n.text("Chargement des fichiers…") } else { i18n.text("{0} fichier(s)").replace("{0}",&matching.with(Vec::len).to_string()) }}</span></div>
            <label for="file-search">{move || i18n.text("Filtrer les fichiers")}</label><input id="file-search" type="search" prop:value=move || filter.get() on:input=move |event| { filter.set(event_target_value(&event)); page.set(0); } />
            <p id="file-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div class="file-browser"><div>
                <nav class="file-list" aria-label=move || i18n.text("Fichiers du projet")><For each={move || matching.with(|files| files.iter().skip(page.get()*PAGE_SIZE).take(PAGE_SIZE).cloned().collect::<Vec<_>>())} key=|entry| (entry.path.clone(), entry.untracked, entry.status.clone()) children=move |entry| {
                    let name = StoredValue::new(entry);
                    view! { <button type="button" class="file-choice" data-file=name.get_value().path aria-pressed=move || (selected.get() == Some((name.get_value().path, name.get_value().untracked))).to_string() disabled=move || pending.get() on:click=move |_| load(Some(name.get_value()))>{if name.get_value().status.is_empty() { String::new() } else { format!("{} · ", name.get_value().status) }}{name.get_value().path}</button> }
                } /><p class="field-help" hidden=move || pending.get() || matching.with(|files| !files.is_empty())>{move || i18n.text("Aucun fichier trouvé.")}</p></nav>
                <div class="file-pages"><button id="previous-files" type="button" disabled=move || page.get()==0 on:click=move |_| page.update(|page| *page = page.saturating_sub(1))>{move || i18n.text("Précédent")}</button><span>{move || format!("{} / {}",page.get()+1,matching.with(Vec::len).div_ceil(PAGE_SIZE).max(1))}</span><button id="next-files" type="button" disabled=move || (page.get()+1)*PAGE_SIZE >= matching.with(Vec::len) on:click=move |_| page.update(|page| *page += 1)>{move || i18n.text("Suivant")}</button></div>
            </div><section class="file-preview" aria-label=move || i18n.text("Aperçu du fichier")><h3>{move || selected.get().map(|(path, _)| path).unwrap_or_default()}</h3><p class="field-help" hidden=move || selected.get().is_some()>{move || i18n.text("Sélectionne un fichier pour le lire.")}</p><p id="file-unavailable" role="status">{move || i18n.text(&unavailable.get())}</p><p class="field-help" hidden=move || pending.get() || selected.get().is_none() || !content.get().is_empty() || !unavailable.get().is_empty() || !error.get().is_empty()>{move || i18n.text("Aucun contenu textuel à afficher.")}</p><pre id="file-content" tabindex="0" hidden=move || selected.get().is_none() || !unavailable.get().is_empty()><code>{move || content.get()}</code></pre></section></div>
        </div>
    }
}
