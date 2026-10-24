use crate::{i18n::I18n, read};
use atelier_core::contracts::{FileListing, FilePreview};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

const PAGE_SIZE: usize = 200;

#[component]
pub fn Files(card_id: String, token: StoredValue<Result<String, String>>) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let id = StoredValue::new(card_id);
    let files = RwSignal::new(Vec::<String>::new());
    let filter = RwSignal::new(String::new());
    let page = RwSignal::new(0usize);
    let selected = RwSignal::new(String::new());
    let content = RwSignal::new(String::new());
    let unavailable = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);
    let matching = Memo::new(move |_| {
        let query = filter.get().to_lowercase();
        files
            .get()
            .into_iter()
            .filter(|path| path.to_lowercase().contains(&query))
            .collect::<Vec<_>>()
    });
    let load = move |path: Option<String>| {
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
        selected.set(path.clone().unwrap_or_default());
        content.set(String::new());
        unavailable.set(String::new());
        error.set(String::new());
        pending.set(true);
        spawn_local(async move {
            let result = async {
                if let Some(path) = path {
                    let params = web_sys::UrlSearchParams::new()
                        .map_err(|_| "Adresse de fichier invalide.")?;
                    params.set("path", &path);
                    let response: FilePreview = read(
                        &token,
                        &format!(
                            "/api/cards/{card}/file?{}",
                            String::from(params.to_string())
                        ),
                    )
                    .await?
                    .json()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.")?;
                    if pending.try_get_untracked().is_some() {
                        content.set(response.content.unwrap_or_default());
                        unavailable.set(response.unavailable.unwrap_or_default());
                    }
                } else {
                    let response: FileListing = read(&token, &format!("/api/cards/{card}/files"))
                        .await?
                        .json()
                        .await
                        .map_err(|_| "Réponse du serveur invalide.")?;
                    if pending.try_get_untracked().is_some() {
                        files.set(response.files);
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
            <p class="field-help">{move || i18n.text("Lecture seule : fichiers suivis et nouveaux fichiers non ignorés par Git.")}</p>
            <div class="editor-actions"><button id="refresh-files" type="button" disabled=move || pending.get() on:click=move |_| load(None)>{move || i18n.text("Actualiser")}</button><span role="status">{move || if pending.get() { i18n.text("Chargement des fichiers…") } else { i18n.text("{0} fichier(s)").replace("{0}",&matching.with(Vec::len).to_string()) }}</span></div>
            <label for="file-search">{move || i18n.text("Filtrer les fichiers")}</label><input id="file-search" type="search" prop:value=move || filter.get() on:input=move |event| { filter.set(event_target_value(&event)); page.set(0); } />
            <p id="file-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div class="file-browser"><div>
                <nav class="file-list" aria-label=move || i18n.text("Fichiers du projet")><For each={move || matching.with(|files| files.iter().skip(page.get()*PAGE_SIZE).take(PAGE_SIZE).cloned().collect::<Vec<_>>())} key=|path| path.clone() children=move |path| {
                    let name = StoredValue::new(path);
                    view! { <button type="button" class="file-choice" data-file=name.get_value() aria-pressed=move || (selected.get() == name.get_value()).to_string() disabled=move || pending.get() on:click=move |_| load(Some(name.get_value()))>{name.get_value()}</button> }
                } /><p class="field-help" hidden=move || pending.get() || matching.with(|files| !files.is_empty())>{move || i18n.text("Aucun fichier trouvé.")}</p></nav>
                <div class="file-pages"><button id="previous-files" type="button" disabled=move || page.get()==0 on:click=move |_| page.update(|page| *page = page.saturating_sub(1))>{move || i18n.text("Précédent")}</button><span>{move || format!("{} / {}",page.get()+1,matching.with(Vec::len).div_ceil(PAGE_SIZE).max(1))}</span><button id="next-files" type="button" disabled=move || (page.get()+1)*PAGE_SIZE >= matching.with(Vec::len) on:click=move |_| page.update(|page| *page += 1)>{move || i18n.text("Suivant")}</button></div>
            </div><section class="file-preview" aria-label=move || i18n.text("Aperçu du fichier")><h3>{move || selected.get()}</h3><p class="field-help" hidden=move || !selected.get().is_empty()>{move || i18n.text("Sélectionne un fichier pour le lire.")}</p><p id="file-unavailable" role="status">{move || i18n.text(&unavailable.get())}</p><pre id="file-content" tabindex="0" hidden=move || selected.get().is_empty() || !unavailable.get().is_empty()><code>{move || content.get()}</code></pre></section></div>
        </div>
    }
}
