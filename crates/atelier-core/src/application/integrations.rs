use super::extensions::{Installed, Method};
use crate::document::Document;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Tickets,
    Time,
}
impl Kind {
    pub fn setting(self) -> &'static str {
        match self {
            Self::Tickets => "ticketProvider",
            Self::Time => "timeProvider",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kinds: Vec<Kind>,
}

pub fn selected(settings: &Document, kind: Kind) -> serde_json::Result<String> {
    Ok(settings
        .get(kind.setting())?
        .unwrap_or_else(|| "local".into()))
}

pub fn catalog(settings: &Document) -> serde_json::Result<Vec<Provider>> {
    let mut providers = vec![
        Provider {
            id: "local".into(),
            name: "message.local_only".into(),
            kinds: vec![Kind::Tickets, Kind::Time],
        },
        Provider {
            id: "jira".into(),
            name: "Jira".into(),
            kinds: vec![Kind::Tickets],
        },
        Provider {
            id: "tempo".into(),
            name: "Tempo".into(),
            kinds: vec![Kind::Time],
        },
    ];
    for installed in settings
        .get::<Vec<Installed>>("processConnectors")?
        .unwrap_or_default()
    {
        let methods = &installed.manifest.methods;
        let mut kinds = Vec::new();
        if methods.iter().any(|method| {
            matches!(
                method,
                Method::TicketsSearch | Method::TicketsRead | Method::TicketsCreate
            )
        }) {
            kinds.push(Kind::Tickets);
        }
        if [Method::TimePreview, Method::TimeSend, Method::TimeReconcile]
            .iter()
            .all(|method| methods.contains(method))
        {
            kinds.push(Kind::Time);
        }
        if !kinds.is_empty() {
            providers.push(Provider {
                id: format!("extension:{}", installed.manifest.id),
                name: installed.manifest.name,
                kinds,
            });
        }
    }
    Ok(providers)
}

pub fn available(settings: &Document, kind: Kind, id: &str) -> serde_json::Result<bool> {
    Ok(catalog(settings)?
        .iter()
        .any(|provider| provider.id == id && provider.kinds.contains(&kind)))
}

// Removing a module changes only future defaults; existing ticket/export identities stay intact.
pub fn removed(settings: &mut Document, module: &str) -> serde_json::Result<()> {
    for kind in [Kind::Tickets, Kind::Time] {
        if selected(settings, kind)? == format!("extension:{module}") {
            settings.set(kind.setting(), &"local")?;
        }
    }
    Ok(())
}

pub fn choose(
    settings: &Document,
    kind: Kind,
    provider: &str,
    expected: &str,
) -> super::task_creation::Result<Document> {
    use super::settings::SettingsFailure;
    if provider.len() > 80 || !available(settings, kind, provider)? {
        return Err(SettingsFailure::Invalid("message.integration_unavailable").into());
    }
    if selected(settings, kind)? != expected {
        return Err(SettingsFailure::Conflict.into());
    }
    let mut next = settings.clone();
    next.set(kind.setting(), &provider)?;
    Ok(next)
}
