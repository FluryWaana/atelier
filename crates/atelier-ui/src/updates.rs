use crate::{i18n::I18n, read, write};
use atelier_core::application::updates::{Phase, Status};
use gloo_net::http::Method;
use leptos::prelude::*;
use std::{cell::Cell, rc::Rc};
use wasm_bindgen_futures::spawn_local;

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Preparing => "Préparation de la mise à jour…",
        Phase::Prepared => "Version prête à confirmer",
        Phase::Waiting => "Arrêt contrôlé en cours…",
        Phase::Activating => "Changement de version…",
        Phase::Trying => "Contrôle du démarrage…",
        Phase::Committed => "Mise à jour vérifiée",
        Phase::RollingBack => "Récupération en cours…",
        Phase::RolledBack => "Version précédente rétablie",
        Phase::Cancelled => "Préparation annulée",
        Phase::Failed => "Préparation interrompue",
        Phase::Uncertain => "Récupération à examiner",
    }
}

#[component]
pub fn Updates(
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    dirty: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let status = RwSignal::new(None::<Status>);
    let archive = RwSignal::new(String::new());
    let signature = RwSignal::new(String::new());
    let approved = RwSignal::new(false);
    let recovery = RwSignal::new(false);
    let cancel = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let uncertain = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let own_dirty = Memo::new(move |_| {
        !archive.get().is_empty()
            || !signature.get().is_empty()
            || approved.get()
            || recovery.get()
            || cancel.get()
    });
    Effect::new(move |_| dirty.set(own_dirty.get()));
    let alive = Rc::new(Cell::new(true));
    let mounted = StoredValue::new_local(alive.clone());
    on_cleanup(move || mounted.with_value(|alive| alive.set(false)));
    let operation = move || status.get().and_then(|status| status.operation);
    let execute = StoredValue::new_local(move |action: &'static str| {
        if pending.get_untracked() || loading.get_untracked() {
            return;
        }
        if matches!(action, "apply" | "recover") && uncertain.get_untracked() {
            return;
        }
        if matches!(action, "apply" | "recover") && blocked.get_untracked() {
            return;
        }
        let credential = match token.get_value() {
            Ok(value) => value,
            Err(message) => {
                error.set(message);
                return;
            }
        };
        let body = if action == "reload" {
            None
        } else if action == "preview" {
            Some(
                serde_json::json!({"archive":archive.get_untracked(),"signature":signature.get_untracked()}),
            )
        } else {
            let Some(operation) = operation() else {
                return;
            };
            let confirmed = match action {
                "apply" => approved.get_untracked(),
                "recover" => recovery.get_untracked(),
                "cancel" => cancel.get_untracked(),
                _ => false,
            };
            if !confirmed {
                return;
            }
            Some(serde_json::json!({"id":operation.id,"confirmed":true}))
        };
        if action == "reload" {
            loading.set(true);
        } else {
            pending.set(true);
        }
        if matches!(action, "apply" | "recover") {
            uncertain.set(true);
        }
        error.set(String::new());
        notice.set(String::new());
        let alive = alive.clone();
        spawn_local(async move {
            let result = async {
                if let Some(body) = body {
                    write(
                        &credential,
                        Method::POST,
                        &format!("/api/updates/{action}"),
                        &body,
                    )
                    .await?;
                }
                read(&credential, "/api/updates")
                    .await?
                    .json::<Status>()
                    .await
                    .map_err(|_| "Réponse du serveur invalide.".to_owned())
            }
            .await;
            if !alive.get() {
                return;
            }
            match result {
                Ok(value) => {
                    status.set(Some(value));
                    uncertain.set(false);
                    approved.set(false);
                    recovery.set(false);
                    cancel.set(false);
                    if action == "preview" {
                        archive.set(String::new());
                        signature.set(String::new());
                    }
                }
                Err(message) => error.set(message),
            }
            if matches!(action, "apply" | "recover") {
                notice.set("La commande peut avoir été reçue. Atelier redémarre si elle est acceptée. Rouvre-le avec le lanceur pour lire le résultat ; ne renvoie pas la commande à l’aveugle.".into());
                approved.set(false);
                recovery.set(false);
            }
            if action == "reload" {
                loading.set(false);
            } else {
                pending.set(false);
            }
        });
    });
    Effect::new(move |_| execute.with_value(|execute| execute("reload")));
    view! {<section id="updates-settings" aria-labelledby="updates-heading" aria-busy=move||(loading.get()||pending.get()).to_string()>
        <h3 id="updates-heading">{move||i18n.text("Mises à jour")}</h3>
        <p>{move||i18n.text("La nouvelle version est vérifiée avant activation. La version précédente reste disponible pour la récupération. Aucune base de données n’est remplacée par ce retour de version.")}</p>
        <p>{move||i18n.text("Version actuelle :")}" "{move||status.get().map(|status|status.current_version)}</p>
        <p hidden=move||status.get().is_none_or(|status|status.installed)>{move||i18n.text("Installe d’abord le paquet Rust avec sa clé de confiance pour utiliser ce parcours de mise à jour.")}</p>
        <button id="reload-updates" type="button" disabled=move||pending.get()||loading.get() on:click=move|_|execute.with_value(|execute|execute("reload"))>{move||i18n.text("Relire l’état de mise à jour")}</button>
        <fieldset disabled=move||pending.get()||loading.get()||status.get().is_none_or(|status|!status.installed)||operation().is_some_and(|operation|operation.state.pending())>
            <label for="update-archive">{move||i18n.text("Archive de mise à jour")}</label><input id="update-archive" maxlength="2000" spellcheck="false" prop:value=move||archive.get() on:input=move|event|{archive.set(event_target_value(&event));approved.set(false);}/>
            <label for="update-signature">{move||i18n.text("Signature fournie avec l’archive")}</label><input id="update-signature" maxlength="2000" spellcheck="false" prop:value=move||signature.get() on:input=move|event|{signature.set(event_target_value(&event));approved.set(false);}/>
            <p>{move||i18n.text("Indique les chemins complets des deux fichiers sur ce poste. Seule la clé de cette installation peut autoriser le paquet.")}</p>
            <button id="preview-update" type="button" disabled=move||archive.get().is_empty()||signature.get().is_empty() on:click=move|_|execute.with_value(|execute|execute("preview"))>{move||i18n.text("Vérifier et préparer la version")}</button>
        </fieldset>
        <div hidden=move||operation().is_none()>
            <h4 id="update-phase">{move||operation().map(|operation|i18n.text(phase_label(operation.state)))}</h4>
            <p>{move||operation().map(|operation|format!("{} → {}",operation.from_version,operation.to_version))}</p>
            <p>{move||operation().map(|operation|i18n.text(&operation.message))}</p>
            <p>{move||i18n.text("Empreinte de la clé :")}" "{move||operation().map(|operation|operation.key_id)}</p>
            <p>{move||i18n.text("Empreinte du paquet :")}" "{move||operation().map(|operation|operation.sha256)}</p>
            <details><summary>{move||i18n.text("Contenu du paquet préparé")}</summary><ul><For each=move||operation().map(|operation|operation.files).unwrap_or_default() key=|file|file.path.clone() children=move|file|view!{<li>{file.path}" · "{file.size}" "{move||i18n.text("octets")}</li>}/></ul></details>
            <p class="field-help" hidden=move||!blocked.get()>{move||i18n.text("Enregistre ou abandonne les autres brouillons avant de redémarrer Atelier.")}</p>
            <fieldset hidden=move||operation().is_none_or(|operation|operation.state!=Phase::Prepared) disabled=move||pending.get()||loading.get()||blocked.get()||uncertain.get()>
                <label><input id="approve-update" type="checkbox" prop:checked=move||approved.get() on:change=move|event|approved.set(event_target_checked(&event))/>{move||i18n.text("Arrêter les exécutions et services, puis activer cette version exacte et vérifier son démarrage.")}</label>
                <button id="apply-update" class="primary" type="button" disabled=move||!approved.get() on:click=move|_|execute.with_value(|execute|execute("apply"))>{move||i18n.text("Mettre à jour et redémarrer")}</button>
            </fieldset>
            <fieldset hidden=move||operation().is_none_or(|operation|!matches!(operation.state,Phase::Preparing|Phase::Prepared)) disabled=move||pending.get()||loading.get()>
                <label><input id="approve-cancel-update" type="checkbox" prop:checked=move||cancel.get() on:change=move|event|cancel.set(event_target_checked(&event))/>{move||i18n.text("Annuler cette préparation sans changer la version active.")}</label><button id="cancel-update" type="button" disabled=move||!cancel.get() on:click=move|_|execute.with_value(|execute|execute("cancel"))>{move||i18n.text("Annuler la préparation")}</button>
            </fieldset>
            <fieldset hidden=move||operation().is_none_or(|operation|!operation.state.maintenance()) disabled=move||pending.get()||loading.get()||blocked.get()||uncertain.get()>
                <label><input id="approve-recover-update" type="checkbox" prop:checked=move||recovery.get() on:change=move|event|recovery.set(event_target_checked(&event))/>{move||i18n.text("Arrêter l’instance liée et tenter le retour à sa version précédente vérifiée, en conservant les données actuelles.")}</label><button id="recover-update" type="button" disabled=move||!recovery.get() on:click=move|_|execute.with_value(|execute|execute("recover"))>{move||i18n.text("Récupérer la version précédente")}</button>
            </fieldset>
        </div>
        <p id="updates-error" class="error" role="alert">{move||i18n.text(&error.get())}</p><p id="updates-notice" role="status">{move||i18n.text(&notice.get())}</p>
    </section>}
}
