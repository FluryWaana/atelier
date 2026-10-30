use super::{connectors, store::Store};
use crate::Result;
use atelier_core::{
    application::extensions::{
        self as protocol, Effect, Installed, Method, RemoteOperation as Operation, RemotePreview,
        Reply, Request, Status,
    },
    contracts::ForgeWriteState as Phase,
    document::Document,
};
use std::collections::BTreeMap;
const LIMIT: usize = 16 * 1024 * 1024;
pub fn pending(operation: &Operation) -> bool {
    matches!(
        operation.state,
        Phase::Preparing | Phase::Sending | Phase::Uncertain
    )
}
pub fn read(settings: &Document) -> Result<Vec<Operation>> {
    if settings
        .raw("connectorEffects")
        .is_some_and(|raw| raw.get().len() > LIMIT)
    {
        return Err("message.connector_write_history_is_too_large".into());
    }
    let operations = settings
        .get::<Vec<Operation>>("connectorEffects")?
        .unwrap_or_default();
    if operations.len() > 1024 {
        return Err("message.connector_write_history_is_full".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for operation in &operations {
        protocol::validate_manifest(&operation.manifest)?;
        match operation.method {
            Method::Action => {}
            Method::TicketsCreate if operation.action.is_empty() => {
                protocol::ticket_creation(&operation.input)?;
            }
            Method::TimeSend if operation.action.is_empty() => {
                super::connector_time::proposal(&operation.input)?;
            }
            Method::TicketsTransition if operation.action.is_empty() => {
                super::connector_ticket_transitions::payload(&operation.input)?;
            }
            Method::RepositoriesPublish if operation.action.is_empty() => {
                protocol::repository_proposal(&operation.input)?;
            }
            _ => return Err("message.unsupported_connector_remote_operation".into()),
        }
        if operation.module != operation.manifest.id
            || !protocol::identifier(&operation.id)
            || operation.version == 0
            || operation.revision == 0
            || !seen.insert(&operation.id)
            || operation.binary.path.is_empty()
            || operation.binary.path.chars().any(char::is_control)
            || operation.binary.path.len() > 32000
            || operation
                .request_id
                .as_ref()
                .is_some_and(|id| !protocol::identifier(id))
            || matches!(
                operation.state,
                Phase::Sending | Phase::Uncertain | Phase::Confirmed | Phase::Reconciled
            ) && operation.request_id.is_none()
            || operation.binary.sha256.len() != 64
            || !operation
                .binary
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || protocol::effect(&operation.manifest, &request(operation, false))? != Effect::Remote
        {
            return Err("message.invalid_connector_write_history".into());
        }
        if let Some(reply) = &operation.reply {
            if reply.protocol != protocol::PROTOCOL
                || reply.module != operation.module
                || !(reply.method == operation.method
                    || reply.method == reconcile_method(operation.method))
                || serde_json::to_vec(reply)?.len() > protocol::OUTPUT_LIMIT
                || matches!(operation.state, Phase::Preparing | Phase::Sending)
                || matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
                    && (reply.status != Status::Ok
                        || operation.request_id.as_deref() != Some(reply.request_id.as_str()))
            {
                return Err("message.invalid_connector_write_result".into());
            }
            protocol::report_items(&protocol::Report {
                id: operation.id.clone(),
                method: operation.method,
                action: Some(operation.action.clone()),
                message: reply.message.clone(),
                output: reply.output.clone(),
                observed_at: operation.updated_at.clone(),
            })?;
            let receipt = protocol::remote_receipt(&reply.output, &operation.id)?;
            if let Some(receipt) = &receipt {
                time::OffsetDateTime::parse(
                    &receipt.observed_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|_| "message.invalid_connector_observation_date")?;
            }
            if matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
                && receipt
                    .as_ref()
                    .is_none_or(|receipt| receipt.outcome != protocol::RemoteOutcome::Applied)
            {
                return Err(
                    "message.a_confirmed_write_requires_a_positive_module_observation".into(),
                );
            }
            if operation.method == Method::TimeSend
                && matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
                && reply
                    .output
                    .get("time")
                    .map(super::connector_time::proposal)
                    .transpose()?
                    .as_ref()
                    != Some(&super::connector_time::proposal(&operation.input)?)
            {
                return Err("message.observed_time_export_mismatch".into());
            }
            if operation.state == Phase::Failed
                && (receipt
                    .as_ref()
                    .is_none_or(|receipt| receipt.outcome != protocol::RemoteOutcome::NotAttempted)
                    || reply.status == Status::Uncertain
                    || operation.request_id.as_deref() != Some(reply.request_id.as_str()))
            {
                return Err("message.invalid_connector_write_result".into());
            }
            if operation.method == Method::TicketsTransition
                && matches!(operation.state, Phase::Confirmed | Phase::Reconciled)
            {
                super::connector_ticket_transitions::observed_ticket(operation)?;
            }
        } else if matches!(operation.state, Phase::Confirmed | Phase::Reconciled) {
            return Err("message.a_confirmed_write_requires_a_positive_module_observation".into());
        }
    }
    Ok(operations)
}
fn save(store: &Store<'_>, operations: &[Operation]) -> Result<()> {
    if serde_json::to_vec(operations)?.len() > LIMIT {
        return Err("message.connector_write_history_is_too_large".into());
    }
    let mut settings = store.settings::<Document>()?;
    settings.set("connectorEffects", &operations)?;
    store.save_settings(&settings)
}
fn now() -> Result<String> {
    Ok(time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?)
}
fn touch(operation: &mut Operation) -> Result<()> {
    operation.version = operation
        .version
        .checked_add(1)
        .ok_or("message.operation_version_exhausted")?;
    operation.updated_at = now()?;
    Ok(())
}
fn same_scope(profile: &Installed, operation: &Operation) -> Result<bool> {
    Ok(profile.binary == operation.binary
        && profile.values == operation.values
        && serde_json::to_value(&profile.manifest)? == serde_json::to_value(&operation.manifest)?)
}
pub fn require_configuration(store: &Store<'_>, profile: &Installed) -> Result<()> {
    for operation in read(&store.settings::<Document>()?)?
        .iter()
        .filter(|operation| operation.module == profile.manifest.id && pending(operation))
    {
        if !same_scope(profile, operation)? {
            return Err(
                "message.this_connector_scope_has_an_unfinished_write_reconcile_it_before".into(),
            );
        }
    }
    Ok(())
}
pub fn require_removal(store: &Store<'_>, module: &str) -> Result<()> {
    if read(&store.settings::<Document>()?)?
        .iter()
        .any(|operation| operation.module == module && pending(operation))
    {
        return Err("message.reconcile_the_connector_write_before_removing_it".into());
    }
    Ok(())
}
pub fn preview(store: &Store<'_>, module: &str, input: &RemotePreview) -> Result<Operation> {
    store.transaction(|store|{
        let owner=super::connector_repositories::publication_owner(store,input.method,&input.input)?;
        for run in store.documents::<Document>(super::store::Documents::Runs)?{if super::runs::active(&run)?{return Err("message.stop_agents_before_preparing_a_connector_remote_write".into());}}
        for entry in store.documents::<Document>(super::store::Documents::Times)?{if entry.has_text("runningSince")?||entry.string_equals("exportState","sending")?||entry.string_equals("exportState","uncertain")?{return Err("message.stop_timers_and_reconcile_exports_before_this_connector_action".into());}}
        for card in store.documents::<Document>(super::store::Documents::Cards)?{if atelier_core::application::jira::unresolved(&card)?||atelier_core::application::forge::unresolved(&card)?||card.get::<Document>("automation")?.is_some_and(|flow|flow.string_equals("state","running").unwrap_or(true)&&!owner.as_ref().is_some_and(|(id,operation)|card.string_equals("id",id).unwrap_or(false)&&flow.string_equals("id",operation).unwrap_or(false))){return Err("message.finish_task_operations_before_this_connector_action".into());}}
        let queue=super::queue::read(store)?;
        if queue.active.as_ref().is_some_and(|active|!owner.as_ref().is_some_and(|(id,_)|*id==active.card_id))||(!queue.paused&&!queue.ids.is_empty()&&owner.is_none()){return Err("message.pause_the_queue_and_finish_its_active_task_before_this_connector".into());}
        let mut operations=read(&store.settings::<Document>()?)?;
        if operations.iter().any(pending){return Err("message.finish_or_reconcile_the_current_connector_write".into());}
        if operations.len()>=1024{return Err("message.connector_write_history_is_full".into());}
        let profile=connectors::profile(store,module,input.expected_revision)?;
        let payload=match input.method {
            Method::Action if input.input.is_null()=>serde_json::json!({"importedCount":super::connector_tickets::imported_count(store,&profile)?}),
            Method::TicketsCreate if input.action.is_empty()=>serde_json::to_value(protocol::ticket_creation(&input.input)?)?,
            Method::TimeSend if input.action.is_empty()=>super::connector_time::prepare_preview(store,&profile,&input.input)?,
            Method::TicketsTransition if input.action.is_empty()=>super::connector_ticket_transitions::prepare_preview(store,&profile,&input.input)?,
            Method::RepositoriesPublish if input.action.is_empty()=>super::connector_repositories::prepare_preview(store,&profile,&input.input)?,
            _=>return Err("message.unsupported_connector_remote_operation".into())
        };
        let timestamp=now()?;
        let operation=Operation{id:format!("effect-{}",uuid::Uuid::new_v4()),module:module.into(),version:1,revision:profile.revision,manifest:profile.manifest.clone(),binary:profile.binary.clone(),values:profile.values.clone(),method:input.method,action:input.action.clone(),input:payload,state:Phase::Preparing,started_at:timestamp.clone(),updated_at:timestamp,reply:None,request_id:None};
        let request=request(&operation,false);
        if protocol::effect(&profile.manifest,&request)?!=Effect::Remote{return Err("message.this_action_is_not_a_declared_remote_write".into());}
        operations.push(operation.clone());
        if serde_json::to_vec(&operations)?.len().saturating_add(protocol::OUTPUT_LIMIT+16*1024)>LIMIT{return Err("message.history_is_full_no_new_remote_write_was_started".into());}
        save(store,&operations)?;Ok(operation)
    })
}
pub fn operation(store: &Store<'_>, module: &str, id: &str, version: u64) -> Result<Operation> {
    read(&store.settings::<Document>()?)?
        .into_iter()
        .find(|operation| {
            operation.module == module && operation.id == id && operation.version == version
        })
        .ok_or_else(|| "message.the_connector_operation_changed_reload_its_preview".into())
}
pub fn current(store: &Store<'_>, module: &str, id: &str) -> Result<Operation> {
    read(&store.settings::<Document>()?)?
        .into_iter()
        .find(|operation| operation.module == module && operation.id == id)
        .ok_or_else(|| "message.connector_operation_not_found".into())
}
fn reconcile_method(method: Method) -> Method {
    if method == Method::TimeSend {
        Method::TimeReconcile
    } else {
        Method::Reconcile
    }
}
fn request(operation: &Operation, reconcile: bool) -> Request {
    Request {
        protocol: protocol::PROTOCOL.into(),
        request_id: "validate-effect".into(),
        module: operation.module.clone(),
        method: if reconcile {
            reconcile_method(operation.method)
        } else {
            operation.method
        },
        operation_id: Some(operation.id.clone()),
        action: if reconcile || operation.method != Method::Action {
            None
        } else {
            Some(operation.action.clone())
        },
        settings: operation.values.clone(),
        secrets: BTreeMap::new(),
        input: if reconcile {
            serde_json::json!({"method":operation.method,"action":if operation.method==Method::Action {Some(&operation.action)}else{None},"input":if operation.method==Method::RepositoriesPublish{super::connector_repositories::publication_input(&operation.input)}else{operation.input.clone()}})
        } else if operation.method == Method::RepositoriesPublish {
            super::connector_repositories::publication_input(&operation.input)
        } else {
            operation.input.clone()
        },
    }
}
pub fn prepare(
    store: &Store<'_>,
    module: &str,
    id: &str,
    version: u64,
    revision: u64,
    reconcile: bool,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<(Installed, Request)> {
    let operation = operation(store, module, id, version)?;
    if operation.state
        != if reconcile {
            Phase::Uncertain
        } else {
            Phase::Preparing
        }
    {
        return Err("message.this_write_cannot_be_sent_again".into());
    }
    let profile = connectors::profile(store, module, revision)?;
    if !same_scope(&profile, &operation)? || (!reconcile && profile.revision != operation.revision)
    {
        return Err(
            "message.connector_scope_or_credentials_changed_reload_and_check_the_oper".into(),
        );
    }
    let mut request = request(&operation, reconcile);
    request.request_id = format!("request-{}", uuid::Uuid::new_v4());
    let tokens = super::settings::load(store, protect)?;
    for field in profile
        .manifest
        .fields
        .iter()
        .filter(|field| field.kind == protocol::FieldKind::Password)
    {
        if let Some(secret) = tokens
            .get::<String>(&format!("extension:{module}:{}", field.id))?
            .filter(|secret| !secret.is_empty())
        {
            request.secrets.insert(field.id.clone(), secret);
        }
    }
    protocol::effect(&profile.manifest, &request)?;
    Ok((profile, request))
}
fn change(
    store: &Store<'_>,
    id: &str,
    version: u64,
    action: impl FnOnce(&mut Operation) -> Result<()>,
) -> Result<Operation> {
    store.transaction(|store| {
        let mut operations = read(&store.settings::<Document>()?)?;
        let operation = operations
            .iter_mut()
            .find(|operation| operation.id == id && operation.version == version)
            .ok_or("message.the_connector_operation_changed_reload_its_preview")?;
        action(operation)?;
        super::connector_time::synchronize(store, operation)?;
        super::connector_ticket_transitions::synchronize(store, operation)?;
        super::connector_repositories::synchronize(store, operation)?;
        touch(operation)?;
        let result = operation.clone();
        save(store, &operations)?;
        Ok(result)
    })
}
pub fn sending(
    store: &Store<'_>,
    module: &str,
    id: &str,
    version: u64,
    revision: u64,
    request_id: &str,
) -> Result<Operation> {
    change(store, id, version, |operation| {
        let profile = connectors::profile(store, module, revision)?;
        if operation.module != module
            || operation.state != Phase::Preparing
            || profile.revision != operation.revision
            || !same_scope(&profile, operation)?
        {
            return Err("message.the_connector_operation_or_scope_changed".into());
        }
        if !protocol::identifier(request_id) {
            return Err("message.invalid_connector_request_identity".into());
        }
        super::connector_time::before_send(store, operation)?;
        super::connector_ticket_transitions::before_send(store, &profile, operation)?;
        super::connector_repositories::before_send(store, operation)?;
        operation.state = Phase::Sending;
        operation.request_id = Some(request_id.into());
        Ok(())
    })
}
pub fn observing(
    store: &Store<'_>,
    module: &str,
    id: &str,
    version: u64,
    revision: u64,
    request_id: &str,
) -> Result<Operation> {
    change(store, id, version, |operation| {
        let profile = connectors::profile(store, module, revision)?;
        if operation.module != module
            || operation.state != Phase::Uncertain
            || !same_scope(&profile, operation)?
            || !protocol::identifier(request_id)
        {
            return Err("message.the_connector_operation_or_scope_changed".into());
        }
        operation.request_id = Some(request_id.into());
        Ok(())
    })
}
pub fn interrupted(store: &Store<'_>, id: &str, version: u64) -> Result<Operation> {
    change(store, id, version, |operation| {
        operation.state = match operation.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Err("message.no_active_write_to_interrupt".into()),
        };
        Ok(())
    })
}
pub fn cancel(store: &Store<'_>, module: &str, id: &str, version: u64) -> Result<Operation> {
    change(store, id, version, |operation| {
        if operation.module != module || operation.state != Phase::Preparing {
            return Err("message.only_an_unsubmitted_preview_can_be_cancelled".into());
        }
        operation.state = Phase::Failed;
        Ok(())
    })
}
pub fn record(
    store: &Store<'_>,
    module: &str,
    id: &str,
    version: u64,
    revision: u64,
    reply: Reply,
    reconcile: bool,
) -> Result<Operation> {
    change(store, id, version, |operation| {
        if reply.protocol != protocol::PROTOCOL
            || operation.request_id.as_deref() != Some(reply.request_id.as_str())
            || serde_json::to_vec(&reply)?.len() > protocol::OUTPUT_LIMIT
        {
            return Err("message.the_result_does_not_match_this_connector_write".into());
        }
        let profile = connectors::profile(store, module, revision)?;
        if operation.module != module
            || !same_scope(&profile, operation)?
            || operation.state
                != if reconcile {
                    Phase::Uncertain
                } else {
                    Phase::Sending
                }
            || reply.module != module
            || reply.method
                != if reconcile {
                    reconcile_method(operation.method)
                } else {
                    operation.method
                }
        {
            return Err("message.the_result_does_not_match_this_connector_write".into());
        }
        let report = protocol::Report {
            id: operation.id.clone(),
            method: operation.method,
            action: Some(operation.action.clone()),
            message: reply.message.clone(),
            output: reply.output.clone(),
            observed_at: now()?,
        };
        protocol::report_items(&report)?;
        let receipt = protocol::remote_receipt(&reply.output, &operation.id)?;
        let outcome = if let Some(receipt) = receipt {
            let date = time::OffsetDateTime::parse(
                &receipt.observed_at,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|_| "message.invalid_connector_observation_date")?;
            let age = time::OffsetDateTime::now_utc() - date;
            if age < time::Duration::seconds(-60) || age > time::Duration::minutes(10) {
                return Err("message.connector_observation_expired_or_dated_in_the_future".into());
            }
            receipt.outcome
        } else {
            protocol::RemoteOutcome::Unknown
        };
        operation.state = if outcome == protocol::RemoteOutcome::NotAttempted
            && reply.status != Status::Uncertain
        {
            Phase::Failed
        } else if reply.status == Status::Ok && outcome == protocol::RemoteOutcome::Applied {
            if reconcile {
                Phase::Reconciled
            } else {
                Phase::Confirmed
            }
        } else {
            Phase::Uncertain
        };
        operation.reply = Some(reply);
        Ok(())
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(|store| {
        let mut operations = read(&store.settings::<Document>()?)?;
        let mut changed = false;
        for operation in &mut operations {
            let next = match operation.state {
                Phase::Preparing => Some(Phase::Failed),
                Phase::Sending => Some(Phase::Uncertain),
                _ => None,
            };
            if let Some(next) = next {
                operation.state = next;
                super::connector_time::synchronize(store, operation)?;
                touch(operation)?;
                changed = true;
            }
        }
        if changed {
            save(store, &operations)?;
        }
        Ok(())
    })
}
pub fn reconciles_time(store: &Store<'_>, path: &str) -> Result<bool> {
    let Some(tail) = path.strip_prefix("/api/extensions/") else {
        return Ok(false);
    };
    let parts = tail.split('/').collect::<Vec<_>>();
    let [module, "operations", id, "reconcile"] = parts.as_slice() else {
        return Ok(false);
    };
    Ok(read(&store.settings::<Document>()?)?
        .iter()
        .any(|operation| {
            operation.module == *module
                && operation.id == *id
                && operation.method == Method::TimeSend
                && operation.state == Phase::Uncertain
        }))
}

pub fn blocks(store: &Store<'_>, path: &str) -> Result<bool> {
    if !read(&store.settings::<Document>()?)?.iter().any(pending) {
        return Ok(false);
    }
    let tail = path.strip_prefix("/api/extensions/");
    let module_configuration = tail.is_some_and(|tail| !tail.is_empty() && !tail.contains('/'));
    let operation = tail
        .and_then(|tail| tail.split_once('/'))
        .is_some_and(|(_, suffix)| {
            suffix == "operations" || suffix.starts_with("operations/") || suffix == "invoke"
        });
    Ok(path != "/api/shutdown" && !operation && !module_configuration)
}
