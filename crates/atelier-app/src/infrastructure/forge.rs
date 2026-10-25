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
impl Forge {
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
    async fn get(&self, mut url: Url) -> Result<Value> {
        if self.configuration.kind == ForgeKind::Azure {
            url.query_pairs_mut().append_pair("api-version", "7.1");
        }
        let request = self
            .client
            .get(url)
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, "application/json");
        let request = if self.configuration.kind == ForgeKind::Github {
            request.header("X-GitHub-Api-Version", "2022-11-28")
        } else {
            request
        };
        let mut response = request
            .send()
            .await
            .map_err(|_| "Connexion à la forge impossible ou trop longue.")?;
        match response.status().as_u16() {
            200 => {}
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
    pub async fn pull_request(&self, repository: &str, number: u64) -> Result<PullRequest> {
        tokio::time::timeout(
            Duration::from_secs(60),
            self.read_pull_request(repository, number),
        )
        .await
        .map_err(|_| "Lecture de PR trop longue.")?
    }
    async fn read_pull_request(&self, repository: &str, number: u64) -> Result<PullRequest> {
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
                    &row["diff_refs"]["head_sha"],
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
        Ok(PullRequest {
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
        })
    }
}
