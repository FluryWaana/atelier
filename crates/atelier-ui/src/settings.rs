use crate::i18n::I18n;
use crate::{field, live::BoardState, write};
use atelier_core::{document::Document, domain::agent_options::providers};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

const TABS: [(&str, &str); 6] = [
    ("general", "Général"),
    ("jira", "Jira"),
    ("forges", "Forges"),
    ("tempo", "Tempo"),
    ("connectors", "Connecteurs"),
    ("updates", "Mises à jour"),
];

#[derive(Clone, PartialEq)]
struct Draft {
    repo: String,
    branch: String,
    agent: String,
    jira_enabled: bool,
    jira_mapping: std::collections::BTreeMap<String, String>,
    jira_revision: u64,
}
impl Draft {
    fn from_document(settings: &Document) -> Self {
        Self {
            repo: field(settings, "repo"),
            branch: field(settings, "baseBranch"),
            agent: field(settings, "agent"),
            jira_enabled: settings
                .get::<bool>("jiraAutoTransition")
                .ok()
                .flatten()
                .unwrap_or(false),
            jira_mapping: settings
                .get("jiraStatusMap")
                .ok()
                .flatten()
                .unwrap_or_default(),
            jira_revision: settings
                .get::<atelier_core::contracts::JiraConfiguration>("jiraConnection")
                .ok()
                .flatten()
                .unwrap_or_default()
                .revision,
        }
    }

    fn fields(&self) -> serde_json::Value {
        serde_json::json!({"repo":self.repo, "baseBranch":self.branch, "agent":self.agent,"jiraAutoTransition":self.jira_enabled,"jiraStatusMap":self.jira_mapping})
    }

    fn patch(&self, previous: &Self) -> serde_json::Value {
        let (next, previous) = (self.fields(), previous.fields());
        let mut patch = serde_json::json!({"expected":{}});
        for field in [
            "repo",
            "baseBranch",
            "agent",
            "jiraAutoTransition",
            "jiraStatusMap",
        ] {
            if next[field] != previous[field] {
                patch[field] = next[field].clone();
                patch["expected"][field] = previous[field].clone();
            }
        }
        if patch.get("jiraAutoTransition").is_some() || patch.get("jiraStatusMap").is_some() {
            patch["expectedJiraRevision"] = serde_json::json!(self.jira_revision);
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
    let tab = RwSignal::new(0usize);
    let jira_dirty = RwSignal::new(false);
    let forge_dirty = RwSignal::new(false);
    let tempo_dirty = RwSignal::new(false);
    let connectors_dirty = RwSignal::new(false);
    let updates_dirty = RwSignal::new(false);
    let repositories_dirty = RwSignal::new(false);
    let updates_blocked = Memo::new(move |_| {
        dirty.get()
            || jira_dirty.get()
            || forge_dirty.get()
            || tempo_dirty.get()
            || connectors_dirty.get()
            || repositories_dirty.get()
    });
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
        if dirty.get_untracked()
            || jira_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || tempo_dirty.get_untracked()
            || connectors_dirty.get_untracked()
            || updates_dirty.get_untracked()
            || repositories_dirty.get_untracked()
        {
            discard.set(true);
        } else {
            close();
        }
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked()
            || jira_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || tempo_dirty.get_untracked()
            || connectors_dirty.get_untracked()
            || updates_dirty.get_untracked()
            || repositories_dirty.get_untracked()
            || pending.get_untracked()
        {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <dialog id="settings-dialog" class="editor" node_ref=dialog aria-labelledby="settings-title" on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text("PRÉFÉRENCES")}</p><h2 id="settings-title">{move || i18n.text("Paramètres de l’application")}</h2></div><button id="close-settings" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("Fermer")}</button></header>
            <label for="interface-language">{move || i18n.text("Langue")}</label><select id="interface-language" prop:value=move || i18n.code() on:change=move |event| { if let Err(message) = i18n.select(&event_target_value(&event)) { error.set(message.into()); } }><option value="fr-FR">"Français (France)"</option><option value="en-US">"English (US)"</option></select><p class="field-help">{move || i18n.text("La langue est conservée dans ce navigateur.")}</p>
            <div class="editor-actions" role="tablist" aria-label=move || i18n.text("Paramètres de l’application")>
                {TABS.into_iter().enumerate().map(move |(index,(id,label))| view! {
                    <button type="button" role="tab" id=format!("settings-tab-{id}") aria-controls=format!("settings-panel-{id}") aria-selected=move || (tab.get()==index).to_string() tabindex=move || if tab.get()==index { "0" } else { "-1" } disabled=move || pending.get() on:click=move |_| tab.set(index) on:keydown=move |event: web_sys::KeyboardEvent| {
                        if pending.get_untracked() { return; }
                        let next=match event.key().as_str() { "ArrowRight"=> (index+1)%TABS.len(), "ArrowLeft"=> (index+TABS.len()-1)%TABS.len(), "Home"=>0, "End"=>TABS.len()-1, _=>return };
                        event.prevent_default(); tab.set(next);
                        if let Some(element)=document().get_element_by_id(&format!("settings-tab-{}",TABS[next].0)).and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok()) { let _=element.focus(); }
                    }>{move || i18n.text(label)}</button>
                }).collect_view()}
            </div>
            <section id="settings-panel-general" role="tabpanel" aria-labelledby="settings-tab-general" hidden=move || tab.get()!=0>
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
                    <h3>{move||i18n.text("Correspondances Jira globales")}</h3>
                    <label><input id="settings-jira-enabled" type="checkbox" prop:checked=move||draft.get().jira_enabled on:change=move|event|draft.update(|draft|draft.jira_enabled=event_target_checked(&event))/>{move||i18n.text("Proposer les statuts Jira correspondants")}</label>
                    <For each=||crate::STATUSES key=|(status,_)|*status children=move|(status,label)|view!{
                        <label for=format!("settings-jira-{status}")>{move||i18n.text(label)}</label>
                        <input id=format!("settings-jira-{status}") maxlength="100" prop:value=move||draft.get().jira_mapping.get(status).cloned().unwrap_or_default() on:input=move|event|draft.update(|draft|{draft.jira_mapping.insert(status.into(),event_target_value(&event));})/>
                    }/>
                    <p class="field-help">{move||i18n.text("Identifiant ou nom du statut Jira cible. Une correspondance ne déclenche aucun envoi lors de son enregistrement.")}</p>
                </fieldset>
                <p id="settings-notice" class="field-help" role="status">{move || i18n.text(&notice.get())}</p>
                <div class="editor-actions"><button id="save-settings" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text(if pending.get() { "Enregistrement…" } else { "Enregistrer" })}</button></div>
            </form>
            <crate::repository_access::RepositoryAccess token pending dirty=repositories_dirty/>
            </section>
            <section id="settings-panel-forges" role="tabpanel" aria-labelledby="settings-tab-forges" hidden=move || tab.get()!=2><crate::forge::ForgeSettings token pending dirty=forge_dirty /></section>
            <section id="settings-panel-jira" role="tabpanel" aria-labelledby="settings-tab-jira" hidden=move || tab.get()!=1><crate::jira::JiraSettings board token pending dirty=jira_dirty /></section>
            <section id="settings-panel-tempo" role="tabpanel" aria-labelledby="settings-tab-tempo" hidden=move || tab.get()!=3><crate::tempo::TempoSettings token pending dirty=tempo_dirty /></section>
            <section id="settings-panel-connectors" role="tabpanel" aria-labelledby="settings-tab-connectors" hidden=move || tab.get()!=4><crate::connectors::Connectors board token pending dirty=connectors_dirty /></section>
            <section id="settings-panel-updates" role="tabpanel" aria-labelledby="settings-tab-updates" hidden=move || tab.get()!=5><crate::updates::Updates token pending dirty=updates_dirty blocked=updates_blocked /></section>
            <p id="settings-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div class="confirmation" role="alert" hidden=move || !discard.get()><p>{move || i18n.text("Abandonner les modifications non enregistrées ?")}</p><button id="discard-settings" class="danger" type="button" on:click=move |_| close()>{move || i18n.text("Abandonner")}</button><button id="keep-settings" type="button" on:click=move |_| discard.set(false)>{move || i18n.text("Continuer à modifier")}</button></div>
        </dialog>
    }
}
