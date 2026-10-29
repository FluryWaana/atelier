use super::{jira, settings, store::Store};
use crate::Result;
use atelier_core::{
    contracts::{JiraConfiguration, TempoConfiguration, TempoConfigurationUpdate, TempoConnection},
    document::Document,
};
mod client;
pub use client::Tempo;
pub(super) use client::validate as validate_payload;

pub fn credentials(
    store: &Store<'_>,
    expected: u64,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<(TempoConfiguration, String)> {
    let configuration = store
        .settings::<Document>()?
        .get::<TempoConfiguration>("tempoConnection")?
        .unwrap_or_default();
    if configuration.revision != expected {
        return Err("message.the_tempo_connection_changed_reload_before_trying_again".into());
    }
    let token = settings::load(store, protect)?
        .get::<String>("tempo")?
        .filter(|token| !token.is_empty())
        .ok_or("message.enter_the_tempo_cloud_token_in_settings")?;
    Ok((configuration, token))
}

pub const BASES: [&str; 3] = [
    "https://api.tempo.io/4",
    "https://api.eu.tempo.io/4",
    "https://api.us.tempo.io/4",
];

pub fn describe(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<TempoConnection> {
    let stored = store.settings::<Document>()?;
    let configuration = stored
        .get::<TempoConfiguration>("tempoConnection")?
        .unwrap_or_default();
    let jira = stored
        .get::<JiraConfiguration>("jiraConnection")?
        .unwrap_or_default();
    let token_set = settings::load(store, protect)?
        .get::<String>("tempo")?
        .is_some_and(|token| !token.is_empty());
    Ok(TempoConnection {
        configuration,
        token_set,
        jira_url: jira.url,
        jira_revision: jira.revision,
    })
}

pub fn configure(
    store: &Store<'_>,
    input: TempoConfigurationUpdate,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<TempoConnection> {
    if !BASES.contains(&input.base.as_str())
        || (input.clear_token && input.token.is_some())
        || input.token.as_ref().is_some_and(|token| {
            token.is_empty()
                || token.len() > 4000
                || token.chars().any(char::is_whitespace)
                || token.chars().any(char::is_control)
        })
    {
        return Err(
            "message.invalid_tempo_settings_choose_a_listed_site_and_a_valid_token".into(),
        );
    }
    jira::require_revision(store, input.expected_jira_revision)?;
    let mut stored = store.settings::<Document>()?;
    let previous = stored
        .get::<TempoConfiguration>("tempoConnection")?
        .unwrap_or_default();
    if previous.revision != input.expected_revision {
        return Err("message.the_tempo_connection_changed_reload_before_trying_again".into());
    }
    let mut tokens = settings::load(store, protect)?;
    if input.clear_token || input.base != previous.base {
        tokens.set("tempo", &"")?;
    }
    if let Some(token) = input.token {
        tokens.set("tempo", &token)?;
    }
    let configuration = TempoConfiguration {
        base: input.base,
        revision: previous
            .revision
            .checked_add(1)
            .ok_or("message.tempo_revision_exhausted")?,
    };
    stored.set("tempoConnection", &configuration)?;
    settings::persist(
        store,
        &atelier_core::application::settings::SettingsUpdate {
            settings: stored,
            tokens,
        },
        protect,
    )?;
    describe(store, protect)
}

pub(super) fn invalidate(stored: &mut Document, previous: Option<&Document>) -> Result<()> {
    let mut configuration = stored
        .get::<TempoConfiguration>("tempoConnection")?
        .unwrap_or_default();
    let minimum = previous
        .map(|previous| previous.get::<TempoConfiguration>("tempoConnection"))
        .transpose()?
        .flatten()
        .unwrap_or_default()
        .revision;
    configuration.revision = configuration
        .revision
        .max(minimum)
        .checked_add(1)
        .ok_or("message.tempo_revision_exhausted")?;
    stored.set("tempoConnection", &configuration)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn protect(value: &str, decrypt: bool) -> Result<String> {
        if decrypt {
            Ok(value
                .strip_prefix("synthetic-envelope:")
                .ok_or("Synthetic envelope required")?
                .into())
        } else {
            Ok(format!("synthetic-envelope:{value}"))
        }
    }
    fn input(revision: u64) -> TempoConfigurationUpdate {
        TempoConfigurationUpdate {
            expected_revision: revision,
            expected_jira_revision: 0,
            base: BASES[0].into(),
            token: Some("synthetic-tempo-secret".into()),
            clear_token: false,
        }
    }
    #[test]
    fn configuration_is_scoped_revisioned_and_atomic_with_its_vault() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let saved = configure(&store, input(0), &mut protect).unwrap();
        assert_eq!(saved.configuration.revision, 1);
        assert!(saved.token_set);
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains("synthetic-tempo-secret")
        );
        assert!(configure(&store, input(0), &mut protect).is_err());
        let mut stale_jira = input(1);
        stale_jira.expected_jira_revision = 1;
        assert!(configure(&store, stale_jira, &mut protect).is_err());
        let mut region = input(1);
        region.base = BASES[1].into();
        region.token = None;
        let cleared = configure(&store, region, &mut protect).unwrap();
        assert_eq!(cleared.configuration.revision, 2);
        assert!(!cleared.token_set);
        let before = store.settings::<Document>().unwrap();
        let vault_before = super::super::token_vault::read_encrypted(&store).unwrap();
        assert!(
            configure(&store, input(2), &mut |value, decrypt| if decrypt {
                protect(value, true)
            } else {
                Err("Synthetic vault failure".into())
            })
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(store.settings::<Document>().unwrap()).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert_eq!(
            super::super::token_vault::read_encrypted(&store).unwrap(),
            vault_before
        );
        let mut restored = Document::default();
        invalidate(&mut restored, Some(&before)).unwrap();
        assert_eq!(
            restored
                .get::<TempoConfiguration>("tempoConnection")
                .unwrap()
                .unwrap()
                .revision,
            3
        );
        super::super::settings::remove_token(&store, "tempo", &mut protect).unwrap();
        assert_eq!(
            describe(&store, &mut protect)
                .unwrap()
                .configuration
                .revision,
            3
        );
    }
    #[test]
    fn arbitrary_sites_and_invalid_tokens_are_refused_before_reading_the_vault() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        for site in [
            "http://api.tempo.io/4",
            "https://api.tempo.io.evil.test/4",
            "https://api.tempo.io/4/",
            "https://user@api.tempo.io/4",
        ] {
            let mut supplied = input(0);
            supplied.base = site.into();
            assert!(
                configure(&store, supplied, &mut |_, _| panic!(
                    "Unexpected vault access"
                ))
                .is_err()
            );
        }
        for token in ["", "synthetic-\r\n-token", " synthetic-token"] {
            let mut supplied = input(0);
            supplied.token = Some(token.into());
            assert!(
                configure(&store, supplied, &mut |_, _| panic!(
                    "Unexpected vault access"
                ))
                .is_err()
            );
        }
    }
}
