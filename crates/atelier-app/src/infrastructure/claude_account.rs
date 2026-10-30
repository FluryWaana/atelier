use crate::Result;
use atelier_core::{
    application::agent_account::{self, Report, State, Window},
    document::Document,
};
use reqwest::{
    Url,
    header::{AUTHORIZATION, HeaderValue},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};
use time::{
    OffsetDateTime,
    format_description::well_known::{Rfc2822, Rfc3339},
};
use tokio::{sync::Mutex, time::Instant};
struct Cached {
    until: Instant,
    report: Report,
}
static CACHE: Mutex<BTreeMap<[u8; 32], Cached>> = Mutex::const_new(BTreeMap::new());
fn now() -> Result<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}
fn token_valid(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 16_384
        && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}
fn normalize(value: &Value) -> Result<Vec<Window>> {
    let values = value.as_object().ok_or("Relevé Claude invalide.")?;
    let mut buckets = serde_json::Map::new();
    for (key, quota) in values {
        let (kind, minutes, suffix) = if let Some(suffix) = key.strip_prefix("five_hour") {
            ("primary", 300, suffix)
        } else if let Some(suffix) = key.strip_prefix("seven_day") {
            ("secondary", 10080, suffix)
        } else {
            continue;
        };
        if !suffix.is_empty()
            && (!suffix.starts_with('_')
                || suffix.len() < 2
                || suffix.len() > 41
                || !suffix[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'))
        {
            continue;
        }
        if quota.is_null() {
            continue;
        }
        if buckets.len() >= 20 {
            return Err("Trop de fenêtres de quotas Claude.".into());
        }
        let resets = if quota["resets_at"].is_null() {
            None
        } else {
            Some(
                OffsetDateTime::parse(
                    quota["resets_at"]
                        .as_str()
                        .filter(|text| text.len() <= 40)
                        .ok_or("Date de quota Claude invalide.")?,
                    &Rfc3339,
                )
                .map_err(|_| "Date de quota Claude invalide.")?
                .unix_timestamp(),
            )
        };
        let name = if suffix.is_empty() {
            "Claude".into()
        } else {
            suffix[1..].replace('_', " ")
        };
        let mut bucket = json!({"limitName":name});
        bucket[kind] = json!({"usedPercent":quota["utilization"],"windowDurationMins":minutes,"resetsAt":resets});
        buckets.insert(key.clone(), bucket);
    }
    Ok(agent_account::windows(
        &json!({"rateLimitsByLimitId":buckets}),
    )?)
}
pub struct Client {
    url: Url,
    client: reqwest::Client,
}
impl Client {
    fn prepare(url: Url, synthetic: bool) -> Result<Self> {
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || url.path() != "/api/oauth/usage"
            || if synthetic {
                url.scheme() != "http"
                    || !url.host_str().is_some_and(|host| {
                        host.parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                    })
            } else {
                url.scheme() != "https" || url.host_str() != Some("api.anthropic.com")
            }
        {
            return Err("Origine des quotas Claude invalide.".into());
        }
        Ok(Self {
            url,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .https_only(!synthetic)
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }
    fn production() -> Result<Self> {
        Self::prepare(
            Url::parse("https://api.anthropic.com/api/oauth/usage")?,
            false,
        )
    }
    pub fn synthetic(origin: &str) -> Result<Self> {
        Self::prepare(Url::parse(origin)?, true)
    }
    pub async fn read(&self, token: &str) -> Result<Report> {
        if !token_valid(token) {
            return Err("Jeton de quotas Claude invalide.".into());
        }
        let mut hash = Sha256::new();
        hash.update(self.url.as_str());
        hash.update([0]);
        hash.update(token.as_bytes());
        let key: [u8; 32] = hash.finalize().into();
        // Diagnostics are already serialized; this bounded cache also deduplicates aliases of one login.
        let mut cache = CACHE.lock().await;
        let at = Instant::now();
        cache.retain(|_, entry| entry.until > at);
        if let Some(entry) = cache.get(&key) {
            return Ok(entry.report.clone());
        }
        if cache.len() >= 100 {
            return Ok(Report::unavailable(State::Unavailable));
        }
        let (mut report, wait) = self.fetch(token).await.unwrap_or_else(|_| {
            (
                Report::unavailable(State::Unavailable),
                Duration::from_secs(300),
            )
        });
        report.observed_at = now()?;
        cache.insert(
            key,
            Cached {
                until: Instant::now()
                    .checked_add(wait)
                    .ok_or("Délai de quotas Claude hors limites.")?,
                report: report.clone(),
            },
        );
        Ok(report)
    }
    async fn fetch(&self, token: &str) -> Result<(Report, Duration)> {
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))?;
        authorization.set_sensitive(true);
        // shortcut: this read-only endpoint is undocumented; requalify when Claude changes its contract.
        let mut response = self
            .client
            .get(self.url.clone())
            .header(AUTHORIZATION, authorization)
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| "Lecture des quotas Claude interrompue.")?;
        if response.status().as_u16() == 429 {
            let date = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| {
                    value
                        .parse::<u32>()
                        .ok()
                        .map(|seconds| {
                            OffsetDateTime::now_utc() + time::Duration::seconds(i64::from(seconds))
                        })
                        .or_else(|| OffsetDateTime::parse(value, &Rfc2822).ok())
                });
            let seconds = date
                .map(|date| (date - OffsetDateTime::now_utc()).whole_nanoseconds())
                .filter(|value| *value > 0)
                .map(|value| ((value + 999_999_999) / 1_000_000_000) as i64)
                .unwrap_or(900)
                .max(300);
            let mut report = Report::unavailable(State::RateLimited);
            report.retry_at = Some(
                (OffsetDateTime::now_utc() + time::Duration::seconds(seconds)).format(&Rfc3339)?,
            );
            return Ok((report, Duration::from_secs(seconds as u64)));
        }
        if matches!(response.status().as_u16(), 401 | 403) {
            return Ok((
                Report::unavailable(State::Disconnected),
                Duration::from_secs(300),
            ));
        }
        if response.status().as_u16() != 200 {
            return Err("Quotas Claude indisponibles.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Réponse de quotas Claude interrompue.")?
        {
            if bytes.len().saturating_add(chunk.len()) > 65_536 {
                return Err("Réponse de quotas Claude trop volumineuse.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Réponse de quotas Claude invalide.")?;
        if super::confidential::reflected(&value, &[token]) {
            return Err("Secret réfléchi dans les quotas Claude.".into());
        }
        let windows = normalize(&value)?;
        let mut report = Report::unavailable(if windows.is_empty() {
            State::Unavailable
        } else {
            State::Available
        });
        report.windows = windows;
        Ok((report, Duration::from_secs(300)))
    }
}
fn credential(environment: &BTreeMap<String, OsString>) -> Result<Value> {
    let root = environment
        .get("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get("USERPROFILE")
                .or_else(|| environment.get("HOME"))
                .map(|home| PathBuf::from(home).join(".claude"))
        })
        .ok_or("Dossier du profil Claude absent.")?;
    let root = super::repository_access::absolute(&root)?;
    let bytes = super::checked_file::CheckedFile::open_in(&root, Path::new(".credentials.json"))?
        .single_link()?
        .read(65_536)?;
    serde_json::from_slice(&bytes).map_err(|_| "Identifiants du profil Claude illisibles.".into())
}
pub(super) async fn from_profile(
    profile: &Document,
    environment: BTreeMap<String, OsString>,
) -> Result<Report> {
    if profile.string_equals("auth", "api")?
        || ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]
            .iter()
            .any(|key| environment.get(*key).is_some_and(|value| !value.is_empty()))
    {
        return Ok(Report::unavailable(State::Api));
    }
    let token = if let Some(token) = environment.get("CLAUDE_CODE_OAUTH_TOKEN") {
        token.to_str().unwrap_or_default().to_owned()
    } else {
        let value = tokio::task::spawn_blocking(move || credential(&environment)).await??;
        let oauth = &value["claudeAiOauth"];
        if !oauth["expiresAt"].is_null()
            && !oauth["expiresAt"].as_i64().is_some_and(|expires| {
                i128::from(expires) > OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000
            })
        {
            return Ok(Report::unavailable(State::Disconnected));
        }
        oauth["accessToken"].as_str().unwrap_or_default().to_owned()
    };
    if !token_valid(&token) {
        return Ok(Report::unavailable(State::Disconnected));
    }
    Client::production()?.read(&token).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn credentials_remain_in_the_selected_directory_and_are_never_refreshed_or_rewritten() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".credentials.json");
        let bytes=serde_json::to_vec(&json!({"claudeAiOauth":{"accessToken":"synthetic-access","refreshToken":"never-use-refresh","expiresAt":0}})).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let environment = BTreeMap::from([(
            "CLAUDE_CONFIG_DIR".into(),
            root.path().as_os_str().to_owned(),
        )]);
        assert_eq!(
            credential(&environment).unwrap()["claudeAiOauth"]["accessToken"],
            "synthetic-access"
        );
        let profile: Document = serde_json::from_str(r#"{"type":"claude","auth":"cli"}"#).unwrap();
        let api: Document = serde_json::from_str(r#"{"type":"claude","auth":"api"}"#).unwrap();
        assert_eq!(
            from_profile(&api, BTreeMap::new()).await.unwrap().state,
            State::Api
        );
        assert!(
            credential(&BTreeMap::from([(
                "CLAUDE_CONFIG_DIR".into(),
                OsString::from(r"\\synthetic-server\share")
            )]))
            .is_err()
        );
        assert_eq!(
            from_profile(&profile, environment.clone())
                .await
                .unwrap()
                .state,
            State::Disconnected
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let link = root.path().join("synthetic-link");
        std::fs::hard_link(&path, &link).unwrap();
        assert!(credential(&environment).is_err());
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&path, vec![b'x'; 65_537]).unwrap();
        assert!(credential(&environment).is_err());
        assert!(!token_valid("synthetic\r\nheader"));
    }
}
