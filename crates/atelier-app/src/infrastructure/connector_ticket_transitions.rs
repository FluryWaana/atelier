use super::{
    connector_tickets,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{
    application::extensions::{
        self as protocol, Installed, Method, RemoteOperation, Reply, Report, Request, Status,
        TicketIdentity, TicketTransitionPayload, TicketTransitions,
    },
    contracts::{Card, ForgeWriteState as Phase},
    document::Document,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    expected: Card,
    #[serde(default)]
    report_id: Option<String>,
    #[serde(default)]
    transition_id: Option<String>,
}
fn identity(link: &Document) -> Result<TicketIdentity> {
    Ok(TicketIdentity {
        instance: link
            .get::<String>("instance")?
            .ok_or("message.ticket_instance_missing")?,
        id: link
            .get::<String>("id")?
            .ok_or("message.ticket_identity_missing")?,
    })
}
fn validate_identity(ticket: &TicketIdentity) -> Result<()> {
    if ticket.instance.chars().any(char::is_control)
        || ticket.instance.contains(['\\', '%'])
        || super::jira::normalize_url(&ticket.instance)? != ticket.instance
    {
        return Err("message.connector_transition_instance_invalid".into());
    }
    Ok(())
}
pub fn report(value: &Value) -> Result<TicketTransitions> {
    let report = protocol::ticket_transitions(value)?;
    validate_identity(&report.ticket)?;
    Ok(report)
}
pub fn payload(value: &Value) -> Result<TicketTransitionPayload> {
    let payload = protocol::ticket_transition_payload(value)?;
    validate_identity(&payload.ticket)?;
    Ok(payload)
}
fn revision(card: &Card) -> Result<String> {
    Ok(crate::infrastructure::session::hexadecimal(
        &Sha256::digest(serde_json::to_vec(
            &atelier_core::application::jira::card_snapshot(card),
        )?),
    ))
}
fn select(
    store: &Store<'_>,
    profile: &Installed,
    value: &Value,
) -> Result<(Selection, Card, TicketIdentity)> {
    let selected: Selection = serde_json::from_str(&value.to_string())
        .map_err(|_| "message.invalid_module_transition_selection")?;
    let card = super::forge_association::snapshot(store, &selected.expected)?;
    let ticket = identity(&connector_tickets::linked(profile, &card)?)?;
    validate_identity(&ticket)?;
    Ok((selected, card, ticket))
}
pub fn read_input(store: &Store<'_>, profile: &Installed, value: &Value) -> Result<Value> {
    let (selection, _, ticket) = select(store, profile, value)?;
    if selection.report_id.is_some() || selection.transition_id.is_some() {
        return Err("message.transition_read_invalid".into());
    }
    Ok(serde_json::to_value(ticket)?)
}
pub fn prepare_preview(store: &Store<'_>, profile: &Installed, value: &Value) -> Result<Value> {
    let (selection, card, ticket) = select(store, profile, value)?;
    let observed = profile
        .report
        .as_ref()
        .filter(|report| {
            report.method == Method::TicketsTransitions
                && Some(&report.id) == selection.report_id.as_ref()
        })
        .ok_or("message.reread_the_ticket_transitions_before_confirming")?;
    let age = time::OffsetDateTime::now_utc()
        - time::OffsetDateTime::parse(
            &observed.observed_at,
            &time::format_description::well_known::Rfc3339,
        )?;
    if age > time::Duration::minutes(10) || age < time::Duration::seconds(-60) {
        return Err("message.the_transition_list_expired_reread_the_ticket".into());
    }
    let observed = report(&observed.output)?;
    if observed.ticket != ticket {
        return Err("message.these_transitions_belong_to_another_ticket".into());
    }
    let transition = observed
        .transitions
        .iter()
        .find(|transition| {
            Some(&transition.id) == selection.transition_id.as_ref() && !transition.requires_fields
        })
        .cloned()
        .ok_or("message.choose_an_available_transition_requiring_no_additional_fields")?;
    let ticket_key = connector_tickets::linked(profile, &card)?
        .get::<String>("key")?
        .ok_or("message.ticket_key_absent")?;
    let payload = TicketTransitionPayload {
        card_id: card.id.clone(),
        card_title: card.title.utf8()?,
        card_revision: revision(&card)?,
        ticket,
        ticket_key,
        from_status: observed.status,
        transition,
    };
    Ok(serde_json::to_value(payload)?)
}
pub fn before_send(
    store: &Store<'_>,
    profile: &Installed,
    operation: &RemoteOperation,
) -> Result<()> {
    if operation.method != Method::TicketsTransition {
        return Ok(());
    }
    let payload = payload(&operation.input)?;
    let card = store
        .document::<Card>(Documents::Cards, &payload.card_id)?
        .ok_or("message.card_not_found")?;
    let card = super::forge_association::snapshot(store, &card)?;
    if identity(&connector_tickets::linked(profile, &card)?)? != payload.ticket
        || revision(&card)? != payload.card_revision
    {
        return Err("message.the_task_or_its_link_changed_since_the_transition_preview".into());
    }
    Ok(())
}
pub fn preflight(request: &Request) -> Result<Option<Request>> {
    if request.method != Method::TicketsTransition {
        return Ok(None);
    }
    let payload = payload(&request.input)?;
    let mut read = request.clone();
    read.request_id = format!("request-{}", uuid::Uuid::new_v4());
    read.method = Method::TicketsTransitions;
    read.operation_id = None;
    read.input = serde_json::to_value(payload.ticket)?;
    Ok(Some(read))
}
pub fn validate_preflight(input: &Value, reply: &Reply) -> Result<()> {
    let payload = payload(input)?;
    if reply.status != Status::Ok || reply.method != Method::TicketsTransitions {
        return Err("message.the_module_did_not_confirm_the_transition_reread".into());
    }
    let current = report(&reply.output)?;
    if current.ticket != payload.ticket
        || current.status != payload.from_status
        || !current
            .transitions
            .iter()
            .any(|transition| transition == &payload.transition)
    {
        return Err("message.the_remote_status_or_transition_changed_nothing_was_sent".into());
    }
    Ok(())
}
pub fn observed_ticket(operation: &RemoteOperation) -> Result<protocol::Ticket> {
    let payload = payload(&operation.input)?;
    let value = operation
        .reply
        .as_ref()
        .and_then(|reply| reply.output.get("ticket"))
        .ok_or("message.no_ticket_observation_after_transition")?;
    let report = Report {
        id: operation.id.clone(),
        method: Method::TicketsRead,
        action: None,
        message: String::new(),
        output: value.clone(),
        observed_at: operation.updated_at.clone(),
    };
    let page = connector_tickets::page(&report)?;
    let ticket = page
        .items
        .into_iter()
        .next()
        .ok_or("message.no_ticket_observation_after_transition")?;
    if ticket.identity() != payload.ticket || ticket.status != payload.transition.status {
        return Err("message.the_observed_ticket_does_not_match_the_confirmed_transition".into());
    }
    Ok(ticket)
}
pub fn synchronize(store: &Store<'_>, operation: &RemoteOperation) -> Result<()> {
    if operation.method != Method::TicketsTransition
        || !matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
    {
        return Ok(());
    }
    let payload = payload(&operation.input)?;
    let observed = observed_ticket(operation)?;
    let mut card = store
        .document::<Card>(Documents::Cards, &payload.card_id)?
        .ok_or("message.card_not_found")?;
    let profile = Installed {
        manifest: operation.manifest.clone(),
        binary: operation.binary.clone(),
        revision: operation.revision,
        values: operation.values.clone(),
        report: None,
    };
    let mut link = connector_tickets::linked(&profile, &card)?;
    if identity(&link)? != payload.ticket {
        return Err("message.the_ticket_link_changed_the_effect_remains_in_its_journal".into());
    }
    link.set("status", &observed.status)?;
    link.set("key", &observed.key)?;
    link.set("url", &observed.url)?;
    card.fields.set("ticket", &link)?;
    card.fields.set(
        "connectorTicketEffect",
        &json!({"operationId":operation.id,"ticket":observed.identity(),"status":observed.status}),
    )?;
    store.put(Documents::Cards, &card)
}

#[cfg(test)]
mod tests {
    use super::super::{connector_effects as effects, connectors};
    use super::*;
    use protocol::{Invocation, RemotePreview};
    fn now() -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    }
    fn setup(store: &Store<'_>) -> Card {
        store.save_settings(&json!({"connectorRevision":1,"processConnectors":[{"manifest":{"id":"synthetic-tickets","name":"Synthetic tickets","description":"No remote account","sourceLocale":"en-US","methods":["check","ticketsRead","ticketsTransitions","ticketsTransition","reconcile"],"fields":[],"tabs":[]},"binary":{"path":"/synthetic/never-executed","sha256":"a".repeat(64)},"revision":1,"values":{},"report":null}]})).unwrap();
        let profile = connectors::profile(store, "synthetic-tickets", 1).unwrap();
        let card:Card=serde_json::from_str(&json!({"id":"11111111-1111-4111-8111-111111111111","title":"Human title","description":"Human description","instructions":"Human instructions","status":"todo","checklist":[{"text":"Human criterion","done":false}],"ticket":{"connector":"extension:synthetic-tickets","scope":connector_tickets::scope(&profile).unwrap(),"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket","key":"SYN-1","url":"https://synthetic.example.test/tenant/tickets/1","status":"Done"}}).to_string()).unwrap();
        store.put(Documents::Cards, &card).unwrap();
        card
    }
    fn choices() -> Value {
        json!({"ticket":{"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket"},"status":"Done","transitions":[{"id":"close","name":"Close synthetic ticket","status":"Closed","requiresFields":false},{"id":"unsupported","name":"Needs fields","status":"Closed","requiresFields":true}]})
    }
    fn preview(store: &Store<'_>, card: &Card) -> RemoteOperation {
        connectors::record(
            store,
            "synthetic-tickets",
            1,
            Report {
                id: "synthetic-choices".into(),
                method: Method::TicketsTransitions,
                action: None,
                message: String::new(),
                output: choices(),
                observed_at: now(),
            },
        )
        .unwrap();
        effects::preview(store,"synthetic-tickets",&RemotePreview{expected_revision:1,method:Method::TicketsTransition,action:String::new(),input:json!({"expected":card,"reportId":"synthetic-choices","transitionId":"close"})}).unwrap()
    }
    fn reply(operation: &RemoteOperation, nonce: &str, status: &str) -> Reply {
        Reply {
            protocol: protocol::PROTOCOL.into(),
            module: operation.module.clone(),
            request_id: nonce.into(),
            method: Method::Reconcile,
            status: Status::Ok,
            message: "Synthetic observation".into(),
            output: json!({"receipt":{"operationId":operation.id,"outcome":"applied","reference":"synthetic-event","observedAt":now()},"ticket":{"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket","key":"SYN-1","url":"https://synthetic.example.test/tenant/tickets/1","title":"Remote title must not replace human text","description":"Remote description must not replace human text","status":status}}),
        }
    }
    #[test]
    fn uncertain_transitions_reconcile_without_replaying_and_preserve_human_fields() {
        let connection = super::super::store::open(std::path::Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        let card = setup(&store);
        let (_, read) = connectors::invocation(
            &store,
            "synthetic-tickets",
            &Invocation {
                expected_revision: 1,
                method: Method::TicketsTransitions,
                action: None,
                input: json!({"expected":card}),
                confirmed: true,
            },
            &mut |value, _| Ok(value.into()),
        )
        .unwrap();
        assert_eq!(
            read.input,
            json!({"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket"})
        );
        let prepared = preview(&store, &card);
        let (_, send) = effects::prepare(
            &store,
            "synthetic-tickets",
            &prepared.id,
            prepared.version,
            1,
            false,
            &mut |value, _| Ok(value.into()),
        )
        .unwrap();
        assert!(send.input.get("expected").is_none());
        assert!(preflight(&send).unwrap().unwrap().operation_id.is_none());
        let metadata = Reply {
            protocol: protocol::PROTOCOL.into(),
            request_id: "synthetic-read".into(),
            module: prepared.module.clone(),
            method: Method::TicketsTransitions,
            status: Status::Ok,
            message: String::new(),
            output: choices(),
        };
        validate_preflight(&send.input, &metadata).unwrap();
        let mut changed = metadata.clone();
        changed.output["status"] = json!("Closed");
        assert!(validate_preflight(&send.input, &changed).is_err());
        let sending = effects::sending(
            &store,
            "synthetic-tickets",
            &prepared.id,
            prepared.version,
            1,
            &send.request_id,
        )
        .unwrap();
        effects::recover(&store).unwrap();
        let uncertain = effects::current(&store, "synthetic-tickets", &sending.id).unwrap();
        assert_eq!(uncertain.state, Phase::Uncertain);
        assert!(
            effects::prepare(
                &store,
                "synthetic-tickets",
                &uncertain.id,
                uncertain.version,
                1,
                false,
                &mut |value, _| Ok(value.into())
            )
            .is_err()
        );
        let mut human = card.clone();
        human.title = "Human edit after interruption".into();
        store.put(Documents::Cards, &human).unwrap();
        let (_, read) = effects::prepare(
            &store,
            "synthetic-tickets",
            &uncertain.id,
            uncertain.version,
            1,
            true,
            &mut |value, _| Ok(value.into()),
        )
        .unwrap();
        assert_eq!(read.method, Method::Reconcile);
        assert!(preflight(&read).unwrap().is_none());
        assert_eq!(read.input["method"], "ticketsTransition");
        assert_eq!(read.operation_id.as_deref(), Some(uncertain.id.as_str()));
        let observing = effects::observing(
            &store,
            "synthetic-tickets",
            &uncertain.id,
            uncertain.version,
            1,
            &read.request_id,
        )
        .unwrap();
        assert!(
            effects::record(
                &store,
                "synthetic-tickets",
                &observing.id,
                observing.version,
                1,
                reply(&observing, &read.request_id, "Done"),
                true
            )
            .is_err()
        );
        let observing = effects::observing(
            &store,
            "synthetic-tickets",
            &observing.id,
            observing.version,
            1,
            "request-second-read",
        )
        .unwrap();
        let observed = effects::record(
            &store,
            "synthetic-tickets",
            &observing.id,
            observing.version,
            1,
            reply(&observing, "request-second-read", "Closed"),
            true,
        )
        .unwrap();
        assert_eq!(observed.state, Phase::Reconciled);
        let saved = store
            .document::<Card>(Documents::Cards, &card.id)
            .unwrap()
            .unwrap();
        assert_eq!(saved.title, human.title);
        for key in ["description", "instructions", "checklist", "status"] {
            assert_eq!(
                saved.fields.get::<Value>(key).unwrap(),
                card.fields.get::<Value>(key).unwrap()
            );
        }
        assert!(
            saved
                .fields
                .get::<Document>("ticket")
                .unwrap()
                .unwrap()
                .string_equals("status", "Closed")
                .unwrap()
        );
        assert!(saved.fields.raw("connectorTicketEffect").is_some());
    }
    #[test]
    fn unsupported_fields_or_a_changed_card_refuse_dispatch_and_allow_cancel() {
        let connection = super::super::store::open(std::path::Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        let card = setup(&store);
        let prepared = preview(&store, &card);
        let profile = connectors::profile(&store, "synthetic-tickets", 1).unwrap();
        assert!(prepare_preview(&store,&profile,&json!({"expected":card,"reportId":"synthetic-choices","transitionId":"unsupported"})).is_err());
        let mut human = card.clone();
        human.title = "Edited after preview".into();
        store.put(Documents::Cards, &human).unwrap();
        assert!(
            effects::sending(
                &store,
                "synthetic-tickets",
                &prepared.id,
                prepared.version,
                1,
                "request-send"
            )
            .is_err()
        );
        effects::cancel(&store, "synthetic-tickets", &prepared.id, prepared.version).unwrap();
        assert_eq!(
            store
                .document::<Card>(Documents::Cards, &card.id)
                .unwrap()
                .unwrap()
                .title,
            human.title
        );
    }
}
