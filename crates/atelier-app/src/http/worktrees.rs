use super::{Shared, failure, object};
use crate::{
    Result,
    infrastructure::{store::Documents, worktrees},
};
use atelier_core::{
    application::{
        card_lifecycle::CardFailure, task_creation::CreationContext, worktrees as usecase,
    },
    contracts::{Card, Worktree, WorktreeState},
    document::Document,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub(super) async fn inspect_status(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> Response {
    let Ok(_permit) = state.file_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.repository_inspection_is_busy_try_again_shortly",
        );
    };
    let lookup = id.clone();
    let card = match state
        .database
        .call(move |store| store.document::<Card>(Documents::Cards, &lookup))
        .await
    {
        Ok(Some(card)) => card,
        Ok(None) => return failure(StatusCode::NOT_FOUND, "message.card_not_found"),
        Err(error) => return failure(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let worktree = match card.fields.get::<Worktree>("worktree") {
        Ok(Some(worktree)) if worktree.state == WorktreeState::Ready => worktree,
        _ => {
            return failure(
                StatusCode::CONFLICT,
                "message.verify_or_finish_the_interrupted_operation_on_this_worktree",
            );
        }
    };
    let repo = card
        .fields
        .get::<String>("repo")
        .ok()
        .flatten()
        .unwrap_or_default();
    if !atelier_core::domain::projects::same_repository(&repo, &worktree.source, cfg!(windows)) {
        return failure(
            StatusCode::CONFLICT,
            "message.the_source_repository_no_longer_matches_the_worktree",
        );
    }
    let (head, clean) = match worktrees::inspect_status(
        &state.prototype_directory,
        &state.prototype_paths,
        &worktree,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return failure(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let expected = worktree.clone();
    let checked = state
        .database
        .call(move |store| {
            let current = store
                .document::<Card>(Documents::Cards, &id)?
                .ok_or(CardFailure::Missing)?;
            if current.fields.get::<Worktree>("worktree")?.as_ref() != Some(&expected)
                || current.fields.get::<String>("repo")?.unwrap_or_default() != repo
            {
                return Err("message.the_worktree_association_changed_during_inspection".into());
            }
            store.timestamp()
        })
        .await;
    match checked {
        Ok(checked_at) => Json(atelier_core::contracts::WorktreeInspection {
            worktree,
            head,
            clean,
            checked_at,
        })
        .into_response(),
        Err(error) => failure(StatusCode::CONFLICT, &error.to_string()),
    }
}

async fn save_services(
    state: &Shared,
    id: String,
    primary: Worktree,
    expected: usecase::ServiceWorktrees,
    next: usecase::ServiceWorktrees,
) -> Result<()> {
    state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                let card = usecase::associate_services(
                    card,
                    &primary,
                    &expected,
                    &next,
                    store.timestamp()?,
                    cfg!(windows),
                )?;
                store.put(Documents::Cards, &card)
            })
        })
        .await
}

async fn prepare_services(
    state: &Shared,
    id: &str,
    primary: &Worktree,
    project: Option<&Document>,
    mut owned: usecase::ServiceWorktrees,
) -> Result<()> {
    let root = &state.prototype_directory;
    let roots = &state.prototype_paths;
    let mut verified = std::collections::BTreeSet::new();
    let mut repositories = usecase::service_repositories(project, primary, cfg!(windows))?;
    for (role, worktree) in &owned {
        repositories
            .entry(role.clone())
            .or_insert_with(|| worktree.source.clone());
    }
    for (role, repo) in repositories {
        let same = |source: &str| {
            atelier_core::domain::projects::same_repository(source, &repo, cfg!(windows))
        };
        if owned
            .get(&role)
            .is_some_and(|worktree| !same(&worktree.source))
        {
            return Err(
                "message.the_service_repository_has_changed_remove_the_associated_worktre".into(),
            );
        }
        let mut worktree =
            if let Some(worktree) = owned.values().find(|worktree| same(&worktree.source)) {
                if worktree.branch != primary.branch {
                    return Err("message.inconsistent_service_worktree_association".into());
                }
                worktree.clone()
            } else {
                worktrees::prepare(root, roots, &repo, primary.branch.clone(), "HEAD".into())
                    .await
                    .map_err(|error| format!("Worktree {role} : {error}"))?
            };
        let mut next = owned.clone();
        next.insert(role.clone(), worktree.clone());
        if !verified.contains(&worktree.path) {
            worktree.state = WorktreeState::Creating;
            for value in next
                .values_mut()
                .filter(|value| value.path == worktree.path)
            {
                *value = worktree.clone();
            }
        }
        save_services(state, id.into(), primary.clone(), owned, next.clone()).await?;
        owned = next;
        if verified.insert(worktree.path.clone()) {
            worktrees::create(root, roots, &worktree)
                .await
                .map_err(|error| format!("Worktree {role} : {error}"))?;
            worktree.state = WorktreeState::Ready;
            let mut next = owned.clone();
            for value in next
                .values_mut()
                .filter(|value| value.path == worktree.path)
            {
                *value = worktree.clone();
            }
            save_services(state, id.into(), primary.clone(), owned, next.clone()).await?;
            owned = next;
        }
    }
    Ok(())
}

async fn remove_services(
    state: &Shared,
    id: &str,
    primary: &Worktree,
    mut owned: usecase::ServiceWorktrees,
    discard: bool,
) -> Result<()> {
    while let Some((role, mut worktree)) = owned
        .iter()
        .next()
        .map(|(role, worktree)| (role.clone(), worktree.clone()))
    {
        worktree.state = WorktreeState::Removing;
        let mut next = owned.clone();
        for value in next
            .values_mut()
            .filter(|value| value.path == worktree.path)
        {
            *value = worktree.clone();
        }
        save_services(state, id.into(), primary.clone(), owned, next.clone()).await?;
        owned = next;
        worktrees::remove(
            &state.prototype_directory,
            &state.prototype_paths,
            &worktree,
            discard,
        )
        .await
        .map_err(|error| format!("Worktree {role} : {error}"))?;
        let mut next = owned.clone();
        next.retain(|_, value| value.path != worktree.path);
        save_services(state, id.into(), primary.clone(), owned, next.clone()).await?;
        owned = next;
    }
    Ok(())
}

async fn save(
    state: &Shared,
    id: String,
    expected: Option<Worktree>,
    next: Option<Worktree>,
) -> Result<Card> {
    state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                let card = usecase::associate(
                    card,
                    expected.as_ref(),
                    next.as_ref(),
                    store.timestamp()?,
                    cfg!(windows),
                )?;
                store.put(Documents::Cards, &card)?;
                Ok(card)
            })
        })
        .await
}

async fn change(state: &Shared, id: String, body: Document, remove: bool) -> Result<Card> {
    let lookup = id.clone();
    let (card, settings, projects) = state
        .database
        .call(move |store| {
            Ok((
                store
                    .document::<Card>(Documents::Cards, &lookup)?
                    .ok_or(CardFailure::Missing)?,
                store.settings::<Document>()?,
                store.documents::<Document>(Documents::Projects)?,
            ))
        })
        .await?;
    let original = card.fields.get::<Worktree>("worktree")?;
    let services = card
        .fields
        .get::<usecase::ServiceWorktrees>("serviceWorktrees")?
        .unwrap_or_default();
    let root = &state.prototype_directory;
    let roots = &state.prototype_paths;
    if remove {
        let discard = usecase::removal(&body)?;
        let worktree = original.ok_or("message.no_associated_worktree")?;
        let mut removing = worktree.clone();
        removing.state = WorktreeState::Removing;
        save(state, id.clone(), Some(worktree), Some(removing.clone())).await?;
        remove_services(state, &id, &removing, services, discard).await?;
        worktrees::remove(root, roots, &removing, discard).await?;
        return save(state, id, Some(removing), None).await;
    }
    let options = usecase::create_options(&card, &settings, &body)?;
    let worktree = match original {
        Some(worktree) => worktree,
        None => {
            let worktree = if let Some(kind) = options.fetch_forge {
                let revision = options
                    .forge_revision
                    .ok_or("message.a_forge_revision_is_required_for_the_https_fetch")?;
                let vault = state.vault.clone();
                let (configuration, token) = state
                    .database
                    .call(move |store| {
                        crate::infrastructure::forge::credentials(
                            store,
                            kind,
                            revision,
                            &mut |value, decrypt| vault.protect(value, decrypt),
                        )
                    })
                    .await?;
                let authentication =
                    crate::infrastructure::git_transport::HttpsAuthentication::new(
                        configuration,
                        options.fetch_repository,
                        token,
                    )?;
                let plan = worktrees::prepare_from_forge_origin(
                    root,
                    roots,
                    &options.repo,
                    options.branch,
                    options.base,
                    &authentication,
                )
                .await?;
                state
                    .database
                    .call(move |store| {
                        crate::infrastructure::forge::require_revision(store, revision)
                    })
                    .await?;
                plan
            } else if options.fetch_origin {
                worktrees::prepare_from_origin(
                    root,
                    roots,
                    &options.repo,
                    options.branch,
                    options.base,
                )
                .await?
            } else {
                worktrees::prepare(root, roots, &options.repo, options.branch, options.base).await?
            };
            save(state, id.clone(), None, Some(worktree.clone())).await?;
            worktree
        }
    };
    if worktree.state == WorktreeState::Removing {
        worktrees::inspect(root, roots, &worktree).await?;
        worktrees::create(root, roots, &worktree).await?;
    } else {
        worktrees::create(root, roots, &worktree).await?;
    }
    let project = projects.iter().find(|project| {
        project
            .get::<String>("repo")
            .ok()
            .flatten()
            .is_some_and(|repo| {
                atelier_core::domain::projects::same_repository(
                    &repo,
                    &worktree.source,
                    cfg!(windows),
                )
            })
    });
    prepare_services(state, &id, &worktree, project, services).await?;
    let mut ready = worktree.clone();
    ready.state = WorktreeState::Ready;
    save(state, id, Some(worktree), Some(ready)).await
}

async fn mutate(state: Shared, id: String, request: Request, remove: bool) -> Response {
    let Ok(_readers) = state.file_reads.try_acquire_many(2) else {
        return failure(
            StatusCode::CONFLICT,
            "message.wait_for_file_reading_to_finish_before_changing_the_worktree",
        );
    };
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = change(&state, id, body, remove).await;
    // Intent records and partial failures must be visible in every open board.
    let _ = state.changed.send(());
    match result {
        Ok(card) => Json(card).into_response(),
        Err(error) => {
            let status = match error.downcast_ref::<CardFailure>() {
                Some(CardFailure::Missing) => StatusCode::NOT_FOUND,
                Some(CardFailure::Conflict(_)) => StatusCode::CONFLICT,
                _ if error.is::<rusqlite::Error>() => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::BAD_REQUEST,
            };
            let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
                "message.the_worktree_could_not_be_saved".to_owned()
            } else {
                error.to_string()
            };
            failure(status, &message)
        }
    }
}

pub(super) async fn create(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, id, request, false).await
}

pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, id, request, true).await
}
