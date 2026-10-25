use crate::{i18n::I18n, read, write};
use atelier_core::application::extensions::{
    Candidate, Connection, Effect, FieldKind, Manifest, Method as ConnectorMethod, Registry, Report,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Default, PartialEq)]
struct Draft {
    values: BTreeMap<String, Value>,
    secrets: BTreeMap<String, String>,
    forget: BTreeSet<String>,
}
impl Draft {
    fn from_connection(connection: &Connection) -> Self {
        Self {
            values: connection.installed.values.clone(),
            ..Self::default()
        }
    }
}

fn translated(i18n: I18n, manifest: &Manifest, message: &str) -> String {
    manifest
        .translations
        .get(i18n.code())
        .and_then(|messages| messages.get(message))
        .cloned()
        .unwrap_or_else(|| message.to_owned())
}

#[derive(Clone)]
enum Action {
    Reload,
    Probe,
    Install,
    Save,
    Remove,
    Invoke(ConnectorMethod, Option<String>),
}

#[component]
pub fn Connectors(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let registry = RwSignal::new(None::<Registry>);
    let loading = RwSignal::new(false);
    let original = RwSignal::new(None::<Connection>);
    let draft = RwSignal::new(Draft::default());
    let module = RwSignal::new(String::new());
    let path = RwSignal::new(String::new());
    let trusted = RwSignal::new(false);
    let candidate = RwSignal::new(None::<(u64, Candidate)>);
    let approved = RwSignal::new(false);
    let reload = RwSignal::new(false);
    let remove = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let profile_dirty = Memo::new(move |_| {
        original
            .get()
            .is_some_and(|connection| draft.get() != Draft::from_connection(&connection))
    });
    let own_dirty = Memo::new(move |_| {
        profile_dirty.get()
            || !module.get().is_empty()
            || !path.get().is_empty()
            || trusted.get()
            || candidate.get().is_some()
    });
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let select = move |connection: Option<Connection>| {
        draft.set(
            connection
                .as_ref()
                .map(Draft::from_connection)
                .unwrap_or_default(),
        );
        original.set(connection);
        remove.set(false);
        error.set(String::new());
        notice.set(String::new());
    };
    let execute = StoredValue::new_local(move |action: Action| {
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
        let selected = original.get_untracked();
        let endpoint = selected
            .as_ref()
            .map(|connection| format!("/api/extensions/{}", connection.installed.manifest.id));
        let revision = selected
            .as_ref()
            .map(|connection| connection.installed.revision);
        let request = match &action {
            Action::Reload => None,
            Action::Probe => {
                if !trusted.get_untracked() || profile_dirty.get_untracked() {
                    return;
                }
                let Some(current) = registry.get_untracked() else {
                    return;
                };
                Some((
                    Method::POST,
                    "/api/extensions/probe".to_owned(),
                    json!({"module":module.get_untracked(),"path":path.get_untracked(),"expectedRevision":current.revision,"confirmed":true}),
                ))
            }
            Action::Install => {
                if !approved.get_untracked() || profile_dirty.get_untracked() {
                    return;
                }
                let Some((revision, candidate)) = candidate.get_untracked() else {
                    return;
                };
                Some((
                    Method::POST,
                    "/api/extensions/install".to_owned(),
                    json!({"expectedRevision":revision,"candidate":candidate,"confirmed":true}),
                ))
            }
            Action::Save => {
                let Some(connection) = selected.as_ref() else {
                    return;
                };
                let next = draft.get_untracked();
                let forgotten: Vec<_> = connection
                    .installed
                    .values
                    .keys()
                    .filter(|key| !next.values.contains_key(*key))
                    .cloned()
                    .collect();
                Some((
                    Method::PUT,
                    endpoint.clone().unwrap(),
                    json!({"expectedRevision":revision,"values":next.values,"forgetValues":forgotten,"secrets":next.secrets,"forgetSecrets":next.forget}),
                ))
            }
            Action::Remove => {
                if !remove.get_untracked() || profile_dirty.get_untracked() {
                    return;
                }
                let Some(endpoint) = endpoint.clone() else {
                    return;
                };
                Some((
                    Method::DELETE,
                    endpoint,
                    json!({"expectedRevision":revision,"confirmed":true}),
                ))
            }
            Action::Invoke(method, action) => {
                if profile_dirty.get_untracked() {
                    return;
                }
                let Some(endpoint) = endpoint.clone() else {
                    return;
                };
                Some((
                    Method::POST,
                    format!("{endpoint}/invoke"),
                    json!({"expectedRevision":revision,"method":method,"action":action,"input":{},"confirmed":true}),
                ))
            }
        };
        let probe_revision = registry.get_untracked().map(|value| value.revision);
        let reading = matches!(action, Action::Reload);
        if reading {
            loading.set(true);
        } else {
            pending.set(true);
        }
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                let response = if let Some((method, path, body)) = request {
                    write(&credential, method, &path, &body).await?
                } else {
                    read(&credential, "/api/extensions").await?
                };
                response
                    .json::<Value>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(value) => {
                    let applied = (|| -> Result<(), String> {
                        match action {
                            Action::Probe => {
                                let found = serde_json::from_value::<Candidate>(value)
                                    .map_err(|_| "Réponse du serveur invalide.")?;
                                candidate.set(Some((
                                    probe_revision.ok_or("Réponse du serveur invalide.")?,
                                    found,
                                )));
                                approved.set(false);
                            }
                            Action::Invoke(_, _) => {
                                let report = serde_json::from_value::<Report>(value)
                                    .map_err(|_| "Réponse du serveur invalide.")?;
                                registry.update(|registry| {
                                    if let Some(registry) = registry
                                        && let Some(connection) =
                                            registry.modules.iter_mut().find(|connection| {
                                                Some(connection.installed.manifest.id.as_str())
                                                    == selected.as_ref().map(|connection| {
                                                        connection.installed.manifest.id.as_str()
                                                    })
                                            })
                                    {
                                        connection.installed.report = Some(report.clone());
                                    }
                                });
                                original.update(|connection| {
                                    if let Some(connection) = connection {
                                        connection.installed.report = Some(report);
                                    }
                                });
                            }
                            _ => {
                                let saved = serde_json::from_value::<Registry>(value)
                                    .map_err(|_| "Réponse du serveur invalide.")?;
                                let id = selected
                                    .as_ref()
                                    .map(|connection| connection.installed.manifest.id.as_str());
                                let next = saved
                                    .modules
                                    .iter()
                                    .find(|connection| {
                                        Some(connection.installed.manifest.id.as_str()) == id
                                    })
                                    .cloned();
                                registry.set(Some(saved));
                                select(next);
                                if matches!(action, Action::Reload | Action::Install) {
                                    module.set(String::new());
                                    path.set(String::new());
                                    trusted.set(false);
                                    candidate.set(None);
                                    approved.set(false);
                                }
                                reload.set(false);
                                notice.set("Connecteurs enregistrés ou relus. Aucun accès distant vérifié par Atelier.".into());
                            }
                        }
                        Ok(())
                    })();
                    if let Err(message) = applied {
                        error.set(message);
                    }
                }
                Err(message) => error.set(message),
            }
            if reading {
                loading.set(false);
            } else {
                pending.set(false);
            }
        });
    });
    Effect::new(move |_| execute.with_value(|execute| execute(Action::Reload)));
    let invalidate = move || {
        candidate.set(None);
        approved.set(false);
        trusted.set(false);
    };
    view! {<section id="connectors-settings" aria-labelledby="connectors-heading" aria-busy=move||(pending.get()||loading.get()).to_string()>
        <h3 id="connectors-heading">{move||i18n.text("Connecteurs locaux")}</h3>
        <fieldset disabled=move||loading.get()>
        <p>{move||i18n.text("Un connecteur est un programme de confiance exécuté avec les droits de ton compte. Ses secrets restent dans le coffre et lui sont transmis pour les opérations explicites.")}</p>
        <p hidden=move||registry.get().is_none_or(|value|value.native_available)>{move||i18n.text("L’exécution de connecteurs n’est pas encore disponible sur cette plateforme.")}</p>
        <button id="reload-connectors" type="button" disabled=move||pending.get() on:click=move|_|{if own_dirty.get_untracked(){reload.set(true);}else{execute.with_value(|execute|execute(Action::Reload));}}>{move||i18n.text("Recharger les connecteurs")}</button>
        <div class="confirmation" hidden=move||!reload.get()><p>{move||i18n.text("Abandonner les brouillons des connecteurs et recharger ?")}</p><button id="confirm-reload-connectors" type="button" disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute(Action::Reload))>{move||i18n.text("Abandonner et recharger")}</button><button type="button" disabled=move||pending.get() on:click=move|_|reload.set(false)>{move||i18n.text("Revenir")}</button></div>
        <p hidden=move||registry.get().is_none_or(|value|!value.modules.is_empty())>{move||i18n.text("Aucun connecteur local installé.")}</p>
        <div class="editor-actions"><For each=move||registry.get().map(|value|value.modules).unwrap_or_default() key=|connection|(connection.installed.manifest.id.clone(),connection.installed.revision,connection.installed.report.as_ref().map(|report|report.id.clone())) children=move|connection|{
            let manifest=StoredValue::new(connection.installed.manifest.clone());
            let connection=StoredValue::new(connection);
            view!{<button type="button" data-connector=manifest.with_value(|manifest|manifest.id.clone()) disabled=move||pending.get()||profile_dirty.get() on:click=move|_|select(Some(connection.get_value()))>{move||manifest.with_value(|manifest|translated(i18n,manifest,&manifest.name))}</button>}
        }/></div>
        <div hidden=move||original.get().is_none()>
            <h4>{move||original.get().map(|connection|translated(i18n,&connection.installed.manifest,&connection.installed.manifest.name))}</h4>
            <p>{move||original.get().map(|connection|translated(i18n,&connection.installed.manifest,&connection.installed.manifest.description))}</p>
            <p>{move||original.get().map(|connection|connection.installed.binary.path)}</p>
            <fieldset disabled=move||pending.get()||registry.get().is_none_or(|value|!value.native_available)>
                <For each=move||original.get().map(|connection|connection.installed.manifest.fields).unwrap_or_default() key=move|field|(original.get_untracked().map(|connection|(connection.installed.manifest.id,connection.installed.revision)),field.id.clone()) children=move|field|{
                    let field=StoredValue::new(field);
                    let key=field.with_value(|field|field.id.clone());
                    let input_id=format!("connector-field-{key}");
                    let key=StoredValue::new(key);
                    let label=move||original.get().map(|connection|field.with_value(|field|translated(i18n,&connection.installed.manifest,&field.label))).unwrap_or_default();
                    let kind=field.with_value(|field|field.kind);
                    let controls=match kind {
                        FieldKind::Password=>view!{
                            <input id=input_id.clone() type="password" autocomplete="new-password" maxlength="4000" prop:value=move||draft.get().secrets.get(&key.get_value()).cloned().unwrap_or_default() disabled=move||draft.get().forget.contains(&key.get_value()) on:input=move|event|draft.update(|draft|{let value=event_target_value(&event);if value.is_empty(){draft.secrets.remove(&key.get_value());}else{draft.secrets.insert(key.get_value(),value);}})/>
                            <p>{move||i18n.text(if original.get().is_some_and(|connection|connection.secret_fields.contains(&key.get_value())){"Secret enregistré. Laisser vide pour le conserver."}else{"Aucun secret enregistré."})}</p>
                            <label><input type="checkbox" prop:checked=move||draft.get().forget.contains(&key.get_value()) on:change=move|event|draft.update(|draft|{if event_target_checked(&event){draft.forget.insert(key.get_value());draft.secrets.remove(&key.get_value());}else{draft.forget.remove(&key.get_value());}})/>{move||i18n.text("Oublier ce secret")}</label>
                        }.into_any(),
                        FieldKind::Checkbox=>view!{<input id=input_id.clone() type="checkbox" prop:checked=move||draft.get().values.get(&key.get_value()).and_then(Value::as_bool).unwrap_or(false) on:change=move|event|draft.update(|draft|{draft.values.insert(key.get_value(),json!(event_target_checked(&event)));})/>}.into_any(),
                        FieldKind::Select=>view!{<select id=input_id.clone() prop:value=move||{let value=draft.get().values.get(&key.get_value()).and_then(Value::as_str).map(str::to_owned);field.with_value(|field|field.options.iter().position(|choice|Some(choice.value.as_str())==value.as_deref()).map(|index|index.to_string()).unwrap_or_default())} on:change=move|event|draft.update(|draft|{let selected=event_target_value(&event).parse::<usize>().ok().and_then(|index|field.with_value(|field|field.options.get(index).map(|choice|choice.value.clone())));if let Some(value)=selected{draft.values.insert(key.get_value(),json!(value));}else{draft.values.remove(&key.get_value());}})><option value="">{move||i18n.text("Non défini")}</option>{field.with_value(|field|field.options.clone()).into_iter().enumerate().map(move|(index,choice)|view!{<option value=index.to_string()>{move||original.get().map(|connection|translated(i18n,&connection.installed.manifest,&choice.label))}</option>}).collect_view()}</select>}.into_any(),
                        FieldKind::Textarea=>view!{<textarea id=input_id.clone() rows="3" maxlength="8000" prop:value=move||draft.get().values.get(&key.get_value()).and_then(Value::as_str).unwrap_or_default().to_owned() on:input=move|event|draft.update(|draft|{draft.values.insert(key.get_value(),json!(event_target_value(&event)));})/>}.into_any(),
                        _=>view!{<input id=input_id.clone() type=if kind==FieldKind::Url{"url"}else{"text"} maxlength="8000" spellcheck="false" prop:value=move||draft.get().values.get(&key.get_value()).and_then(Value::as_str).unwrap_or_default().to_owned() on:input=move|event|draft.update(|draft|{draft.values.insert(key.get_value(),json!(event_target_value(&event)));})/>}.into_any(),
                    };
                    view!{<label for=input_id>{label}</label>{controls}<button type="button" hidden=kind==FieldKind::Password disabled=move||!draft.get().values.contains_key(&key.get_value()) on:click=move|_|draft.update(|draft|{draft.values.remove(&key.get_value());})>{move||i18n.text("Retirer cette valeur")}</button>}
                }/>
                <button id="save-connector" type="button" disabled=move||!profile_dirty.get() on:click=move|_|execute.with_value(|execute|execute(Action::Save))>{move||i18n.text("Enregistrer le connecteur")}</button>
                <button id="check-connector" type="button" disabled=move||profile_dirty.get() on:click=move|_|execute.with_value(|execute|execute(Action::Invoke(ConnectorMethod::Check,None)))>{move||i18n.text("Exécuter le contrôle du connecteur")}</button>
                <For each=move||original.get().map(|connection|connection.installed.manifest.tabs.into_iter().flat_map(|tab|tab.actions).filter(|action|action.effect==Effect::Read).collect::<Vec<_>>()).unwrap_or_default() key=move|action|(original.get_untracked().map(|connection|(connection.installed.manifest.id,connection.installed.revision)),action.id.clone()) children=move|action|{let action=StoredValue::new(action);view!{<button type="button" disabled=move||profile_dirty.get() on:click=move|_|execute.with_value(|execute|execute(Action::Invoke(ConnectorMethod::Action,Some(action.with_value(|action|action.id.clone())))))>{move||original.get().map(|connection|action.with_value(|action|translated(i18n,&connection.installed.manifest,&action.label)))}</button>}}/>
                <button id="remove-connector" class="danger" type="button" disabled=move||profile_dirty.get() on:click=move|_|remove.set(true)>{move||i18n.text("Retirer le connecteur…")}</button>
            </fieldset>
            <div class="confirmation" hidden=move||!remove.get()><p>{move||i18n.text("Retirer le connecteur et ses secrets ? Les tâches importées seront conservées.")}</p><button id="confirm-remove-connector" type="button" disabled=move||pending.get() on:click=move|_|execute.with_value(|execute|execute(Action::Remove))>{move||i18n.text("Confirmer")}</button><button type="button" disabled=move||pending.get() on:click=move|_|remove.set(false)>{move||i18n.text("Annuler")}</button></div>
            <div role="status"><p>{move||original.get().and_then(|connection|connection.installed.report.map(|report|report.observed_at))}</p><p>{move||original.get().and_then(|connection|connection.installed.report.map(|report|translated(i18n,&connection.installed.manifest,&report.message)))}</p></div>
        </div>
        <h4>{move||i18n.text("Ajouter un connecteur")}</h4>
        <fieldset disabled=move||pending.get()||profile_dirty.get()||registry.get().is_none_or(|value|!value.native_available)>
            <label for="connector-module">{move||i18n.text("Identifiant du module")}</label><input id="connector-module" maxlength="64" spellcheck="false" prop:value=move||module.get() on:input=move|event|{module.set(event_target_value(&event));invalidate();}/>
            <label for="connector-path">{move||i18n.text("Programme du connecteur")}</label><input id="connector-path" maxlength="32000" spellcheck="false" prop:value=move||path.get() on:input=move|event|{path.set(event_target_value(&event));invalidate();}/>
            <label><input id="connector-trusted" type="checkbox" prop:checked=move||trusted.get() on:change=move|event|{trusted.set(event_target_checked(&event));candidate.set(None);approved.set(false);}/>{move||i18n.text("Je fais confiance à ce programme et autorise sa lecture de manifeste.")}</label>
            <button id="probe-connector" type="button" disabled=move||!trusted.get()||module.get().is_empty()||path.get().is_empty() on:click=move|_|execute.with_value(|execute|execute(Action::Probe))>{move||i18n.text("Lire le manifeste")}</button>
            <div hidden=move||candidate.get().is_none()><h4>{move||candidate.get().map(|(_,candidate)|translated(i18n,&candidate.manifest,&candidate.manifest.name))}</h4><p>{move||candidate.get().map(|(_,candidate)|translated(i18n,&candidate.manifest,&candidate.manifest.description))}</p><p>{move||candidate.get().map(|(_,candidate)|candidate.binary.path)}</p><p>{move||candidate.get().map(|(_,candidate)|candidate.binary.sha256)}</p>
                <label><input id="approve-connector" type="checkbox" prop:checked=move||approved.get() on:change=move|event|approved.set(event_target_checked(&event))/>{move||i18n.text("Installer ce manifeste et ce programme exacts.")}</label><button id="install-connector" type="button" disabled=move||!approved.get() on:click=move|_|execute.with_value(|execute|execute(Action::Install))>{move||i18n.text("Installer le connecteur")}</button>
            </div>
        </fieldset>
        </fieldset>
        <p id="connectors-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="connectors-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
