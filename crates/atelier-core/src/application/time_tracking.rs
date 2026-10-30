use crate::{document::Document, domain::time::time_fields};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub struct TimeConflict;
impl std::fmt::Display for TimeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("message.this_time_entry_changed_elsewhere_reload_it_before_trying_again")
    }
}
impl std::error::Error for TimeConflict {}

pub trait TimeRepository {
    fn card(&self, id: &str) -> Result<Option<Document>>;
    fn entry(&self, id: &str) -> Result<Option<Document>>;
    fn entries(&self) -> Result<Vec<Document>>;
    fn save(&self, entry: &Document) -> Result<()>;
    fn remove(&self, id: &str) -> Result<()>;
}

pub trait TimeClock {
    fn now(&self) -> i64;
    fn parse(&self, timestamp: &str) -> Option<i64>;
    fn format(&self, milliseconds: i64) -> Result<String>;
}

pub fn initialize(repository: &impl TimeRepository) -> Result<()> {
    for mut entry in repository.entries()? {
        if entry.truthy("runningSince")? {
            entry.remove("runningSince");
            entry.set("interrupted", &true)?;
        }
        if entry.string_equals("exportState", "sending")? {
            entry.set("exportState", &"uncertain")?;
            if let Some(mut intent) =
                entry.get::<crate::contracts::TempoExportIntent>("tempoExport")?
            {
                intent.state = crate::contracts::ForgeWriteState::Uncertain;
                entry.set("tempoExport", &intent)?;
            }
        }
        repository.save(&entry)?;
    }
    Ok(())
}

pub fn tick(repository: &impl TimeRepository, clock: &impl TimeClock) -> Result<bool> {
    let mut changed = false;
    for mut entry in repository.entries()? {
        if !entry.truthy("runningSince")? {
            continue;
        }
        let Some(started) = entry
            .get::<String>("runningSince")
            .ok()
            .flatten()
            .and_then(|timestamp| clock.parse(&timestamp))
        else {
            continue;
        };
        let elapsed = ((i128::from(clock.now()) - i128::from(started)) / 1000).max(0);
        if elapsed == 0 {
            continue;
        }
        let seconds = entry
            .get::<f64>("seconds")?
            .ok_or("message.saved_duration_invalid")?;
        let seconds = (seconds + elapsed as f64).min(86400.0);
        entry.set("seconds", &seconds)?;
        if seconds == 86400.0 {
            entry.remove("runningSince");
        } else {
            entry.set(
                "runningSince",
                &clock.format(i64::try_from(i128::from(started) + elapsed * 1000)?)?,
            )?;
        }
        repository.save(&entry)?;
        changed = true;
    }
    Ok(changed)
}

pub fn close(repository: &impl TimeRepository, clock: &impl TimeClock) -> Result<bool> {
    let mut changed = tick(repository, clock)?;
    for mut entry in repository.entries()? {
        if entry.truthy("runningSince")? {
            entry.remove("runningSince");
            repository.save(&entry)?;
            changed = true;
        }
    }
    Ok(changed)
}

fn apply_fields(entry: &mut Document, body: &Document) -> Result<()> {
    let fields = time_fields(body)?;
    entry.set("seconds", &fields.seconds)?;
    entry.set("date", &fields.date)?;
    entry.set("startTime", &fields.start_time)?;
    entry.set("description", &fields.description)?;
    Ok(())
}

pub fn execute(
    repository: &impl TimeRepository,
    clock: &impl TimeClock,
    action: &str,
    id: &str,
    body: &Document,
    new_id: impl FnOnce() -> String,
    exporting: impl Fn(&str) -> bool,
) -> Result<Document> {
    if action == "create" {
        let card_id = body
            .get::<String>("cardId")
            .map_err(|_| "message.invalid_card_id")?
            .filter(|id| !id.trim().is_empty())
            .ok_or("message.invalid_card_id")?;
        let card = repository.card(&card_id)?.ok_or("message.card_not_found")?;
        if card.truthy("archived")? {
            return Err("message.restore_this_task_before_logging_time".into());
        }
        let start = body
            .get::<bool>("start")
            .map_err(|_| "message.invalid_timer_action")?
            .unwrap_or(false);
        let mut entry = Document::default();
        entry.set("cardId", &card_id)?;
        apply_fields(&mut entry, body)?;
        if start {
            for existing in repository.entries()? {
                if existing.truthy("runningSince")? {
                    return Err("message.stop_the_active_timer_before_starting_another".into());
                }
            }
            entry.set("seconds", &0)?;
            entry.set("runningSince", &clock.format(clock.now())?)?;
        }
        entry.set("id", &new_id())?;
        entry.set("exportState", &"draft")?;
        repository.save(&entry)?;
        return Ok(entry);
    }
    let mut entry = repository
        .entry(id)?
        .ok_or("message.time_entry_not_found")?;
    let entry_id = entry
        .get::<String>("id")?
        .ok_or("message.time_id_invalid")?;
    if exporting(&entry_id) {
        return Err("message.sending".into());
    }
    if matches!(action, "update" | "remove")
        && let Some(expected) = body.get::<serde_json::Value>("expected")?
        && expected != serde_json::to_value(&entry)?
    {
        return Err(TimeConflict.into());
    }
    match action {
        "stop" => {
            tick(repository, clock)?;
            entry = repository
                .entry(&entry_id)?
                .ok_or("message.time_entry_not_found")?;
            entry.remove("runningSince");
        }
        "update" | "remove" => {
            if entry.truthy("runningSince")? || !entry.string_equals("exportState", "draft")? {
                return Err(
                    "message.stop_the_timer_only_entries_that_have_not_been_sent_can_be_edite"
                        .into(),
                );
            }
            if action == "remove" {
                if entry.raw("tempoExportHistory").is_some()
                    || entry.raw("tempoExportEffect").is_some()
                    || entry.raw("connectorExport").is_some()
                {
                    return Err("message.keep_this_entry_it_contains_export_history".into());
                }
                repository.remove(&entry_id)?;
                let mut response = Document::default();
                response.set("ok", &true)?;
                return Ok(response);
            }
            apply_fields(&mut entry, body)?;
            entry.remove("tempoExportPreview");
            entry.remove("error");
            entry.set("interrupted", &false)?;
        }
        "reset-export" => {
            if entry.raw("connectorExport").is_some() {
                return Err(
                    "message.reconcile_this_export_in_the_module_journal_no_implicit_resend_i"
                        .into(),
                );
            }
            if entry.raw("tempoExport").is_some() {
                return Err(
                    "message.use_explicit_confirmation_with_history_to_unlock_this_tempo_expo"
                        .into(),
                );
            }
            if !entry.string_equals("exportState", "uncertain")?
                || body.get::<bool>("confirmed").ok().flatten() != Some(true)
            {
                return Err("message.first_check_in_tempo_that_this_entry_does_not_exist".into());
            }
            entry.set("exportState", &"draft")?;
            for key in ["exportPayload", "exportJira", "error"] {
                entry.remove(key);
            }
        }
        _ => return Err("message.unknown_action".into()),
    }
    repository.save(&entry)?;
    Ok(entry)
}
