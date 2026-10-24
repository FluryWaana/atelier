use super::task_creation::Result;
use crate::document::Document;

#[derive(Debug)]
pub enum BackupFailure {
    Invalid(&'static str),
    Conflict(&'static str),
}
impl std::fmt::Display for BackupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid(message) | Self::Conflict(message) => message,
        })
    }
}
impl std::error::Error for BackupFailure {}

pub fn require_idle(
    providers_busy: bool,
    servers_running: bool,
    operations_busy: bool,
    times: &[Document],
) -> Result<()> {
    if providers_busy {
        return Err(BackupFailure::Conflict(
            "Attendez la fin des exécutions et mises à jour avant de modifier les fournisseurs.",
        )
        .into());
    }
    if servers_running {
        return Err(BackupFailure::Conflict(
            "Arrête les serveurs Front / Back avant cette opération.",
        )
        .into());
    }
    let mut busy = operations_busy;
    for entry in times {
        busy |= entry.truthy("runningSince")?
            || entry.string_equals("exportState", "sending")?
            || entry.string_equals("exportState", "uncertain")?;
    }
    if busy {
        return Err(BackupFailure::Conflict("Mets la file en pause, arrête les agents et chronomètres, puis termine ou vérifie les envois Tempo avant cette opération.").into());
    }
    Ok(())
}

pub fn restore_name(body: &Document) -> Result<String> {
    if !body
        .raw("confirmed")
        .is_some_and(|value| value.get() == "true")
    {
        return Err(BackupFailure::Invalid("Confirme la restauration de cette sauvegarde.").into());
    }
    body.get::<String>("name")
        .ok()
        .flatten()
        .ok_or_else(|| BackupFailure::Invalid("Sauvegarde locale introuvable.").into())
}

pub fn pause_restored_queue(settings: &mut Document) -> Result<()> {
    let mut queue: Document = serde_json::from_str(r#"{"ids":[],"paused":true,"message":""}"#)?;
    if let Some(value) = settings.raw("taskQueue") {
        queue.overlay(&Document::spread(value)?);
    }
    queue.set("paused", &true)?;
    queue.set(
        "message",
        &"File en pause ; la tâche active peut se terminer.",
    )?;
    settings.set("taskQueue", &queue)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_guards_keep_error_order_and_restore_only_with_explicit_confirmation() {
        assert!(
            require_idle(true, true, true, &[])
                .unwrap_err()
                .to_string()
                .contains("fournisseurs")
        );
        assert!(
            require_idle(false, true, true, &[])
                .unwrap_err()
                .to_string()
                .contains("Front / Back")
        );
        for value in [
            r#"{"runningSince":1}"#,
            r#"{"exportState":"sending"}"#,
            r#"{"exportState":"uncertain"}"#,
        ] {
            let entry = serde_json::from_str(value).unwrap();
            assert!(matches!(
                require_idle(false, false, false, &[entry])
                    .unwrap_err()
                    .downcast_ref::<BackupFailure>(),
                Some(BackupFailure::Conflict(_))
            ));
        }
        require_idle(
            false,
            false,
            false,
            &[serde_json::from_str(r#"{"runningSince":0,"exportState":"exported"}"#).unwrap()],
        )
        .unwrap();
        assert!(require_idle(false, false, true, &[]).is_err());
        for value in ["null", "false", "1", "\"true\""] {
            let body = serde_json::from_str(&format!(r#"{{"confirmed":{value},"name":"saved"}}"#))
                .unwrap();
            assert!(
                restore_name(&body)
                    .unwrap_err()
                    .to_string()
                    .contains("Confirme")
            );
        }
        let mut settings = serde_json::from_str(
            r#"{"taskQueue":{"ids":["\ud800"],"paused":false,"future":1},"other":"\udc00"}"#,
        )
        .unwrap();
        pause_restored_queue(&mut settings).unwrap();
        let queue = settings.get::<Document>("taskQueue").unwrap().unwrap();
        assert!(queue.truthy("paused").unwrap());
        assert_eq!(queue.raw("ids").unwrap().get(), r#"["\ud800"]"#);
        assert_eq!(queue.get::<i32>("future").unwrap(), Some(1));
        assert_eq!(settings.raw("other").unwrap().get(), r#""\udc00""#);
        for (value, field) in [
            (r#"["\ud800"]"#, Some(r#""\ud800""#)),
            (r#""\udc00x""#, Some(r#""\udc00""#)),
            ("null", None),
            ("3", None),
        ] {
            let mut settings =
                serde_json::from_str(&format!(r#"{{"taskQueue":{value}}}"#)).unwrap();
            pause_restored_queue(&mut settings).unwrap();
            let queue = settings.get::<Document>("taskQueue").unwrap().unwrap();
            assert_eq!(queue.raw("0").map(|value| value.get()), field);
            assert!(queue.truthy("paused").unwrap());
        }
    }
}
