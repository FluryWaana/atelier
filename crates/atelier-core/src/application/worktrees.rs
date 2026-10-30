use super::card_lifecycle::CardFailure;
use crate::Result;
use crate::{
    contracts::{Card, ForgeKind, Worktree},
    document::Document,
    domain::projects::same_repository,
};
use std::collections::BTreeMap;

pub type ServiceWorktrees = BTreeMap<String, Worktree>;

pub fn service_repositories(
    project: Option<&Document>,
    primary: &Worktree,
    windows: bool,
) -> Result<BTreeMap<String, String>> {
    let mut sources = BTreeMap::new();
    if let Some(project) = project {
        for role in ["front", "back"] {
            let repo = project
                .get::<String>(&format!("{role}Repo"))?
                .unwrap_or_default();
            let command = project
                .get::<String>(&format!("{role}Command"))?
                .unwrap_or_default();
            if !command.is_empty()
                && !repo.is_empty()
                && !same_repository(&repo, &primary.source, windows)
            {
                sources.insert(role.into(), repo);
            }
        }
    }
    Ok(sources)
}

pub fn associate_services(
    mut card: Card,
    primary: &Worktree,
    expected: &ServiceWorktrees,
    next: &ServiceWorktrees,
    timestamp: String,
    windows: bool,
) -> Result<Card> {
    if card.fields.get::<Worktree>("worktree")?.as_ref() != Some(primary)
        || card
            .fields
            .get::<ServiceWorktrees>("serviceWorktrees")?
            .unwrap_or_default()
            != *expected
    {
        return Err(CardFailure::Conflict(
            "message.the_worktree_association_changed_refresh_the_task",
        )
        .into());
    }
    for (role, worktree) in next {
        if !matches!(role.as_str(), "front" | "back")
            || worktree.branch != primary.branch
            || same_repository(&worktree.source, &primary.source, windows)
            || same_repository(&worktree.path, &primary.path, windows)
            || next.values().any(|other| {
                same_repository(&other.source, &worktree.source, windows) && other != worktree
            })
        {
            return Err(
                CardFailure::Invalid("message.inconsistent_service_worktree_association").into(),
            );
        }
    }
    if next.is_empty() {
        card.fields.remove("serviceWorktrees");
    } else {
        card.fields.set("serviceWorktrees", next)?;
    }
    card.fields.set("updatedAt", &timestamp)?;
    Ok(card)
}

pub fn interrupted(card: &Card) -> Result<bool> {
    let primary = card.fields.get::<Worktree>("worktree")?;
    let services = card
        .fields
        .get::<ServiceWorktrees>("serviceWorktrees")?
        .unwrap_or_default();
    Ok(primary
        .iter()
        .chain(services.values())
        .any(|worktree| worktree.state != crate::contracts::WorktreeState::Ready))
}

pub struct CreateOptions {
    pub repo: String,
    pub branch: String,
    pub base: String,
    pub fetch_origin: bool,
    pub fetch_forge: Option<ForgeKind>,
    pub fetch_repository: String,
    pub forge_revision: Option<u64>,
}

pub fn validate_reference(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 180
        || value.starts_with('-')
        || value.contains(['\0', '\n', '\r'])
        || value.contains("@{")
    {
        return Err(
            CardFailure::Invalid("message.invalid_branch_name_or_starting_reference").into(),
        );
    }
    Ok(())
}

pub fn create_options(card: &Card, settings: &Document, body: &Document) -> Result<CreateOptions> {
    if card.fields.flag("archived")? {
        return Err(CardFailure::Conflict("message.restore_this_task_before_resuming_work").into());
    }
    let repo = card
        .fields
        .get::<String>("repo")?
        .ok_or("message.choose_the_source_git_repository_for_this_card")?
        .clone();
    let branch = body
        .get::<String>("branch")?
        .filter(|value| !value.is_empty())
        .or(card
            .fields
            .get::<String>("branch")?
            .filter(|value| !value.is_empty()))
        .unwrap_or_else(|| format!("atelier/{}", card.id));
    let base = body
        .get::<String>("base")?
        .filter(|value| !value.is_empty())
        .or(card
            .fields
            .get::<String>("baseBranch")?
            .filter(|value| !value.is_empty()))
        .or(settings
            .get::<String>("baseBranch")?
            .filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "HEAD".into());
    validate_reference(&branch)?;
    validate_reference(&base)?;
    let fetch_origin = body.get::<bool>("fetchOrigin")?.unwrap_or(false);
    let fetch_forge = body.get::<Option<ForgeKind>>("fetchForge")?.flatten();
    let fetch_repository = body.get::<String>("fetchRepository")?.unwrap_or_default();
    let forge_revision = body.get::<Option<u64>>("forgeRevision")?.flatten();
    if let Some(kind) = fetch_forge {
        if !fetch_origin || forge_revision.is_none() {
            return Err(
                "message.confirm_refreshing_and_reload_the_forge_connection_before_the_ht".into(),
            );
        }
        super::forge::repository(kind, &fetch_repository, 1)?;
    } else if !fetch_repository.is_empty() || forge_revision.is_some() {
        return Err("message.select_the_forge_for_the_https_fetch_or_remove_its_parameters".into());
    }
    Ok(CreateOptions {
        repo,
        branch,
        base,
        fetch_origin,
        fetch_forge,
        fetch_repository,
        forge_revision,
    })
}

pub fn removal(body: &Document) -> Result<bool> {
    if body.get::<bool>("confirmed")? != Some(true) {
        return Err(CardFailure::Invalid(
            "message.confirm_worktree_removal_its_branch_will_be_preserved",
        )
        .into());
    }
    Ok(body.get::<bool>("discardChanges")?.unwrap_or(false))
}

pub fn associate(
    mut card: Card,
    expected: Option<&Worktree>,
    next: Option<&Worktree>,
    timestamp: String,
    windows: bool,
) -> Result<Card> {
    if card.fields.get::<Worktree>("worktree")?.as_ref() != expected {
        return Err(CardFailure::Conflict(
            "message.the_worktree_association_changed_refresh_the_task",
        )
        .into());
    }
    if let Some(worktree) = next {
        let repo = card
            .fields
            .get::<String>("repo")?
            .ok_or("message.repository_missing")?
            .clone();
        if !same_repository(&repo, &worktree.source, windows) {
            return Err(CardFailure::Conflict(
                "message.the_task_repository_changed_during_the_operation",
            )
            .into());
        }
        card.fields.set("branch", &worktree.branch)?;
        card.fields.set("worktree", worktree)?;
    } else {
        if !card
            .fields
            .get::<ServiceWorktrees>("serviceWorktrees")?
            .unwrap_or_default()
            .is_empty()
        {
            return Err(CardFailure::Conflict(
                "message.remove_the_service_worktrees_before_the_main_worktree",
            )
            .into());
        }
        card.fields.remove("worktree");
    }
    card.fields.set("updatedAt", &timestamp)?;
    Ok(card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::WorktreeState;
    use serde_json::json;

    #[test]
    fn shared_service_associations_preserve_primary_and_refuse_divergent_aliases() {
        let primary = Worktree {
            path: "/work/main".into(),
            source: "/repo".into(),
            branch: "task".into(),
            base: "HEAD".into(),
            base_commit: "a".repeat(40),
            state: WorktreeState::Ready,
        };
        let service = Worktree {
            path: "/work/service".into(),
            source: "/service".into(),
            state: WorktreeState::Creating,
            ..primary.clone()
        };
        let card: Card = serde_json::from_str(
            &json!({"id":"task","title":"Synthetic","repo":"/repo","worktree":primary}).to_string(),
        )
        .unwrap();
        let next = ServiceWorktrees::from([
            ("front".into(), service.clone()),
            ("back".into(), service.clone()),
        ]);
        let associated = associate_services(
            card.clone(),
            &primary,
            &ServiceWorktrees::new(),
            &next,
            "now".into(),
            false,
        )
        .unwrap();
        assert!(interrupted(&associated).unwrap());
        assert!(
            associate(
                associated.clone(),
                Some(&primary),
                None,
                "now".into(),
                false
            )
            .is_err()
        );
        assert!(
            associate_services(
                associated.clone(),
                &primary,
                &ServiceWorktrees::new(),
                &next,
                "now".into(),
                false
            )
            .is_err()
        );
        let mut divergent = next.clone();
        divergent.get_mut("front").unwrap().path = "/work/other".into();
        assert!(
            associate_services(
                associated.clone(),
                &primary,
                &next,
                &divergent,
                "now".into(),
                false
            )
            .is_err()
        );
        let cleared = associate_services(
            associated,
            &primary,
            &next,
            &ServiceWorktrees::new(),
            "now".into(),
            false,
        )
        .unwrap();
        assert!(!interrupted(&cleared).unwrap());
        assert_eq!(
            cleared.fields.get::<Worktree>("worktree").unwrap(),
            Some(primary.clone())
        );
        let project: Document = serde_json::from_str(&json!({"frontRepo":"/repo","frontCommand":"synthetic","backRepo":"/service","backCommand":"synthetic"}).to_string()).unwrap();
        assert_eq!(
            service_repositories(Some(&project), &primary, false).unwrap(),
            BTreeMap::from([("back".into(), "/service".into())])
        );
    }

    #[test]
    fn authenticated_fetch_requires_an_explicit_scope_and_credential_revision() {
        let card: Card = serde_json::from_str(
            &json!({"id":"task","title":"Synthetic","repo":"/repo"}).to_string(),
        )
        .unwrap();
        for body in [
            json!({"fetchForge":"github","fetchRepository":"owner/repo","forgeRevision":1}),
            json!({"fetchOrigin":true,"fetchForge":"github","fetchRepository":"owner/repo"}),
            json!({"fetchOrigin":true,"fetchRepository":"owner/repo","forgeRevision":1}),
            json!({"fetchOrigin":true,"fetchForge":"github","fetchRepository":"../repo","forgeRevision":1}),
            json!({"fetchOrigin":true,"fetchForge":"github","fetchRepository":"owner/repo","forgeRevision":"1"}),
        ] {
            assert!(
                create_options(
                    &card,
                    &Document::default(),
                    &serde_json::from_str(&body.to_string()).unwrap()
                )
                .is_err()
            );
        }
        let body = serde_json::from_str(&json!({"fetchOrigin":true,"fetchForge":"gitlab","fetchRepository":"group/nested/repo","forgeRevision":7}).to_string()).unwrap();
        let options = create_options(&card, &Document::default(), &body).unwrap();
        assert_eq!(options.fetch_forge, Some(ForgeKind::Gitlab));
        assert_eq!(options.forge_revision, Some(7));
        assert_eq!(options.fetch_repository, "group/nested/repo");
    }

    #[test]
    fn confirmations_are_explicit_and_stale_associations_cannot_overwrite_a_task() {
        for body in [
            json!({}),
            json!({"confirmed":false}),
            json!({"confirmed":"true"}),
            json!({"confirmed":true,"discardChanges":"true"}),
        ] {
            assert!(removal(&serde_json::from_str(&body.to_string()).unwrap()).is_err());
        }
        assert!(!removal(&serde_json::from_str(r#"{"confirmed":true}"#).unwrap()).unwrap());
        let card: Card = serde_json::from_str(
            &json!({"id":"task","title":"Synthetic","repo":"/repo","description":"keep"})
                .to_string(),
        )
        .unwrap();
        let options = create_options(&card, &Document::default(), &Document::default()).unwrap();
        assert!(!options.fetch_origin);
        assert!(
            create_options(
                &card,
                &Document::default(),
                &serde_json::from_str(r#"{"fetchOrigin":true}"#).unwrap()
            )
            .unwrap()
            .fetch_origin
        );
        assert!(
            create_options(
                &card,
                &Document::default(),
                &serde_json::from_str(r#"{"fetchOrigin":"true"}"#).unwrap()
            )
            .is_err()
        );
        assert_eq!(
            (options.branch.as_str(), options.base.as_str()),
            ("atelier/task", "HEAD")
        );
        let worktree = Worktree {
            path: "/managed/worktree".into(),
            source: "/repo".into(),
            branch: options.branch,
            base: options.base,
            base_commit: "a".repeat(40),
            state: WorktreeState::Creating,
        };
        let reserved = associate(card.clone(), None, Some(&worktree), "now".into(), false).unwrap();
        assert!(associate(reserved.clone(), None, None, "later".into(), false).is_err());
        let mut relocated = reserved.clone();
        relocated.fields.set("repo", &"/different").unwrap();
        assert!(
            associate(
                relocated,
                Some(&worktree),
                Some(&worktree),
                "later".into(),
                false
            )
            .is_err()
        );
        let released = associate(reserved, Some(&worktree), None, "later".into(), false).unwrap();
        assert!(
            released
                .fields
                .get::<Worktree>("worktree")
                .unwrap()
                .is_none()
        );
        assert!(
            released
                .fields
                .string_equals("description", "keep")
                .unwrap()
        );
        for reference in ["--force", "@{-1}", "bad\nref", ""] {
            assert!(validate_reference(reference).is_err());
        }
    }
}
