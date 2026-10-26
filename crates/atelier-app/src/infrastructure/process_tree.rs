use crate::Result;
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::ContainedChild;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::ContainedChild;
async fn bounded(reader: impl AsyncRead + Unpin, limit: usize, stream: &str) -> Result<Vec<u8>> {
    let count = u64::try_from(limit.checked_add(1).ok_or("Limite de sortie invalide.")?)?;
    let mut bytes = Vec::new();
    reader.take(count).read_to_end(&mut bytes).await?;
    if bytes.len() > limit {
        return Err(format!("Limite de sortie {stream} dépassée.").into());
    }
    Ok(bytes)
}

pub async fn capture(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
    limit: usize,
) -> Result<std::process::Output> {
    capture_guarded(command, input, timeout, limit, std::future::pending()).await
}

pub async fn capture_guarded(
    command: &mut Command,
    input: &[u8],
    timeout: Duration,
    limit: usize,
    guard: impl std::future::Future<Output = crate::Error>,
) -> Result<std::process::Output> {
    use std::process::Stdio;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ContainedChild::spawn(command).await?;
    let mut stdin = child
        .child
        .stdin
        .take()
        .ok_or("Entrée du processus absente.")?;
    let stdout = child
        .child
        .stdout
        .take()
        .ok_or("Sortie du processus absente.")?;
    let stderr = child
        .child
        .stderr
        .take()
        .ok_or("Sortie d’erreur du processus absente.")?;
    let operation = async {
        let input = async {
            // Like Node runCommand, a child may close stdin without consuming the whole input.
            let _ = stdin.write_all(input).await;
            drop(stdin);
            Ok::<(), crate::Error>(())
        };
        let (stdout, stderr, (), status) = tokio::try_join!(
            bounded(stdout, limit, "stdout"),
            bounded(stderr, limit, "stderr"),
            input,
            child.wait()
        )?;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    let result = tokio::select! {
        result = tokio::time::timeout(timeout, operation) => match result {
            Ok(result) => result,
            Err(_) => Err("Durée maximale du processus dépassée.".into()),
        },
        error = guard => Err(error),
    };
    if result.is_err() {
        child.stop().await?;
    }
    result
}
