use super::{
    jira,
    store::{Documents, Store},
    time_tracking::Clock,
};
use crate::Result;
use atelier_core::{
    application::{
        jira::{self as domain, JiraFailure},
        time_tracking::TimeClock,
    },
    contracts::{
        Card, JiraConfiguration, JiraCreateRequest, JiraCreationIntent, JiraIssue,
        JiraWriteState as Phase,
    },
    document::Document,
};
fn conflict() -> crate::Error {
    JiraFailure::OperationConflict(
        "message.the_card_or_jira_creation_changed_reload_before_trying_again",
    )
    .into()
}
pub fn revalidate(store: &Store<'_>, expected: &Card, revision: u64) -> Result<Card> {
    jira::require_revision(store, revision)?;
    let current = store
        .document::<Card>(Documents::Cards, &expected.id)?
        .ok_or_else(conflict)?;
    if serde_json::to_value(domain::card_snapshot(&current))?
        != serde_json::to_value(domain::card_snapshot(expected))?
    {
        return Err(conflict());
    }
    Ok(current)
}
pub fn reserve(store: &Store<'_>, request: &JiraCreateRequest, operation: &str) -> Result<()> {
    store.transaction(|store| {
        if !request.confirmed
            || !domain::project_key(&request.project_key)
            || request.issue_type.subtask
            || request.issue_type.id.is_empty()
            || request.issue_type.id.len() > 64
            || !request
                .issue_type
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            || request.issue_type.name.is_empty()
            || request.issue_type.name.len() > 400
            || request.issue_type.name.chars().any(char::is_control)
        {
            return Err("message.confirm_the_jira_project_and_issue_type_to_create".into());
        }
        let mut card = revalidate(store, &request.expected, request.revision)?;
        if card.fields.flag("archived")?
            || card.fields.has_object("ticket")?
            || domain::unresolved(&card.fields)?
        {
            return Err(
                "message.restore_the_card_and_finish_pending_operations_before_creating_a".into(),
            );
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && super::runs::active(&run)? {
                return Err("message.stop_the_run_before_creating_the_jira_issue".into());
            }
        }
        domain::creation_text(&card)?;
        let configuration = store
            .settings::<Document>()?
            .get::<JiraConfiguration>("jiraConnection")?
            .ok_or_else(conflict)?;
        if configuration.url.is_empty() {
            return Err("message.configure_the_jira_connection_in_settings".into());
        }
        let intent = JiraCreationIntent {
            id: operation.into(),
            state: Phase::Preparing,
            revision: request.revision,
            base: configuration.url,
            project_key: request.project_key.clone(),
            issue_type: request.issue_type.clone(),
            started_at: Clock.format(Clock.now())?,
            result: None,
        };
        card.fields.set("jiraCreation", &intent)?;
        store.put(Documents::Cards, &card)
    })
}
fn active(store: &Store<'_>, card_id: &str, operation: &str) -> Result<(Card, JiraCreationIntent)> {
    let card = store
        .document::<Card>(Documents::Cards, card_id)?
        .ok_or_else(conflict)?;
    let intent = card
        .fields
        .get::<JiraCreationIntent>("jiraCreation")?
        .filter(|intent| intent.id == operation)
        .ok_or_else(conflict)?;
    Ok((card, intent))
}
pub fn sending(store: &Store<'_>, expected: &Card, operation: &str) -> Result<Card> {
    let card_id = expected.id.as_str();
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, operation)?;
        jira::require_revision(store, intent.revision)?;
        let mut before = domain::card_snapshot(expected);
        before.fields.remove("jiraCreation");
        let mut after = domain::card_snapshot(&card);
        after.fields.remove("jiraCreation");
        if serde_json::to_value(before)? != serde_json::to_value(after)? {
            return Err(conflict());
        }
        if intent.state != Phase::Preparing || card.fields.has_object("ticket")? {
            return Err(conflict());
        }
        intent.state = Phase::Sending;
        card.fields.set("jiraCreation", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn failed_or_uncertain(store: &Store<'_>, card_id: &str, operation: &str) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, operation)?;
        intent.state = match intent.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Err(conflict()),
        };
        card.fields.set("jiraCreation", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn record(store: &Store<'_>, card_id: &str, operation: &str, issue: JiraIssue) -> Result<Card> {
    store.transaction(|store|{
        let (mut card,mut intent)=active(store,card_id,operation)?;
        if !matches!(intent.state,Phase::Sending|Phase::Uncertain) || !domain::issue_identity(&issue.id,&issue.key) || issue.url!=format!("{}/browse/{}",intent.base.trim_end_matches('/'),issue.key) || card.fields.has_object("ticket")?{return Err(conflict());}
        if intent.result.as_ref().is_some_and(|known|known.id!=issue.id){return Err("message.this_issue_does_not_match_the_identified_creation".into());}
        let mut duplicate=false;
        for other in store.documents::<Card>(Documents::Cards)? {
            if other.id==card.id{continue;}
            if let Some(ticket)=other.fields.get::<Document>("ticket")?{
                let base=ticket.get::<String>("instance")?.or(ticket.get::<String>("base")?);
                duplicate|=ticket.string_equals("connector","jira")? && base.as_deref()==Some(intent.base.as_str()) && ticket.string_equals("id",&issue.id)?;
            }
        }
        intent.state=if duplicate{Phase::Uncertain}else{Phase::Confirmed};intent.result=Some(issue.clone());
        if !duplicate{card.fields.set("ticket",&serde_json::json!({"connector":"jira","instance":intent.base,"id":issue.id,"key":issue.key,"url":issue.url,"status":issue.status}))?;card.fields.set("updatedAt",&Clock.format(Clock.now())?)?;}
        card.fields.set("jiraCreation",&intent)?;store.put(Documents::Cards,&card)?;Ok(card)
    })
}
pub fn close_uncertainty(store: &Store<'_>, expected: &Card, revision: u64) -> Result<Card> {
    store.transaction(|store| {
        let mut card = revalidate(store, expected, revision)?;
        let mut intent = card
            .fields
            .get::<JiraCreationIntent>("jiraCreation")?
            .filter(|intent| intent.state == Phase::Uncertain)
            .ok_or_else(conflict)?;
        let current = store
            .settings::<Document>()?
            .get::<JiraConfiguration>("jiraConnection")?
            .ok_or_else(conflict)?;
        if current.url != intent.base {
            return Err("message.reconnect_the_jira_site_of_the_uncertain_creation".into());
        }
        intent.state = Phase::Dismissed;
        card.fields.set("jiraCreation", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(|store| {
        for mut card in store.documents::<Document>(Documents::Cards)? {
            if let Some(mut intent) = card.get::<JiraCreationIntent>("jiraCreation")? {
                intent.state = match intent.state {
                    Phase::Preparing => Phase::Failed,
                    Phase::Sending => Phase::Uncertain,
                    _ => continue,
                };
                card.set("jiraCreation", &intent)?;
                store.put(Documents::Cards, &card)?;
            }
        }
        Ok(())
    })
}
