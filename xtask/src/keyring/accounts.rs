use super::{KeyringPlatform, Result, execute_fixture, execute_keyring, keyring_protect, session};
use serde_json::{Value, json};
use std::{fs, path::Path};

const MARKER: &str = "atelier-m02-synthetic-keyring";
const PLAIN: &str = "synthetic-account-é-🔑";

fn account() -> Result<u32> {
    #[cfg(unix)]
    {
        // geteuid reads the actual process identity, not an inherited environment variable.
        Ok(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        Err("POSIX account probe requires a native POSIX process".into())
    }
}

fn different_account(record: &Value, current: u32) -> Result<&str> {
    let uid = record["account"].as_u64().ok_or("Missing native account")?;
    if uid == u64::from(current) {
        return Err("A different native OS account is required".into());
    }
    record["encrypted"]
        .as_str()
        .ok_or("Missing synthetic ciphertext".into())
}

fn read(directory: &Path, name: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(directory.join(name))?)?)
}

fn write_shared(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only synthetic ciphertext/UID/service metadata crosses the account boundary.
        fs::set_permissions(path, fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

pub async fn probe(directory: &Path, phase: &str) -> Result<()> {
    if !cfg!(target_os = "linux")
        || std::env::var("ATELIER_TEST_DISPOSABLE_KEYRING").as_deref() != Ok("1")
        || fs::read_to_string(directory.join(".keyring-fixture"))? != MARKER
    {
        return Err("Account probe requires the disposable Linux CI fixture".into());
    }
    let uid = account()?;
    if !["prepare", "other", "verify", "cleanup"].contains(&phase) {
        return Err("Unknown account fixture phase".into());
    }
    let foreign = match phase {
        "other" => Some(read(directory, "first.json")?),
        "verify" => Some(read(directory, "other.json")?),
        _ => None,
    };
    if let Some(record) = &foreign {
        different_account(record, uid)?;
    }
    let service = if phase == "prepare" {
        let service = format!("atelier-kanban-test-{}", session::new_token()?);
        write_shared(&directory.join("service"), service.as_bytes())?;
        service
    } else {
        fs::read_to_string(directory.join("service"))?
    };
    if !service
        .strip_prefix("atelier-kanban-test-")
        .is_some_and(|suffix| {
            suffix.len() == 64 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err("Invalid synthetic service".into());
    }
    let mut execute = async |command: &str, args: &[&str], input: Option<&str>| {
        execute_fixture(&service, None, command, args, input).await
    };
    let platform = KeyringPlatform::Linux;
    let result: Result<()> = async {
        match phase {
            "prepare" | "other" => {
                let encrypted = keyring_protect(platform, PLAIN, false, &mut execute).await?;
                if keyring_protect(platform, &encrypted, true, &mut execute).await? != PLAIN {
                    return Err("Same-account positive control failed".into());
                }
                if let Some(record) = &foreign {
                    if keyring_protect(platform, different_account(record, uid)?, true, &mut execute).await.is_ok() {
                        return Err("Another account decrypted the synthetic secret".into());
                    }
                    if keyring_protect(platform, &encrypted, true, &mut execute).await? != PLAIN {
                        return Err("Refusal changed the second account key".into());
                    }
                }
                let name = if phase == "prepare" { "first.json" } else { "other.json" };
                write_shared(&directory.join(name), &serde_json::to_vec(&json!({"account":uid,"encrypted":encrypted}))?)?;
            }
            "verify" => {
                let foreign = read(directory, "other.json")?;
                if keyring_protect(platform, different_account(&foreign, uid)?, true, &mut execute).await.is_ok() {
                    return Err("First account decrypted the other account secret".into());
                }
                let original = read(directory, "first.json")?;
                if original["account"] != uid || keyring_protect(platform, original["encrypted"].as_str().ok_or("Missing original ciphertext")?, true, &mut execute).await? != PLAIN {
                    return Err("Original account lost its secret".into());
                }
                println!("Linux keyring: distinct native accounts, positive controls and reciprocal refusal verified.");
            }
            "cleanup" => {}
            _ => return Err("Unknown account fixture phase".into()),
        }
        Ok(())
    }.await;
    if phase != "prepare" || result.is_err() {
        let cleanup = execute_keyring(
            "secret-tool",
            &["clear", "application", &service, "purpose", "token-key"],
            None,
        )
        .await;
        result?;
        let status = cleanup?.0;
        if status != 0 && !(phase == "cleanup" && status == 1) {
            return Err("Synthetic account key cleanup failed".into());
        }
    } else {
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_reused_account_cannot_satisfy_the_probe() {
        assert!(different_account(&json!({"encrypted":"synthetic"}), 42).is_err());
        assert!(different_account(&json!({"account":42,"encrypted":"synthetic"}), 42).is_err());
        assert!(different_account(&json!({"account":43}), 42).is_err());
        assert_eq!(
            different_account(&json!({"account":43,"encrypted":"synthetic"}), 42).unwrap(),
            "synthetic"
        );
    }

    #[cfg(unix)]
    #[test]
    fn only_synthetic_records_are_readable_across_accounts() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("record");
        write_shared(&path, b"synthetic ciphertext").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(fs::read(&path).unwrap(), b"synthetic ciphertext");
        let current = account().unwrap();
        assert!(
            different_account(&json!({"account":current,"encrypted":"synthetic"}), current)
                .is_err()
        );
    }
}
