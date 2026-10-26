use super::{settings, store::Store};
use crate::Result;
use atelier_core::{
    application::settings::SettingsUpdate,
    contracts::{
        ForgeConfiguration, ForgeConnection, ForgeConnections, ForgeKind, ForgeUpdate, PullRequest,
    },
    document::Document,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
mod checks;
pub use atelier_core::application::forge::same_pull_request;

fn conflict() -> crate::Error {
    atelier_core::application::forge::ForgeFailure::Conflict.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_routes_and_browser_links_keep_distinct_trusted_origins_and_encode_repository_names() {
        let github = Forge::new(
            ForgeConfiguration {
                kind: ForgeKind::Github,
                base: "https://github.com".into(),
            },
            "synthetic-token".into(),
            1,
        )
        .unwrap();
        assert_eq!(
            github
                .endpoint(true, &["repos", "owner", "repo", "pulls", "1"])
                .unwrap()
                .as_str(),
            "https://api.github.com/repos/owner/repo/pulls/1"
        );
        assert_eq!(
            github
                .endpoint(false, &["owner", "repo", "pull", "1"])
                .unwrap()
                .as_str(),
            "https://github.com/owner/repo/pull/1"
        );
        let gitlab = Forge::new(
            ForgeConfiguration {
                kind: ForgeKind::Gitlab,
                base: "https://gitlab.example.test/prefix".into(),
            },
            "synthetic-token".into(),
            1,
        )
        .unwrap();
        assert_eq!(
            gitlab
                .endpoint(
                    true,
                    &["projects", "group/nested/repo", "merge_requests", "1"]
                )
                .unwrap()
                .as_str(),
            "https://gitlab.example.test/prefix/api/v4/projects/group%2Fnested%2Frepo/merge_requests/1"
        );
    }
}
fn token_key(kind: ForgeKind) -> String {
    format!("forge.{}", kind.name())
}
pub(super) fn browser_link(
    configuration: &ForgeConfiguration,
    repository: &str,
    number: u64,
) -> Result<String> {
    let parts =
        atelier_core::application::forge::repository(configuration.kind, repository, number)?;
    let mut url = Url::parse(&configuration.base)?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| "Adresse de forge invalide.")?;
        path.pop_if_empty();
        for (index, part) in parts.iter().enumerate() {
            if configuration.kind == ForgeKind::Azure && index == 1 {
                path.push("_git");
            }
            path.push(part);
        }
        match configuration.kind {
            ForgeKind::Github => {
                path.push("pull");
            }
            ForgeKind::Gitlab => {
                path.push("-").push("merge_requests");
            }
            ForgeKind::Azure => {
                path.push("pullrequest");
            }
        }
        path.push(&number.to_string());
    }
    Ok(url.into())
}
fn configurations(store: &Store<'_>) -> Result<BTreeMap<ForgeKind, ForgeConfiguration>> {
    Ok(store
        .settings::<Document>()?
        .get("forgeConnections")?
        .unwrap_or_default())
}
pub fn revision(store: &Store<'_>) -> Result<u64> {
    Ok(store
        .settings::<Document>()?
        .get("forgeRevision")?
        .unwrap_or(0))
}
pub fn require_revision(store: &Store<'_>, expected: u64) -> Result<()> {
    if revision(store)? != expected {
        return Err(conflict());
    }
    Ok(())
}
pub fn public(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<ForgeConnections> {
    let configurations = configurations(store)?;
    let tokens = settings::load(store, protect)?;
    describe(revision(store)?, &configurations, &tokens)
}
fn describe(
    revision: u64,
    configurations: &BTreeMap<ForgeKind, ForgeConfiguration>,
    tokens: &Document,
) -> Result<ForgeConnections> {
    let mut connections = Vec::new();
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        let configuration = configurations
            .get(&kind)
            .cloned()
            .unwrap_or(ForgeConfiguration {
                kind,
                base: match kind {
                    ForgeKind::Github => "https://github.com",
                    ForgeKind::Gitlab => "https://gitlab.com",
                    ForgeKind::Azure => "",
                }
                .into(),
            });
        connections.push(ForgeConnection {
            configuration,
            token_set: tokens
                .get::<String>(&token_key(kind))?
                .is_some_and(|token| !token.is_empty()),
        });
    }
    Ok(ForgeConnections {
        revision,
        connections,
    })
}
pub fn normalize(configuration: &ForgeConfiguration) -> Result<ForgeConfiguration> {
    let url = Url::parse(&configuration.base).map_err(|_| "Adresse de forge invalide.")?;
    if configuration.base.len() > 2000
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || configuration.base.chars().any(char::is_control)
    {
        return Err("Adresse de forge HTTPS requise, sans identifiants ni paramètres.".into());
    }
    if configuration.kind == ForgeKind::Github
        && (url.host_str() == Some("api.github.com")
            || (url.host_str() == Some("github.com") && url.port_or_known_default() != Some(443)))
    {
        return Err("Renseigne l’adresse du site GitHub, pas celle de son API.".into());
    }
    let path = url.path().trim_end_matches('/');
    if path.to_ascii_lowercase().contains("%2f")
        || path.to_ascii_lowercase().contains("%5c")
        || path.contains('\\')
    {
        return Err("Chemin de forge invalide.".into());
    }
    match configuration.kind {
        ForgeKind::Github if !path.is_empty() => {
            return Err("Une connexion GitHub attend la racine du site.".into());
        }
        ForgeKind::Azure
            if url.host_str() != Some("dev.azure.com")
                || url.port_or_known_default() != Some(443)
                || path.split('/').filter(|part| !part.is_empty()).count() != 1 =>
        {
            return Err(
                "Renseigne https://dev.azure.com/ORGANISATION pour Azure DevOps Services.".into(),
            );
        }
        _ => {}
    }
    Ok(ForgeConfiguration {
        kind: configuration.kind,
        base: url.as_str().trim_end_matches('/').into(),
    })
}
pub fn configure(
    store: &Store<'_>,
    input: ForgeUpdate,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<ForgeConnections> {
    require_revision(store, input.expected_revision)?;
    let configuration = normalize(&input.configuration)?;
    if input.clear_token && input.token.is_some() {
        return Err(
            "Choisis la conservation, le remplacement ou le retrait du jeton de forge.".into(),
        );
    }
    if input.token.as_ref().is_some_and(|token| {
        token.is_empty()
            || token.len() > 4000
            || token.chars().any(|c| c.is_control() || c.is_whitespace())
    }) {
        return Err("Jeton de forge invalide.".into());
    }
    if input
        .token
        .as_ref()
        .is_some_and(|token| configuration.base.contains(token))
    {
        return Err("Ne place pas le jeton dans l’adresse de forge.".into());
    }
    let mut stored = store.settings::<Document>()?;
    let mut configurations = configurations(store)?;
    let mut tokens = settings::load(store, protect)?;
    let key = token_key(configuration.kind);
    if configurations.get(&configuration.kind) != Some(&configuration) || input.clear_token {
        tokens.set(&key, &"")?;
    }
    if let Some(token) = input.token {
        tokens.set(&key, &token)?;
    }
    configurations.insert(configuration.kind, configuration);
    stored.set("forgeConnections", &configurations)?;
    stored.set(
        "forgeRevision",
        &input
            .expected_revision
            .checked_add(1)
            .ok_or("Révision de forge épuisée.")?,
    )?;
    let result = describe(input.expected_revision + 1, &configurations, &tokens)?;
    settings::persist(
        store,
        &SettingsUpdate {
            settings: stored,
            tokens,
        },
        protect,
    )?;
    Ok(result)
}
pub fn credentials(
    store: &Store<'_>,
    kind: ForgeKind,
    expected: u64,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<(ForgeConfiguration, String)> {
    let configuration = configuration(store, kind, expected)?;
    let token = settings::load(store, protect)?
        .get::<String>(&token_key(kind))?
        .filter(|token| !token.is_empty())
        .ok_or("Renseigne le jeton de cette forge avant la lecture.")?;
    Ok((configuration, token))
}
pub(super) fn configuration(
    store: &Store<'_>,
    kind: ForgeKind,
    expected: u64,
) -> Result<ForgeConfiguration> {
    require_revision(store, expected)?;
    let configuration = configurations(store)?
        .remove(&kind)
        .ok_or("Configure cette forge avant la lecture.")?;
    if configuration.kind != kind {
        return Err("Configuration de forge incohérente.".into());
    }
    Ok(configuration)
}
pub(super) fn invalidate(stored: &mut Document, previous: &Document) -> Result<()> {
    if stored.raw("forgeConnections").is_none() && previous.raw("forgeConnections").is_none() {
        return Ok(());
    }
    let revision = stored
        .get::<u64>("forgeRevision")?
        .unwrap_or(0)
        .max(previous.get::<u64>("forgeRevision")?.unwrap_or(0))
        .checked_add(1)
        .ok_or("Révision de forge épuisée.")?;
    stored.set("forgeRevision", &revision)?;
    Ok(())
}

pub struct Forge {
    client: reqwest::Client,
    configuration: ForgeConfiguration,
    base: Url,
    authorization: HeaderValue,
    token: String,
    revision: u64,
}
pub struct PreparedDraft {
    expected: PullRequest,
    draft: bool,
    operation: String,
    method: reqwest::Method,
    url: Url,
    body: Value,
    mutation: Option<&'static str>,
}
pub struct PreparedPublication {
    configuration: ForgeConfiguration,
    revision: u64,
    url: Url,
    body: Value,
}
impl Forge {
    pub async fn list_pull_requests(
        &self,
        repository: &str,
    ) -> Result<Vec<atelier_core::contracts::PullRequestSummary>> {
        tokio::time::timeout(
            Duration::from_secs(60),
            self.read_pull_requests(repository, false),
        )
        .await
        .map_err(|_| "Liste de PR/MR trop longue.")?
    }
    async fn read_pull_requests(
        &self,
        repository: &str,
        all: bool,
    ) -> Result<Vec<atelier_core::contracts::PullRequestSummary>> {
        let parts =
            atelier_core::application::forge::repository(self.configuration.kind, repository, 1)?;
        if repository.contains(&self.token) {
            return Err("Dépôt ou numéro de PR invalide.".into());
        }
        let identity = match self.configuration.kind {
            ForgeKind::Github => None,
            ForgeKind::Gitlab => {
                let project = self
                    .get(self.endpoint(true, &["projects", repository])?)
                    .await?;
                if self.text(&project["path_with_namespace"], 1000)? != repository {
                    return Err(
                        "Le dépôt GitLab a changé de chemin. Utilise son chemin actuel.".into(),
                    );
                }
                Some(
                    project["id"]
                        .as_u64()
                        .ok_or("Identifiant de projet GitLab invalide.")?
                        .to_string(),
                )
            }
            ForgeKind::Azure => {
                let project = self
                    .get(
                        self.endpoint(true, &[parts[0], "_apis", "git", "repositories", parts[1]])?,
                    )
                    .await?;
                Some(self.text(&project["id"], 64)?.to_owned())
            }
        };
        let mut result = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        let mut bytes = 0usize;
        for page in 1..=100 {
            let mut url = match self.configuration.kind {
                ForgeKind::Github => {
                    self.endpoint(true, &["repos", parts[0], parts[1], "pulls"])?
                }
                ForgeKind::Gitlab => {
                    self.endpoint(true, &["projects", repository, "merge_requests"])?
                }
                ForgeKind::Azure => self.endpoint(
                    true,
                    &[
                        parts[0],
                        "_apis",
                        "git",
                        "repositories",
                        parts[1],
                        "pullrequests",
                    ],
                )?,
            };
            match self.configuration.kind {
                ForgeKind::Github => {
                    url.query_pairs_mut()
                        .append_pair("state", if all { "all" } else { "open" })
                        .append_pair("per_page", "100")
                        .append_pair("page", &page.to_string());
                }
                ForgeKind::Gitlab => {
                    url.query_pairs_mut()
                        .append_pair("state", if all { "all" } else { "opened" })
                        .append_pair("scope", "all")
                        .append_pair("per_page", "100")
                        .append_pair("page", &page.to_string());
                }
                ForgeKind::Azure => {
                    url.query_pairs_mut()
                        .append_pair("searchCriteria.status", if all { "all" } else { "active" })
                        .append_pair("$top", "100")
                        .append_pair("$skip", &result.len().to_string());
                }
            }
            let value = self.get(url).await?;
            bytes = bytes.saturating_add(serde_json::to_vec(&value)?.len());
            if bytes > 8 * 1024 * 1024 {
                return Err(
                    "Liste de PR/MR trop volumineuse. Aucun résultat partiel affiché.".into(),
                );
            }
            let rows = if self.configuration.kind == ForgeKind::Azure {
                value["value"].as_array()
            } else {
                value.as_array()
            }
            .ok_or("Pagination de PR/MR invalide.")?;
            if rows.len() > 100 {
                return Err("Pagination de PR/MR invalide.".into());
            }
            for row in rows {
                let (number, source, target, draft) = match self.configuration.kind {
                    ForgeKind::Github => {
                        if self.text(&row["base"]["repo"]["full_name"], 1000)? != repository
                            || if all {
                                !matches!(row["state"].as_str(), Some("open" | "closed"))
                            } else {
                                row["state"] != "open"
                            }
                        {
                            return Err(
                                "La liste de PR/MR ne correspond pas au dépôt demandé.".into()
                            );
                        }
                        (
                            row["number"].as_u64(),
                            &row["head"]["ref"],
                            &row["base"]["ref"],
                            row["draft"].as_bool(),
                        )
                    }
                    ForgeKind::Gitlab => {
                        if row["target_project_id"].as_u64().map(|id| id.to_string()) != identity
                            || if all {
                                !matches!(
                                    row["state"].as_str(),
                                    Some("opened" | "closed" | "merged" | "locked")
                                )
                            } else {
                                row["state"] != "opened"
                            }
                        {
                            return Err(
                                "La liste de PR/MR ne correspond pas au dépôt demandé.".into()
                            );
                        }
                        (
                            row["iid"].as_u64(),
                            &row["source_branch"],
                            &row["target_branch"],
                            row["draft"].as_bool(),
                        )
                    }
                    ForgeKind::Azure => {
                        if Some(self.text(&row["repository"]["id"], 64)?.to_owned()) != identity
                            || if all {
                                !matches!(
                                    row["status"].as_str(),
                                    Some("active" | "completed" | "abandoned")
                                )
                            } else {
                                row["status"] != "active"
                            }
                        {
                            return Err(
                                "La liste de PR/MR ne correspond pas au dépôt demandé.".into()
                            );
                        }
                        (
                            row["pullRequestId"].as_u64(),
                            &row["sourceRefName"],
                            &row["targetRefName"],
                            row["isDraft"].as_bool(),
                        )
                    }
                };
                let number = number.ok_or("Identité de PR/MR invalide.")?;
                atelier_core::application::forge::repository(
                    self.configuration.kind,
                    repository,
                    number,
                )?;
                if !ids.insert(number) {
                    return Err(
                        "Pagination de PR/MR répétée ou modifiée. Relance la lecture.".into(),
                    );
                }
                let branch = |value: &Value| -> Result<String> {
                    let value = self.text(value, 1000)?;
                    Ok(if self.configuration.kind == ForgeKind::Azure {
                        value
                            .strip_prefix("refs/heads/")
                            .filter(|value| !value.is_empty())
                            .ok_or("Référence de branche Azure invalide.")?
                    } else {
                        value
                    }
                    .to_owned())
                };
                result.push(atelier_core::contracts::PullRequestSummary {
                    number,
                    title: self.text(&row["title"], 8000)?.to_owned(),
                    url: browser_link(&self.configuration, repository, number)?,
                    draft: draft.ok_or("État brouillon de PR absent.")?,
                    source_branch: branch(source)?,
                    target_branch: branch(target)?,
                });
            }
            // A short page is not proof of completion: some servers lower their page size.
            if rows.is_empty() {
                if serde_json::to_vec(&result)?.len() > 8 * 1024 * 1024 {
                    return Err(
                        "Liste de PR/MR trop volumineuse. Aucun résultat partiel affiché.".into(),
                    );
                }
                return Ok(result);
            }
        }
        Err("Liste de PR/MR limitée à 100 pages ; aucun résultat partiel affiché.".into())
    }
    pub fn new(configuration: ForgeConfiguration, token: String, revision: u64) -> Result<Self> {
        let normalized = normalize(&configuration)?;
        Self::prepare(normalized, token, revision, false)
    }
    /// Loopback doubles only; the HTTP application never calls this constructor.
    pub fn synthetic(
        configuration: ForgeConfiguration,
        token: String,
        revision: u64,
    ) -> Result<Self> {
        let base = Url::parse(&configuration.base)?;
        if base.scheme() != "http"
            || !base
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|host| host.is_loopback())
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !token.starts_with("synthetic-")
        {
            return Err("Forge synthétique loopback requise.".into());
        }
        Self::prepare(configuration, token, revision, true)
    }
    fn prepare(
        configuration: ForgeConfiguration,
        token: String,
        revision: u64,
        synthetic: bool,
    ) -> Result<Self> {
        if token.is_empty()
            || token.len() > 4000
            || token.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("Jeton de forge invalide.".into());
        }
        let authorization = match configuration.kind {
            ForgeKind::Azure => format!("Basic {}", STANDARD.encode(format!(":{token}"))),
            _ => format!("Bearer {token}"),
        };
        let mut authorization =
            HeaderValue::from_str(&authorization).map_err(|_| "Jeton de forge invalide.")?;
        authorization.set_sensitive(true);
        let mut client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .https_only(!synthetic)
            .user_agent("atelier-kanban-rust");
        if synthetic {
            client = client.no_proxy();
        }
        Ok(Self {
            client: client.build()?,
            base: Url::parse(&configuration.base)?,
            configuration,
            authorization,
            token,
            revision,
        })
    }
    fn endpoint(&self, api: bool, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        if api
            && self.configuration.kind == ForgeKind::Github
            && url.host_str() == Some("github.com")
        {
            url.set_host(Some("api.github.com"))?;
        }
        let public_github = url.host_str() == Some("api.github.com");
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| "Adresse de forge invalide.")?;
            path.pop_if_empty();
            if api {
                match self.configuration.kind {
                    ForgeKind::Github if !public_github => {
                        path.push("api").push("v3");
                    }
                    ForgeKind::Gitlab => {
                        path.push("api").push("v4");
                    }
                    _ => {}
                }
            }
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(url)
    }
    fn text<'a>(&self, value: &'a Value, limit: usize) -> Result<&'a str> {
        value
            .as_str()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= limit
                    && !value.chars().any(char::is_control)
                    && !value.contains(&self.token)
            })
            .ok_or_else(|| "Métadonnées de PR invalides ou confidentielles.".into())
    }
    fn commit(&self, value: &Value) -> Result<Option<String>> {
        if value.is_null() {
            return Ok(None);
        }
        let text = self.text(value, 64)?;
        if ![40, 64].contains(&text.len()) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Commit de PR invalide.".into());
        }
        Ok(Some(text.to_ascii_lowercase()))
    }
    async fn get(&self, url: Url) -> Result<Value> {
        self.request(reqwest::Method::GET, url, None).await
    }
    async fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<Value> {
        self.request_inner(method, url, body, false).await
    }
    async fn get_unpaged(&self, url: Url) -> Result<Value> {
        self.request_inner(reqwest::Method::GET, url, None, true)
            .await
    }
    async fn request_inner(
        &self,
        method: reqwest::Method,
        mut url: Url,
        body: Option<Value>,
        unpaged: bool,
    ) -> Result<Value> {
        let creating = method == reqwest::Method::POST;
        if self.configuration.kind == ForgeKind::Azure
            && !url.query_pairs().any(|(key, _)| key == "api-version")
        {
            url.query_pairs_mut().append_pair("api-version", "7.1");
        }
        let request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, "application/json");
        let request = if self.configuration.kind == ForgeKind::Github {
            request.header("X-GitHub-Api-Version", "2022-11-28")
        } else {
            request
        };
        let request = if let Some(body) = body {
            request.json(&body)
        } else {
            request
        };
        let mut response = request
            .send()
            .await
            .map_err(|_| "Connexion à la forge impossible ou trop longue.")?;
        match response.status().as_u16() {
            200 => {}
            201 if creating => {}
            401 | 403 => {
                return Err(
                    "La forge refuse l’accès. Vérifie le jeton et les droits de lecture.".into(),
                );
            }
            404 => return Err("Dépôt ou PR introuvable, ou accès non autorisé.".into()),
            429 => {
                return Err(
                    "Limite de requêtes de la forge atteinte. Attends avant de réessayer.".into(),
                );
            }
            300..=399 => {
                return Err("Redirection de forge refusée. Vérifie l’adresse et le dépôt.".into());
            }
            _ => return Err("Lecture de la PR impossible sur la forge.".into()),
        }
        if unpaged
            && response
                .headers()
                .get_all("x-ms-continuationtoken")
                .iter()
                .any(|value| !value.to_str().is_ok_and(|value| value.trim().is_empty()))
        {
            return Err("Pagination de forge inattendue. Aucun résultat partiel affiché.".into());
        }
        if response
            .content_length()
            .is_some_and(|length| length > 4 * 1024 * 1024)
            || !response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
        {
            return Err("Réponse de forge invalide ou trop volumineuse.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Lecture de la réponse de forge impossible.")?
        {
            if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                return Err("Réponse de forge trop volumineuse.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "Réponse JSON de forge invalide.".into())
    }
    fn graphql_endpoint(&self) -> Result<Url> {
        match self.configuration.kind {
            ForgeKind::Github if self.base.host_str() == Some("github.com") => {
                Ok(Url::parse("https://api.github.com/graphql")?)
            }
            ForgeKind::Github => self.endpoint(false, &["api", "graphql"]),
            ForgeKind::Gitlab => self.endpoint(false, &["api", "graphql"]),
            ForgeKind::Azure => Err("GraphQL indisponible pour cette forge.".into()),
        }
    }
    /// Reads only; the caller persists `sending` after this preflight and before `send_draft`.
    pub async fn prepare_draft(
        &self,
        expected: &PullRequest,
        draft: bool,
        operation: &str,
    ) -> Result<PreparedDraft> {
        if expected.kind != self.configuration.kind
            || expected.base_url != self.configuration.base
            || expected.revision != self.revision
            || uuid::Uuid::parse_str(operation).is_err()
        {
            return Err("Intention de forge incohérente.".into());
        }
        let current = self
            .pull_request(&expected.repository, expected.number)
            .await?;
        if !same_pull_request(expected, &current)
            || current.status != "active"
            || current.fork
            || current.draft == draft
        {
            return Err("La PR/MR a changé. Relis-la avant de modifier son état brouillon.".into());
        }
        let parts = atelier_core::application::forge::repository(
            current.kind,
            &current.repository,
            current.number,
        )?;
        let number = current.number.to_string();
        let (method, url, body, mutation) = match current.kind {
            ForgeKind::Github => {
                // Resolve the node through the exact REST repository, never an arbitrary caller-supplied ID.
                let row = self
                    .get(self.endpoint(true, &["repos", parts[0], parts[1], "pulls", &number])?)
                    .await?;
                if row["number"].as_u64() != Some(current.number)
                    || self.text(&row["base"]["repo"]["full_name"], 1000)? != current.repository
                    || self.commit(&row["head"]["sha"])? != current.source_commit
                    || self.commit(&row["base"]["sha"])? != current.target_commit
                    || row["draft"].as_bool() != Some(current.draft)
                    || row["state"] != "open"
                {
                    return Err(
                        "La PR/MR a changé. Relis-la avant de modifier son état brouillon.".into(),
                    );
                }
                let id = self.text(&row["node_id"], 256)?;
                let (mutation, input) = if draft {
                    (
                        "convertPullRequestToDraft",
                        "ConvertPullRequestToDraftInput!",
                    )
                } else {
                    (
                        "markPullRequestReadyForReview",
                        "MarkPullRequestReadyForReviewInput!",
                    )
                };
                let query = format!(
                    "mutation($input:{input}){{{mutation}(input:$input){{clientMutationId pullRequest{{isDraft}}}}}}"
                );
                (
                    reqwest::Method::POST,
                    self.graphql_endpoint()?,
                    serde_json::json!({"query":query,"variables":{"input":{"pullRequestId":id,"clientMutationId":operation}}}),
                    Some(mutation),
                )
            }
            ForgeKind::Gitlab => {
                // The dedicated mutation preserves concurrent human edits to the MR title.
                let query = "mutation($input:MergeRequestSetDraftInput!){mergeRequestSetDraft(input:$input){clientMutationId errors mergeRequest{draft}}}";
                (
                    reqwest::Method::POST,
                    self.graphql_endpoint()?,
                    serde_json::json!({"query":query,"variables":{"input":{"projectPath":current.repository,"iid":number,"draft":draft,"clientMutationId":operation}}}),
                    Some("mergeRequestSetDraft"),
                )
            }
            ForgeKind::Azure => (
                reqwest::Method::PATCH,
                self.endpoint(
                    true,
                    &[
                        parts[0],
                        "_apis",
                        "git",
                        "repositories",
                        parts[1],
                        "pullrequests",
                        &number,
                    ],
                )?,
                serde_json::json!({"isDraft":draft}),
                None,
            ),
        };
        Ok(PreparedDraft {
            expected: current,
            draft,
            operation: operation.into(),
            method,
            url,
            body,
            mutation,
        })
    }
    /// Consumed once; no retry, redirection or compensating remote write.
    pub async fn send_draft(&self, prepared: PreparedDraft) -> Result<PullRequest> {
        let PreparedDraft {
            expected: current,
            draft,
            operation,
            method,
            url,
            body,
            mutation,
        } = prepared;
        if current.kind != self.configuration.kind
            || current.base_url != self.configuration.base
            || current.revision != self.revision
        {
            return Err("Intention de forge incohérente.".into());
        }
        let result = self.request(method, url, Some(body)).await?;
        if let Some(mutation) = mutation {
            let payload = &result["data"][mutation];
            let observed_draft = if current.kind == ForgeKind::Gitlab {
                payload["mergeRequest"]["draft"].as_bool()
            } else {
                payload["pullRequest"]["isDraft"].as_bool()
            };
            if result
                .get("errors")
                .is_some_and(|errors| !errors.as_array().is_some_and(Vec::is_empty))
                || current.kind == ForgeKind::Gitlab
                    && !payload["errors"].as_array().is_some_and(Vec::is_empty)
                || payload["clientMutationId"].as_str() != Some(operation.as_str())
                || observed_draft != Some(draft)
            {
                return Err(
                    "Écriture de forge incertaine. Relis l’état distant sans réenvoyer.".into(),
                );
            }
        }
        let observed = self
            .pull_request(&current.repository, current.number)
            .await?;
        if observed.draft != draft {
            return Err(
                "Écriture de forge incertaine. Relis l’état distant sans réenvoyer.".into(),
            );
        }
        Ok(observed)
    }
    pub async fn prepare_publication(
        &self,
        preview: &atelier_core::contracts::ForgePublicationPreview,
        operation: &str,
    ) -> Result<PreparedPublication> {
        let parts = atelier_core::application::forge::repository(
            self.configuration.kind,
            &preview.repository,
            1,
        )?;
        let marker = publication_marker(operation)?;
        if preview.configuration.kind != self.configuration.kind
            || preview.configuration.base != self.configuration.base
            || preview.revision != self.revision
            || preview.worktree.branch == preview.target
            || preview.title.trim().is_empty()
            || preview.title.encode_utf16().count() > 255
            || preview.title.chars().any(char::is_control)
            || preview.body.encode_utf16().count() > 3500
            || preview
                .body
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
            || [
                &preview.title,
                &preview.body,
                &preview.repository,
                &preview.worktree.branch,
                &preview.target,
            ]
            .iter()
            .any(|value| value.contains(&self.token))
        {
            return Err("Aperçu de publication invalide ou confidentiel.".into());
        }
        for branch in [&preview.worktree.branch, &preview.target] {
            atelier_core::application::worktrees::validate_reference(branch)?;
        }
        for commit in [&preview.source_commit, &preview.target_commit] {
            self.commit(&Value::String(commit.clone()))?
                .ok_or("Commit de publication absent.")?;
        }
        let candidates = self
            .list_pull_requests(&preview.repository)
            .await?
            .into_iter()
            .filter(|pr| {
                pr.source_branch == preview.worktree.branch && pr.target_branch == preview.target
            })
            .collect::<Vec<_>>();
        if candidates.len() > 20 {
            return Err(
                "Trop de PR/MR candidates. La publication est refusée sans résultat partiel."
                    .into(),
            );
        }
        for candidate in candidates {
            let pr = self
                .pull_request(&preview.repository, candidate.number)
                .await?;
            if !pr.fork
                && pr.status == "active"
                && pr.source_branch == preview.worktree.branch
                && pr.target_branch == preview.target
            {
                return Err("Une PR/MR ouverte existe déjà pour ces branches. Associe-la au lieu de créer un doublon.".into());
            }
        }
        let body = format!("{}\n\n{marker}", preview.body);
        let (url, body) = match self.configuration.kind {
            ForgeKind::Github => (
                self.endpoint(true, &["repos", parts[0], parts[1], "pulls"])?,
                serde_json::json!({"head":preview.worktree.branch,"base":preview.target,"title":preview.title,"body":body,"draft":preview.draft,"maintainer_can_modify":false}),
            ),
            ForgeKind::Gitlab => (
                self.endpoint(true, &["projects", &preview.repository, "merge_requests"])?,
                serde_json::json!({"source_branch":preview.worktree.branch,"target_branch":preview.target,"title":preview.title,"description":body,"remove_source_branch":false,"allow_collaboration":false}),
            ),
            ForgeKind::Azure => (
                self.endpoint(
                    true,
                    &[
                        parts[0],
                        "_apis",
                        "git",
                        "repositories",
                        parts[1],
                        "pullrequests",
                    ],
                )?,
                serde_json::json!({"sourceRefName":format!("refs/heads/{}",preview.worktree.branch),"targetRefName":format!("refs/heads/{}",preview.target),"title":preview.title,"description":body,"isDraft":preview.draft}),
            ),
        };
        Ok(PreparedPublication {
            configuration: self.configuration.clone(),
            revision: self.revision,
            url,
            body,
        })
    }
    /// Returns a candidate identity to persist before any further remote read.
    pub async fn create_publication(&self, prepared: PreparedPublication) -> Result<u64> {
        if prepared.configuration.kind != self.configuration.kind
            || prepared.configuration.base != self.configuration.base
            || prepared.revision != self.revision
        {
            return Err("Intention de publication incohérente.".into());
        }
        let row = self
            .request(reqwest::Method::POST, prepared.url, Some(prepared.body))
            .await?;
        let field = match self.configuration.kind {
            ForgeKind::Github => "number",
            ForgeKind::Gitlab => "iid",
            ForgeKind::Azure => "pullRequestId",
        };
        row[field]
            .as_u64()
            .filter(|number| *number > 0 && *number <= i32::MAX as u64)
            .ok_or_else(|| {
                "Création de PR/MR incertaine. Relis la forge sans renvoyer la demande.".into()
            })
    }
    pub async fn reconcile_publication(
        &self,
        preview: &atelier_core::contracts::ForgePublicationPreview,
        operation: &str,
        known: Option<u64>,
    ) -> Result<Option<PullRequest>> {
        tokio::time::timeout(
            Duration::from_secs(120),
            self.find_publication(preview, operation, known),
        )
        .await
        .map_err(|_| "Recherche de publication trop longue. L’incertitude est conservée.")?
    }
    async fn find_publication(
        &self,
        preview: &atelier_core::contracts::ForgePublicationPreview,
        operation: &str,
        known: Option<u64>,
    ) -> Result<Option<PullRequest>> {
        if self.configuration.kind != preview.configuration.kind
            || self.configuration.base != preview.configuration.base
        {
            return Err("Reconnecte le site de forge de la publication incertaine.".into());
        }
        let marker = publication_marker(operation)?;
        let candidates = if let Some(number) = known {
            vec![number]
        } else {
            let rows = self.read_pull_requests(&preview.repository, true).await?;
            let candidates = rows
                .into_iter()
                .filter(|pr| {
                    pr.source_branch == preview.worktree.branch
                        && pr.target_branch == preview.target
                })
                .map(|pr| pr.number)
                .collect::<Vec<_>>();
            if candidates.len() > 20 {
                return Err("Trop de PR/MR candidates. L’incertitude est conservée.".into());
            }
            candidates
        };
        let mut found = None;
        for number in candidates {
            let (pr, body, _) = self
                .read_pull_request_content(&preview.repository, number, true)
                .await?;
            if body.lines().any(|line| line == marker) {
                if pr.fork
                    || pr.source_branch != preview.worktree.branch
                    || pr.target_branch != preview.target
                    || found.is_some()
                {
                    return Err(
                        "Publication retrouvée ambiguë ou déplacée. L’incertitude est conservée."
                            .into(),
                    );
                }
                found = Some(pr);
            }
        }
        Ok(found)
    }
    pub async fn pull_request(&self, repository: &str, number: u64) -> Result<PullRequest> {
        tokio::time::timeout(
            Duration::from_secs(60),
            self.read_pull_request_content(repository, number, false),
        )
        .await
        .map_err(|_| "Lecture de PR trop longue.")?
        .map(|(pr, _, _)| pr)
    }
    async fn read_pull_request_content(
        &self,
        repository: &str,
        number: u64,
        include_body: bool,
    ) -> Result<(PullRequest, String, Value)> {
        let parts = atelier_core::application::forge::repository(
            self.configuration.kind,
            repository,
            number,
        )?;
        if repository.contains(&self.token) {
            return Err("Dépôt ou numéro de PR invalide.".into());
        }
        let number_text = number.to_string();
        let (row, url) = match self.configuration.kind {
            ForgeKind::Github => (
                self.get(
                    self.endpoint(true, &["repos", parts[0], parts[1], "pulls", &number_text])?,
                )
                .await?,
                self.endpoint(false, &[parts[0], parts[1], "pull", &number_text])?,
            ),
            ForgeKind::Gitlab => {
                let project = self
                    .get(self.endpoint(true, &["projects", repository])?)
                    .await?;
                if self.text(&project["path_with_namespace"], 1000)? != repository {
                    return Err(
                        "Le dépôt GitLab a changé de chemin. Utilise son chemin actuel.".into(),
                    );
                }
                let project_id = project["id"]
                    .as_u64()
                    .ok_or("Identifiant de projet GitLab invalide.")?;
                let row = self
                    .get(self.endpoint(
                        true,
                        &["projects", repository, "merge_requests", &number_text],
                    )?)
                    .await?;
                if row["target_project_id"].as_u64() != Some(project_id) {
                    return Err("La MR ne correspond pas au dépôt demandé.".into());
                }
                let mut web = parts.clone();
                web.extend(["-", "merge_requests", &number_text]);
                (row, self.endpoint(false, &web)?)
            }
            ForgeKind::Azure => {
                let project = self
                    .get(
                        self.endpoint(true, &[parts[0], "_apis", "git", "repositories", parts[1]])?,
                    )
                    .await?;
                let repo_id = self.text(&project["id"], 64)?;
                let row = self
                    .get(self.endpoint(
                        true,
                        &[
                            parts[0],
                            "_apis",
                            "git",
                            "repositories",
                            parts[1],
                            "pullrequests",
                            &number_text,
                        ],
                    )?)
                    .await?;
                if self.text(&row["repository"]["id"], 64)? != repo_id {
                    return Err("La PR ne correspond pas au dépôt demandé.".into());
                }
                (
                    row,
                    self.endpoint(
                        false,
                        &[parts[0], "_git", parts[1], "pullrequest", &number_text],
                    )?,
                )
            }
        };
        let (id, title, status, draft, source, target, source_commit, target_commit, fork) =
            match self.configuration.kind {
                ForgeKind::Github => {
                    if !self
                        .text(&row["base"]["repo"]["full_name"], 1000)?
                        .eq_ignore_ascii_case(repository)
                    {
                        return Err("La PR ne correspond pas au dépôt demandé.".into());
                    }
                    let fork = self.text(&row["head"]["repo"]["full_name"], 1000)?
                        != self.text(&row["base"]["repo"]["full_name"], 1000)?;
                    (
                        row["number"].as_u64(),
                        &row["title"],
                        if row["merged"]
                            .as_bool()
                            .ok_or("État de fusion GitHub absent.")?
                        {
                            "completed"
                        } else {
                            match row["state"].as_str() {
                                Some("open") => "active",
                                Some("closed") => "abandoned",
                                _ => "unknown",
                            }
                        },
                        row["draft"].as_bool(),
                        &row["head"]["ref"],
                        &row["base"]["ref"],
                        &row["head"]["sha"],
                        &row["base"]["sha"],
                        fork,
                    )
                }
                ForgeKind::Gitlab => (
                    row["iid"].as_u64(),
                    &row["title"],
                    match row["state"].as_str() {
                        Some("opened") => "active",
                        Some("merged") => "completed",
                        Some("closed") => "abandoned",
                        _ => "unknown",
                    },
                    row["draft"].as_bool(),
                    &row["source_branch"],
                    &row["target_branch"],
                    if row["sha"].is_null() {
                        &row["diff_refs"]["head_sha"]
                    } else {
                        &row["sha"]
                    },
                    &row["diff_refs"]["start_sha"],
                    row["source_project_id"].as_u64() != row["target_project_id"].as_u64(),
                ),
                ForgeKind::Azure => (
                    row["pullRequestId"].as_u64(),
                    &row["title"],
                    match row["status"].as_str() {
                        Some("active") => "active",
                        Some("completed") => "completed",
                        Some("abandoned") => "abandoned",
                        _ => "unknown",
                    },
                    row["isDraft"].as_bool(),
                    &row["sourceRefName"],
                    &row["targetRefName"],
                    &row["lastMergeSourceCommit"]["commitId"],
                    &row["lastMergeTargetCommit"]["commitId"],
                    !row["forkSource"].is_null(),
                ),
            };
        if id != Some(number) || url.as_str().contains(&self.token) {
            return Err("Identité de PR incohérente ou confidentielle.".into());
        }
        let source = self.text(source, 1000)?;
        let target = self.text(target, 1000)?;
        let branch = |value: &str| -> Result<String> {
            if self.configuration.kind == ForgeKind::Azure {
                Ok(value
                    .strip_prefix("refs/heads/")
                    .filter(|value| !value.is_empty())
                    .ok_or("Référence de branche Azure invalide.")?
                    .into())
            } else {
                Ok(value.into())
            }
        };
        let body = if include_body {
            let value = if self.configuration.kind == ForgeKind::Github {
                &row["body"]
            } else {
                &row["description"]
            };
            let text = if value.is_null() {
                ""
            } else {
                value.as_str().ok_or("Contenu de PR/MR invalide.")?
            };
            if text.len() > 128 * 1024
                || text.contains(&self.token)
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
            {
                return Err("Contenu de PR/MR invalide ou confidentiel.".into());
            }
            text.to_owned()
        } else {
            String::new()
        };
        Ok((
            PullRequest {
                revision: self.revision,
                kind: self.configuration.kind,
                repository: repository.into(),
                number,
                title: self.text(title, 8000)?.into(),
                url: browser_link(&self.configuration, repository, number)?,
                status: status.into(),
                draft: draft.ok_or("État brouillon de PR absent.")?,
                source_branch: branch(source)?,
                target_branch: branch(target)?,
                source_commit: self.commit(source_commit)?,
                target_commit: self.commit(target_commit)?,
                fork,
                base_url: self.configuration.base.clone(),
                checked_at: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)?,
            },
            body,
            row,
        ))
    }
}

pub fn publication_marker(operation: &str) -> Result<String> {
    let id =
        uuid::Uuid::parse_str(operation).map_err(|_| "Identifiant de publication invalide.")?;
    if id.to_string() != operation {
        return Err("Identifiant de publication invalide.".into());
    }
    Ok(format!("<!-- atelier-publication:{operation} -->"))
}
