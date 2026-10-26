use crate::{field, i18n::I18n, write};
use atelier_core::{
    contracts::{AgentAuthentication, AgentDiagnostic},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn AgentDiagnostics(
    profile: RwSignal<Option<Document>>,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let result = RwSignal::new(None::<AgentDiagnostic>);
    let checking = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    Effect::new(move |_| {
        let _ = profile.get();
        result.set(None);
        error.set(String::new());
    });
    view! {
        <section class="task-files" hidden=move || profile.get().is_none() aria-labelledby="agent-diagnostic-heading">
            <h3 id="agent-diagnostic-heading">{move || i18n.text("Installation et connexion")}</h3>
            <p class="field-help">{move || i18n.text("Contrôle explicite du profil enregistré : version et état de connexion du CLI. Aucun prompt envoyé et aucune requête payante de validation de clé.")}</p>
            <button id="diagnose-profile" type="button" disabled=move || pending.get() || blocked.get() || profile.get().is_none() on:click=move |_| {
                if pending.get_untracked() || blocked.get_untracked() { return; }
                let Some(profile) = profile.get_untracked() else { return; };
                let token = match token.get_value() { Ok(token)=>token, Err(message)=>{error.set(message);return;} };
                pending.set(true); checking.set(true); result.set(None); error.set(String::new());
                spawn_local(async move {
                    let response = async { write(&token, Method::POST, &format!("/api/agent-providers/{}/diagnose", field(&profile,"id")), &serde_json::json!({"expected":profile})).await?.json::<AgentDiagnostic>().await.map_err(|_| "Réponse du serveur invalide.".to_owned()) }.await;
                    if pending.try_get_untracked().is_none() { return; }
                    match response { Ok(diagnostic)=>result.set(Some(diagnostic)), Err(message)=>error.set(message) }
                    pending.set(false); checking.set(false);
                });
            }>{move || i18n.text(if checking.get() { "Vérification…" } else { "Vérifier l’installation et la connexion" })}</button>
            <p class="field-help" hidden=move || !blocked.get()>{move || i18n.text("Enregistre ou recharge le profil avant de le vérifier.")}</p>
            <p id="agent-diagnostic-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div id="agent-diagnostic-result" role="status">{move || result.get().map(|diagnostic| view! {
                <p class="field-help">{move || i18n.text("Résultat du dernier contrôle")}</p>
                <p><strong>{move || i18n.text("Exécutable")}</strong>" : "<span id="agent-diagnostic-executable">{diagnostic.executable}</span></p>
                <p><strong>{move || i18n.text("Version")}</strong>" : "<span id="agent-diagnostic-version">{diagnostic.version.unwrap_or_else(|| i18n.text("Indisponible"))}</span></p>
                <p class="error">{diagnostic.version_error.map(|message| i18n.text(&message))}</p>
                <p id="agent-diagnostic-auth">{i18n.text(match diagnostic.authentication {
                    AgentAuthentication::Present=>"Connexion CLI présente, accès au service non vérifié.",
                    AgentAuthentication::Absent=>"Aucune connexion CLI.",
                    AgentAuthentication::Configured=>"Clé présente, validité non vérifiée.",
                    AgentAuthentication::MissingKey=>"Aucune clé API enregistrée.",
                    AgentAuthentication::Unavailable=>"Connexion non vérifiée.",
                })}</p>
                <p class="error">{diagnostic.authentication_error.map(|message| i18n.text(&message))}</p>
                {diagnostic.account.map(|account| view! { <p><strong>{move || i18n.text("Compte")}</strong>" : "{account}</p> })}
                {diagnostic.auth_method.map(|method| view! { <p><strong>{move || i18n.text("Authentification")}</strong>" : "{method}</p> })}
                {diagnostic.account_report.map(|report|view!{<AccountQuotas report/>})}
            })}</div>
            <crate::agent_cli::AgentCliUpdate profile diagnostic=Signal::derive(move||result.get()) token pending blocked on_attempt=Callback::new(move |_|result.set(None))/>
        </section>
    }
}

#[component]
fn AccountQuotas(report: atelier_core::application::agent_account::Report) -> impl IntoView {
    use atelier_core::application::agent_account::State;
    let i18n = expect_context::<I18n>();
    let now = crate::time::clock();
    let observed = js_sys::Date::parse(&report.observed_at);
    view! {<section id="agent-account-quotas" class="task-files" aria-label=move||i18n.text("Dernier relevé des quotas")>
        <h4>{move||i18n.text("Dernier relevé des quotas")}</h4>
        <p>{report.observed_at}</p>
        <p>{i18n.text(match report.state{State::Available=>"Limites déclarées par le service ; aucun accès à un modèle n’en est déduit.",State::Api=>"Les limites API sont distinctes des quotas d’abonnement.",State::Disconnected=>"Connecte le CLI puis relance la vérification.",State::Unsupported=>"Ce mode de connexion ne fournit pas ces quotas.",State::Unavailable=>"Quotas indisponibles. Aucune valeur n’est déduite.",State::RateLimited=>"Le service limite les lectures. Attends la prochaine tentative autorisée."})}</p>
        {report.retry_at.map(|date|view!{<p>{i18n.text("Prochaine tentative : {0}").replace("{0}",&date)}</p>})}
        {report.account.map(|account|view!{<p>{i18n.text("Compte")}" : "{account}</p>})}
        {report.subscription.map(|plan|view!{<p>{i18n.text("Abonnement")}" : "{plan}</p>})}
        <p class="field-help">{move||i18n.text("Actualise avec le contrôle du profil. Le relevé expire après dix minutes ou à la réinitialisation ; aucune lecture automatique du compte.")}</p>
        <For each=move||report.windows.clone() key=|window|format!("{}:{}:{:?}:{:?}",window.name,window.kind,window.window_minutes,window.resets_at) children=move|window|{
            let stale=move||!observed.is_finite()||now.get()-observed>=600_000.0||window.resets_at.is_some_and(|date|date as f64*1000.0<=now.get());
            let label=if window.kind=="secondary"{"Fenêtre secondaire"}else{"Fenêtre principale"};
            let minutes=window.window_minutes;let amount=f64::from(window.remaining_tenths)/10.0;
            let reset=window.resets_at.map(|date|String::from(js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(date as f64*1000.0)).to_iso_string()));
            view!{<article class="task-files agent-quota-window"><h5>{window.name}</h5><p>{move||i18n.text(label)}{minutes.map(|minutes|format!(" · {minutes} min"))}</p>
                <p class="agent-quota-value">{move||if stale(){i18n.text("À actualiser")}else{i18n.text("{0} % restants").replace("{0}",&format!("{amount:.1}"))}}</p>
                <progress max="100" value=amount hidden=stale aria-label=move||i18n.text(label)></progress>
                <p>{reset.map(|reset|i18n.text("Réinitialisation : {0}").replace("{0}",&reset)).unwrap_or_else(||i18n.text("Réinitialisation non communiquée"))}</p>
            </article>}
        }/>
    </section>}
}
