use super::process_tree::ContainedChild;
use crate::Result;
use atelier_core::application::agent_account::{Probe, Report};
use serde_json::Value;
use std::{collections::BTreeMap, ffi::OsString, path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
const LIMIT: usize = 1024 * 1024;
pub(super) fn reflected(value: &Value, secrets: &[String]) -> bool {
    let contains = |text: &str| {
        secrets
            .iter()
            .any(|secret| !secret.is_empty() && text.contains(secret))
    };
    match value {
        Value::String(value) => contains(value),
        Value::Number(value) => contains(&value.to_string()),
        Value::Array(values) => values.iter().any(|value| reflected(value, secrets)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| contains(key) || reflected(value, secrets)),
        _ => false,
    }
}
async fn stream(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut tokio::process::ChildStdout,
    secrets: Vec<String>,
) -> Result<Report> {
    let mut probe = Probe::default();
    let mut next = vec![probe.start()];
    let mut reader = BufReader::new(stdout.take((LIMIT + 1) as u64));
    let mut total = 0usize;
    for _ in 0..64 {
        for value in next.drain(..) {
            let mut bytes = serde_json::to_vec(&value)?;
            bytes.push(b'\n');
            stdin.write_all(&bytes).await?;
        }
        let mut bytes = Vec::new();
        let length = reader.read_until(b'\n', &mut bytes).await?;
        total = total.saturating_add(length);
        if length == 0 || total > LIMIT || !bytes.ends_with(b"\n") {
            return Err("Réponse de quotas absente, incomplète ou trop volumineuse.".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Réponse de quotas invalide.")?;
        if reflected(&value, &secrets) {
            return Err("Réponse de quotas contenant une donnée confidentielle refusée.".into());
        }
        let (send, report) = probe.receive(value)?;
        next = send;
        if let Some(report) = report {
            return Ok(report);
        }
    }
    Err("Trop de messages pendant la lecture des quotas.".into())
}
pub(super) async fn read(
    binary: &Path,
    environment: &BTreeMap<String, OsString>,
) -> Result<Report> {
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("app-server")
        .current_dir(binary.parent().ok_or("Dossier de l’exécutable absent.")?)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let secrets = environment
        .iter()
        .filter(|(key, _)| key.contains("KEY") || key.contains("TOKEN"))
        .filter_map(|(_, value)| {
            value
                .to_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .collect();
    let mut child = ContainedChild::spawn(&mut command).await?;
    let mut stdin = child
        .child
        .stdin
        .take()
        .ok_or("Entrée du diagnostic absente.")?;
    let mut stdout = child
        .child
        .stdout
        .take()
        .ok_or("Sortie du diagnostic absente.")?;
    let mut stderr_pipe = child
        .child
        .stderr
        .take()
        .ok_or("Sortie d’erreur du diagnostic absente.")?;
    let stderr = async {
        let mut bytes = Vec::new();
        (&mut stderr_pipe)
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > LIMIT {
            return Err("Sortie du diagnostic trop volumineuse.".into());
        }
        Ok::<(), crate::Error>(())
    };
    let work = async {
        // Keep the pipe endpoints live until containment cleanup has finished.
        let result = stream(&mut stdin, &mut stdout, secrets).await;
        child.stop().await?;
        result
    };
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let (report, ()) = tokio::try_join!(work, stderr)?;
        Ok::<_, crate::Error>(report)
    })
    .await
    .unwrap_or_else(|_| Err("Lecture des quotas interrompue ou trop longue.".into()));
    if result.is_err() {
        child.stop().await?;
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn reflected_credentials_in_keys_labels_and_numeric_fields_are_rejected() {
        let secrets = vec!["synthetic-private-token".into(), "2000000000".into()];
        for value in [
            json!({"limitName":"synthetic-private-token"}),
            json!({"synthetic-private-token":null}),
            json!({"primary":{"resetsAt":2000000000u64}}),
        ] {
            assert!(reflected(&value, &secrets));
        }
        assert!(!reflected(&json!({"primary":{"usedPercent":25}}), &secrets));
    }
}
