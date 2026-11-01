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

/// Classifies at most eight causes without exposing error messages, URLs or sessions.
pub fn transport_error_details(error: &(dyn std::error::Error + 'static)) -> String {
    let mut causes = Vec::new();
    let mut current = Some(error);
    for _ in 0..8 {
        let Some(error) = current else { break };
        causes.push(
            if let Some(error) = error.downcast_ref::<reqwest::Error>() {
                format!(
                    "reqwest(timeout={}, connect={}, request={}, body={}, decode={})",
                    error.is_timeout(),
                    error.is_connect(),
                    error.is_request(),
                    error.is_body(),
                    error.is_decode()
                )
            } else if let Some(error) = error.downcast_ref::<hyper::Error>() {
                format!(
                    "hyper(incomplete={}, closed={}, canceled={})",
                    error.is_incomplete_message(),
                    error.is_closed(),
                    error.is_canceled()
                )
            } else if let Some(error) = error.downcast_ref::<std::io::Error>() {
                format!("io(kind={:?}, os={:?})", error.kind(), error.raw_os_error())
            } else {
                "cause non classée".into()
            },
        );
        current = error.source();
    }
    if current.is_some() {
        causes.push("causes suivantes omises".into());
    }
    causes.join(" | ")
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
            std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
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

#[cfg(test)]
mod tests {
    use std::io::{Error, ErrorKind};

    #[test]
    fn transport_diagnostics_preserve_nested_io_codes_without_error_text_or_unbounded_sources() {
        #[derive(Debug)]
        struct PrivateCause(Box<dyn std::error::Error>);
        impl std::fmt::Display for PrivateCause {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("synthetic-secret http://127.0.0.1/?session=synthetic-secret")
            }
        }
        impl std::error::Error for PrivateCause {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(self.0.as_ref())
            }
        }
        let mut error: Box<dyn std::error::Error> = Box::new(PrivateCause(Box::new(
            std::io::Error::from_raw_os_error(10053),
        )));
        let diagnostic = super::transport_error_details(error.as_ref());
        assert!(diagnostic.contains("io(kind="));
        assert!(diagnostic.contains("os=Some(10053)"));
        assert!(!diagnostic.contains("synthetic-secret"));
        assert!(!diagnostic.contains("http"));
        for _ in 0..20 {
            error = Box::new(PrivateCause(error));
        }
        let diagnostic = super::transport_error_details(error.as_ref());
        assert_eq!(diagnostic.split(" | ").count(), 9);
        assert!(diagnostic.ends_with("causes suivantes omises"));
        assert!(diagnostic.len() < 1024);
    }

    #[test]
    fn disconnected_recognizes_transport_breaks_but_not_unknown_server_states() {
        for (kind, expected) in [
            (ErrorKind::ConnectionRefused, true),
            (ErrorKind::ConnectionReset, true),
            (ErrorKind::ConnectionAborted, true),
            (ErrorKind::TimedOut, false),
            (ErrorKind::PermissionDenied, false),
            (ErrorKind::InvalidData, false),
        ] {
            assert_eq!(
                super::disconnected(&Error::from(kind)),
                expected,
                "{kind:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn winsock_aborted_connection_is_a_transport_break() {
        let error = Error::from_raw_os_error(10053);
        assert_eq!(error.kind(), ErrorKind::ConnectionAborted);
        assert!(super::disconnected(&error));
    }
}
