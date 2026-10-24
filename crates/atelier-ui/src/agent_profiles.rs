use crate::{field, i18n::I18n, live::BoardState, read, write};
use atelier_core::document::Document;
use gloo_net::http::Method;
use leptos::prelude::*;
use std::collections::BTreeMap;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

const FIELDS: [&str; 14] = [
    "id",
    "name",
    "type",
    "auth",
    "model",
    "effort",
    "command",
    "configDir",
    "accountLabel",
    "npmPackage",
    "analyzeArgs",
    "editArgs",
    "versionArgs",
    "statusArgs",
];

#[derive(Clone, PartialEq)]
struct Draft {
    values: BTreeMap<&'static str, String>,
    key: String,
    clear_key: bool,
}
impl Draft {
    fn new(profile: Option<&Document>) -> Self {
        let mut values = BTreeMap::new();
        for key in FIELDS {
            let value = if key.ends_with("Args") {
                profile
                    .and_then(|profile| profile.raw(key))
                    .map(|raw| raw.get().to_owned())
                    .unwrap_or_else(|| {
                        if key == "versionArgs" {
                            r#"["--version"]"#
                        } else {
                            "[]"
                        }
                        .into()
                    })
            } else {
                profile
                    .map(|profile| field(profile, key))
                    .unwrap_or_default()
            };
            values.insert(key, value);
        }
        if values["type"].is_empty() {
            values.insert("type", "codex".into());
        }
        if values["auth"].is_empty() {
            values.insert("auth", "cli".into());
        }
        Self {
            values,
            key: String::new(),
            clear_key: false,
        }
    }

    fn body(&self, original: Option<&Document>) -> Result<serde_json::Value, String> {
        let mut body = serde_json::json!({"expected":original, "apiKey":self.key, "clearApiKey":self.clear_key});
        for key in FIELDS {
            if key.ends_with("Args") {
                if self.values["type"] == "command" {
                    let args: Vec<String> = serde_json::from_str(&self.values[key])
                        .map_err(|_| "Les arguments doivent être une liste JSON.".to_owned())?;
                    body[key] = serde_json::json!(args);
                }
            } else {
                body[key] =
                    serde_json::json!(if key == "npmPackage" && self.values["type"] != "command" {
                        ""
                    } else {
                        &self.values[key]
                    });
            }
        }
        Ok(body)
    }
}

#[derive(Clone, Copy)]
enum Confirmation {
    Close,
    Reload,
    Remove,
}

async fn catalog(token: &str) -> Result<Vec<Document>, String> {
    read(token, "/api/agent-providers")
        .await?
        .json()
        .await
        .map_err(|_| "Réponse du serveur invalide.".into())
}

#[component]
pub fn AgentProfiles(
    board: BoardState,
    token: StoredValue<Result<String, String>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let items = RwSignal::new(Vec::<Document>::new());
    let original = RwSignal::new(None::<Document>);
    let baseline = RwSignal::new(Draft::new(None));
    let draft = RwSignal::new(baseline.get_untracked());
    let editing = RwSignal::new(false);
    let dirty = Memo::new(move |_| draft.get() != baseline.get());
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let confirmation = RwSignal::new(None::<Confirmation>);
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    dialog.on_load(move |node| {
        let _ = node.show_modal();
    });
    let focus = |id: &'static str| {
        request_animation_frame(move || {
            if let Some(element) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id(id))
                && let Ok(element) = element.dyn_into::<web_sys::HtmlElement>()
            {
                let _ = element.focus();
            }
        })
    };
    let select = move |profile: Option<Document>| {
        let next = Draft::new(profile.as_ref());
        baseline.set(next.clone());
        draft.set(next);
        original.set(profile);
        editing.set(true);
        error.set(String::new());
        notice.set(String::new());
        confirmation.set(None);
        focus("profile-name");
    };
    let close = move || {
        if let Some(node) = dialog.get_untracked() {
            node.close();
        }
        on_close.run(());
        focus("manage-agents");
    };
    let ask = move |action: Confirmation| {
        confirmation.set(Some(action));
        focus("confirm-profile-action");
    };
    let request_close = move || {
        if pending.get_untracked() {
            return;
        }
        if dirty.get_untracked() {
            ask(Confirmation::Close);
        } else {
            close();
        }
    };
    let refresh = move |reload: bool| {
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
        let id = draft.get_untracked().values["id"].clone();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = catalog(&token).await;
            if pending.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok(profiles) => {
                    if reload {
                        let current = profiles
                            .iter()
                            .find(|profile| field(profile, "id") == id)
                            .cloned();
                        if current.is_some() {
                            select(current);
                        } else {
                            select(None);
                            editing.set(false);
                            focus("new-profile");
                        }
                    }
                    items.set(profiles);
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    refresh(false);
    let execute = move |remove: bool| {
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
        let id = draft.get_untracked().values["id"].clone();
        let (method, path, body) = if remove {
            (
                Method::DELETE,
                format!("/api/agent-providers/{id}"),
                serde_json::json!({"expected":original.get_untracked()}),
            )
        } else {
            let body = match draft
                .get_untracked()
                .body(original.get_untracked().as_ref())
            {
                Ok(body) => body,
                Err(message) => {
                    error.set(message);
                    return;
                }
            };
            (Method::POST, "/api/agent-providers".into(), body)
        };
        pending.set(true);
        error.set(String::new());
        notice.set(String::new());
        confirmation.set(None);
        spawn_local(async move {
            let result = async {
                write(&token, method, &path, &body)
                    .await?
                    .json::<Vec<Document>>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            let _ = board.refresh(&token).await;
            if pending.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok(profiles) => {
                    if remove {
                        select(None);
                        editing.set(false);
                        focus("new-profile");
                    } else {
                        select(
                            profiles
                                .iter()
                                .find(|profile| field(profile, "id") == id)
                                .cloned(),
                        );
                    }
                    items.set(profiles);
                    notice.set(
                        if remove {
                            "Profil retiré."
                        } else {
                            "Profil enregistré."
                        }
                        .into(),
                    );
                }
                Err(message) => error.set(message),
            }
            pending.set(false);
        });
    };
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if dirty.get_untracked() || pending.get_untracked() {
            event.prevent_default();
            event.set_return_value("");
        }
    });
    on_cleanup(move || unload.remove());
    view! {
        <dialog id="agents-dialog" class="editor" node_ref=dialog aria-labelledby="agents-title" aria-busy=move || pending.get().to_string() on:cancel=move |event: web_sys::Event| { event.prevent_default(); request_close(); }>
            <header class="editor-heading"><div><p class="eyebrow">{move || i18n.text("AGENTS")}</p><h2 id="agents-title">{move || i18n.text("Profils d’agents")}</h2></div><button id="close-agents" type="button" disabled=move || pending.get() on:click=move |_| request_close()>{move || i18n.text("Fermer")}</button></header>
            <p class="field-help">{move || i18n.text("Un profil définit un agent et ses réglages. L’enregistrer ne lance aucune commande. Les diagnostics et exécutions seront raccordés ensuite.")}</p>
            <div class="editor-actions"><button id="new-profile" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| select(None)>{move || i18n.text("Nouveau profil")}</button><button id="refresh-profiles" type="button" disabled=move || pending.get() || dirty.get() on:click=move |_| refresh(false)>{move || i18n.text("Actualiser la liste")}</button></div>
            <ul class="profile-list"><For each=move || items.get() key=|profile| field(profile,"id") children=move |profile| {
                let id = StoredValue::new(field(&profile,"id"));
                let current = move || items.with(|items| items.iter().find(|profile| field(profile,"id") == id.get_value()).cloned());
                view! { <li><button type="button" data-profile=id.get_value() aria-pressed=move || original.get().is_some_and(|profile| field(&profile,"id") == id.get_value()).to_string() disabled=move || pending.get() || dirty.get() on:click=move |_| select(current())>{move || current().map(|profile| format!("{} · {}", field(&profile,"name"), field(&profile,"type"))).unwrap_or_default()}</button></li> }
            } /></ul>
            <p hidden=move || !dirty.get() class="field-help">{move || i18n.text("Enregistre ou recharge ce profil avant d’en choisir un autre.")}</p>
            <form hidden=move || !editing.get() on:submit=move |event| { event.prevent_default(); execute(false); }>
                <fieldset disabled=move || pending.get()>
                    <label for="profile-id">{move || i18n.text("Identifiant du profil")}</label><input id="profile-id" required maxlength="64" pattern="[a-z][a-z0-9-]*" readonly=move || original.get().is_some() prop:value=move || draft.get().values["id"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("id", event_target_value(&event)); }) />
                    <label for="profile-name">{move || i18n.text("Nom du profil")}</label><input id="profile-name" required maxlength="100" prop:value=move || draft.get().values["name"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("name", event_target_value(&event)); }) />
                    <label for="profile-type">{move || i18n.text("Type d’agent")}</label><select id="profile-type" disabled=move || original.get().is_some() prop:value=move || draft.get().values["type"].clone() on:change=move |event| draft.update(|draft| { draft.values.insert("type",event_target_value(&event)); draft.values.insert("auth","cli".into()); draft.values.insert("effort",String::new()); draft.key.clear(); draft.clear_key=false; })><option value="codex">"Codex"</option><option value="claude">"Claude Code"</option><option value="command">{move || i18n.text("Commande personnalisée")}</option></select>
                    {[ ("command", "Exécutable (facultatif pour Codex et Claude)", "2000"), ("configDir", "Dossier de configuration (facultatif)", "2000"), ("accountLabel", "Libellé du compte (facultatif)", "200"), ("model", "Modèle de l’agent", "180"), ("effort", "Niveau de réflexion", "40") ].into_iter().map(move |(key,label,limit)| view! { <label for=format!("profile-{key}")>{move || i18n.text(label)}</label><input id=format!("profile-{key}") maxlength=limit spellcheck="false" prop:value=move || draft.get().values[key].clone() on:input=move |event| draft.update(|draft| { draft.values.insert(key,event_target_value(&event)); }) /> }).collect_view()}
                    <p class="field-help">{move || i18n.text("L’exécutable ne contient pas ses arguments. Le modèle et le niveau de réflexion peuvent rester vides pour utiliser les valeurs de l’agent.")}</p>
                    <div hidden=move || draft.get().values["type"] != "codex">
                        <label for="profile-auth">{move || i18n.text("Authentification")}</label><select id="profile-auth" prop:value=move || draft.get().values["auth"].clone() on:change=move |event| draft.update(|draft| { draft.values.insert("auth",event_target_value(&event)); draft.key.clear(); draft.clear_key=false; })><option value="cli">{move || i18n.text("Connexion CLI existante")}</option><option value="api">{move || i18n.text("Clé API via Codex CLI")}</option></select>
                        <p class="field-help" hidden=move || draft.get().values["auth"] == "api">{move || i18n.text("Enregistrer une connexion CLI supprime la clé API de ce profil.")}</p>
                        <div hidden=move || draft.get().values["auth"] != "api"><label for="profile-key">{move || i18n.text("Clé API")}</label><input id="profile-key" type="password" autocomplete="new-password" maxlength="4000" disabled=move || draft.get().clear_key prop:value=move || draft.get().key on:input=move |event| draft.update(|draft| draft.key=event_target_value(&event)) />
                            <p id="profile-key-state" class="field-help">{move || i18n.text(if original.get().is_some_and(|profile| profile.get::<bool>("apiKeySet").ok().flatten() == Some(true)) { "Secret enregistré. Laisser vide pour le conserver." } else { "Aucun secret enregistré." })}</p>
                            <label class="check-filter"><input id="profile-clear-key" type="checkbox" prop:checked=move || draft.get().clear_key on:change=move |event| draft.update(|draft| { draft.clear_key=event_target_checked(&event); if draft.clear_key { draft.key.clear(); } }) />{move || i18n.text("Oublier la clé API")}</label>
                        </div>
                    </div>
                    <div hidden=move || draft.get().values["type"] != "command">
                        <p class="field-help">{move || i18n.text("Arguments en listes JSON de textes, sans shell intermédiaire. Le prompt sera transmis sur l’entrée standard ; {model} et {effort} seront remplacés.")}</p>
                        {[("analyzeArgs","Arguments pour analyser"),("editArgs","Arguments pour modifier"),("versionArgs","Arguments pour lire la version"),("statusArgs","Arguments pour lire le compte")].into_iter().map(move |(key,label)| view! { <label for=format!("profile-{key}")>{move || i18n.text(label)}</label><textarea id=format!("profile-{key}") rows="2" maxlength="410000" spellcheck="false" prop:value=move || draft.get().values[key].clone() on:input=move |event| draft.update(|draft| { draft.values.insert(key,event_target_value(&event)); }) /> }).collect_view()}
                        <label for="profile-npmPackage">{move || i18n.text("Paquet npm pour vérifier les mises à jour (facultatif)")}</label><input id="profile-npmPackage" maxlength="200" prop:value=move || draft.get().values["npmPackage"].clone() on:input=move |event| draft.update(|draft| { draft.values.insert("npmPackage",event_target_value(&event)); }) />
                    </div>
                </fieldset>
                <div class="editor-actions"><button id="save-profile" class="primary" type="submit" disabled=move || pending.get() || !dirty.get()>{move || i18n.text("Enregistrer")}</button><button id="reload-profile" type="button" disabled=move || pending.get() on:click=move |_| { if dirty.get_untracked() { ask(Confirmation::Reload); } else { refresh(true); } }>{move || i18n.text("Recharger le profil")}</button><button id="remove-profile" type="button" class="danger" hidden=move || original.get().is_none() disabled=move || pending.get() || dirty.get() on:click=move |_| ask(Confirmation::Remove)>{move || i18n.text("Retirer le profil…")}</button></div>
            </form>
            <div class="confirmation" hidden=move || confirmation.get().is_none()><p>{move || i18n.text(if matches!(confirmation.get(),Some(Confirmation::Remove)) { "Retirer ce profil et sa clé ? Les profils utilisés par des tâches ou projets doivent d’abord être remplacés." } else { "Abandonner les modifications non enregistrées ?" })}</p><button id="confirm-profile-action" type="button" class="danger" disabled=move || pending.get() on:click=move |_| { let action=confirmation.get_untracked(); confirmation.set(None); match action { Some(Confirmation::Close)=>close(), Some(Confirmation::Reload)=>refresh(true), Some(Confirmation::Remove)=>execute(true), None=>{} } }>{move || i18n.text("Confirmer")}</button><button id="cancel-profile-action" type="button" disabled=move || pending.get() on:click=move |_| { confirmation.set(None); focus("profile-name"); }>{move || i18n.text("Annuler")}</button></div>
            <p id="profiles-error" class="error" role="alert">{move || i18n.text(&error.get())}</p><p id="profiles-notice" role="status">{move || i18n.text(&notice.get())}</p>
        </dialog>
    }
}
