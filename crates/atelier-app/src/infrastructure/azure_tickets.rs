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
    serde_json::from_str(r#"{"id":"azure-tickets","name":"Azure Work Items","description":"description","sourceLocale":"fr-FR","methods":["check","ticketsSearch","ticketsRead","ticketsCreate","reconcile"],"fields":[{"id":"url","label":"organization","kind":"url","options":[],"credentialScope":true},{"id":"project","label":"project-id","kind":"text","options":[],"credentialScope":true},{"id":"filter","label":"title-filter","kind":"text","options":[],"credentialScope":false},{"id":"token","label":"access-token","kind":"password","options":[],"credentialScope":false}],"tabs":[],"translations":{"fr-FR":{"description":"Recherche, lecture et création confirmée de tickets Azure DevOps.","organization":"Organisation Azure (https://dev.azure.com/organisation)","project-id":"Identifiant UUID du projet Azure","title-filter":"Titre contenant (facultatif)","access-token":"PAT Azure avec lecture ; écriture requise pour créer","azure-project":"Projet Azure","read-complete":"Lecture Azure terminée ; aucune écriture distante.","creation-observed":"Résultat de création Azure observé.","azure-ticket":"Ticket Azure","ticket-title":"Titre","ticket-address":"Adresse du ticket","azure-invalid-https-azure-organization":"Organisation Azure HTTPS invalide.","azure-invalid-azure-project-uuid":"Identifiant UUID du projet Azure invalide.","azure-invalid-azure-token":"Jeton Azure invalide.","azure-invalid-azure-organization":"Organisation Azure invalide.","azure-azure-could-not-be-read-or-took-too-long":"Lecture Azure impossible ou trop longue.","azure-unexpected-paginated-azure-response-no-partial-r-adcfcd67":"Réponse Azure paginée inattendue. Aucun résultat partiel affiché.","azure-azure-response-interrupted":"Réponse Azure interrompue.","azure-azure-response-is-too-large":"Réponse Azure trop volumineuse.","azure-invalid-azure-response":"Réponse Azure invalide.","azure-the-azure-response-contains-unexpected-confidential-data":"La réponse Azure contient des données confidentielles inattendues.","azure-invalid-azure-operation-identifier":"Identifiant d’opération Azure invalide.","azure-the-ticket-cannot-contain-the-azure-token":"Le ticket ne peut pas contenir le jeton Azure.","azure-invalid-azure-effect-lookup":"Recherche de l’effet Azure invalide.","azure-multiple-tickets-carry-this-azure-operation-identity":"Plusieurs tickets portent l’identité de cette opération Azure.","azure-invalid-azure-ticket-observation":"Observation du ticket Azure invalide.","azure-the-observed-azure-ticket-does-not-match-the-con-eafef4f4":"Le ticket Azure observé ne correspond pas à la création confirmée.","azure-the-retrieved-azure-project-does-not-match-the-s-afc82078":"Le projet Azure relu ne correspond pas au projet sélectionné.","azure-the-azure-ticket-does-not-match-the-selected-project":"Le ticket Azure ne correspond pas au projet sélectionné.","azure-invalid-azure-ticket-list":"Liste de tickets Azure invalide.","azure-incomplete-azure-ticket-list":"Liste de tickets Azure incomplète.","azure-repeated-azure-ticket":"Ticket Azure répété.","azure-azure-ticket-outside-the-selection":"Ticket Azure étranger à la sélection.","azure-the-ticket-belongs-to-another-azure-organization-cc46f31b":"Le ticket appartient à une autre organisation ou un autre projet Azure.","azure-azure-ticket-missing":"Ticket Azure absent.","azure-invalid-azure-title-filter":"Filtre de titre Azure invalide.","azure-azure-search-must-return-a-ticket-list":"La recherche Azure doit retourner une liste de tickets.","azure-invalid-azure-search-page":"Page de recherche Azure invalide.","azure-azure-pagination-is-repeated-or-unordered":"Pagination Azure répétée ou non ordonnée.","azure-invalid-azure-ticket-id":"Identifiant de ticket Azure invalide.","azure-invalid-azure-text":"Texte Azure invalide.","azure-azure-text-is-too-large-or-invalid":"Texte Azure trop volumineux ou invalide.","azure-azure-description-is-too-large-after-conversion":"Description Azure trop volumineuse après conversion.","azure-enter-an-explicit-azure-pat":"Renseigne un PAT Azure explicite.","azure-azure-operation-identifier-required":"Identifiant d’opération Azure requis.","azure-azure-reconciliation-is-outside-the-confirmed-scope":"Rapprochement Azure hors périmètre.","azure-invalid-azure-cursor":"Curseur Azure invalide.","azure-azure-instance-required":"Instance Azure requise.","azure-azure-id-required":"Identifiant Azure requis.","azure-azure-operation-unavailable":"Opération Azure indisponible.","azure-connector-request-too-large":"Requête de connecteur trop volumineuse.","azure-invalid-azure-request":"Requête Azure invalide.","azure-read-forbidden":"Azure refuse la lecture. Vérifie le PAT et sa portée.","azure-rate-limited":"Limite Azure atteinte. Réessaie plus tard.","azure-response-rejected":"Réponse Azure refusée. Aucun détail distant affiché."},"en-US":{"description":"Search, read and confirm creation of Azure DevOps tickets.","organization":"Azure organization (https://dev.azure.com/organization)","project-id":"Azure project UUID","title-filter":"Title containing (optional)","access-token":"Azure PAT with read access; write access required to create","azure-project":"Azure project","read-complete":"Azure read complete; no remote write.","creation-observed":"Azure creation result observed.","azure-ticket":"Azure ticket","ticket-title":"Title","ticket-address":"Ticket address","azure-invalid-https-azure-organization":"Invalid HTTPS Azure organization.","azure-invalid-azure-project-uuid":"Invalid Azure project UUID.","azure-invalid-azure-token":"Invalid Azure token.","azure-invalid-azure-organization":"Invalid Azure organization.","azure-azure-could-not-be-read-or-took-too-long":"Azure could not be read or took too long.","azure-unexpected-paginated-azure-response-no-partial-r-adcfcd67":"Unexpected paginated Azure response. No partial result is shown.","azure-azure-response-interrupted":"Azure response interrupted.","azure-azure-response-is-too-large":"Azure response is too large.","azure-invalid-azure-response":"Invalid Azure response.","azure-the-azure-response-contains-unexpected-confidential-data":"The Azure response contains unexpected confidential data.","azure-invalid-azure-operation-identifier":"Invalid Azure operation identifier.","azure-the-ticket-cannot-contain-the-azure-token":"The ticket cannot contain the Azure token.","azure-invalid-azure-effect-lookup":"Invalid Azure effect lookup.","azure-multiple-tickets-carry-this-azure-operation-identity":"Multiple tickets carry this Azure operation identity.","azure-invalid-azure-ticket-observation":"Invalid Azure ticket observation.","azure-the-observed-azure-ticket-does-not-match-the-con-eafef4f4":"The observed Azure ticket does not match the confirmed creation.","azure-the-retrieved-azure-project-does-not-match-the-s-afc82078":"The retrieved Azure project does not match the selected project.","azure-the-azure-ticket-does-not-match-the-selected-project":"The Azure ticket does not match the selected project.","azure-invalid-azure-ticket-list":"Invalid Azure ticket list.","azure-incomplete-azure-ticket-list":"Incomplete Azure ticket list.","azure-repeated-azure-ticket":"Repeated Azure ticket.","azure-azure-ticket-outside-the-selection":"Azure ticket outside the selection.","azure-the-ticket-belongs-to-another-azure-organization-cc46f31b":"The ticket belongs to another Azure organization or project.","azure-azure-ticket-missing":"Azure ticket missing.","azure-invalid-azure-title-filter":"Invalid Azure title filter.","azure-azure-search-must-return-a-ticket-list":"Azure search must return a ticket list.","azure-invalid-azure-search-page":"Invalid Azure search page.","azure-azure-pagination-is-repeated-or-unordered":"Azure pagination is repeated or unordered.","azure-invalid-azure-ticket-id":"Invalid Azure ticket ID.","azure-invalid-azure-text":"Invalid Azure text.","azure-azure-text-is-too-large-or-invalid":"Azure text is too large or invalid.","azure-azure-description-is-too-large-after-conversion":"Azure description is too large after conversion.","azure-enter-an-explicit-azure-pat":"Enter an explicit Azure PAT.","azure-azure-operation-identifier-required":"Azure operation identifier required.","azure-azure-reconciliation-is-outside-the-confirmed-scope":"Azure reconciliation is outside the confirmed scope.","azure-invalid-azure-cursor":"Invalid Azure cursor.","azure-azure-instance-required":"Azure instance required.","azure-azure-id-required":"Azure ID required.","azure-azure-operation-unavailable":"Azure operation unavailable.","azure-connector-request-too-large":"Connector request is too large.","azure-invalid-azure-request":"Invalid Azure request.","azure-read-forbidden":"Azure refused the read. Check the PAT and its scope.","azure-rate-limited":"Azure limit reached. Try again later.","azure-response-rejected":"Azure response refused. No remote detail is shown."}}}"#).expect("static Azure connector manifest")
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
            return Err("azure-invalid-https-azure-organization".into());
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
            return Err("azure-invalid-https-azure-organization".into());
        }
        let project = uuid::Uuid::parse_str(project)
            .map_err(|_| "azure-invalid-azure-project-uuid")?
            .to_string();
        if token.is_empty()
            || token.len() > 4000
            || token.chars().any(char::is_whitespace)
            || base.as_str().contains(&token)
            || project.contains(&token)
        {
            return Err("azure-invalid-azure-token".into());
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
                .map_err(|_| "azure-invalid-azure-organization")?;
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
        self.response(request).await
    }
    async fn response(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let mut response = request
            .header(AUTHORIZATION, self.authorization.clone())
            .send()
            .await
            .map_err(|_| "azure-azure-could-not-be-read-or-took-too-long")?;
        if response.status().as_u16() != 200 {
            return Err(match response.status().as_u16() {
                401 | 403 => "azure-read-forbidden",
                429 => "azure-rate-limited",
                _ => "azure-response-rejected",
            }
            .into());
        }
        if response.headers().contains_key("x-ms-continuationtoken") {
            return Err("azure-unexpected-paginated-azure-response-no-partial-r-adcfcd67".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "azure-azure-response-interrupted")?
        {
            if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
                return Err("azure-azure-response-is-too-large".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "azure-invalid-azure-response")?;
        if exposes(&value, &self.token) {
            return Err("azure-the-azure-response-contains-unexpected-confidential-data".into());
        }
        Ok(value)
    }
    pub async fn create(&self, operation: &str, input: &Value, reconcile: bool) -> Result<Value> {
        if !protocol::identifier(operation) {
            return Err("azure-invalid-azure-operation-identifier".into());
        }
        let creation = protocol::ticket_creation(input)?;
        if exposes(input, &self.token) {
            return Err("azure-the-ticket-cannot-contain-the-azure-token".into());
        }
        let name = self.project_name().await?;
        let tag = format!("atelier-{operation}");
        let mut query_url = self.endpoint(true, &["_apis", "wit", "wiql"])?;
        query_url.query_pairs_mut().append_pair("$top", "2");
        let query = format!(
            "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = @project AND [System.Tags] CONTAINS '{tag}' ORDER BY [System.Id] ASC"
        );
        let found = self
            .request(query_url, Some(json!({"query":query})))
            .await?;
        if found["queryType"] != "flat" {
            return Err("azure-invalid-azure-effect-lookup".into());
        }
        let rows = found["workItems"]
            .as_array()
            .ok_or("azure-invalid-azure-effect-lookup")?;
        if rows.len() > 1 {
            return Err("azure-multiple-tickets-carry-this-azure-operation-identity".into());
        }
        let html = format!(
            "<div>{}</div>",
            html_escape::encode_text(&creation.description).replace('\n', "<br>")
        );
        let expected_description = plain_description(&html)?;
        let id = if let Some(row) = rows.first() {
            Some(number(&row["id"])?)
        } else if reconcile {
            None
        } else {
            let kind = format!("${}", creation.kind);
            let value = self
                .response(
                    self.client
                        .post(self.endpoint(true, &["_apis", "wit", "workitems", &kind])?)
                        .header(reqwest::header::CONTENT_TYPE, "application/json-patch+json")
                        .json(&json!([
                            {"op":"add","path":"/fields/System.Title","value":creation.title},
                            {"op":"add","path":"/fields/System.Description","value":html},
                            {"op":"add","path":"/fields/System.Tags","value":tag}
                        ])),
                )
                .await?;
            Some(number(&value["id"])?)
        };
        let observed_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        let Some(id) = id else {
            return Ok(
                json!({"receipt":{"operationId":operation,"outcome":"unknown","reference":operation,"observedAt":observed_at},"items":[]}),
            );
        };
        let value=self.request(self.endpoint(true,&["_apis","wit","workitemsbatch"])?,Some(json!({"ids":[id],"fields":["System.Id","System.Title","System.Description","System.State","System.TeamProject","System.Tags","System.WorkItemType"],"errorPolicy":"Fail"}))).await?;
        let rows = value["value"]
            .as_array()
            .ok_or("azure-invalid-azure-ticket-observation")?;
        if value["count"] != 1 || rows.len() != 1 || number(&rows[0]["id"])? != id {
            return Err("azure-invalid-azure-ticket-observation".into());
        }
        let row = &rows[0];
        let ticket = self.ticket(row, &name)?;
        if !row["fields"]["System.Tags"]
            .as_str()
            .is_some_and(|tags| tags.split(';').any(|value| value.trim() == tag))
            || row["fields"]["System.WorkItemType"].as_str() != Some(creation.kind.as_str())
            || ticket.title != creation.title
            || ticket.description != expected_description
        {
            return Err("azure-the-observed-azure-ticket-does-not-match-the-con-eafef4f4".into());
        }
        Ok(
            json!({"receipt":{"operationId":operation,"outcome":"applied","reference":ticket.url,"observedAt":observed_at},"ticket":ticket,"items":[{"label":"azure-ticket","value":ticket.key},{"label":"ticket-title","value":ticket.title},{"label":"ticket-address","value":ticket.url}]}),
        )
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
            return Err("azure-the-retrieved-azure-project-does-not-match-the-s-afc82078".into());
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
            return Err("azure-the-azure-ticket-does-not-match-the-selected-project".into());
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
            return Err("azure-the-azure-response-contains-unexpected-confidential-data".into());
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
            .ok_or("azure-invalid-azure-ticket-list")?;
        if value["count"].as_u64() != Some(rows.len() as u64) || rows.len() != ids.len() {
            return Err("azure-incomplete-azure-ticket-list".into());
        }
        let mut found = std::collections::BTreeMap::new();
        for row in rows {
            let ticket = self.ticket(row, name)?;
            if found.insert(ticket.id.clone(), ticket).is_some() {
                return Err("azure-repeated-azure-ticket".into());
            }
        }
        ids.iter()
            .map(|id| {
                found
                    .remove(&id.to_string())
                    .ok_or_else(|| "azure-azure-ticket-outside-the-selection".into())
            })
            .collect()
    }
    pub async fn read(&self, instance: &str, id: &str) -> Result<Ticket> {
        if instance != self.instance() {
            return Err("azure-the-ticket-belongs-to-another-azure-organization-cc46f31b".into());
        }
        let id = parse_id(id)?;
        let name = self.project_name().await?;
        self.details(&[id], &name)
            .await?
            .pop()
            .ok_or_else(|| "azure-azure-ticket-missing".into())
    }
    pub async fn search(&self, filter: &str, cursor: Option<&str>) -> Result<TicketPage> {
        if filter.encode_utf16().count() > 200 || filter.chars().any(char::is_control) {
            return Err("azure-invalid-azure-title-filter".into());
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
            return Err("azure-azure-search-must-return-a-ticket-list".into());
        }
        let rows = value["workItems"]
            .as_array()
            .filter(|rows| rows.len() <= PAGE + 1)
            .ok_or("azure-invalid-azure-search-page")?;
        let mut ids = Vec::new();
        let mut last = after;
        let mut seen = BTreeSet::new();
        for row in rows {
            let id = number(&row["id"])?;
            if id <= last || !seen.insert(id) {
                return Err("azure-azure-pagination-is-repeated-or-unordered".into());
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
        .ok_or_else(|| "azure-invalid-azure-ticket-id".into())
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
        .map_err(|_| "azure-invalid-azure-ticket-id")?;
    if id.to_string() != value {
        return Err("azure-invalid-azure-ticket-id".into());
    }
    number(&json!(id))
}
fn text(value: &Value, limit: usize, multiline: bool) -> Result<String> {
    let text = value.as_str().ok_or("azure-invalid-azure-text")?;
    if (!multiline && text.trim().is_empty())
        || text.encode_utf16().count() > limit
        || text
            .chars()
            .any(|c| c.is_control() && (!multiline || !matches!(c, '\r' | '\n' | '\t')))
    {
        return Err("azure-azure-text-is-too-large-or-invalid".into());
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
        return Err("azure-azure-description-is-too-large-after-conversion".into());
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
            .ok_or("azure-enter-an-explicit-azure-pat")?;
        let client = Azure::new(setting("url"), setting("project"), token)?;
        match request.method {
            Method::TicketsCreate => {
                client
                    .create(
                        request
                            .operation_id
                            .as_deref()
                            .ok_or("azure-azure-operation-identifier-required")?,
                        &request.input,
                        false,
                    )
                    .await?
            }
            Method::Reconcile => {
                if request.input["method"] != "ticketsCreate" || !request.input["action"].is_null()
                {
                    return Err("azure-azure-reconciliation-is-outside-the-confirmed-scope".into());
                }
                client
                    .create(
                        request
                            .operation_id
                            .as_deref()
                            .ok_or("azure-azure-operation-identifier-required")?,
                        &request.input["input"],
                        true,
                    )
                    .await?
            }
            Method::Check => {
                json!({"items":[{"label":"azure-project","value":client.project_name().await?}]})
            }
            Method::TicketsSearch => {
                let cursor = match request.input.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(value)) => Some(value.as_str()),
                    _ => return Err("azure-invalid-azure-cursor".into()),
                };
                serde_json::to_value(client.search(setting("filter"), cursor).await?)?
            }
            Method::TicketsRead => serde_json::to_value(
                client
                    .read(
                        request.input["instance"]
                            .as_str()
                            .ok_or("azure-azure-instance-required")?,
                        request.input["id"]
                            .as_str()
                            .ok_or("azure-azure-id-required")?,
                    )
                    .await?,
            )?,
            _ => return Err("azure-azure-operation-unavailable".into()),
        }
    };
    Ok(Reply {
        protocol: protocol::PROTOCOL.into(),
        request_id: request.request_id,
        module: request.module,
        method: request.method,
        status: Status::Ok,
        message: if matches!(request.method, Method::TicketsCreate | Method::Reconcile) {
            "creation-observed"
        } else {
            "read-complete"
        }
        .into(),
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
        return Err("azure-connector-request-too-large".into());
    }
    let request: Request =
        serde_json::from_slice(&input).map_err(|_| "azure-invalid-azure-request")?;
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
        return Err("azure-azure-response-is-too-large".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
