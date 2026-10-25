use crate::Result;
use atelier_core::{contracts::TempoPayload, document::Document};
use reqwest::{
    Url,
    header::{AUTHORIZATION, HeaderValue},
};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};

pub struct Tempo {
    client: reqwest::Client,
    base: Url,
    token: String,
    authorization: HeaderValue,
    last_request: tokio::sync::Mutex<Option<tokio::time::Instant>>,
}

fn confidential(value: &Value, token: &str) -> bool {
    match value {
        Value::String(value) => value.contains(token),
        Value::Array(values) => values.iter().any(|value| confidential(value, token)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(token) || confidential(value, token)),
        _ => false,
    }
}

pub fn validate(payload: &TempoPayload) -> Result<()> {
    if payload.issue_id == 0
        || payload.issue_id > 9_007_199_254_740_991
        || payload.author_account_id.is_empty()
        || payload.author_account_id.len() > 300
        || payload
            .author_account_id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
        || !(1..=86400).contains(&payload.time_spent_seconds)
        || payload.description.is_empty()
        || payload.description.encode_utf16().count() > 4500
        || payload
            .description
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("Données de temps Tempo invalides.".into());
    }
    let fields: Document = serde_json::from_str(&serde_json::json!({"seconds":payload.time_spent_seconds,"date":payload.start_date,"startTime":payload.start_time,"description":"Date validation"}).to_string())?;
    atelier_core::domain::time::time_fields(&fields)?;
    Ok(())
}

fn identifier(value: &Value) -> Result<u64> {
    value["tempoWorklogId"]
        .as_u64()
        .filter(|id| *id > 0 && *id <= i64::MAX as u64)
        .ok_or_else(|| "Identité de journal Tempo invalide.".into())
}

fn matches(payload: &TempoPayload, row: &Value) -> bool {
    row["issue"]["id"].as_u64() == Some(payload.issue_id)
        && row["author"]["accountId"].as_str() == Some(payload.author_account_id.as_str())
        && row["timeSpentSeconds"].as_u64() == Some(u64::from(payload.time_spent_seconds))
        && row["startDate"].as_str() == Some(payload.start_date.as_str())
        && row["startTime"].as_str() == Some(payload.start_time.as_str())
        && row["description"].as_str() == Some(payload.description.as_str())
}

impl Tempo {
    pub fn new(base: &str, token: String) -> Result<Self> {
        if !super::BASES.contains(&base) {
            return Err("Site Tempo non autorisé.".into());
        }
        Self::prepare(Url::parse(base)?, token, false)
    }
    /// Local synthetic HTTP doubles only; the application never uses this constructor.
    pub fn synthetic(base: &str, token: String) -> Result<Self> {
        let base = Url::parse(base)?;
        if base.scheme() != "http"
            || !base
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|ip| ip.is_loopback())
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !token.starts_with("synthetic-")
        {
            return Err("Serveur Tempo synthétique loopback requis.".into());
        }
        Self::prepare(base, token, true)
    }
    fn prepare(base: Url, token: String, synthetic: bool) -> Result<Self> {
        if token.is_empty()
            || token.len() > 4000
            || token.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err("Jeton Tempo invalide.".into());
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "Jeton Tempo invalide.")?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .https_only(!synthetic)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            client,
            base,
            token,
            authorization,
            last_request: tokio::sync::Mutex::new(None),
        })
    }
    fn endpoint(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        let mut path = url
            .path_segments_mut()
            .map_err(|_| "Adresse Tempo invalide.")?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
        drop(path);
        Ok(url)
    }
    async fn request(&self, url: Url, body: Option<&TempoPayload>) -> Result<(Value, usize)> {
        let mut previous = self.last_request.lock().await;
        if let Some(previous) = *previous {
            tokio::time::sleep_until(previous + Duration::from_secs(1)).await;
        }
        *previous = Some(tokio::time::Instant::now());
        drop(previous);
        let request = if let Some(body) = body {
            self.client.post(url).json(body)
        } else {
            self.client.get(url)
        };
        let mut response = request
            .header(AUTHORIZATION, self.authorization.clone())
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| "Connexion Tempo interrompue ou indisponible.")?;
        if !response.status().is_success() {
            return Err(format!("Tempo : HTTP {}. Vérifie les droits et les attributs requis ; aucun nouvel envoi automatique.",response.status().as_u16()).into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Lecture Tempo interrompue.")?
        {
            if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                return Err("Réponse Tempo trop volumineuse.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Réponse JSON Tempo invalide.")?;
        if confidential(&value, &self.token) {
            return Err("Réponse Tempo contenant des données confidentielles inattendues.".into());
        }
        Ok((value, bytes.len()))
    }
    pub fn validate(&self, payload: &TempoPayload) -> Result<()> {
        validate(payload)?;
        if confidential(&serde_json::to_value(payload)?, &self.token) {
            return Err("Le temps contient des données confidentielles inattendues.".into());
        }
        Ok(())
    }
    /// One attempt only. Persist the returned ID before any subsequent read.
    pub async fn create(&self, payload: &TempoPayload) -> Result<u64> {
        self.validate(payload)?;
        let (row, _) = self
            .request(self.endpoint(&["worklogs"])?, Some(payload))
            .await?;
        identifier(&row)
    }
    pub async fn read(&self, id: u64, payload: &TempoPayload) -> Result<u64> {
        self.validate(payload)?;
        if id == 0 || id > i64::MAX as u64 {
            return Err("Identité de journal Tempo invalide.".into());
        }
        let (row, _) = self
            .request(self.endpoint(&["worklogs", &id.to_string()])?, None)
            .await?;
        if identifier(&row)? != id || !matches(payload, &row) {
            return Err("Le journal Tempo ne correspond pas aux données de l’envoi.".into());
        }
        Ok(id)
    }
    pub async fn find(&self, entry_id: &str, payload: &TempoPayload) -> Result<Option<u64>> {
        self.validate(payload)?;
        if uuid::Uuid::parse_str(entry_id).is_err() {
            return Err("Identité de temps locale invalide.".into());
        }
        let marker = format!("[atelier:{entry_id}]");
        if !payload.description.ends_with(&marker) {
            return Err("Marqueur d’export Tempo absent.".into());
        }
        let mut ids = BTreeSet::new();
        let mut found = None;
        let mut offset = 0usize;
        let mut size = 0usize;
        for _ in 0..100 {
            let mut url = self.endpoint(&["worklogs", "user", &payload.author_account_id])?;
            url.query_pairs_mut()
                .append_pair("from", &payload.start_date)
                .append_pair("to", &payload.start_date)
                .append_pair("limit", "100")
                .append_pair("offset", &offset.to_string());
            let (value, bytes) = self.request(url, None).await?;
            size = size
                .checked_add(bytes)
                .ok_or("Pagination Tempo trop volumineuse.")?;
            if size > 8 * 1024 * 1024 {
                return Err("Pagination Tempo trop volumineuse.".into());
            }
            let rows = value["results"]
                .as_array()
                .filter(|rows| rows.len() <= 100)
                .ok_or("Page Tempo invalide.")?;
            if rows.is_empty() {
                if value["metadata"]["next"]
                    .as_str()
                    .is_some_and(|next| !next.is_empty())
                {
                    return Err("Pagination Tempo incohérente.".into());
                }
                return Ok(found);
            }
            for row in rows {
                let id = identifier(row)?;
                if !ids.insert(id) {
                    return Err("Pagination Tempo répétée ; résultat incomplet refusé.".into());
                }
                if row["description"]
                    .as_str()
                    .is_some_and(|description| description.contains(&marker))
                {
                    if !matches(payload, row) || found.is_some() {
                        return Err("Plusieurs journaux ou données différentes portent le marqueur Tempo. Vérifie l’envoi sans le rejouer.".into());
                    }
                    found = Some(id);
                }
            }
            offset += rows.len();
        }
        Err("Trop de pages Tempo pour qualifier le rapprochement.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_and_nested_secrets_are_rejected_after_json_decoding() {
        let value: Value =
            serde_json::from_str(r#"{"outer":[{"nested":"\u0073ynthetic-token"}]}"#).unwrap();
        assert!(confidential(&value, "synthetic-token"));
        let value: Value =
            serde_json::from_str(r#"{"\u0073ynthetic-token":"not a public key"}"#).unwrap();
        assert!(confidential(&value, "synthetic-token"));
        assert!(!confidential(
            &serde_json::json!({"description":"Public synthetic data"}),
            "synthetic-token"
        ));
    }
}
