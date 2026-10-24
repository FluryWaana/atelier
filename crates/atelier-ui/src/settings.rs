use crate::i18n::I18n;
use crate::{field, live::BoardState, write};
use atelier_core::{document::Document, domain::agent_options::providers};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, PartialEq)]
struct Draft {
    repo: String,
    branch: String,
    agent: String,
}
impl Draft {
    fn from_document(settings: &Document) -> Self {
        Self {
            repo: field(settings, "repo"),
            branch: field(settings, "baseBranch"),
            agent: field(settings, "agent"),
        }
    }

    fn fields(&self) -> serde_json::Value {
        serde_json::json!({"repo":self.repo, "baseBranch":self.branch, "agent":self.agent})
    }

    fn patch(&self, previous: &Self) -> serde_json::Value {
        let (next, previous) = (self.fields(), previous.fields());
        let mut patch = serde_json::json!({"expected":{}});
        for field in ["repo", "baseBranch", "agent"] {
            if next[field] != previous[field] {
                patch[field] = next[field].clone();
                patch["expected"][field] = previous[field].clone();
            }
        }
        patch
    }
}

#[component]
pub fn Settings(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let original = RwSignal::new(Draft::from_document(&board.settings.get_untracked()));
    let draft = RwSignal::new(original.get_untracked());
    let dirty = Memo::new(move |_| original.get() != draft.get());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let discard = RwSignal::new(false);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let close = move || {
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
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
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <dialog id="settings-dialog" class="editor" node_ref=dialog aria-labelledby="settings-title" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text("PRÉFÉRENCES")}</p><h2 id="settings-title">{move || i18n.text("Paramètres de l’application")}</h2></div><button id="close-settings" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("Fermer")}</button></header>
            <label for="interface-language">{move || i18n.text("Langue")}</label><select id="interface-language" prop:value=move || i18n.code() on:change=move |event| { if let Err(message) = i18n.select(&event_target_value(&event)) { error.set(message.into()); } }><option value="fr-FR">"Français (France)"</option><option value="en-US">"English (US)"</option></select><p class="field-help">{move || i18n.text("La langue est conservée dans ce navigateur.")}</p>
            <p class="field-help">{move || i18n.text("Ces valeurs s’appliquent aux nouvelles tâches. Les projets peuvent définir leurs propres préférences.")}</p>
            <form on:submit=move |event| {
                event.prevent_default();
                if pending.get_untracked() || !dirty.get_untracked() { return; }
                let token = match token.get_value() { Ok(token) => token, Err(message) => { error.set(message); return; } };
                let body = draft.get_untracked().patch(&original.get_untracked());
                pending.set(true); error.set(String::new()); notice.set(String::new());
                spawn_local(async move {
                    let result = async {
                        let response = write(&token, Method::PUT, "/api/settings", &body).await?;
                        let saved: Document = response.json().await.map_err(|_| "Réponse du serveur invalide.".to_owned())?;
                        let saved = Draft::from_document(&saved);
                        original.set(saved.clone()); draft.set(saved);
                        notice.set("Paramètres enregistrés.".into());
                        Ok::<(), String>(())
                    }.await;
                    let _ = board.refresh(&token).await;
                    if let Err(message) = result { error.set(message); }
                    pending.set(false);
                });
            }>
                <fieldset disabled=move || pending.get()>
                    <label for="settings-agent">{move || i18n.text("Agent par défaut")}</label><select id="settings-agent" autofocus prop:value=move || draft.get().agent on:change=move |event| draft.update(|value| value.agent = event_target_value(&event))>
                        <For each=move || providers(&board.settings.get()).unwrap_or_default() key=|provider| field(provider,"id") children=move |provider| view! { <option value=field(&provider,"id")>{field(&provider,"name")}</option> } />
                    </select>
                    <p class="field-help">{move || i18n.text("Cette préférence ne lance aucun agent. Gère les profils depuis le bouton Agents du tableau.")}</p>
                    <label for="settings-repo">{move || i18n.text("Dépôt Git source par défaut")}</label><input id="settings-repo" maxlength="2000" spellcheck="false" prop:value=move || draft.get().repo on:input=move |event| draft.update(|value| value.repo = event_target_value(&event)) />
                    <p class="field-help">{move || i18n.text("Chemin complet d’un dossier existant. Laisser vide pour choisir le projet à chaque création.")}</p>
                    <label for="settings-branch">{move || i18n.text("Référence Git de départ par défaut")}</label><input id="settings-branch" maxlength="180" placeholder="HEAD" spellcheck="false" prop:value=move || draft.get().branch on:input=move |event| draft.update(|value| value.branch = event_target_value(&event)) />
                </fieldset>
                <p id="settings-error" class="error" role="alert">{move || i18n.text(&error.get())}</p><p id="settings-notice" class="field-help" role="status">{move || i18n.text(&notice.get())}</p>
                <div class="editor-actions"><button id="save-settings" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text(if pending.get() { "Enregistrement…" } else { "Enregistrer" })}</button></div>
            </form>
            <div class="confirmation" role="alert" hidden=move || !discard.get()><p>{move || i18n.text("Abandonner les modifications non enregistrées ?")}</p><button id="discard-settings" class="danger" type="button" on:click=move |_| close()>{move || i18n.text("Abandonner")}</button><button id="keep-settings" type="button" on:click=move |_| discard.set(false)>{move || i18n.text("Continuer à modifier")}</button></div>
        </dialog>
    }
}
