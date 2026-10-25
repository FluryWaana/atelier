use crate::{document::Document, domain::time::time_fields};
use serde_json::value::RawValue;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub struct TimeConflict;
impl std::fmt::Display for TimeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cette saisie de temps a changé ailleurs. Recharge-la avant de réessayer.")
    }
}
impl std::error::Error for TimeConflict {}

pub trait TimeRepository {
    fn card(&self, id: Option<&RawValue>) -> Result<Option<Document>>;
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
            .ok_or("Durée enregistrée invalide.")?;
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
        let card_id = body.raw("cardId");
        let card = repository.card(card_id)?.ok_or("Carte introuvable.")?;
        if card.truthy("archived")? {
            return Err("Restaure cette tâche avant de saisir du temps.".into());
        }
        let start = body
            .get::<bool>("start")
            .map_err(|_| "Action de chronomètre invalide.")?
            .unwrap_or(false);
        let mut entry = Document::default();
        entry.set("cardId", &card_id)?;
        apply_fields(&mut entry, body)?;
        if start {
            for existing in repository.entries()? {
                if existing.truthy("runningSince")? {
                    return Err("Arrête le chronomètre actif avant d’en démarrer un autre.".into());
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
        .ok_or("Entrée de temps introuvable.")?;
    let entry_id = entry
        .get::<String>("id")?
        .ok_or("Identifiant de temps invalide.")?;
    if exporting(&entry_id) {
        return Err("Envoi en cours.".into());
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
                .ok_or("Entrée de temps introuvable.")?;
            entry.remove("runningSince");
        }
        "update" | "remove" => {
            if entry.truthy("runningSince")? || !entry.string_equals("exportState", "draft")? {
                return Err(
                    "Arrête le chronomètre. Seules les entrées non envoyées sont modifiables."
                        .into(),
                );
            }
            if action == "remove" {
                repository.remove(&entry_id)?;
                let mut response = Document::default();
                response.set("ok", &true)?;
                return Ok(response);
            }
            apply_fields(&mut entry, body)?;
            entry.remove("error");
            entry.set("interrupted", &false)?;
        }
        "reset-export" => {
            if !entry.string_equals("exportState", "uncertain")?
                || body.get::<bool>("confirmed").ok().flatten() != Some(true)
            {
                return Err("Vérifie d’abord dans Tempo que cette entrée n’existe pas.".into());
            }
            entry.set("exportState", &"draft")?;
            for key in ["exportPayload", "exportJira", "error"] {
                entry.remove(key);
            }
        }
        _ => return Err("Action inconnue.".into()),
    }
    repository.save(&entry)?;
    Ok(entry)
}
