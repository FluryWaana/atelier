use crate::{field, i18n::I18n, read, write};
use atelier_core::{
    application::agent_cli::{Availability, Operation, Phase},
    contracts::AgentDiagnostic,
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;
#[component]
pub fn AgentCliUpdate(
    profile: RwSignal<Option<Document>>,
    diagnostic: Signal<Option<AgentDiagnostic>>,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
    on_attempt: Callback<()>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let journal = RwSignal::new(None::<Operation>);
    let loading = RwSignal::new(false);
    let generation = RwSignal::new(0u64);
    let approved = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let info = Signal::derive(move || {
        diagnostic
            .get()
            .and_then(|diagnostic| diagnostic.update_info)
    });
    let key = Memo::new(move |_| {
        profile.get().and_then(|profile| {
            info.get()
                .filter(|info| info.availability == Availability::Available && info.image.is_some())
                .map(|info| serde_json::json!({"expected":profile,"info":info}).to_string())
        })
    });
    Effect::new(move |_| {
        let _ = key.get();
        approved.set(String::new());
    });
    let reading = alive.clone();
    let load = StoredValue::new_local(move || {
        if pending.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        generation.update(|generation| *generation = generation.wrapping_add(1));
        let expected = generation.get_untracked();
        loading.set(true);
        let alive = reading.clone();
        spawn_local(async move {
            let result = async {
                read(&credential, "/api/agent-providers/cli-update")
                    .await?
                    .json::<Option<Operation>>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() && generation.get_untracked() == expected {
                loading.set(false);
                match result {
                    Ok(value) => journal.set(value),
                    Err(message) => error.set(message),
                }
            }
        });
    });
    Effect::new(move |_| {
        let _ = profile.get();
        if !pending.get() {
            load.with_value(|load| load());
        }
    });
    let writing = alive.clone();
    let execute = move |_| {
        if pending.get_untracked()
            || loading.get_untracked()
            || blocked.get_untracked()
            || key
                .get_untracked()
                .is_none_or(|key| key != approved.get_untracked())
        {
            return;
        }
        let (Some(profile), Some(info)) = (profile.get_untracked(), info.get_untracked()) else {
            return;
        };
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let path = format!("/api/agent-providers/{}/cli-update", field(&profile, "id"));
        let body = serde_json::json!({"expected":profile,"info":info,"confirmed":true});
        pending.set(true);
        approved.set(String::new());
        error.set(String::new());
        on_attempt.run(());
        let alive = writing.clone();
        spawn_local(async move {
            let result = async {
                write(&credential, Method::POST, &path, &body)
                    .await?
                    .json::<Operation>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if alive.get() {
                pending.set(false);
                match result {
                    Ok(operation) => journal.set(Some(operation)),
                    Err(message) => error.set(message),
                }
            }
        });
    };
    let busy = move || {
        journal.with(|journal| {
            journal.as_ref().is_some_and(|operation| {
                matches!(operation.phase, Phase::Preparing | Phase::Running)
            })
        })
    };
    let selected = move || {
        journal.get().filter(|operation| {
            profile
                .get()
                .is_some_and(|profile| field(&profile, "id") == operation.provider)
        })
    };
    view! {<section class="task-files" aria-labelledby="cli-update-heading" aria-busy=move||(loading.get()||pending.get()).to_string()>
        <h4 id="cli-update-heading">{move||i18n.text("Mise à jour du CLI")}</h4>
        <p>{move||info.get().map(|info|i18n.text(match info.availability{Availability::Available=>"Une version plus récente est disponible.",Availability::Current=>"Aucune version stable plus récente signalée.",Availability::Unknown=>"Version publiée indisponible ; aucune mise à jour proposée.",Availability::Managed=>"Cette installation utilise son gestionnaire habituel pour les mises à jour.",Availability::Custom=>"Programme personnalisé : utilise son gestionnaire habituel pour les mises à jour."})).unwrap_or_else(||i18n.text("Lance le diagnostic du profil pour vérifier sa version et ses mises à jour."))}</p>
        <p>{move||info.get().and_then(|info|info.latest_version).map(|version|i18n.text("Version publiée : {0}").replace("{0}",&version)).unwrap_or_default()}</p>
        <div hidden=move||key.get().is_none()>
            <p class="field-help">{move||i18n.text("Ferme les autres sessions de ce CLI et arrête les opérations Atelier. La commande native peut installer une version stable plus récente que celle affichée. Aucun retour automatique à une ancienne version du CLI.")}</p>
            <label><input id="confirm-cli-update" type="checkbox" disabled=move||pending.get()||loading.get()||blocked.get()||busy() prop:checked=move||key.get().is_some_and(|key|key==approved.get()) on:change=move|event|approved.set(if event_target_checked(&event){key.get_untracked().unwrap_or_default()}else{String::new()})/>{move||i18n.text("Mettre à jour uniquement le CLI du profil et de l’installation affichés.")}</label>
            <button id="update-agent-cli" type="button" disabled=move||pending.get()||loading.get()||blocked.get()||busy()||key.get().is_none_or(|key|key!=approved.get()) on:click=execute>{move||i18n.text("Confirmer la mise à jour du CLI")}</button>
        </div>
        <p hidden=move||!busy()>{move||i18n.text("Une mise à jour CLI est en cours. Relis son état avant une autre action.")}</p>
        {move||selected().map(|operation|view!{<article class="task-files" id="cli-update-result"><p>{i18n.text(match operation.phase{Phase::Preparing=>"Mise à jour CLI préparée",Phase::Running=>"Mise à jour CLI en cours",Phase::Observed=>"Version CLI observée",Phase::Failed=>"Mise à jour CLI non lancée",Phase::Uncertain=>"Résultat CLI incertain"})}</p><p>{operation.updated_at}</p><p>{i18n.text(&operation.message)}</p><p>{i18n.text("Version réellement observée : {0}").replace("{0}",operation.actual_version.as_deref().unwrap_or("—"))}</p><p>{operation.target_reached.map(|reached|i18n.text(if reached{"Version cible atteinte selon le CLI."}else{"La version cible n’a pas été observée."})).unwrap_or_default()}</p></article>})}
        <button id="reload-cli-update" type="button" disabled=move||pending.get()||loading.get() on:click=move |_|{error.set(String::new());load.with_value(|load|load());}>{move||i18n.text("Relire l’état de la mise à jour CLI")}</button>
        <p class="error" role="alert">{move||i18n.text(&error.get())}</p>
    </section>}
}
