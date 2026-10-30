use super::{
    jira_transition,
    store::{Documents, Store},
    time_tracking::Clock,
};
use crate::Result;
use atelier_core::{
    application::{
        jira::{self, JiraFailure},
        task_creation::CreationContext,
        task_editing,
        time_tracking::TimeClock,
    },
    contracts::{
        Card, JiraConfiguration, JiraIssueDetail, JiraRefreshApply, JiraRefreshPreview,
        JiraRefreshRequest, JiraRefreshState as Phase,
    },
    document::Document,
    domain::text,
};
fn conflict() -> crate::Error {
    JiraFailure::OperationConflict(
        "message.the_card_or_jira_preview_changed_reload_the_preview_before_tryin",
    )
    .into()
}
fn same_content(left: &Card, right: &Card) -> Result<bool> {
    Ok(serde_json::to_value(jira::card_snapshot(left))?
        == serde_json::to_value(jira::card_snapshot(right))?)
}
fn mapping(store: &Store<'_>, card: &Card) -> Result<serde_json::Value> {
    let settings = store.settings::<serde_json::Value>()?;
    let projects = store.documents::<serde_json::Value>(Documents::Projects)?;
    let settings = atelier_core::domain::projects::settings_for_project(
        &settings,
        &projects,
        &card.fields.get::<String>("repo")?.unwrap_or_default(),
        cfg!(windows),
    );
    Ok(
        serde_json::json!({"jiraAutoTransition":settings["jiraAutoTransition"],"jiraStatusMap":settings["jiraStatusMap"],"kanban":settings["kanban"],"jiraConnection":settings["jiraConnection"]}),
    )
}
pub fn reserve(store: &Store<'_>, input: &JiraRefreshRequest, operation: &str) -> Result<String> {
    store.transaction(|store| {
        jira_transition::revalidate(store, &input.expected, input.revision)?;
        let (mut card, issue) =
            jira_transition::snapshot(store, &input.expected.id, input.revision)?;
        if jira::unresolved(&card.fields)? || card.fields.flag("archived")? {
            return Err(conflict());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && super::runs::active(&run)? {
                return Err("message.stop_the_run_before_synchronizing_the_jira_issue".into());
            }
        }
        let base = store
            .settings::<Document>()?
            .get::<JiraConfiguration>("jiraConnection")?
            .ok_or_else(conflict)?
            .url;
        let preview = JiraRefreshPreview {
            id: operation.into(),
            state: Phase::Reading,
            revision: input.revision,
            base,
            issue_id: issue.clone(),
            expected: jira::card_snapshot(&card),
            detail: None,
            created_at: Clock.format(Clock.now())?,
            mapping: mapping(store, &card)?,
            mapped_status: None,
        };
        card.fields.set("jiraRefresh", &preview)?;
        store.put(Documents::Cards, &card)?;
        Ok(issue)
    })
}
pub fn ready(
    store: &Store<'_>,
    card_id: &str,
    operation: &str,
    detail: JiraIssueDetail,
) -> Result<Card> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, card_id)?
            .ok_or_else(conflict)?;
        let mut preview = card
            .fields
            .get::<JiraRefreshPreview>("jiraRefresh")?
            .filter(|preview| preview.id == operation && preview.state == Phase::Reading)
            .ok_or_else(conflict)?;
        if !same_content(&card, &preview.expected)?
            || detail.connection_revision != preview.revision
            || detail.issue.id != preview.issue_id
            || jira_transition::snapshot(store, card_id, preview.revision)?.1 != preview.issue_id
        {
            return Err(conflict());
        }
        if !jira::issue_identity(&detail.issue.id, &detail.issue.key)
            || detail.issue.url
                != format!(
                    "{}/browse/{}",
                    preview.base.trim_end_matches('/'),
                    detail.issue.key
                )
        {
            return Err(conflict());
        }
        if mapping(store, &card)? != preview.mapping {
            return Err(conflict());
        }
        preview.mapped_status = atelier_core::application::projects::mapped_jira_status(
            &preview.mapping,
            &detail.issue.status_id,
            &detail.issue.status,
        );
        preview.state = Phase::Ready;
        preview.detail = Some(detail);
        preview.created_at = Clock.format(Clock.now())?;
        card.fields.set("jiraRefresh", &preview)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn interrupted(store: &Store<'_>, card_id: &str, operation: &str) -> Result<()> {
    let mut card = store
        .document::<Document>(Documents::Cards, card_id)?
        .ok_or_else(conflict)?;
    if let Some(mut preview) = card
        .get::<JiraRefreshPreview>("jiraRefresh")?
        .filter(|preview| preview.id == operation && preview.state == Phase::Reading)
    {
        preview.state = Phase::Interrupted;
        card.set("jiraRefresh", &preview)?;
        store.put(Documents::Cards, &card)?;
    }
    Ok(())
}
pub fn apply(store: &Store<'_>, input: &JiraRefreshApply) -> Result<Card> {
    store.transaction(|store| {
        jira_transition::revalidate(store, &input.expected, input.revision)?;
        let (mut card, issue) =
            jira_transition::snapshot(store, &input.expected.id, input.revision)?;
        let preview = card
            .fields
            .get::<JiraRefreshPreview>("jiraRefresh")?
            .filter(|preview| {
                preview.id == input.preview_id
                    && preview.state == Phase::Ready
                    && preview.revision == input.revision
            })
            .ok_or_else(conflict)?;
        if !same_content(&card, &preview.expected)?
            || issue != preview.issue_id
            || jira::unresolved(&card.fields)?
            || card.fields.flag("archived")?
        {
            return Err(conflict());
        }
        let created = Clock.parse(&preview.created_at).ok_or_else(conflict)?;
        if Clock
            .now()
            .checked_sub(created)
            .is_none_or(|age| !(0..=600_000).contains(&age))
        {
            return Err("message.this_jira_preview_expired_reload_it_before_applying_it".into());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && super::runs::active(&run)? {
                return Err("message.stop_the_run_before_synchronizing_the_jira_issue".into());
            }
        }
        if input.apply_status {
            if mapping(store, &card)? != preview.mapping {
                return Err(conflict());
            }
            let status = preview.mapped_status.as_ref().ok_or_else(conflict)?;
            card.fields.set("status", status)?;
        }
        let detail = preview.detail.ok_or_else(conflict)?;
        if detail.issue.id != issue || detail.connection_revision != input.revision {
            return Err(conflict());
        }
        let title = text(&detail.issue.title, 300).map_err(JiraFailure::Invalid)?;
        let description = text(&detail.description, 20_000).map_err(JiraFailure::Invalid)?;
        if title.is_empty() {
            return Err(JiraFailure::Invalid("message.a_jira_title_is_required").into());
        }
        let changed = card.title != title
            || card
                .fields
                .get::<String>("description")?
                .unwrap_or_default()
                != description;
        card.title = title;
        card.fields.set("description", &description)?;
        let mut ticket = card
            .fields
            .get::<Document>("ticket")?
            .ok_or_else(conflict)?;
        ticket.set("key", &detail.issue.key)?;
        ticket.set("url", &detail.issue.url)?;
        ticket.set("status", &detail.issue.status)?;
        ticket.set("statusId", &detail.issue.status_id)?;
        card.fields.set("ticket", &ticket)?;
        card.fields.remove("jiraRefresh");
        card.fields
            .set("updatedAt", &CreationContext::timestamp(store)?)?;
        if changed {
            let invalidation = task_editing::invalidate_reviews(
                &card.id,
                &mut card.fields,
                store.documents(Documents::Runs)?,
                task_editing::REVIEW_CHANGE_REASON,
            )?;
            for run in invalidation.runs {
                store.put(Documents::Runs, &run)?;
            }
        }
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(|store| {
        for mut card in store.documents::<Document>(Documents::Cards)? {
            if let Some(mut preview) = card
                .get::<JiraRefreshPreview>("jiraRefresh")?
                .filter(|preview| preview.state == Phase::Reading)
            {
                preview.state = Phase::Interrupted;
                card.set("jiraRefresh", &preview)?;
                store.put(Documents::Cards, &card)?;
            }
        }
        Ok(())
    })
}
