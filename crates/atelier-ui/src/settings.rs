use crate::i18n::I18n;
use crate::{field, live::BoardState, write};
use atelier_core::{document::Document, domain::agent_options::providers};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

const TABS: [(&str, &str); 6] = [
    ("general", "message.general"),
    ("jira", "Jira"),
    ("forges", "message.forges"),
    ("tempo", "Tempo"),
    ("connectors", "message.connectors"),
    ("updates", "message.updates"),
];

#[derive(Clone, PartialEq)]
struct Draft {
    agent: String,
}
impl Draft {
    fn from_document(settings: &Document) -> Self {
        Self {
            agent: field(settings, "agent"),
        }
    }
    fn patch(&self, previous: &Self) -> serde_json::Value {
        serde_json::json!({"agent":self.agent,"expected":{"agent":previous.agent}})
    }
}
#[component]
pub fn Settings(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
    #[prop(default = "general")] initial_tab: &'static str,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let original = RwSignal::new(Draft::from_document(&board.settings.get_untracked()));
    let draft = RwSignal::new(original.get_untracked());
    let dirty = Memo::new(move |_| original.get() != draft.get());
    let pending = RwSignal::new(false);
    let tab = RwSignal::new(
        TABS.iter()
            .position(|(id, _)| *id == initial_tab)
            .unwrap_or(0),
    );
    let jira_dirty = RwSignal::new(false);
    let forge_dirty = RwSignal::new(false);
    let tempo_dirty = RwSignal::new(false);
    let connectors_dirty = RwSignal::new(false);
    let updates_dirty = RwSignal::new(false);
    let updates_blocked = Memo::new(move |_| {
        dirty.get()
            || jira_dirty.get()
            || forge_dirty.get()
            || tempo_dirty.get()
            || connectors_dirty.get()
    });
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let discard = RwSignal::new(false);
    let navigation = StoredValue::new(None::<Callback<()>>);
    let close_focus = StoredValue::new(String::new());
    let close = move || {
        if let Some(next) = navigation.get_value() {
            navigation.set_value(None);
            next.run(());
            return;
        }
        on_close.run(());
        request_animation_frame(move || {
            if let Some(element) = document()
                .get_element_by_id("manage-settings")
                .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = element.focus();
            }
        });
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
        {
            close_focus.set_value(
                document()
                    .active_element()
                    .map(|element| element.id())
                    .unwrap_or_default(),
            );
            discard.set(true);
            request_animation_frame(move || {
                if let Some(element) = document()
                    .get_element_by_id("discard-settings")
                    .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    element.scroll_into_view();
                    let _ = element.focus();
                }
            });
        } else {
            close();
        }
    };
    let guard = expect_context::<crate::NavigationGuard>();
    guard.1.update(|generation| *generation += 1);
    let guard_generation = guard.1.get_untracked();
    guard.0.set(Some(Callback::new(move |next: Callback<()>| {
        if pending.get_untracked() {
            return;
        }
        navigation.set_value(Some(next));
        request_close();
    })));
    on_cleanup(move || {
        if guard.1.get_untracked() == guard_generation {
            guard.0.set(None);
        }
    });
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked()
            || jira_dirty.get_untracked()
            || forge_dirty.get_untracked()
            || tempo_dirty.get_untracked()
            || connectors_dirty.get_untracked()
            || updates_dirty.get_untracked()
            || pending.get_untracked()
        {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <section id="settings-page" aria-label=move ||i18n.text("message.application_settings") on:keydown=move |event: web_sys::KeyboardEvent| { if event.key()=="Escape" && !event.default_prevented() && document().query_selector("dialog[open]").ok().flatten().is_none() {event.prevent_default(); request_close();} }>
            <div id="settings-dialog" class="settings-page">
            <nav id="settings-navigation" class="settings-navigation" role="tablist" aria-orientation="vertical" aria-label=move || i18n.text("message.settings_sections")>
                {TABS.into_iter().enumerate().map(move |(index,(id,label))| view! {
                    <button class="settings-tab" class:selected-tab=move ||tab.get()==index type="button" role="tab" id=format!("settings-tab-{id}") aria-controls=format!("settings-panel-{id}") aria-selected=move || (tab.get()==index).to_string() tabindex=move || if tab.get()==index { "0" } else { "-1" } disabled=move || pending.get() on:click=move |_| tab.set(index) on:keydown=move |event: web_sys::KeyboardEvent| {
                        if pending.get_untracked() { return; }
                        let next=match event.key().as_str() { "ArrowRight"|"ArrowDown"=> (index+1)%TABS.len(), "ArrowLeft"|"ArrowUp"=> (index+TABS.len()-1)%TABS.len(), "Home"=>0, "End"=>TABS.len()-1, _=>return };
                        event.prevent_default(); tab.set(next);
                        if let Some(element)=document().get_element_by_id(&format!("settings-tab-{}",TABS[next].0)).and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok()) { let _=element.focus(); }
                    }>{move || i18n.text(label)}</button>
                }).collect_view()}
            </nav>
            <div class="settings-content">
            <section id="settings-panel-general" role="tabpanel" aria-labelledby="settings-tab-general" hidden=move || tab.get()!=0>
            <section class="form-section"><label for="interface-language">{move || i18n.text("message.language")}</label><select id="interface-language" prop:value=move || i18n.code() on:change=move |event| { if let Err(message) = i18n.select(&event_target_value(&event)) { error.set(message.into()); } }><option value="fr-FR">"Français (France)"</option><option value="en-US">"English (US)"</option></select><p class="hint">{move || i18n.text("message.the_language_is_saved_in_this_browser")}</p></section>
            <form id="settings-form" on:submit=move |event| {
                event.prevent_default();
                if pending.get_untracked() || !dirty.get_untracked() { return; }
                let token = match token.get_value() { Ok(token) => token, Err(message) => { error.set(message); return; } };
                let body = draft.get_untracked().patch(&original.get_untracked());
                pending.set(true); error.set(String::new()); notice.set(String::new());
                spawn_local(async move {
                    let result = async {
                        let response = write(&token, Method::PUT, "/api/settings", &body).await?;
                        let saved: Document = response.json().await.map_err(|_| "message.invalid_server_response".to_owned())?;
                        let saved = Draft::from_document(&saved);
                        original.set(saved.clone()); draft.set(saved);
                        notice.set("message.settings_saved".into());
                        Ok::<(), String>(())
                    }.await;
                    let _ = board.refresh(&token).await;
                    if let Err(message) = result { error.set(message); }
                    pending.set(false);
                });
            }>
                <fieldset disabled=move || pending.get()>
                    <section class="form-section"><div class="section-heading"><div><h3>{move ||i18n.text("message.agents_models")}</h3><p class="hint">{move ||i18n.text("message.your_preferences_for_new_tasks")}</p></div></div>
                    <label for="settings-agent">{move || i18n.text("message.default_agent")}</label><select id="settings-agent" autofocus prop:value=move || draft.get().agent on:change=move |event| draft.update(|value| value.agent = event_target_value(&event))>
                        <For each=move || providers(&board.settings.get()).unwrap_or_default() key=|provider| field(provider,"id") children=move |provider| view! { <option value=field(&provider,"id")>{field(&provider,"name")}</option> } />
                    </select>
                    <p class="hint">{move || i18n.text("message.used_for_new_tasks_unless_the_project_defines_a_preferred_agent")}</p></section>
                </fieldset>
                <p id="settings-notice" class="field-help" role="status">{move || i18n.text(&notice.get())}</p>
                <div class="form-actions"><button id="save-settings" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text(if pending.get() { "message.saving" } else { "message.save_general_settings" })}</button></div>
            </form>
            </section>
            <section id="settings-panel-forges" role="tabpanel" aria-labelledby="settings-tab-forges" hidden=move || tab.get()!=2><crate::forge::ForgeSettings token pending dirty=forge_dirty /></section>
            <section id="settings-panel-jira" role="tabpanel" aria-labelledby="settings-tab-jira" hidden=move || tab.get()!=1><crate::jira::JiraSettings board token pending dirty=jira_dirty /></section>
            <section id="settings-panel-tempo" role="tabpanel" aria-labelledby="settings-tab-tempo" hidden=move || tab.get()!=3><crate::tempo::TempoSettings token pending dirty=tempo_dirty /></section>
            <section id="settings-panel-connectors" role="tabpanel" aria-labelledby="settings-tab-connectors" hidden=move || tab.get()!=4><crate::connectors::Connectors board token pending dirty=connectors_dirty /></section>
            <section id="settings-panel-updates" role="tabpanel" aria-labelledby="settings-tab-updates" hidden=move || tab.get()!=5><crate::updates::Updates token pending dirty=updates_dirty blocked=updates_blocked /></section>
            </div></div>
            <p id="settings-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div class="confirmation" role="alert" hidden=move || !discard.get()><p>{move || i18n.text("message.discard_unsaved_changes")}</p><button id="discard-settings" class="danger" type="button" on:click=move |_| close()>{move || i18n.text("message.discard")}</button><button id="keep-settings" type="button" on:click=move |_| {navigation.set_value(None);discard.set(false);if let Some(element)=document().get_element_by_id(&close_focus.get_value()).and_then(|element|element.dyn_into::<web_sys::HtmlElement>().ok()){let _=element.focus();}} >{move || i18n.text("message.keep_editing")}</button></div>
        </section>
    }
}
