use crate::Result;
use atelier_core::application::extensions::{
    self as protocol, Manifest, Method, Reply, Request, Status, Ticket, TicketPage,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{
    Url,
    header::{AUTHORIZATION, HeaderValue},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::LazyLock, time::Duration};
const PAGE: usize = 8; // Eight maximum-size UTF-8 descriptions fit the bounded connector reply.

pub fn manifest() -> Manifest {
    serde_json::from_value(json!({"id":"azure-tickets","name":"Azure Work Items","description":"Recherche et lecture de tickets Azure DevOps avec import local confirmé.","sourceLocale":"fr-FR","methods":["check","ticketsSearch","ticketsRead"],"fields":[{"id":"url","label":"Organisation Azure (https://dev.azure.com/organisation)","kind":"url","options":[],"credentialScope":true},{"id":"project","label":"Identifiant UUID du projet Azure","kind":"text","options":[],"credentialScope":true},{"id":"filter","label":"Titre contenant (facultatif)","kind":"text","options":[],"credentialScope":false},{"id":"token","label":"PAT Azure avec lecture des work items","kind":"password","options":[],"credentialScope":false}],"tabs":[],"translations":{"en-US":{"Recherche et lecture de tickets Azure DevOps avec import local confirmé.":"Search and read Azure DevOps tickets with confirmed local import.","Organisation Azure (https://dev.azure.com/organisation)":"Azure organization (https://dev.azure.com/organization)","Identifiant UUID du projet Azure":"Azure project UUID","Titre contenant (facultatif)":"Title containing (optional)","PAT Azure avec lecture des work items":"Azure PAT with work item read access","Projet Azure":"Azure project","Lecture Azure terminée ; aucune écriture distante.":"Azure read complete; no remote write."}}})).expect("static Azure connector manifest")
}

pub struct Azure {
    base: Url,
    project: String,
    token: String,
    authorization: HeaderValue,
    client: reqwest::Client,
}
impl Azure {
    pub fn new(base: &str, project: &str, token: String) -> Result<Self> {
        Self::prepare(base, project, token, false)
    }
    pub fn synthetic(base: &str, project: &str, token: String) -> Result<Self> {
        Self::prepare(base, project, token, true)
    }
    fn prepare(base: &str, project: &str, token: String, synthetic: bool) -> Result<Self> {
        if base.chars().any(|c| c.is_control() || c.is_whitespace())
            || base.contains(['\\', '%'])
            || base.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err("Organisation Azure HTTPS invalide.".into());
        }
        let base = Url::parse(base)?;
        let path = base.path().trim_matches('/');
        if (!synthetic && (base.scheme() != "https" || base.host_str() != Some("dev.azure.com")))
            || (synthetic
                && (base.scheme() != "http"
                    || !base.host_str().is_some_and(|host| {
                        host.parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                    })))
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || path.is_empty()
            || path.len() > 100
            || !path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("Organisation Azure HTTPS invalide.".into());
        }
        let project = uuid::Uuid::parse_str(project)
            .map_err(|_| "Identifiant UUID du projet Azure invalide.")?
            .to_string();
        if token.is_empty()
            || token.len() > 4000
            || token.chars().any(char::is_whitespace)
            || base.as_str().contains(&token)
            || project.contains(&token)
        {
            return Err("Jeton Azure invalide.".into());
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Basic {}", STANDARD.encode(format!(":{token}"))))?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .https_only(!synthetic)
            .build()?;
        Ok(Self {
            base,
            project,
            token,
            authorization,
            client,
        })
    }
    fn endpoint(&self, project: bool, parts: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| "Organisation Azure invalide.")?;
            path.pop_if_empty();
            if project {
                path.push(&self.project);
            }
            for part in parts {
                path.push(part);
            }
        }
        url.query_pairs_mut().append_pair("api-version", "7.1");
        Ok(url)
    }
    async fn request(&self, url: Url, body: Option<Value>) -> Result<Value> {
        let request = if let Some(body) = body {
            self.client.post(url).json(&body)
        } else {
            self.client.get(url)
        };
        let mut response = request
            .header(AUTHORIZATION, self.authorization.clone())
            .send()
            .await
            .map_err(|_| "Lecture Azure impossible ou trop longue.")?;
        if response.status().as_u16() != 200 {
            return Err(match response.status().as_u16() {
                401 | 403 => "Azure refuse la lecture. Vérifie le PAT et sa portée.",
                429 => "Limite Azure atteinte. Réessaie plus tard.",
                _ => "Réponse Azure refusée. Aucun détail distant affiché.",
            }
            .into());
        }
        if response.headers().contains_key("x-ms-continuationtoken") {
            return Err("Réponse Azure paginée inattendue. Aucun résultat partiel affiché.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Réponse Azure interrompue.")?
        {
            if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
                return Err("Réponse Azure trop volumineuse.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Réponse Azure invalide.")?;
        if exposes(&value, &self.token) {
            return Err(
                "La réponse Azure contient des données confidentielles inattendues.".into(),
            );
        }
        Ok(value)
    }
    pub async fn project_name(&self) -> Result<String> {
        let value = self
            .request(
                self.endpoint(false, &["_apis", "projects", &self.project])?,
                None,
            )
            .await?;
        if value["id"]
            .as_str()
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .map(|id| id.to_string())
            .as_deref()
            != Some(self.project.as_str())
        {
            return Err("Le projet Azure relu ne correspond pas au projet sélectionné.".into());
        }
        text(&value["name"], 300, false)
    }
    fn instance(&self) -> String {
        format!(
            "{}/{}",
            self.base.as_str().trim_end_matches('/'),
            self.project
        )
    }
    fn ticket(&self, row: &Value, project_name: &str) -> Result<Ticket> {
        let id = number(&row["id"])?;
        if row["fields"]["System.TeamProject"].as_str() != Some(project_name)
            || row["fields"]["System.Id"].as_u64() != Some(id)
        {
            return Err("Le ticket Azure ne correspond pas au projet sélectionné.".into());
        }
        let description = if row["fields"]["System.Description"].is_null() {
            String::new()
        } else {
            plain_description(&text(&row["fields"]["System.Description"], 200_000, true)?)?
        };
        let ticket = Ticket {
            instance: self.instance(),
            id: id.to_string(),
            key: format!("AB#{id}"),
            title: text(&row["fields"]["System.Title"], 300, false)?,
            description,
            url: format!("{}/_workitems/edit/{id}", self.instance()),
            status: text(&row["fields"]["System.State"], 200, false)?,
        };
        if exposes(&serde_json::to_value(&ticket)?, &self.token) {
            return Err(
                "La réponse Azure contient des données confidentielles inattendues.".into(),
            );
        }
        Ok(ticket)
    }
    async fn details(&self, ids: &[u64], name: &str) -> Result<Vec<Ticket>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let value=self.request(self.endpoint(true,&["_apis","wit","workitemsbatch"])?,Some(json!({"ids":ids,"fields":["System.Id","System.Title","System.Description","System.State","System.TeamProject"],"errorPolicy":"Fail"}))).await?;
        let rows = value["value"]
            .as_array()
            .ok_or("Liste de tickets Azure invalide.")?;
        if value["count"].as_u64() != Some(rows.len() as u64) || rows.len() != ids.len() {
            return Err("Liste de tickets Azure incomplète.".into());
        }
        let mut found = std::collections::BTreeMap::new();
        for row in rows {
            let ticket = self.ticket(row, name)?;
            if found.insert(ticket.id.clone(), ticket).is_some() {
                return Err("Ticket Azure répété.".into());
            }
        }
        ids.iter()
            .map(|id| {
                found
                    .remove(&id.to_string())
                    .ok_or_else(|| "Ticket Azure étranger à la sélection.".into())
            })
            .collect()
    }
    pub async fn read(&self, instance: &str, id: &str) -> Result<Ticket> {
        if instance != self.instance() {
            return Err(
                "Le ticket appartient à une autre organisation ou un autre projet Azure.".into(),
            );
        }
        let id = parse_id(id)?;
        let name = self.project_name().await?;
        self.details(&[id], &name)
            .await?
            .pop()
            .ok_or_else(|| "Ticket Azure absent.".into())
    }
    pub async fn search(&self, filter: &str, cursor: Option<&str>) -> Result<TicketPage> {
        if filter.encode_utf16().count() > 200 || filter.chars().any(char::is_control) {
            return Err("Filtre de titre Azure invalide.".into());
        }
        let after = cursor.map(parse_id).transpose()?.unwrap_or(0);
        let name = self.project_name().await?;
        let clause = if filter.is_empty() {
            String::new()
        } else {
            format!(
                " AND [System.Title] CONTAINS '{}'",
                filter.replace('\'', "''")
            )
        };
        let query = format!(
            "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = @project AND [System.Id] > {after}{clause} ORDER BY [System.Id] ASC"
        );
        let mut url = self.endpoint(true, &["_apis", "wit", "wiql"])?;
        url.query_pairs_mut()
            .append_pair("$top", &(PAGE + 1).to_string());
        let value = self.request(url, Some(json!({"query":query}))).await?;
        if value["queryType"] != "flat" {
            return Err("La recherche Azure doit retourner une liste de tickets.".into());
        }
        let rows = value["workItems"]
            .as_array()
            .filter(|rows| rows.len() <= PAGE + 1)
            .ok_or("Page de recherche Azure invalide.")?;
        let mut ids = Vec::new();
        let mut last = after;
        let mut seen = BTreeSet::new();
        for row in rows {
            let id = number(&row["id"])?;
            if id <= last || !seen.insert(id) {
                return Err("Pagination Azure répétée ou non ordonnée.".into());
            }
            ids.push(id);
            last = id;
        }
        let more = ids.len() > PAGE;
        ids.truncate(PAGE);
        let next = if more {
            ids.last().map(u64::to_string)
        } else {
            None
        };
        Ok(TicketPage {
            items: self.details(&ids, &name).await?,
            next,
        })
    }
}
fn number(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|id| *id > 0 && *id <= i32::MAX as u64)
        .ok_or_else(|| "Identifiant de ticket Azure invalide.".into())
}
fn exposes(value: &Value, secret: &str) -> bool {
    match value {
        Value::String(value) => value.contains(secret),
        Value::Array(values) => values.iter().any(|value| exposes(value, secret)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(secret) || exposes(value, secret)),
        _ => value.to_string().contains(secret),
    }
}
fn parse_id(value: &str) -> Result<u64> {
    let id = value
        .parse::<u64>()
        .map_err(|_| "Identifiant de ticket Azure invalide.")?;
    if id.to_string() != value {
        return Err("Identifiant de ticket Azure invalide.".into());
    }
    number(&json!(id))
}
fn text(value: &Value, limit: usize, multiline: bool) -> Result<String> {
    let text = value.as_str().ok_or("Texte Azure invalide.")?;
    if (!multiline && text.trim().is_empty())
        || text.encode_utf16().count() > limit
        || text
            .chars()
            .any(|c| c.is_control() && (!multiline || !matches!(c, '\r' | '\n' | '\t')))
    {
        return Err("Texte Azure trop volumineux ou invalide.".into());
    }
    Ok(text.into())
}
fn plain_description(html: &str) -> Result<String> {
    static HIDDEN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?is)<!--.*?-->|<script\b[^>]*>.*?</script\s*>|<style\b[^>]*>.*?</style\s*>",
        )
        .unwrap()
    });
    static TAGS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?s)<(?:"[^"]*"|'[^']*'|[^'">])*?>"#).unwrap());
    let visible = HIDDEN.replace_all(html, "");
    let text = TAGS.replace_all(&visible, |captures: &regex::Captures<'_>| {
        let tag = captures[0]
            .trim_matches(['<', '>'])
            .trim()
            .to_ascii_lowercase();
        if tag == "br"
            || tag == "br/"
            || tag.starts_with("br ")
            || ["/p", "/div", "/li", "/tr", "/h1", "/h2", "/h3", "/pre"].contains(&tag.as_str())
        {
            "\n"
        } else {
            ""
        }
    });
    let text = html_escape::decode_html_entities(&text).trim().to_owned();
    if text.encode_utf16().count() > 20_000
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err("Description Azure trop volumineuse après conversion.".into());
    }
    Ok(text)
}
pub async fn execute(request: Request) -> Result<Reply> {
    let manifest = manifest();
    protocol::effect(&manifest, &request)?;
    let output = if request.method == Method::Describe {
        serde_json::to_value(manifest)?
    } else {
        let setting = |key: &str| {
            request
                .settings
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        let token = request
            .secrets
            .get("token")
            .cloned()
            .ok_or("Renseigne un PAT Azure explicite.")?;
        let client = Azure::new(setting("url"), setting("project"), token)?;
        match request.method {
            Method::Check => {
                json!({"items":[{"label":"Projet Azure","value":client.project_name().await?}]})
            }
            Method::TicketsSearch => {
                let cursor = match request.input.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(value)) => Some(value.as_str()),
                    _ => return Err("Curseur Azure invalide.".into()),
                };
                serde_json::to_value(client.search(setting("filter"), cursor).await?)?
            }
            Method::TicketsRead => serde_json::to_value(
                client
                    .read(
                        request.input["instance"]
                            .as_str()
                            .ok_or("Instance Azure requise.")?,
                        request.input["id"]
                            .as_str()
                            .ok_or("Identifiant Azure requis.")?,
                    )
                    .await?,
            )?,
            _ => return Err("Opération Azure indisponible.".into()),
        }
    };
    Ok(Reply {
        protocol: protocol::PROTOCOL.into(),
        request_id: request.request_id,
        module: request.module,
        method: request.method,
        status: Status::Ok,
        message: "Lecture Azure terminée ; aucune écriture distante.".into(),
        output,
    })
}

pub async fn command() -> Result<()> {
    use std::io::{Read, Write};
    let mut input = Vec::new();
    std::io::stdin()
        .take((protocol::INPUT_LIMIT + 1) as u64)
        .read_to_end(&mut input)?;
    if input.len() > protocol::INPUT_LIMIT {
        return Err("Requête de connecteur trop volumineuse.".into());
    }
    let request: Request = serde_json::from_slice(&input).map_err(|_| "Requête Azure invalide.")?;
    let expected = request.clone();
    let reply = match execute(request).await {
        Ok(reply) => reply,
        Err(error) => Reply {
            protocol: protocol::PROTOCOL.into(),
            request_id: expected.request_id,
            module: expected.module,
            method: expected.method,
            status: Status::Error,
            message: error.to_string(),
            output: Value::Null,
        },
    };
    let output = serde_json::to_vec(&reply)?;
    if output.len() > protocol::OUTPUT_LIMIT {
        return Err("Réponse Azure trop volumineuse.".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
