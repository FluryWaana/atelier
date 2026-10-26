use super::{settings, store::Store};
use crate::Result;
use atelier_core::{
    application::jira,
    contracts::{
        JiraConfiguration, JiraConfigurationUpdate, JiraConnection, JiraCreationField,
        JiraCreationMetadata, JiraIssue, JiraIssueDetail, JiraIssueType, JiraMode, JiraSearch,
        JiraStatus, JiraTransition, JiraTransitions,
    },
    document::Document,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

pub fn normalize_url(value: &str) -> std::result::Result<String, &'static str> {
    if value.len() > 2000 {
        return Err("Adresse Jira trop longue.");
    }
    let url = Url::parse(value).map_err(|_| "Adresse Jira HTTPS invalide.")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Adresse Jira HTTPS requise, sans identifiants ni paramètres.");
    }
    Ok(url.as_str().trim_end_matches('/').into())
}

pub fn describe(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<JiraConnection> {
    let configuration = store
        .settings::<Document>()?
        .get::<JiraConfiguration>("jiraConnection")?
        .unwrap_or_default();
    let tokens = settings::load(store, protect)?;
    Ok(JiraConnection {
        configuration,
        token_set: tokens
            .get::<String>("jira")?
            .is_some_and(|token| !token.is_empty()),
    })
}

pub fn configure(
    store: &Store<'_>,
    input: JiraConfigurationUpdate,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<JiraConnection> {
    let mut stored = store.settings::<Document>()?;
    let previous = stored
        .get::<JiraConfiguration>("jiraConnection")?
        .unwrap_or_default();
    let (configuration, token) = jira::configure(&previous, input, normalize_url)?;
    let mut tokens = settings::load(store, protect)?;
    if let Some(token) = token {
        tokens.set("jira", &token)?;
    }
    if configuration.url != previous.url
        || configuration.email != previous.email
        || configuration.mode != previous.mode
    {
        tokens.set("tempo", &"")?;
        stored.set("jiraAutoTransition", &false)?;
        super::tempo::invalidate(&mut stored, None)?;
    }
    stored.set("jiraConnection", &configuration)?;
    let result = JiraConnection {
        configuration,
        token_set: tokens
            .get::<String>("jira")?
            .is_some_and(|token| !token.is_empty()),
    };
    settings::persist(
        store,
        &atelier_core::application::settings::SettingsUpdate {
            settings: stored,
            tokens,
        },
        protect,
    )?;
    Ok(result)
}

pub fn credentials(
    store: &Store<'_>,
    expected: u64,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<(JiraConfiguration, String)> {
    let configuration = store
        .settings::<Document>()?
        .get::<JiraConfiguration>("jiraConnection")?
        .unwrap_or_default();
    if configuration.revision != expected {
        return Err(jira::JiraFailure::Conflict.into());
    }
    if configuration.url.is_empty() {
        return Err("Renseigne l’adresse Jira avant la lecture.".into());
    }
    let token = settings::load(store, protect)?
        .get::<String>("jira")?
        .filter(|token| !token.is_empty())
        .ok_or("Renseigne le jeton Jira avant la recherche.")?;
    Ok((configuration, token))
}

pub fn require_revision(store: &Store<'_>, expected: u64) -> Result<()> {
    if store
        .settings::<Document>()?
        .get::<JiraConfiguration>("jiraConnection")?
        .unwrap_or_default()
        .revision
        != expected
    {
        return Err(jira::JiraFailure::Conflict.into());
    }
    Ok(())
}

pub(super) fn invalidate_configuration(
    settings: &mut Document,
    minimum: Option<u64>,
) -> Result<bool> {
    let current = settings.get::<JiraConfiguration>("jiraConnection")?;
    if current.is_none() && minimum.is_none() {
        return Ok(false);
    }
    let mut configuration = current.unwrap_or_default();
    configuration.revision = configuration
        .revision
        .max(minimum.unwrap_or(0))
        .checked_add(1)
        .ok_or("Révision Jira épuisée.")?;
    settings.set("jiraConnection", &configuration)?;
    Ok(true)
}

pub struct Jira {
    client: reqwest::Client,
    base: Url,
    configuration: JiraConfiguration,
    authorization: HeaderValue,
    token: String,
    gateway: tokio::sync::OnceCell<Url>,
}

impl Jira {
    pub async fn account_id(&self) -> Result<String> {
        if self.configuration.mode != JiraMode::Cloud {
            return Err("L’export Tempo nécessite Jira Cloud.".into());
        }
        let row = self
            .request(self.endpoint(&["rest", "api", "3", "myself"])?, None)
            .await?;
        let account = row["accountId"]
            .as_str()
            .filter(|account| {
                !account.is_empty()
                    && account.len() <= 300
                    && !account.chars().any(|c| c.is_control() || c.is_whitespace())
                    && !account.contains(&self.token)
            })
            .ok_or("Identifiant du compte Jira indisponible.")?;
        Ok(account.into())
    }
    pub fn new(configuration: JiraConfiguration, token: String) -> Result<Self> {
        let base = Url::parse(&normalize_url(&configuration.url)?)?;
        Self::prepare(configuration, token, base, false)
    }
    /// Explicit constructor for loopback doubles; the HTTP application never calls this path.
    pub fn synthetic(configuration: JiraConfiguration, token: String) -> Result<Self> {
        let base = Url::parse(&configuration.url)?;
        let loopback = base
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|host| host.is_loopback());
        if base.scheme() != "http"
            || !loopback
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !token.starts_with("synthetic-")
        {
            return Err("Serveur de test Jira synthétique requis.".into());
        }
        Self::prepare(configuration, token, base, true)
    }
    fn prepare(
        configuration: JiraConfiguration,
        token: String,
        base: Url,
        synthetic: bool,
    ) -> Result<Self> {
        if token.is_empty()
            || token.len() > 4000
            || token.chars().any(char::is_control)
            || configuration.email.len() > 300
            || configuration.email.chars().any(char::is_control)
            || (configuration.mode == JiraMode::Cloud
                && (configuration.email.is_empty()
                    || !configuration.email.contains('@')
                    || configuration.email.contains(':')))
        {
            return Err("Identifiants Jira invalides.".into());
        }
        let authorization = match configuration.mode {
            JiraMode::Cloud => format!(
                "Basic {}",
                STANDARD.encode(format!("{}:{token}", configuration.email))
            ),
            JiraMode::DataCenter => format!("Bearer {token}"),
        };
        let mut authorization =
            HeaderValue::from_str(&authorization).map_err(|_| "Identifiants Jira invalides.")?;
        authorization.set_sensitive(true);
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .https_only(!synthetic);
        if synthetic {
            builder = builder.no_proxy();
        }
        Ok(Self {
            client: builder.build()?,
            base,
            configuration,
            authorization,
            token,
            gateway: tokio::sync::OnceCell::new(),
        })
    }
    fn endpoint(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| "Adresse Jira invalide.")?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(url)
    }
    async fn send(&self, url: Url, body: Option<&Value>) -> Result<reqwest::Response> {
        let request = if let Some(body) = body {
            self.client.post(url).json(body)
        } else {
            self.client.get(url)
        };
        request
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "Jira ne répond pas dans le délai prévu."
                } else {
                    "Connexion Jira impossible."
                }
            })
            .map_err(Into::into)
    }
    async fn request(&self, url: Url, body: Option<&Value>) -> Result<Value> {
        let target = if let Some(gateway) = self.gateway.get() {
            gateway_request(gateway, &url)?
        } else {
            url.clone()
        };
        let mut response = self.send(target, body).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            && self.configuration.mode == JiraMode::Cloud
            && self.gateway.get().is_none()
            && tenant_site(&self.base)
        {
            drop(response);
            let tenant = self
                .client
                .get(self.endpoint(&["_edge", "tenant_info"])?)
                .header(ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| "Identification du site Jira impossible.")?;
            let info = read_json(tenant, 64 * 1024).await?;
            let id = info["cloudId"]
                .as_str()
                .filter(|id| id.len() == 36 && !id.contains(&self.token))
                .ok_or("Identifiant du site Jira invalide.")?;
            let gateway = gateway_base(id)?;
            let _ = self.gateway.set(gateway.clone());
            response = self.send(gateway_request(&gateway, &url)?, body).await?;
        }
        read_json(response, 4 * 1024 * 1024).await
    }
    fn issue(&self, row: &Value) -> Result<JiraIssue> {
        let id = row["id"].as_str().ok_or("Identifiant Jira invalide.")?;
        let key = row["key"].as_str().ok_or("Clé Jira invalide.")?;
        if !jira::issue_identity(id, key) {
            return Err("La pagination Jira est incohérente. Réessaie la recherche.".into());
        }
        let title = row["fields"]["summary"]
            .as_str()
            .filter(|title| {
                !title.trim().is_empty()
                    && title.len() <= 1200
                    && !title.chars().any(char::is_control)
            })
            .ok_or("Titre Jira invalide.")?;
        let status = row["fields"]["status"]["name"]
            .as_str()
            .filter(|status| status.len() <= 400 && !status.chars().any(char::is_control))
            .ok_or("Statut Jira invalide.")?;
        let status_id = row["fields"]["status"]["id"]
            .as_str()
            .filter(|id| id.len() <= 100 && id.bytes().all(|byte| byte.is_ascii_digit()))
            .unwrap_or_default();
        if [id, key, title, status, status_id]
            .iter()
            .any(|value| value.contains(&self.token))
        {
            return Err("La réponse Jira contient des données confidentielles inattendues.".into());
        }
        let issue = JiraIssue {
            id: id.into(),
            key: key.into(),
            title: title.into(),
            status: status.into(),
            status_id: status_id.into(),
            url: self.endpoint(&["browse", key])?.to_string(),
        };
        if issue.url.contains(&self.token) {
            return Err("La réponse Jira contient des données confidentielles inattendues.".into());
        }
        Ok(issue)
    }
    pub async fn detail(&self, id: &str) -> Result<JiraIssueDetail> {
        if id.is_empty() || id.len() > 32 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Identifiant Jira invalide.".into());
        }
        let version = if self.configuration.mode == JiraMode::Cloud {
            "3"
        } else {
            "2"
        };
        let mut url = self.endpoint(&["rest", "api", version, "issue", id])?;
        url.query_pairs_mut()
            .append_pair("fields", "summary,status,description");
        let row = tokio::time::timeout(Duration::from_secs(60), self.request(url, None))
            .await
            .map_err(|_| "Lecture du ticket Jira trop longue.")??;
        let issue = self.issue(&row)?;
        if issue.id != id {
            return Err("Identifiant du ticket Jira incohérent.".into());
        }
        let description = jira::description(
            row["fields"]
                .get("description")
                .ok_or("Description Jira absente.")?,
        )?;
        if description.contains(&self.token) {
            return Err("La réponse Jira contient des données confidentielles inattendues.".into());
        }
        Ok(JiraIssueDetail {
            connection_revision: self.configuration.revision,
            issue,
            description,
        })
    }
    fn issue_endpoint(&self, id: &str, suffix: Option<&str>) -> Result<Url> {
        if id.is_empty() || id.len() > 32 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Identifiant Jira invalide.".into());
        }
        let version = if self.configuration.mode == JiraMode::Cloud {
            "3"
        } else {
            "2"
        };
        let mut url = self.endpoint(&["rest", "api", version, "issue", id])?;
        if let Some(suffix) = suffix {
            url.path_segments_mut()
                .map_err(|_| "Adresse Jira invalide.")?
                .push(suffix);
        }
        Ok(url)
    }
    fn status_value(&self, value: &Value) -> Result<JiraStatus> {
        let id = value["id"]
            .as_str()
            .filter(|id| {
                !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_digit())
            })
            .ok_or("Identifiant de statut Jira invalide.")?;
        let name = value["name"]
            .as_str()
            .filter(|name| {
                !name.is_empty() && name.len() <= 400 && !name.chars().any(char::is_control)
            })
            .ok_or("Statut Jira invalide.")?;
        if [id, name].iter().any(|value| value.contains(&self.token)) {
            return Err("La réponse Jira contient des données confidentielles inattendues.".into());
        }
        Ok(JiraStatus {
            id: id.into(),
            name: name.into(),
        })
    }
    pub async fn status(&self, id: &str) -> Result<JiraStatus> {
        let mut url = self.issue_endpoint(id, None)?;
        url.query_pairs_mut().append_pair("fields", "status");
        let row = self.request(url, None).await?;
        if row["id"].as_str() != Some(id) {
            return Err("Identifiant du ticket Jira incohérent.".into());
        }
        self.status_value(&row["fields"]["status"])
    }
    pub async fn transitions(&self, id: &str) -> Result<JiraTransitions> {
        let current = self.status(id).await?;
        let mut url = self.issue_endpoint(id, Some("transitions"))?;
        url.query_pairs_mut()
            .append_pair("expand", "transitions.fields");
        let value = self.request(url, None).await?;
        let rows = value["transitions"]
            .as_array()
            .filter(|rows| rows.len() <= 100)
            .ok_or("Liste de transitions Jira invalide.")?;
        let mut transitions = Vec::new();
        let mut ids = BTreeSet::new();
        for row in rows {
            let id = row["id"]
                .as_str()
                .filter(|id| {
                    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_digit())
                })
                .ok_or("Transition Jira invalide.")?;
            let name = row["name"]
                .as_str()
                .filter(|name| {
                    !name.is_empty() && name.len() <= 400 && !name.chars().any(char::is_control)
                })
                .ok_or("Transition Jira invalide.")?;
            if !ids.insert(id) || [id, name].iter().any(|value| value.contains(&self.token)) {
                return Err("Liste de transitions Jira invalide.".into());
            }
            let fields = row["fields"]
                .as_object()
                .filter(|fields| fields.len() <= 100)
                .ok_or("Champs de transition Jira absents ou invalides.")?;
            let mut requires_fields = false;
            for field in fields.values() {
                let required = field["required"]
                    .as_bool()
                    .ok_or("Champs de transition Jira invalides.")?;
                requires_fields |= required && field["hasDefaultValue"].as_bool() != Some(true);
            }
            transitions.push(JiraTransition {
                id: id.into(),
                name: name.into(),
                target: self.status_value(&row["to"])?,
                requires_fields,
            });
        }
        Ok(JiraTransitions {
            connection_revision: self.configuration.revision,
            current,
            transitions,
        })
    }
    /// No retry or gateway discovery after a write: a lost response may hide a committed effect.
    pub async fn send_transition(&self, issue: &str, transition: &JiraTransition) -> Result<()> {
        if transition.requires_fields
            || transition.id.is_empty()
            || transition.id.len() > 64
            || !transition.id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err("Transition Jira invalide ou exigeant des champs supplémentaires.".into());
        }
        let url = self.issue_endpoint(issue, Some("transitions"))?;
        let target = if let Some(gateway) = self.gateway.get() {
            gateway_request(gateway, &url)?
        } else {
            url
        };
        let response = self
            .send(target, Some(&json!({"transition":{"id":transition.id}})))
            .await?;
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(
                "Envoi Jira non confirmé. Vérifie le ticket avant toute nouvelle tentative.".into(),
            );
        }
        Ok(())
    }
    fn creation_endpoint(&self, project: &str, issue_type: Option<&str>) -> Result<Url> {
        if !jira::project_key(project) {
            return Err("Clé de projet Jira invalide.".into());
        }
        let version = if self.configuration.mode == JiraMode::Cloud {
            "3"
        } else {
            "2"
        };
        let mut url = self.endpoint(&[
            "rest",
            "api",
            version,
            "issue",
            "createmeta",
            project,
            "issuetypes",
        ])?;
        if let Some(id) = issue_type {
            if id.is_empty() || id.len() > 64 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("Type de ticket Jira invalide.".into());
            }
            url.path_segments_mut()
                .map_err(|_| "Adresse Jira invalide.")?
                .push(id);
        }
        Ok(url)
    }
    fn public_metadata_text<'a>(&self, value: &'a Value, limit: usize) -> Result<&'a str> {
        value
            .as_str()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= limit
                    && !value.chars().any(char::is_control)
                    && !value.contains(&self.token)
            })
            .ok_or_else(|| "Métadonnées Jira invalides ou confidentielles.".into())
    }
    async fn metadata_pages<T>(
        &self,
        url: Url,
        row_key: &str,
        id_key: &str,
        limit: usize,
        parse: impl Fn(&Value) -> Result<T>,
    ) -> Result<Vec<T>> {
        let mut result = Vec::new();
        let mut ids = BTreeSet::new();
        let mut total = None;
        for _ in 0..100 {
            let mut target = url.clone();
            target
                .query_pairs_mut()
                .append_pair("startAt", &result.len().to_string())
                .append_pair("maxResults", "100");
            let page = self.request(target, None).await?;
            let expected = page["total"]
                .as_u64()
                .filter(|total| *total <= limit as u64)
                .ok_or("Métadonnées Jira trop volumineuses ou invalides.")?;
            if total.is_some_and(|total| total != expected)
                || page["startAt"].as_u64() != Some(result.len() as u64)
            {
                return Err("Pagination des métadonnées Jira incohérente.".into());
            }
            total = Some(expected);
            let rows = page[row_key]
                .as_array()
                .filter(|rows| rows.len() <= 100)
                .ok_or("Page de métadonnées Jira invalide.")?;
            for row in rows {
                let id = self.public_metadata_text(&row[id_key], 128)?;
                if !ids.insert(id.to_owned()) || result.len() >= limit {
                    return Err("Métadonnées Jira dupliquées ou trop volumineuses.".into());
                }
                result.push(parse(row)?);
            }
            if let Some(last) = page.get("isLast") {
                if last.as_bool() != Some(result.len() as u64 == expected) {
                    return Err("Pagination des métadonnées Jira incohérente.".into());
                }
            }
            if result.len() as u64 == expected {
                return Ok(result);
            }
            if result.len() as u64 > expected || rows.is_empty() {
                return Err("Pagination des métadonnées Jira incohérente.".into());
            }
        }
        Err("Métadonnées Jira trop volumineuses.".into())
    }
    pub async fn issue_types(&self, project: &str) -> Result<Vec<JiraIssueType>> {
        tokio::time::timeout(Duration::from_secs(60), self.issue_types_pages(project))
            .await
            .map_err(|_| "Lecture des métadonnées Jira trop longue.")?
    }
    async fn issue_types_pages(&self, project: &str) -> Result<Vec<JiraIssueType>> {
        let url = self.creation_endpoint(project, None)?;
        let key = if self.configuration.mode == JiraMode::Cloud {
            "issueTypes"
        } else {
            "values"
        };
        self.metadata_pages(url, key, "id", 1000, |row| {
            let id = self.public_metadata_text(&row["id"], 64)?;
            if !id.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("Type de ticket Jira invalide.".into());
            }
            Ok(JiraIssueType {
                id: id.into(),
                name: self.public_metadata_text(&row["name"], 400)?.into(),
                subtask: row["subtask"]
                    .as_bool()
                    .ok_or("Type de ticket Jira invalide.")?,
            })
        })
        .await
    }
    pub async fn creation_metadata(
        &self,
        project: &str,
        issue_type: &str,
    ) -> Result<JiraCreationMetadata> {
        tokio::time::timeout(
            Duration::from_secs(60),
            self.creation_metadata_pages(project, issue_type),
        )
        .await
        .map_err(|_| "Lecture des métadonnées Jira trop longue.")?
    }
    async fn creation_metadata_pages(
        &self,
        project: &str,
        issue_type: &str,
    ) -> Result<JiraCreationMetadata> {
        let selected = self
            .issue_types(project)
            .await?
            .into_iter()
            .find(|kind| kind.id == issue_type && !kind.subtask)
            .ok_or("Type de ticket Jira indisponible ou sous-tâche non prise en charge.")?;
        let url = self.creation_endpoint(project, Some(issue_type))?;
        let key = if self.configuration.mode == JiraMode::Cloud {
            "fields"
        } else {
            "values"
        };
        let fields = self
            .metadata_pages(url, key, "fieldId", 2000, |row| {
                let id = self.public_metadata_text(&row["fieldId"], 128)?;
                if !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    return Err("Champ de création Jira invalide.".into());
                }
                let operations = row["operations"]
                    .as_array()
                    .filter(|operations| {
                        operations.len() <= 20
                            && operations.iter().all(|operation| {
                                operation
                                    .as_str()
                                    .is_some_and(|operation| operation.len() <= 50)
                            })
                    })
                    .ok_or("Opérations de champ Jira invalides.")?;
                let has_default = match row.get("hasDefaultValue") {
                    None => false,
                    Some(value) => value
                        .as_bool()
                        .ok_or("Valeur par défaut de champ Jira invalide.")?,
                };
                Ok(JiraCreationField {
                    id: id.into(),
                    name: self.public_metadata_text(&row["name"], 400)?.into(),
                    required: row["required"]
                        .as_bool()
                        .ok_or("Champ de création Jira invalide.")?,
                    has_default,
                    can_set: operations
                        .iter()
                        .any(|operation| operation.as_str() == Some("set")),
                })
            })
            .await?;
        Ok(JiraCreationMetadata {
            revision: self.configuration.revision,
            project_key: project.into(),
            issue_type: selected,
            fields,
        })
    }
    pub async fn create_issue(
        &self,
        metadata: &JiraCreationMetadata,
        card: &atelier_core::contracts::Card,
    ) -> Result<JiraIssue> {
        if metadata.revision != self.configuration.revision
            || !jira::project_key(&metadata.project_key)
        {
            return Err("Métadonnées de création Jira périmées.".into());
        }
        jira::validate_creation(metadata, card)?;
        let description = jira::creation_text(card)?;
        let title = card.title.utf8().map_err(|_| "Titre de carte invalide.")?;
        let mut fields = json!({"project":{"key":metadata.project_key},"issuetype":{"id":metadata.issue_type.id},"summary":title});
        if !description.is_empty() {
            fields["description"] = if self.configuration.mode == JiraMode::Cloud {
                json!({"type":"doc","version":1,"content":description.split('\n').map(|line|json!({"type":"paragraph","content":if line.is_empty(){vec![]}else{vec![json!({"type":"text","text":line})]}})).collect::<Vec<_>>()})
            } else {
                json!(description)
            };
        }
        let version = if self.configuration.mode == JiraMode::Cloud {
            "3"
        } else {
            "2"
        };
        let url = self.endpoint(&["rest", "api", version, "issue"])?;
        let url = if let Some(gateway) = self.gateway.get() {
            gateway_request(gateway, &url)?
        } else {
            url
        };
        let result = read_json_status(
            self.send(url, Some(&json!({"fields":fields}))).await?,
            4 * 1024 * 1024,
            201,
        )
        .await?;
        let id = self.public_metadata_text(&result["id"], 32)?;
        let key = self.public_metadata_text(&result["key"], 100)?;
        if !jira::issue_identity(id, key)
            || key.rsplit_once('-').map(|(project, _)| project)
                != Some(metadata.project_key.as_str())
        {
            return Err("Réponse de création Jira incertaine. Vérifie le ticket dans Jira.".into());
        }
        let browse = self.endpoint(&["browse", key])?.to_string();
        if browse.contains(&self.token) {
            return Err("Réponse de création Jira incertaine. Vérifie le ticket dans Jira.".into());
        }
        Ok(JiraIssue {
            id: id.into(),
            key: key.into(),
            title,
            status: String::new(),
            status_id: String::new(),
            url: browse,
        })
    }
    pub async fn issue_by_id(&self, id: &str) -> Result<JiraIssue> {
        let mut url = self.issue_endpoint(id, None)?;
        url.query_pairs_mut()
            .append_pair("fields", "summary,status");
        let row = self.request(url, None).await?;
        let issue = self.issue(&row)?;
        if issue.id != id {
            return Err("Identifiant du ticket Jira incohérent.".into());
        }
        Ok(issue)
    }
    pub async fn issue_by_key(&self, key: &str) -> Result<JiraIssue> {
        if !jira::issue_identity("1", key) {
            return Err("Clé Jira invalide.".into());
        }
        let version = if self.configuration.mode == JiraMode::Cloud {
            "3"
        } else {
            "2"
        };
        let mut url = self.endpoint(&["rest", "api", version, "issue", key])?;
        url.query_pairs_mut()
            .append_pair("fields", "summary,status");
        let row = self.request(url, None).await?;
        let issue = self.issue(&row)?;
        if issue.key != key {
            return Err("La clé Jira a changé. Utilise la clé actuelle du ticket.".into());
        }
        Ok(issue)
    }
    pub async fn search(&self) -> Result<JiraSearch> {
        if self.configuration.query.trim().is_empty()
            || self.configuration.query.len() > 16000
            || self.configuration.query.chars().count() > 4000
        {
            return Err("Requête JQL requise (4 000 caractères maximum).".into());
        }
        tokio::time::timeout(Duration::from_secs(60), self.search_pages())
            .await
            .map_err(|_| "La recherche Jira a dépassé une minute. Réduis son périmètre.")?
    }
}

fn tenant_site(base: &Url) -> bool {
    base.scheme() == "https"
        && base.username().is_empty()
        && base.password().is_none()
        && base.query().is_none()
        && base.fragment().is_none()
        && base.port_or_known_default() == Some(443)
        && base.path() == "/"
        && base
            .host_str()
            .and_then(|host| host.strip_suffix(".atlassian.net"))
            .is_some_and(|site| {
                !site.is_empty()
                    && site
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
}
fn gateway_base(id: &str) -> Result<Url> {
    if id.len() != 36 {
        return Err("Identifiant du site Jira invalide.".into());
    }
    let id = uuid::Uuid::parse_str(id).map_err(|_| "Identifiant du site Jira invalide.")?;
    Ok(Url::parse(&format!(
        "https://api.atlassian.com/ex/jira/{}",
        id.hyphenated()
    ))?)
}
fn gateway_request(gateway: &Url, request: &Url) -> Result<Url> {
    if !request.path().starts_with("/rest/api/3/") {
        return Err("Chemin Jira Cloud invalide.".into());
    }
    let mut target = gateway.clone();
    target.set_path(&format!("{}{}", gateway.path(), request.path()));
    target.set_query(request.query());
    Ok(target)
}
async fn read_json(response: reqwest::Response, limit: usize) -> Result<Value> {
    read_json_status(response, limit, 200).await
}
async fn read_json_status(
    mut response: reqwest::Response,
    limit: usize,
    success: u16,
) -> Result<Value> {
    match response.status().as_u16() {
        status if status == success => {}
        401 | 403 => {
            return Err("Jira refuse l’accès. Vérifie le compte, les droits et le jeton.".into());
        }
        429 => {
            return Err("Limite de requêtes Jira atteinte. Attends avant de réessayer.".into());
        }
        300..=399 => {
            return Err("Redirection Jira refusée. Vérifie l’adresse configurée.".into());
        }
        _ => {
            return Err("La recherche Jira a échoué. Vérifie la requête et le service.".into());
        }
    }
    let json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
        });
    if !json
        || response
            .content_length()
            .is_some_and(|length| length > limit as u64)
    {
        return Err("Réponse Jira invalide ou trop volumineuse.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Lecture de la réponse Jira impossible.")?
    {
        if bytes.len() + chunk.len() > limit {
            return Err("Réponse Jira trop volumineuse.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Réponse JSON Jira invalide.".into())
}

impl Jira {
    async fn search_pages(&self) -> Result<JiraSearch> {
        let mut issues = Vec::new();
        let mut ids = BTreeSet::new();
        let mut cursors = BTreeSet::new();
        let mut cursor = None::<String>;
        let mut size = 256usize;
        let cloud = self.configuration.mode == JiraMode::Cloud;
        let url = self.endpoint(if cloud {
            &["rest", "api", "3", "search", "jql"]
        } else {
            &["rest", "api", "2", "search"]
        })?;
        for _ in 0..100 {
            let mut body = json!({"jql":self.configuration.query,"maxResults":100,"fields":["summary","status"]});
            if cloud {
                if let Some(cursor) = &cursor {
                    body["nextPageToken"] = json!(cursor);
                }
            } else {
                body["startAt"] = json!(issues.len());
            }
            let page = self.request(url.clone(), Some(&body)).await?;
            let rows = page["issues"]
                .as_array()
                .filter(|rows| rows.len() <= 100)
                .ok_or("Page Jira invalide.")?;
            if !cloud && page["total"].as_u64().is_some_and(|total| total > 10_000) {
                return Err("Recherche Jira trop large. Réduis la requête JQL.".into());
            }
            for row in rows {
                let issue = self.issue(row)?;
                if !ids.insert(issue.id.clone()) {
                    return Err("La pagination Jira est incohérente. Réessaie la recherche.".into());
                }
                size += serde_json::to_vec(&issue)?.len() + 1;
                if size > 8 * 1024 * 1024 || issues.len() >= 10_000 {
                    return Err("Recherche Jira trop large. Réduis la requête JQL.".into());
                }
                issues.push(issue);
            }
            if cloud {
                if page["isLast"]
                    .as_bool()
                    .ok_or("Fin de pagination Jira invalide.")?
                {
                    return Ok(JiraSearch {
                        connection_revision: self.configuration.revision,
                        issues,
                    });
                }
                let next = page["nextPageToken"]
                    .as_str()
                    .filter(|token| !token.is_empty() && token.len() <= 4000)
                    .ok_or("Curseur Jira invalide.")?;
                if !cursors.insert(next.to_owned()) {
                    return Err("La pagination Jira est incohérente. Réessaie la recherche.".into());
                }
                cursor = Some(next.into());
            } else {
                let total = page["total"].as_u64().ok_or("Total Jira invalide.")?;
                if page["startAt"].as_u64() != Some((issues.len() - rows.len()) as u64) {
                    return Err("Décalage de page Jira incohérent.".into());
                }
                if issues.len() as u64 > total {
                    return Err("Total Jira incohérent. Réessaie la recherche.".into());
                }
                if issues.len() as u64 == total {
                    return Ok(JiraSearch {
                        connection_revision: self.configuration.revision,
                        issues,
                    });
                }
                if rows.is_empty() {
                    return Err("Page Jira vide avant la fin des résultats.".into());
                }
            }
        }
        Err("Recherche Jira trop large. Réduis la requête JQL.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloud_gateway_has_a_fixed_origin_and_never_accepts_redirects_or_untrusted_tenant_names() {
        let site = Url::parse("https://synthetic.atlassian.net").unwrap();
        assert!(tenant_site(&site));
        for site in [
            "http://synthetic.atlassian.net",
            "https://synthetic.atlassian.net.evil.test",
            "https://nested.synthetic.atlassian.net",
            "https://synthetic.atlassian.net/jira",
            "https://synthetic.atlassian.net:444",
            "https://user:secret@synthetic.atlassian.net",
        ] {
            assert!(!tenant_site(&Url::parse(site).unwrap()));
        }
        let gateway = gateway_base("00000000-0000-4000-8000-000000000001").unwrap();
        let request = Url::parse("https://synthetic.atlassian.net/rest/api/3/search/jql").unwrap();
        let target = gateway_request(&gateway, &request).unwrap();
        assert_eq!(target.host_str(), Some("api.atlassian.com"));
        assert_eq!(
            target.path(),
            "/ex/jira/00000000-0000-4000-8000-000000000001/rest/api/3/search/jql"
        );
        for id in [
            "synthetic-token",
            "../cloud",
            "https://evil.test",
            "00000000000040008000000000000001",
        ] {
            assert!(gateway_base(id).is_err());
        }
        assert!(
            gateway_request(
                &gateway,
                &Url::parse("https://synthetic.atlassian.net/browse/SYN-1").unwrap()
            )
            .is_err()
        );
    }
}
