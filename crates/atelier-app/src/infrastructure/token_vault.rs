use super::{database::Database, session, store::Store};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy)]
pub enum KeyringPlatform {
    Linux,
    Macos,
}

impl KeyringPlatform {
    fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "darwin",
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Envelope {
    format: String,
    platform: String,
    iv: String,
    tag: String,
    data: String,
}

fn unhex<const N: usize>(value: &str) -> Result<[u8; N]> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Encodage du coffre invalide.".into());
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)?;
    }
    Ok(bytes)
}

pub async fn keyring_protect(
    platform: KeyringPlatform,
    value: &str,
    decrypt: bool,
    execute: &mut impl AsyncFnMut(&str, &[&str], Option<&str>) -> Result<(i32, String)>,
) -> Result<String> {
    // Validate the envelope before querying a keyring or attempting to create a key.
    let envelope = if decrypt {
        let envelope: Envelope = serde_json::from_str(value)?;
        if envelope.format != "keyring-aes-256-gcm-v1" || envelope.platform != platform.name() {
            return Err(
                "Ce coffre appartient à un autre système. Renseignez de nouveau les jetons.".into(),
            );
        }
        Some(envelope)
    } else {
        None
    };
    let (command, lookup, missing) = match platform {
        KeyringPlatform::Linux => (
            "secret-tool",
            vec![
                "lookup",
                "application",
                "atelier-kanban",
                "purpose",
                "token-key",
            ],
            1,
        ),
        KeyringPlatform::Macos => (
            "security",
            vec![
                "find-generic-password",
                "-s",
                "atelier-kanban",
                "-a",
                "token-key",
                "-w",
            ],
            44,
        ),
    };
    let (mut status, mut key) = execute(command, &lookup, None).await?;
    if status == missing && !decrypt {
        let generated = session::new_token()?;
        let (saved, _) = match platform {
            KeyringPlatform::Linux => {
                execute(
                    command,
                    &[
                        "store",
                        "--label=Atelier Kanban",
                        "application",
                        "atelier-kanban",
                        "purpose",
                        "token-key",
                    ],
                    Some(&generated),
                )
                .await?
            }
            KeyringPlatform::Macos => {
                execute(
                    command,
                    &["-i"],
                    Some(&format!(
                        "add-generic-password -s atelier-kanban -a token-key -w {generated}\n"
                    )),
                )
                .await?
            }
        };
        if saved != 0 {
            return Err("Impossible d’enregistrer la clé dans le trousseau système.".into());
        }
        (status, key) = execute(command, &lookup, None).await?;
    }
    if status != 0 {
        return Err("Déverrouillez le trousseau système pour accéder aux jetons.".into());
    }
    let key = unhex::<32>(key.trim())?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key).map_err(|_| "Clé du trousseau système invalide.")?,
    );
    if let Some(envelope) = envelope {
        let nonce = Nonce::assume_unique_for_key(unhex(&envelope.iv)?);
        let mut data = STANDARD.decode(envelope.data)?;
        data.extend_from_slice(&unhex::<16>(&envelope.tag)?);
        let plain = key
            .open_in_place(nonce, Aad::empty(), &mut data)
            .map_err(|_| "Coffre illisible ou altéré.")?;
        Ok(String::from_utf8(plain.to_vec())?)
    } else {
        // Every encryption uses a fresh 96-bit CSPRNG nonce, matching the existing Node format.
        let mut iv = [0; 12];
        getrandom::fill(&mut iv).map_err(|_| "Générateur aléatoire indisponible.")?;
        let mut data = value.as_bytes().to_vec();
        let tag = key
            .seal_in_place_separate_tag(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut data)
            .map_err(|_| "Chiffrement du coffre impossible.")?;
        Ok(serde_json::to_string(&Envelope {
            format: "keyring-aes-256-gcm-v1".into(),
            platform: platform.name().into(),
            iv: session::hexadecimal(&iv),
            tag: session::hexadecimal(tag.as_ref()),
            data: STANDARD.encode(data),
        })?)
    }
}

async fn bounded_output(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(65_537).read_to_end(&mut bytes).await?;
    if bytes.len() > 65_536 {
        return Err("Réponse du trousseau trop volumineuse.".into());
    }
    Ok(bytes)
}

pub async fn execute_keyring(
    command: &str,
    args: &[&str],
    input: Option<&str>,
) -> Result<(i32, String)> {
    let mut command = tokio::process::Command::new(command);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|_| "Coffre système inaccessible. Sur Linux, installez secret-tool (libsecret) et déverrouillez le trousseau.")?;
    let mut stdin = child.stdin.take().ok_or("Entrée du trousseau absente.")?;
    let stdout = child.stdout.take().ok_or("Sortie du trousseau absente.")?;
    let stderr = child.stderr.take().ok_or("Sortie du trousseau absente.")?;
    let operation = async {
        let write = async {
            if let Some(input) = input {
                stdin.write_all(input.as_bytes()).await?;
            }
            drop(stdin);
            Ok::<_, crate::Error>(())
        };
        let wait = async { Ok::<_, crate::Error>(child.wait().await?) };
        let (_, stdout, _, status) =
            tokio::try_join!(write, bounded_output(stdout), bounded_output(stderr), wait)?;
        Ok::<_, crate::Error>((status.code().unwrap_or(-1), String::from_utf8(stdout)?))
    };
    let result = tokio::time::timeout(Duration::from_secs(15), operation).await;
    if !matches!(&result, Ok(Ok(_))) {
        let _ = child.kill().await;
    }
    result.map_err(|_| "Le trousseau système ne répond pas.")?
}

pub async fn protect(value: &str, decrypt: bool) -> Result<String> {
    #[cfg(unix)]
    static KEYRING_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    #[cfg(unix)]
    let _guard = KEYRING_LOCK.lock().await;
    #[cfg(windows)]
    {
        session::protect(value, decrypt)
    }
    #[cfg(target_os = "linux")]
    {
        keyring_protect(KeyringPlatform::Linux, value, decrypt, &mut execute_keyring).await
    }
    #[cfg(target_os = "macos")]
    {
        keyring_protect(KeyringPlatform::Macos, value, decrypt, &mut execute_keyring).await
    }
}

pub fn read_encrypted(store: &Store<'_>) -> Result<Option<String>> {
    initialize(store)?;
    Ok(store
        .connection
        .query_row("SELECT encrypted FROM token_vault WHERE id=1", [], |row| {
            row.get(0)
        })
        .optional()?)
}

fn initialize(store: &Store<'_>) -> Result<()> {
    store.connection.execute_batch("CREATE TABLE IF NOT EXISTS token_vault (id INTEGER PRIMARY KEY CHECK(id=1), encrypted TEXT NOT NULL)")?;
    Ok(())
}

pub fn save_encrypted(store: &Store<'_>, encrypted: Option<&str>) -> Result<()> {
    initialize(store)?;
    if let Some(encrypted) = encrypted {
        store.connection.execute("INSERT INTO token_vault VALUES (1,?) ON CONFLICT(id) DO UPDATE SET encrypted=excluded.encrypted", [encrypted])?;
    } else {
        store.connection.execute("DELETE FROM token_vault", [])?;
    }
    Ok(())
}

pub async fn load(database: &Database) -> Result<Value> {
    match database.call(read_encrypted).await? {
        None => Ok(json!({})),
        Some(encrypted) => Ok(serde_json::from_str(&protect(&encrypted, true).await?)?),
    }
}

pub async fn encrypt_tokens(
    tokens: &Value,
    protect: &mut impl AsyncFnMut(&str, bool) -> Result<String>,
) -> Result<Option<String>> {
    let values = tokens.as_object().ok_or("Jetons invalides.")?;
    if values.values().all(|value| match value {
        Value::Null => true,
        Value::Bool(value) => !value,
        Value::Number(value) => value.as_f64() == Some(0.0),
        Value::String(value) => value.is_empty(),
        _ => false,
    }) {
        return Ok(None);
    }
    Ok(Some(protect(&serde_json::to_string(tokens)?, false).await?))
}

pub async fn save(database: &Database, tokens: &Value) -> Result<()> {
    let encrypted = encrypt_tokens(tokens, &mut protect).await?;
    database
        .call(move |store| save_encrypted(store, encrypted.as_deref()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn keyring_formats_authenticate_and_never_pass_keys_on_the_command_line() {
        for platform in [KeyringPlatform::Linux, KeyringPlatform::Macos] {
            let mut stored = String::new();
            let mut execute = async |command: &str,
                                     args: &[&str],
                                     input: Option<&str>|
                   -> Result<(i32, String)> {
                assert_eq!(
                    command,
                    if matches!(platform, KeyringPlatform::Linux) {
                        "secret-tool"
                    } else {
                        "security"
                    }
                );
                if let Some(input) = input {
                    stored = if args == ["-i"] {
                        input.split("-w ").nth(1).unwrap().trim().into()
                    } else {
                        input.into()
                    };
                    assert!(!args.join(" ").contains(&stored));
                    return Ok((0, String::new()));
                }
                Ok((
                    if stored.is_empty() {
                        if matches!(platform, KeyringPlatform::Linux) {
                            1
                        } else {
                            44
                        }
                    } else {
                        0
                    },
                    stored.clone(),
                ))
            };
            let plain = "jeton-é-🔑";
            let encrypted = keyring_protect(platform, plain, false, &mut execute)
                .await
                .unwrap();
            assert!(!encrypted.contains(plain));
            assert_eq!(
                keyring_protect(platform, &encrypted, true, &mut execute)
                    .await
                    .unwrap(),
                plain
            );
            assert_ne!(
                keyring_protect(platform, plain, false, &mut execute)
                    .await
                    .unwrap(),
                encrypted
            );
            let mut changed: Value = serde_json::from_str(&encrypted).unwrap();
            changed["tag"] = json!("00".repeat(16));
            assert!(
                keyring_protect(platform, &changed.to_string(), true, &mut execute)
                    .await
                    .is_err()
            );
            changed["platform"] = json!("another");
            assert!(
                keyring_protect(platform, &changed.to_string(), true, &mut execute)
                    .await
                    .is_err()
            );
            let mut locked = async |_: &str,
                                    _: &[&str],
                                    _: Option<&str>|
                   -> Result<(i32, String)> { Err("locked".into()) };
            assert!(
                keyring_protect(platform, plain, false, &mut locked)
                    .await
                    .is_err()
            );
            assert!(
                keyring_protect(platform, &encrypted, true, &mut locked)
                    .await
                    .is_err()
            );
            let mut missing =
                async |_: &str, _: &[&str], input: Option<&str>| -> Result<(i32, String)> {
                    assert!(
                        input.is_none(),
                        "Decryption must never create a replacement key"
                    );
                    Ok((
                        if matches!(platform, KeyringPlatform::Linux) {
                            1
                        } else {
                            44
                        },
                        String::new(),
                    ))
                };
            assert!(
                keyring_protect(platform, &encrypted, true, &mut missing)
                    .await
                    .is_err()
            );
            let mut wrong_key =
                async |_: &str, _: &[&str], _: Option<&str>| -> Result<(i32, String)> {
                    Ok((0, "11".repeat(32)))
                };
            assert!(
                keyring_protect(platform, &encrypted, true, &mut wrong_key)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn encryption_failure_and_transaction_failure_preserve_previous_secrets() {
        let root = tempfile::tempdir().unwrap();
        let database = Database::open(&root.path().join("vault.sqlite")).unwrap();
        assert_eq!(load(&database).await.unwrap(), json!({}));
        database
            .call(|store| save_encrypted(store, Some("synthetic-previous")))
            .await
            .unwrap();
        let mut unavailable =
            async |_: &str, _: bool| -> Result<String> { Err("unavailable".into()) };
        assert!(
            encrypt_tokens(&json!({"jira":"synthetic-replacement"}), &mut unavailable)
                .await
                .is_err()
        );
        assert_eq!(
            database.call(read_encrypted).await.unwrap().as_deref(),
            Some("synthetic-previous")
        );
        assert!(
            database
                .call(|store| store.transaction(|store| {
                    save_encrypted(store, Some("synthetic-next"))?;
                    store.save_settings(&json!({"changed":true}))?;
                    Err::<(), crate::Error>("injected failure".into())
                }))
                .await
                .is_err()
        );
        assert_eq!(
            database.call(read_encrypted).await.unwrap().as_deref(),
            Some("synthetic-previous")
        );
        assert_eq!(
            database.call(|store| store.settings()).await.unwrap(),
            json!({})
        );
        assert!(
            encrypt_tokens(&json!({"jira":"","tempo":null}), &mut unavailable)
                .await
                .unwrap()
                .is_none()
        );
        save(&database, &json!({})).await.unwrap();
        assert_eq!(load(&database).await.unwrap(), json!({}));
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn keyring_output_is_bounded_without_exposing_its_content() {
        let bytes = vec![b'x'; 65_537];
        assert!(bounded_output(bytes.as_slice()).await.is_err());
        assert!(
            execute_keyring("atelier-nonexistent-keyring-fixture", &[], None)
                .await
                .is_err()
        );
    }
}
