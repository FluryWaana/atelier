use super::{
    card_lifecycle::CardFailure,
    task_creation::{CreationContext, Result, options, present, provider, text},
};
use crate::{
    contracts::Card,
    document::{Document, JsonString},
    domain::{
        checklist::checklist,
        tasks::{STATUSES, dependencies},
    },
};
use serde_json::{Value, value::RawValue};

pub trait EditingContext: CreationContext {
    fn cards(&self) -> Result<Vec<Document>>;
}

pub struct Edit {
    pub card: Card,
    pub sync_status: bool,
    pub requirements_changed: bool,
}

pub const REVIEW_CHANGE_REASON: &str =
    "message.requirements_or_criteria_changed_run_the_review_again";

fn check_expected(card: &Card, body: &Document) -> Result<()> {
    let Some(expected) = body.get::<Value>("expected")? else {
        return Ok(());
    };
    let invalid = || CardFailure::Invalid("message.invalid_edit_condition");
    let expected = expected.as_object().ok_or_else(invalid)?;
    let fields = [
        "title",
        "description",
        "instructions",
        "status",
        "checklist",
        "blockedBy",
        "agent",
        "model",
        "effort",
    ];
    if expected
        .keys()
        .any(|key| !fields.contains(&key.as_str()) || body.raw(key).is_none())
        || fields
            .iter()
            .any(|key| body.raw(key).is_some() && !expected.contains_key(*key))
    {
        return Err(invalid().into());
    }
    let conflict = || {
        CardFailure::Conflict(
            "message.this_task_changed_elsewhere_your_draft_is_preserved_close_and_re",
        )
    };
    for (key, previous) in expected {
        let current = if key == "title" {
            serde_json::to_string(&card.title)?
        } else {
            card.fields
                .raw(key)
                .map(|value| value.get().to_owned())
                .unwrap_or_else(|| {
                    if key == "checklist" || key == "blockedBy" {
                        "[]".into()
                    } else {
                        "\"\"".into()
                    }
                })
        };
        let current: Value = serde_json::from_str(&current).map_err(|_| conflict())?;
        if &current != previous {
            return Err(conflict().into());
        }
    }
    Ok(())
}

pub struct ReviewInvalidation {
    pub runs: Vec<Document>,
    pub automation_changed: bool,
}

pub fn invalidate_reviews(
    card_id: &str,
    fields: &mut Document,
    runs: Vec<Document>,
    reason: &str,
) -> Result<ReviewInvalidation> {
    let mut changed = Vec::new();
    for mut run in runs {
        if run.string_equals("cardId", card_id)? && run.string_equals("kind", "pr-review")? {
            run.set("reviewStaleReason", &reason)?;
            changed.push(run);
        }
    }
    let mut automation_changed = false;
    if let Some(raw) = fields
        .raw("automation")
        .filter(|raw| raw.get().starts_with('{'))
    {
        let mut automation: Document = serde_json::from_str(raw.get())?;
        if automation.string_equals("state", "ready")?
            || automation.string_equals("state", "blocked")?
        {
            automation.set("state", &"interrupted")?;
            automation.set("step", &"tests")?;
            automation.set("message", &reason)?;
            fields.set("automation", &automation)?;
            automation_changed = true;
        }
    }
    Ok(ReviewInvalidation {
        runs: changed,
        automation_changed,
    })
}

fn same_scalar(a: Option<&RawValue>, b: Option<&RawValue>) -> Result<bool> {
    let (Some(a), Some(b)) = (a, b) else {
        return Ok(a.is_none() && b.is_none());
    };
    match (a.get().as_bytes()[0], b.get().as_bytes()[0]) {
        (b'"', b'"') => Ok(serde_json::from_str::<JsonString>(a.get())?
            == serde_json::from_str::<JsonString>(b.get())?),
        (b'{' | b'[', _) | (_, b'{' | b'[') => Ok(false),
        (b'"', _) | (_, b'"') => Ok(false),
        _ => {
            let a: Value = serde_json::from_str(a.get())?;
            let b: Value = serde_json::from_str(b.get())?;
            Ok(a.as_f64().zip(b.as_f64()).map_or(a == b, |(a, b)| a == b))
        }
    }
}

pub fn requirements_changed(before: &Card, after: &Card) -> Result<bool> {
    if before.title != after.title {
        return Ok(true);
    }
    let empty = RawValue::from_string("\"\"".into())?;
    for key in ["description", "instructions"] {
        let a = if before.fields.truthy(key)? {
            before.fields.raw(key)
        } else {
            None
        }
        .unwrap_or(&empty);
        let b = if after.fields.truthy(key)? {
            after.fields.raw(key)
        } else {
            None
        }
        .unwrap_or(&empty);
        if a.get() != b.get() && !same_scalar(Some(a), Some(b))? {
            return Ok(true);
        }
    }
    let criteria = |card: &Card| -> Result<Vec<Document>> {
        Ok(if card.fields.truthy("checklist")? {
            card.fields.get("checklist")?.unwrap_or_default()
        } else {
            Vec::new()
        })
    };
    let a = criteria(before)?;
    let b = criteria(after)?;
    if a.len() != b.len() {
        return Ok(true);
    }
    let null = RawValue::from_string("null".into())?;
    for (a, b) in a.iter().zip(&b) {
        let a = a.raw("text").unwrap_or(&null);
        let b = b.raw("text").unwrap_or(&null);
        if a.get() != b.get() && !same_scalar(Some(a), Some(b))? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn prepare(
    context: &impl EditingContext,
    original: &Card,
    body: &Document,
    busy: bool,
) -> Result<Edit> {
    let mut card = original.clone();
    if card.fields.truthy("ticket")?
        && (body.raw("title").is_some() || body.raw("description").is_some())
    {
        return Err(CardFailure::Conflict(
            "message.this_card_is_linked_to_an_issue_reload_the_task_before_editing_i",
        )
        .into());
    }
    if card.fields.truthy("archived")? {
        return Err(
            CardFailure::Conflict("message.restore_this_task_before_resuming_work").into(),
        );
    }
    if busy {
        return Err(
            CardFailure::Conflict("message.stop_the_agent_before_editing_this_card").into(),
        );
    }
    check_expected(&card, body)?;
    // Validate stored requirements before touching the card.
    requirements_changed(&card, &card)?;
    if card.fields.truthy("worktree")? {
        let empty = RawValue::from_string("\"\"".into())?;
        for key in ["repo", "branch", "baseBranch"] {
            let previous = if key == "baseBranch" && !card.fields.truthy(key)? {
                Some(empty.as_ref())
            } else {
                card.fields.raw(key)
            };
            if body.raw(key).is_some() && !same_scalar(body.raw(key), previous)? {
                return Err(CardFailure::Invalid(
                    "message.remove_the_worktree_before_changing_the_repository_or_branch",
                )
                .into());
            }
        }
    }
    if let Some(raw) = body.raw("status") {
        let status = body
            .get::<String>("status")
            .ok()
            .flatten()
            .unwrap_or_default();
        if !STATUSES.contains(&status.as_str()) {
            return Err(CardFailure::Invalid("message.invalid_status").into());
        }
        card.fields.set("status", &raw)?;
    }
    if body.raw("title").is_some() {
        card.title = text(body.raw("title"), 300)?;
        if card.title.0.is_empty() {
            return Err(CardFailure::Invalid("message.title_required").into());
        }
    }
    if body.raw("description").is_some() {
        card.fields
            .set("description", &text(body.raw("description"), 20_000)?)?;
    }
    if let Some(repo) = body.raw("repo") {
        let repo = if body.truthy("repo")? {
            context.repository(repo)?
        } else {
            JsonString::default()
        };
        card.fields.set("repo", &repo)?;
    }
    if body.raw("agent").is_some() {
        let agent = body
            .get::<JsonString>("agent")
            .map_err(|_| CardFailure::Invalid("message.invalid_agent"))?
            .ok_or(CardFailure::Invalid("message.invalid_agent"))?;
        provider(&context.settings()?, &agent)?;
        if !same_scalar(body.raw("agent"), card.fields.raw("agent"))? {
            card.fields.set("agentExplicit", &true)?;
        }
        card.fields.set("agent", &agent)?;
    }
    let unchanged_agent = same_scalar(card.fields.raw("agent"), original.fields.raw("agent"))?;
    let agent = card
        .fields
        .get::<JsonString>("agent")
        .map_err(|_| CardFailure::Invalid("message.invalid_agent"))?
        .ok_or(CardFailure::Invalid("message.invalid_agent"))?;
    let mut selected = Document::default();
    for key in ["model", "effort"] {
        if let Some(value) = present(body.raw(key))
            .or_else(|| unchanged_agent.then(|| card.fields.raw(key)).flatten())
        {
            selected.set(key, &value)?;
        } else if !unchanged_agent {
            selected.set(key, &"")?;
        }
    }
    let selected = options(&agent, &selected, &context.settings()?)?;
    for key in ["model", "effort"] {
        card.fields.set(key, &selected[key])?;
    }
    for key in ["branch", "baseBranch"] {
        if body.raw(key).is_some() {
            card.fields.set(key, &text(body.raw(key), 180)?)?;
        }
    }
    if let Some(value) = body.raw("checklist") {
        card.fields.set(
            "checklist",
            &checklist(value).map_err(CardFailure::Invalid)?,
        )?;
    }
    if let Some(value) = body.raw("blockedBy") {
        let invalid = CardFailure::Invalid(
            "message.invalid_dependencies_20_tasks_maximum_without_duplicates_or_self",
        );
        let value = serde_json::from_str::<Value>(value.get()).map_err(|_| invalid)?;
        let mut references = Vec::new();
        for other in context.cards()? {
            let mut reference = serde_json::Map::new();
            for key in ["id", "blockedBy"] {
                if let Some(raw) = other.raw(key) {
                    reference.insert(
                        key.into(),
                        serde_json::from_str(raw.get()).unwrap_or(Value::Null),
                    );
                }
            }
            references.push(Value::Object(reference));
        }
        card.fields.set(
            "blockedBy",
            &dependencies(&card.id, &value, &references).map_err(CardFailure::Invalid)?,
        )?;
    }
    if body.raw("instructions").is_some() {
        card.fields
            .set("instructions", &text(body.raw("instructions"), 20_000)?)?;
    }
    if let Some(raw) = body.raw("mode") {
        if !body.string_equals("mode", "edit")? && !body.string_equals("mode", "analyze")? {
            return Err(CardFailure::Invalid("message.invalid_mode").into());
        }
        card.fields.set("mode", &raw)?;
    }
    let sync_status = body.raw("status").is_some()
        && !same_scalar(body.raw("status"), original.fields.raw("status"))?;
    card.fields.set("updatedAt", &context.timestamp()?)?;
    let requirements_changed = requirements_changed(original, &card)?;
    Ok(Edit {
        card,
        sync_status,
        requirements_changed,
    })
}
