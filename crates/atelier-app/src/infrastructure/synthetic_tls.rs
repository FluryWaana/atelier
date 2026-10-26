use crate::Result;
use reqwest::Url;
use std::time::Duration;

pub(super) fn client(address: &str, token: &str, authority: &[u8]) -> Result<reqwest::Client> {
    let base = Url::parse(address)?;
    if address.len() > 2000
        || address.chars().any(char::is_control)
        || base.scheme() != "https"
        || !base
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback())
        || !base.port().is_some_and(|port| port != 0)
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || !token.starts_with("synthetic-")
        || authority.len() > 65_536
    {
        return Err("Serveur TLS synthétique loopback et ancre explicite requis.".into());
    }
    Ok(reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .tls_certs_only([reqwest::Certificate::from_pem(authority)?])
        .user_agent("atelier-kanban-rust")
        .build()?)
}
