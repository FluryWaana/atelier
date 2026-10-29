use super::{settings, store::Store};
use crate::Result;
use atelier_core::{
    application::{
        extensions::{
            self as protocol, Candidate, Connection, Installation, Installed, Registry, Report,
        },
        settings::SettingsUpdate,
    },
    document::Document,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn profiles(settings: &Document) -> Result<Vec<Installed>> {
    if settings
        .raw("processConnectors")
        .is_some_and(|raw| raw.get().len() > 2 * 1024 * 1024)
    {
        return Err("Les paramètres des connecteurs dépassent 2 Mio.".into());
    }
    let profiles = settings
        .get::<Vec<Installed>>("processConnectors")?
        .unwrap_or_default();
    if profiles.len() > 16 {
        return Err("Seize connecteurs de processus maximum sont autorisés.".into());
    }
    let mut ids = BTreeSet::new();
    for profile in &profiles {
        protocol::validate_manifest(&profile.manifest)?;
        if profile.revision == 0
            || profile.revision > revision(settings)?
            || !ids.insert(&profile.manifest.id)
            || profile.binary.path.is_empty()
            || profile.binary.path.len() > 32_000
            || profile.binary.path.chars().any(char::is_control)
            || profile.binary.sha256.len() != 64
            || !profile
                .binary
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("Registre de connecteurs invalide ou ambigu.".into());
        }
        let request = protocol::Request {
            protocol: protocol::PROTOCOL.into(),
            request_id: "validate-profile".into(),
            module: profile.manifest.id.clone(),
            method: protocol::Method::Check,
            operation_id: None,
            action: None,
            settings: profile.values.clone(),
            secrets: BTreeMap::new(),
            input: serde_json::Value::Null,
        };
        protocol::effect(&profile.manifest, &request)?;
    }
    Ok(profiles)
}
fn revision(settings: &Document) -> Result<u64> {
    Ok(settings.get::<u64>("connectorRevision")?.unwrap_or(0))
}
fn advance(settings: &mut Document) -> Result<u64> {
    let next = revision(settings)?
        .checked_add(1)
        .ok_or("Révision des connecteurs épuisée.")?;
    settings.set("connectorRevision", &next)?;
    Ok(next)
}
fn save_profiles(settings: &mut Document, profiles: &[Installed]) -> Result<()> {
    let bytes = serde_json::to_vec(profiles)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Les paramètres des connecteurs dépassent 2 Mio.".into());
    }
    settings.set("processConnectors", &profiles)?;
    self::profiles(settings)?;
    Ok(())
}
fn secret_name(id: &str, field: &str) -> String {
    format!("extension:{id}:{field}")
}
pub fn describe(
    store: &Store<'_>,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Registry> {
    let settings = store.settings::<Document>()?;
    let profiles = profiles(&settings)?;
    let tokens = settings::load(store, protect)?;
    let mut modules = Vec::new();
    for installed in profiles {
        let mut secret_fields = BTreeSet::new();
        for field in installed
            .manifest
            .fields
            .iter()
            .filter(|field| field.kind == protocol::FieldKind::Password)
        {
            if tokens
                .get::<String>(&secret_name(&installed.manifest.id, &field.id))?
                .is_some_and(|value| !value.is_empty())
            {
                secret_fields.insert(field.id.clone());
            }
        }
        modules.push(Connection {
            installed,
            secret_fields,
        });
    }
    Ok(Registry {
        revision: revision(&settings)?,
        native_available: true,
        modules,
        builtin_azure_path: {
            std::env::current_exe()
                .ok()
                .filter(|path| {
                    path.file_name().and_then(|name| name.to_str())
                        == Some(if cfg!(windows) {
                            "atelier.exe"
                        } else {
                            "atelier"
                        })
                })
                .map(|path| super::task_creation::path_string(&path).utf8())
                .transpose()?
        },
    })
}
pub fn require_registry(store: &Store<'_>, expected: u64) -> Result<()> {
    if revision(&store.settings::<Document>()?)? != expected {
        return Err("Les connecteurs ont changé. Recharge avant de réessayer.".into());
    }
    Ok(())
}
pub fn profile(store: &Store<'_>, id: &str, expected: u64) -> Result<Installed> {
    profiles(&store.settings::<Document>()?)?
        .into_iter()
        .find(|profile| profile.manifest.id == id && profile.revision == expected)
        .ok_or_else(|| {
            "Ce connecteur a changé ou a été retiré. Recharge avant de réessayer.".into()
        })
}
pub fn install(
    store: &Store<'_>,
    input: &Installation,
    actual: Candidate,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Registry> {
    require_registry(store, input.expected_revision)?;
    if !input.confirmed || serde_json::to_value(&actual)? != serde_json::to_value(&input.candidate)?
    {
        return Err("message.confirm_connector_manifest".into());
    }
    protocol::validate_manifest(&actual.manifest)?;
    let mut stored = store.settings::<Document>()?;
    let mut profiles = profiles(&stored)?;
    if profiles
        .iter()
        .any(|profile| profile.manifest.id == actual.manifest.id)
        || profiles.len() >= 16
    {
        return Err("Ce connecteur est déjà installé ou la limite de seize est atteinte.".into());
    }
    let forget = actual
        .manifest
        .fields
        .iter()
        .map(|field| secret_name(&actual.manifest.id, &field.id))
        .collect::<Vec<_>>();
    let revision = advance(&mut stored)?;
    profiles.push(Installed {
        manifest: actual.manifest,
        binary: actual.binary,
        revision,
        values: BTreeMap::new(),
        report: None,
    });
    save_profiles(&mut stored, &profiles)?;
    let mut tokens = settings::load(store, protect)?;
    for name in forget {
        tokens.remove(&name);
    }
    settings::persist(
        store,
        &SettingsUpdate {
            settings: stored,
            tokens,
        },
        protect,
    )?;
    describe(store, protect)
}

pub fn configure(
    store: &Store<'_>,
    id: &str,
    input: &protocol::Configuration,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Registry> {
    let original = profile(store, id, input.expected_revision)?;
    let mut stored = store.settings::<Document>()?;
    let mut profiles = profiles(&stored)?;
    let mut next = original.clone();
    let public = original
        .manifest
        .fields
        .iter()
        .filter(|field| field.kind != protocol::FieldKind::Password)
        .map(|field| field.id.as_str())
        .collect::<BTreeSet<_>>();
    if input
        .forget_values
        .iter()
        .any(|key| !public.contains(key.as_str()) || input.values.contains_key(key))
    {
        return Err("Paramètre de connecteur inconnu ou contradictoire.".into());
    }
    for key in &input.forget_values {
        next.values.remove(key);
    }
    for (key, value) in &input.values {
        next.values.insert(key.clone(), value.clone());
    }
    super::connector_effects::require_configuration(store, &next)?;
    let fields = original
        .manifest
        .fields
        .iter()
        .filter(|field| field.kind == protocol::FieldKind::Password)
        .map(|field| field.id.clone())
        .collect::<BTreeSet<_>>();
    if input.forget_secrets.iter().any(|id| !fields.contains(id))
        || input
            .secrets
            .keys()
            .any(|id| !fields.contains(id) || input.forget_secrets.contains(id))
        || input.secrets.values().any(|value| {
            value.is_empty() || value.len() > 4000 || value.contains(['\0', '\r', '\n'])
        })
    {
        return Err("Secret de connecteur inconnu ou contradictoire.".into());
    }
    let mut request = protocol::Request {
        protocol: protocol::PROTOCOL.into(),
        request_id: "validate-configuration".into(),
        module: id.into(),
        method: protocol::Method::Check,
        operation_id: None,
        action: None,
        settings: next.values.clone(),
        secrets: input.secrets.clone(),
        input: serde_json::Value::Null,
    };
    super::extensions::values(&next.manifest, &request)?;
    let mut tokens = settings::load(store, protect)?;
    let scope_changed = original.manifest.fields.iter().any(|field| {
        (field.kind == protocol::FieldKind::Url || field.credential_scope)
            && original.values.get(&field.id) != next.values.get(&field.id)
    });
    if scope_changed {
        for field in &fields {
            tokens.remove(&secret_name(id, field));
        }
    }
    for field in &input.forget_secrets {
        tokens.remove(&secret_name(id, field));
    }
    for (field, value) in &input.secrets {
        tokens.set(&secret_name(id, field), value)?;
    }
    let mut secrets = BTreeMap::new();
    for field in &fields {
        if let Some(value) = tokens
            .get::<String>(&secret_name(id, field))?
            .filter(|value| !value.is_empty())
        {
            secrets.insert(field.clone(), value);
        }
    }
    request.secrets = secrets;
    super::extensions::values(&next.manifest, &request)?;
    next.revision = advance(&mut stored)?;
    next.report = None;
    *profiles
        .iter_mut()
        .find(|profile| profile.manifest.id == id)
        .ok_or("Connecteur introuvable.")? = next;
    save_profiles(&mut stored, &profiles)?;
    settings::persist(
        store,
        &SettingsUpdate {
            settings: stored,
            tokens,
        },
        protect,
    )?;
    describe(store, protect)
}
pub fn remove(
    store: &Store<'_>,
    id: &str,
    expected: u64,
    confirmed: bool,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Registry> {
    if !confirmed {
        return Err("Confirme le retrait du connecteur et de ses secrets locaux. Les cartes sont conservées.".into());
    }
    let original = profile(store, id, expected)?;
    super::connector_effects::require_removal(store, id)?;
    let mut stored = store.settings::<Document>()?;
    let mut profiles = profiles(&stored)?;
    let mut tokens = settings::load(store, protect)?;
    for field in &original.manifest.fields {
        tokens.remove(&secret_name(id, &field.id));
    }
    profiles.retain(|profile| profile.manifest.id != id);
    advance(&mut stored)?;
    save_profiles(&mut stored, &profiles)?;
    settings::persist(
        store,
        &SettingsUpdate {
            settings: stored,
            tokens,
        },
        protect,
    )?;
    describe(store, protect)
}
pub fn invocation(
    store: &Store<'_>,
    id: &str,
    input: &protocol::Invocation,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<(Installed, protocol::Request)> {
    if !input.confirmed {
        return Err("Confirme l’appel du connecteur sélectionné.".into());
    }
    let profile = profile(store, id, input.expected_revision)?;
    let mut request = protocol::Request {
        protocol: protocol::PROTOCOL.into(),
        request_id: format!("request-{}", uuid::Uuid::new_v4()),
        module: id.into(),
        method: input.method,
        operation_id: None,
        action: input.action.clone(),
        settings: profile.values.clone(),
        secrets: BTreeMap::new(),
        input: input.input.clone(),
    };
    let effect = protocol::effect(&profile.manifest, &request)?;
    if effect != protocol::Effect::Read
        || ![
            protocol::Method::Check,
            protocol::Method::TicketsSearch,
            protocol::Method::TicketsRead,
            protocol::Method::TicketsTransitions,
            protocol::Method::TimePreview,
            protocol::Method::RepositoriesPreview,
            protocol::Method::RepositoriesRefresh,
            protocol::Method::RepositoriesReview,
            protocol::Method::Action,
        ]
        .contains(&request.method)
    {
        return Err("Cette opération de connecteur n’est pas disponible.".into());
    }
    if request.method == protocol::Method::TimePreview {
        request.input = super::connector_time::preview_input(store, &request.input)?;
    }
    if matches!(
        request.method,
        protocol::Method::RepositoriesPreview
            | protocol::Method::RepositoriesRefresh
            | protocol::Method::RepositoriesReview
    ) {
        request.input = serde_json::to_value(super::connector_repositories::selection(
            store,
            &profile,
            &request.input,
        )?)?;
    }
    if request.method == protocol::Method::TicketsTransitions {
        request.input =
            super::connector_ticket_transitions::read_input(store, &profile, &request.input)?;
    }
    let tokens = settings::load(store, protect)?;
    for field in profile
        .manifest
        .fields
        .iter()
        .filter(|field| field.kind == protocol::FieldKind::Password)
    {
        if let Some(value) = tokens
            .get::<String>(&secret_name(id, &field.id))?
            .filter(|value| !value.is_empty())
        {
            request.secrets.insert(field.id.clone(), value);
        }
    }
    protocol::effect(&profile.manifest, &request)?;
    Ok((profile, request))
}
pub fn record(store: &Store<'_>, id: &str, expected: u64, report: Report) -> Result<()> {
    protocol::report_items(&report)?;
    if matches!(
        report.method,
        protocol::Method::TicketsSearch | protocol::Method::TicketsRead
    ) {
        super::connector_tickets::page(&report)?;
    }
    if report.method == protocol::Method::TimePreview {
        super::connector_time::validate_report(store, &report)?;
    }
    if report.method == protocol::Method::TicketsTransitions {
        super::connector_ticket_transitions::report(&report.output)?;
    }
    store.transaction(|store| {
        let profile = profile(store, id, expected)?;
        if matches!(
            report.method,
            protocol::Method::RepositoriesPreview
                | protocol::Method::RepositoriesRefresh
                | protocol::Method::RepositoriesReview
        ) {
            super::connector_repositories::record(store, &profile, &report)?;
        }
        let mut stored = store.settings::<Document>()?;
        let mut profiles = profiles(&stored)?;
        profiles
            .iter_mut()
            .find(|profile| profile.manifest.id == id)
            .ok_or("Connecteur introuvable.")?
            .report = Some(report);
        save_profiles(&mut stored, &profiles)?;
        store.save_settings(&stored)
    })
}
pub(super) fn invalidate(stored: &mut Document, previous: &Document) -> Result<()> {
    let mut modules = profiles(stored)?;
    let minimum = revision(stored)?.max(revision(previous)?).max(
        modules
            .iter()
            .map(|profile| profile.revision)
            .max()
            .unwrap_or(0),
    );
    stored.set("connectorRevision", &minimum)?;
    let revision = advance(stored)?;
    for profile in &mut modules {
        profile.revision = revision;
        profile.report = None;
    }
    save_profiles(stored, &modules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
    fn candidate() -> Candidate {
        Candidate {binary:protocol::Binary {path:"C:/synthetic/module.exe".into(),sha256:"ab".repeat(32)},manifest:serde_json::from_value(json!({"id":"synthetic-demo","name":"Synthetic demo","description":"No account or remote effect","sourceLocale":"en-US","methods":["check"],"fields":[{"id":"site","label":"Site","kind":"url","credentialScope":true},{"id":"enabled","label":"Enabled","kind":"checkbox"},{"id":"token","label":"Token","kind":"password"}],"tabs":[],"translations":{}})).unwrap()}
    }
    #[test]
    fn registry_installation_removal_and_restore_never_reuse_a_previous_generation() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        store
            .save_settings(&json!({"future":{"keep":1},"taskTemplates":[]}))
            .unwrap();
        let valid = candidate();
        let input = Installation {
            expected_revision: 0,
            candidate: valid.clone(),
            confirmed: false,
        };
        assert!(
            install(&store, &input, valid.clone(), &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        let mut bad = valid.clone();
        bad.binary.sha256 = "invalid".into();
        let bad_input = Installation {
            expected_revision: 0,
            candidate: bad.clone(),
            confirmed: true,
        };
        assert!(
            install(&store, &bad_input, bad, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        assert!(describe(&store, &mut protect).unwrap().modules.is_empty());
        let input = Installation {
            confirmed: true,
            ..input
        };
        let installed = install(&store, &input, valid.clone(), &mut protect).unwrap();
        assert_eq!(installed.revision, 1);
        assert_eq!(installed.modules[0].installed.revision, 1);
        assert!(
            install(&store, &input, valid.clone(), &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        assert!(
            remove(&store, "synthetic-demo", 1, false, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        let removed = remove(&store, "synthetic-demo", 1, true, &mut protect).unwrap();
        assert_eq!(removed.revision, 2);
        assert!(removed.modules.is_empty());
        let input = Installation {
            expected_revision: 2,
            candidate: valid.clone(),
            confirmed: true,
        };
        let installed = install(&store, &input, valid, &mut protect).unwrap();
        assert_eq!(installed.modules[0].installed.revision, 3);
        assert!(profile(&store, "synthetic-demo", 1).is_err());
        let previous = store.settings::<Document>().unwrap();
        let mut restored = previous.clone();
        invalidate(&mut restored, &previous).unwrap();
        assert_eq!(revision(&restored).unwrap(), 4);
        assert_eq!(profiles(&restored).unwrap()[0].revision, 4);
        assert_eq!(
            restored.get::<serde_json::Value>("future").unwrap(),
            Some(json!({"keep":1}))
        );
        let mut unsupported = restored;
        unsupported.set("connectorRevision", &0).unwrap();
        assert!(profiles(&unsupported).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn connector_credentials_are_private_scoped_revisioned_and_atomic() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        super::super::token_vault::save_encrypted(
            &store,
            Some("synthetic-envelope:{\"jira\":\"synthetic-unrelated-jira\"}"),
        )
        .unwrap();
        let candidate = candidate();
        let input = Installation {
            expected_revision: 0,
            candidate: candidate.clone(),
            confirmed: true,
        };
        install(&store, &input, candidate, &mut protect).unwrap();
        let mut update = protocol::Configuration {
            expected_revision: 1,
            values: BTreeMap::from([("site".into(), json!("https://synthetic-one.example.test"))]),
            forget_values: BTreeSet::new(),
            secrets: BTreeMap::from([("token".into(), "synthetic-module-key".into())]),
            forget_secrets: BTreeSet::new(),
        };
        let saved = configure(&store, "synthetic-demo", &update, &mut protect).unwrap();
        assert_eq!(
            saved.modules[0].secret_fields,
            BTreeSet::from(["token".into()])
        );
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains("synthetic-module-key")
        );
        assert!(
            !serde_json::to_string(&store.settings::<Document>().unwrap())
                .unwrap()
                .contains("synthetic-module-key")
        );
        assert!(
            configure(&store, "synthetic-demo", &update, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        let call = protocol::Invocation {
            expected_revision: 2,
            method: protocol::Method::Check,
            action: None,
            input: serde_json::Value::Null,
            confirmed: true,
        };
        let (_, request) = invocation(&store, "synthetic-demo", &call, &mut protect).unwrap();
        assert_eq!(
            request.secrets,
            BTreeMap::from([("token".into(), "synthetic-module-key".into())])
        );
        let mut unknown = call.clone();
        unknown.method = protocol::Method::TimeSend;
        assert!(
            invocation(&store, "synthetic-demo", &unknown, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        update.expected_revision = 2;
        update.values.insert("enabled".into(), json!("false"));
        assert!(
            configure(&store, "synthetic-demo", &update, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        update.values.remove("enabled");
        update
            .values
            .insert("token".into(), json!("must-not-be-public"));
        assert!(
            configure(&store, "synthetic-demo", &update, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        update.values.remove("token");
        update
            .values
            .insert("site".into(), json!("http://synthetic-one.example.test"));
        assert!(
            configure(&store, "synthetic-demo", &update, &mut |_, _| panic!(
                "Unexpected vault access"
            ))
            .is_err()
        );
        update
            .values
            .insert("site".into(), json!("https://synthetic-two.example.test"));
        update.secrets.clear();
        let cleared = configure(&store, "synthetic-demo", &update, &mut protect).unwrap();
        assert_eq!(cleared.modules[0].installed.revision, 3);
        assert!(cleared.modules[0].secret_fields.is_empty());
        assert_eq!(
            settings::load(&store, &mut protect)
                .unwrap()
                .get::<String>("jira")
                .unwrap()
                .as_deref(),
            Some("synthetic-unrelated-jira")
        );
        let before = store.settings::<Document>().unwrap();
        let encrypted = super::super::token_vault::read_encrypted(&store).unwrap();
        update.expected_revision = 3;
        update
            .secrets
            .insert("token".into(), "synthetic-new-key".into());
        assert!(
            configure(
                &store,
                "synthetic-demo",
                &update,
                &mut |value, decrypt| if decrypt {
                    protect(value, true)
                } else {
                    Err("Synthetic vault failure".into())
                }
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(store.settings::<Document>().unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        assert_eq!(
            super::super::token_vault::read_encrypted(&store).unwrap(),
            encrypted
        );
        update.values.clear();
        update.forget_values.insert("site".into());
        update.secrets.clear();
        let reset = configure(&store, "synthetic-demo", &update, &mut protect).unwrap();
        assert!(!reset.modules[0].installed.values.contains_key("site"));
    }
}
