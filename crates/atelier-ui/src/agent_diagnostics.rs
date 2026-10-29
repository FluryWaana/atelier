use crate::{field, i18n::I18n, write};
use atelier_core::{
    contracts::{AgentAuthentication, AgentDiagnostic},
    document::Document,
};
use gloo_net::http::Method;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, Copy)]
pub struct AgentStatuses(pub RwSignal<atelier_core::application::agent_status::Checks>);

pub fn current_status(checks: RwSignal<atelier_core::application::agent_status::Checks>, id: &str, settings: &Document) -> Option<AgentDiagnostic> {
    let profile = atelier_core::domain::agent_options::providers(settings).ok()?.into_iter().find(|profile| field(profile, "id") == id)?;
    checks.with(|checks| checks.diagnostic(id, &profile).cloned())
}

#[component]
pub fn AgentDiagnostics(
    profile: RwSignal<Option<Document>>,
    token: StoredValue<Result<String, String>>,
    pending: RwSignal<bool>,
    blocked: Memo<bool>,
) -> impl IntoView {
    let i18n = expect_context::<I18n>();
    let statuses = use_context::<AgentStatuses>();
    let result = RwSignal::new(None::<AgentDiagnostic>);
    let checking = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let generation = RwSignal::new(0u64);
    let check = move |selected: Document| {
        let token = match token.get_value() { Ok(token) => token, Err(message) => { error.set(message); return; } };
        let request_generation = generation.get_untracked().wrapping_add(1);
        generation.set(request_generation);
        let status_id = field(&selected, "id");
        let mut status_request = 0;
        if let Some(statuses) = statuses { statuses.0.update(|checks| status_request = checks.begin(&status_id, &selected)); }
        checking.set(true); result.set(None); error.set(String::new());
        spawn_local(async move {
            let path = format!("/api/agent-providers/{}/diagnose", field(&selected,"id"));
            let response = async { write(&token, Method::POST, &path, &serde_json::json!({"expected":selected,"statusOnly":true})).await?.json::<AgentDiagnostic>().await.map_err(|_| "message.invalid_server_response".to_owned()) }.await;
            if let Some(statuses) = statuses { statuses.0.update(|checks| checks.finish(&status_id, status_request, response.as_ref().ok().cloned())); }
            if checking.try_get_untracked().is_none() || generation.get_untracked() != request_generation || profile.get_untracked().as_ref().and_then(|value| serde_json::to_value(value).ok()) != serde_json::to_value(&selected).ok() { return; }
            checking.set(false);
            match response { Ok(diagnostic) => result.set(Some(diagnostic)), Err(message) => { error.set(message); return; } }
            let response = async { write(&token, Method::POST, &path, &serde_json::json!({"expected":selected})).await?.json::<AgentDiagnostic>().await.map_err(|_| "message.invalid_server_response".to_owned()) }.await;
            if let (Some(statuses), Ok(diagnostic)) = (statuses, &response) { statuses.0.update(|checks| checks.finish(&status_id, status_request, Some(diagnostic.clone()))); }
            if checking.try_get_untracked().is_none() || generation.get_untracked() != request_generation { return; }
            match response { Ok(diagnostic) => { result.set(Some(diagnostic)); }, Err(message) => error.set(message) }
        });
    };
    Effect::new(move |_| {
        let selected = profile.get();
        generation.update(|value| *value = value.wrapping_add(1));
        result.set(None);
        error.set(String::new());
        checking.set(false);
        if let Some(selected) = selected { check(selected); }
    });
    view! {
        <section class="task-files" hidden=move || profile.get().is_none() aria-labelledby="agent-diagnostic-heading">
            <h3 id="agent-diagnostic-heading">{move || i18n.text("message.installation_and_login")}</h3>
            <p class="field-help">{move || i18n.text("message.on_linux_macos_stopping_targets_the_process_group_a_program_that")}</p>
            <button id="diagnose-profile" type="button" disabled=move || pending.get() || checking.get() || blocked.get() || profile.get().is_none() on:click=move |_| {
                if pending.get_untracked() || blocked.get_untracked() { return; }
                let Some(profile) = profile.get_untracked() else { return; };
                check(profile);
            }>{move || i18n.text(if checking.get() { "message.checking" } else { "message.check_installation_and_login" })}</button>
            <p class="field-help" hidden=move || !blocked.get()>{move || i18n.text("message.save_or_reload_the_profile_before_checking_it")}</p>
            <p id="agent-diagnostic-error" class="error" role="alert">{move || i18n.text(&error.get())}</p>
            <div id="agent-diagnostic-result" role="status">{move || result.get().map(|diagnostic| view! {
                <p class="field-help">{move || i18n.text("message.last_check_result")}</p>
                <p><strong>{move || i18n.text("message.executable")}</strong>" : "<span id="agent-diagnostic-executable">{diagnostic.executable}</span></p>
                <p><strong>{move || i18n.text("message.version")}</strong>" : "<span id="agent-diagnostic-version">{diagnostic.version.unwrap_or_else(|| i18n.text("message.unavailable"))}</span></p>
                <p class="error">{diagnostic.version_error.map(|message| i18n.text(&message))}</p>
                <p id="agent-diagnostic-auth">{i18n.text(match diagnostic.authentication {
                    AgentAuthentication::Present=>"message.cli_login_present_service_access_has_not_been_verified",
                    AgentAuthentication::Absent=>"message.no_cli_login",
                    AgentAuthentication::Configured=>"message.key_present_validity_has_not_been_verified",
                    AgentAuthentication::MissingKey=>"message.no_saved_api_key",
                    AgentAuthentication::Unavailable=>"message.login_not_checked",
                })}</p>
                <p class="error">{diagnostic.authentication_error.map(|message| i18n.text(&message))}</p>
                {diagnostic.account.map(|account| view! { <p><strong>{move || i18n.text("message.account")}</strong>" : "{account}</p> })}
                {diagnostic.auth_method.map(|method| view! { <p><strong>{move || i18n.text("message.authentication")}</strong>" : "{method}</p> })}
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
    view! {<section id="agent-account-quotas" class="task-files" aria-label=move||i18n.text("message.last_quota_report")>
        <h4>{move||i18n.text("message.last_quota_report")}</h4>
        <p id="agent-account-observed">{move || i18n.date(observed)}</p>
        <p>{i18n.text(match report.state{State::Available=>"message.limits_reported_by_the_service_they_do_not_establish_access_to_a",State::Api=>"message.api_limits_are_separate_from_subscription_quotas",State::Disconnected=>"message.sign_in_through_the_cli_then_check_again",State::Unsupported=>"message.auth_mode_quotas_unsupported",State::Unavailable=>"message.quotas_unavailable_no_values_are_inferred",State::RateLimited=>"message.the_service_is_limiting_reads_wait_until_the_next_permitted_atte"})}</p>
        {report.retry_at.map(|date|view!{<p>{move || i18n.text("message.next_attempt_0").replace("{0}", &i18n.date(js_sys::Date::parse(&date)))}</p>})}
        {report.account.map(|account|view!{<p>{i18n.text("message.account")}" : "{account}</p>})}
        {report.subscription.map(|plan|view!{<p>{i18n.text("message.subscription")}" : "{plan}</p>})}
        <p class="field-help">{move||i18n.text("message.refresh_through_the_profile_check_the_report_expires_after_ten_m")}</p>
        <For each=move||report.windows.clone() key=|window|format!("{}:{}:{:?}:{:?}",window.name,window.kind,window.window_minutes,window.resets_at) children=move|window|{
            let stale=move||!observed.is_finite()||now.get()-observed>=600_000.0||window.resets_at.is_some_and(|date|date as f64*1000.0<=now.get());
            let label=if window.kind=="secondary"{"message.secondary_window"}else{"message.primary_window"};
            let minutes=window.window_minutes;let amount=f64::from(window.remaining_tenths)/10.0;
            let reset=window.resets_at;
            view!{<article class="task-files agent-quota-window"><h5>{window.name}</h5><p>{move || minutes.map(|minutes| i18n.text("message.quota_period").replace("{0}", &quota_duration(u64::from(minutes), &i18n.text("message.quota_day_short")))).unwrap_or_else(|| i18n.text(label))}</p>
                <p class="agent-quota-value">{move||if stale(){i18n.text("message.refresh_required")}else{i18n.text("message.0_remaining").replace("{0}",&format!("{amount:.1}"))}}</p>
                <progress max="100" value=amount hidden=stale aria-label=move||i18n.text(label)></progress>
                <p>{move || reset.map(|reset| remaining_quota_minutes(reset, now.get()).map(|minutes| i18n.text("message.quota_resets_in").replace("{0}", &quota_duration(minutes, &i18n.text("message.quota_day_short")))).unwrap_or_else(|| i18n.text("message.refresh_required"))).unwrap_or_else(||i18n.text("message.reset_time_not_reported"))}</p>
            </article>}
        }/>
    </section>}
}

fn quota_duration(minutes: u64, day: &str) -> String {
    if minutes == 0 {
        return "0 min".into();
    }
    [
        (minutes / 1440, day),
        (minutes % 1440 / 60, "h"),
        (minutes % 60, "min"),
    ]
    .into_iter()
    .filter(|(value, _)| *value > 0)
    .map(|(value, unit)| format!("{value} {unit}"))
    .collect::<Vec<_>>()
    .join(" ")
}

fn remaining_quota_minutes(reset_seconds: i64, now_milliseconds: f64) -> Option<u64> {
    let remaining = (reset_seconds as f64 * 1000.0 - now_milliseconds) / 60_000.0;
    (remaining.is_finite() && remaining > 0.0).then(|| remaining.ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::{quota_duration, remaining_quota_minutes};

    #[test]
    fn quota_periods_use_days_hours_and_minutes() {
        assert_eq!(quota_duration(10080, "j"), "7 j");
        assert_eq!(quota_duration(300, "j"), "5 h");
        assert_eq!(quota_duration(1561, "j"), "1 j 2 h 1 min");
        assert_eq!(quota_duration(2880, "d"), "2 d");
        assert_eq!(quota_duration(0, "j"), "0 min");
    }

    #[test]
    fn countdown_rounds_up_and_stops_at_reset() {
        assert_eq!(remaining_quota_minutes(3600, 0.0), Some(60));
        assert_eq!(remaining_quota_minutes(3600, 3_599_999.0), Some(1));
        assert_eq!(remaining_quota_minutes(3600, 3_600_000.0), None);
        assert_eq!(remaining_quota_minutes(3600, 3_600_001.0), None);
        assert_eq!(remaining_quota_minutes(3600, f64::NAN), None);
    }
}
