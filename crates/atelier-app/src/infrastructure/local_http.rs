use super::session;
use crate::Result;
use reqwest::{
    Client, Method, StatusCode, Url,
    header::{AUTHORIZATION, CONNECTION, HeaderMap},
};
use serde_json::Value;
use std::time::Duration;

pub struct LocalHttp {
    client: Client,
}

pub struct LocalResponse {
    pub status: StatusCode,
    body: Vec<u8>,
}

impl LocalResponse {
    pub fn json(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

pub fn server_port(address: &str) -> Result<u16> {
    let port = address
        .strip_prefix("http://127.0.0.1:")
        .ok_or("Adresse du serveur local invalide.")?;
    if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Adresse du serveur local invalide.".into());
    }
    let port: u16 = port
        .parse()
        .map_err(|_| "Adresse du serveur local invalide.")?;
    if port == 0 {
        return Err("Adresse du serveur local invalide.".into());
    }
    Ok(port)
}

fn disconnected(error: &(dyn std::error::Error + 'static)) -> bool {
    if let Some(error) = error.downcast_ref::<hyper::Error>()
        && error.is_incomplete_message()
    {
        return true;
    }
    if let Some(error) = error.downcast_ref::<reqwest::Error>()
        && error.is_connect()
        && !error.is_timeout()
    {
        return true;
    }
    if let Some(error) = error.downcast_ref::<std::io::Error>()
        && matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset
        )
    {
        return true;
    }
    error.source().is_some_and(disconnected)
}

impl LocalHttp {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .pool_max_idle_per_host(0)
                .build()?,
        })
    }

    pub async fn request(
        &self,
        address: &str,
        method: Method,
        headers: HeaderMap,
        body: Option<&str>,
        timeout: Duration,
    ) -> Result<LocalResponse> {
        let address = Url::parse(address).map_err(|_| "Adresse du serveur local invalide.")?;
        if address.scheme() != "http"
            || address.host_str() != Some("127.0.0.1")
            || !address.username().is_empty()
            || address.password().is_some()
            || address.fragment().is_some()
        {
            return Err("Adresse du serveur local invalide.".into());
        }
        let mut request = self
            .client
            .request(method, address)
            .headers(headers)
            .header(CONNECTION, "close")
            .timeout(timeout);
        if let Some(body) = body {
            request = request.body(body.to_owned());
        }
        let mut response = request.send().await?;
        if response.status().is_redirection() {
            return Err("Redirection du serveur local refusée.".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > 65_536 - body.len() {
                return Err("Réponse du serveur local trop volumineuse.".into());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(LocalResponse {
            status: response.status(),
            body,
        })
    }

    pub async fn authenticated_health(
        &self,
        address: &str,
        load_token: impl FnOnce() -> Result<Option<String>>,
    ) -> Result<Option<Value>> {
        let port = server_port(address)?;
        match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => {
                drop(listener);
                return Ok(None);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(error) => return Err(error.into()),
        }
        let address = format!("http://127.0.0.1:{port}");
        let challenge = session::new_token()?;
        // Only this credential-free proof GET may repeat; authenticated reads and mutations do not.
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            match self
                .request(
                    &format!("{address}/api/health?challenge={challenge}"),
                    Method::GET,
                    HeaderMap::new(),
                    None,
                    Duration::from_secs(2),
                )
                .await
            {
                Ok(response) => break response,
                Err(error)
                    if error
                        .downcast_ref::<reqwest::Error>()
                        .is_some_and(reqwest::Error::is_timeout) =>
                {
                    if attempt == 3 {
                        return Err(error);
                    }
                }
                Err(error) if disconnected(error.as_ref()) => return Ok(None),
                Err(error) => return Err(error),
            }
        };
        if !response.status.is_success() {
            return Err(
                "Identité du serveur local non vérifiée. Ferme le programme qui utilise ce port."
                    .into(),
            );
        }
        let data = response.json()?;
        let proof = data["proof"]
            .as_str()
            .filter(|proof| proof.len() == 64 && proof.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or(
                "Identité du serveur local non vérifiée. Ferme le programme qui utilise ce port.",
            )?;
        // Read only after the server answers: startup binds the port before writing the session.
        let token = load_token()?.filter(|token| !token.is_empty()).ok_or(
            "Identité du serveur local non vérifiée. Ferme le programme qui utilise ce port.",
        )?;
        if !session::same_secret(proof, &session::proof(&token, &challenge, port)?) {
            return Err(
                "Identité du serveur local non vérifiée. Ferme le programme qui utilise ce port."
                    .into(),
            );
        }
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Bearer {token}").parse()?);
        let response = self
            .request(
                &format!("{address}/api/health"),
                Method::GET,
                headers,
                None,
                Duration::from_secs(2),
            )
            .await?;
        if !response.status.is_success() {
            return Err("Session locale refusée. Rouvre Atelier avec son lanceur.".into());
        }
        Ok(Some(response.json()?))
    }
}
