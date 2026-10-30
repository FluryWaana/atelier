use super::{database::Database, session, store::Store};
use crate::Result;
use atelier_core::document::Document;
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
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
    keyring_protect_service(platform, "atelier-kanban", value, decrypt, execute).await
}

pub const PROTOTYPE_SERVICE_PREFIX: &str = "atelier-kanban-prototype-";

fn validate_prototype_service(service: &str) -> Result<()> {
    let id = service
        .strip_prefix(PROTOTYPE_SERVICE_PREFIX)
        .ok_or("Service du prototype invalide.")?;
    unhex::<32>(id)?;
    Ok(())
}

pub async fn keyring_protect_scoped(
    platform: KeyringPlatform,
    service: &str,
    value: &str,
    decrypt: bool,
    execute: &mut impl AsyncFnMut(&str, &[&str], Option<&str>) -> Result<(i32, String)>,
) -> Result<String> {
    validate_prototype_service(service)?;
    keyring_protect_service(platform, service, value, decrypt, execute).await
}

async fn keyring_protect_service(
    platform: KeyringPlatform,
    service: &str,
    value: &str,
    decrypt: bool,
    execute: &mut impl AsyncFnMut(&str, &[&str], Option<&str>) -> Result<(i32, String)>,
) -> Result<String> {
    // Validate the envelope before querying a keyring or attempting to create a key.
    let envelope = if decrypt {
        let envelope: Envelope = serde_json::from_str(value)?;
        if envelope.format != "keyring-aes-256-gcm-v1" || envelope.platform != platform.name() {
            return Err(
                "message.this_vault_belongs_to_another_system_enter_the_tokens_again".into(),
            );
        }
        Some(envelope)
    } else {
        None
    };
    let (command, lookup, missing) = match platform {
        KeyringPlatform::Linux => (
            "secret-tool",
            vec!["lookup", "application", service, "purpose", "token-key"],
            1,
        ),
        KeyringPlatform::Macos => (
            "security",
            vec![
                "find-generic-password",
                "-s",
                service,
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
                        service,
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
                        "add-generic-password -s {service} -a token-key -w {generated}\n"
                    )),
                )
                .await?
            }
        };
        if saved != 0 {
            return Err("message.cannot_save_the_key_to_the_system_keyring".into());
        }
        (status, key) = execute(command, &lookup, None).await?;
    }
    if status != 0 {
        return Err("message.unlock_the_system_keyring_to_access_tokens".into());
    }
    let key = unhex::<32>(key.trim())?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key).map_err(|_| "message.invalid_system_keyring_key")?,
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
        // Every encryption uses a fresh 96-bit CSPRNG nonce to prevent nonce reuse.
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

#[derive(Clone)]
pub struct PrototypeVault {
    #[cfg(unix)]
    service: String,
    #[cfg(unix)]
    runtime: tokio::runtime::Handle,
}

impl PrototypeVault {
    pub fn new(service: String) -> Result<Self> {
        validate_prototype_service(&service)?;
        Ok(Self {
            #[cfg(unix)]
            service,
            #[cfg(unix)]
            runtime: tokio::runtime::Handle::try_current()?,
        })
    }

    // Called only on the dedicated SQLite thread, before opening its write transaction.
    pub fn protect(&self, value: &str, decrypt: bool) -> Result<String> {
        #[cfg(windows)]
        {
            session::protect(value, decrypt)
        }
        #[cfg(unix)]
        {
            let platform = if cfg!(target_os = "macos") {
                KeyringPlatform::Macos
            } else {
                KeyringPlatform::Linux
            };
            self.runtime.block_on(keyring_protect_scoped(
                platform,
                &self.service,
                value,
                decrypt,
                &mut execute_keyring,
            ))
        }
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
    #[cfg(target_os = "macos")]
    let (scoped_args, scoped_input) = {
        let configured = std::env::var("ATELIER_TEST_KEYCHAIN").ok();
        let isolated = std::env::var("CI").as_deref() == Ok("true")
            && std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() == Ok("1")
            && std::env::var("ATELIER_TEST_DISPOSABLE_KEYRING").as_deref() == Ok("1");
        if let Some(path) = &configured {
            if !isolated {
                return Err("Le trousseau de test exige la campagne CI jetable.".into());
            }
            let path = std::path::Path::new(path);
            let parent = path.parent().ok_or("Trousseau synthétique invalide.")?;
            if path
                .file_name()
                .is_none_or(|name| name != "fixture.keychain-db")
                || !parent.file_name().is_some_and(|name| {
                    name.to_str()
                        .is_some_and(|name| name.starts_with(".atelier-final-keychain-"))
                })
            {
                return Err("Trousseau synthétique non dédié.".into());
            }
            let directory = super::repository_access::absolute(parent)?;
            let marker = super::checked_file::CheckedFile::open_in(
                &directory,
                std::path::Path::new(".keychain-fixture"),
            )?
            .single_link()?
            .private()?
            .read(64)?;
            if marker != b"atelier-final-keychain/1\n" {
                return Err("Marqueur de trousseau synthétique invalide.".into());
            }
        }
        scope_macos_keyring(
            command,
            args,
            input,
            configured.as_deref(),
            std::env::var("CI").as_deref() == Ok("true"),
        )?
    };
    #[cfg(target_os = "macos")]
    let (args, input) = (
        scoped_args.iter().map(String::as_str).collect::<Vec<_>>(),
        scoped_input.as_deref(),
    );
    let mut command = tokio::process::Command::new(command);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|_| "message.system_vault_inaccessible_on_linux_install_secret_tool_libsecret")?;
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

#[cfg(any(target_os = "macos", test))]
fn scope_macos_keyring(
    command: &str,
    args: &[&str],
    input: Option<&str>,
    keychain: Option<&str>,
    ci: bool,
) -> Result<(Vec<String>, Option<String>)> {
    let mut args: Vec<String> = args.iter().map(|arg| (*arg).into()).collect();
    let mut input = input.map(str::to_owned);
    if command != "security" {
        return Ok((args, input));
    }
    let read = matches!(
        args.first().map(String::as_str),
        Some("find-generic-password")
    ) && args.len() == 6;
    let delete = matches!(
        args.first().map(String::as_str),
        Some("delete-generic-password")
    ) && args.len() == 5;
    let write = args.len() == 1
        && args[0] == "-i"
        && input.as_ref().is_some_and(|input| {
            let words: Vec<_> = input.split_ascii_whitespace().collect();
            words.len() == 7
                && words[0] == "add-generic-password"
                && words[1] == "-s"
                && words[3] == "-a"
                && words[4] == "token-key"
                && words[5] == "-w"
        });
    if read || delete || write {
        if let Some(keychain) = keychain {
            if !std::path::Path::new(keychain).is_absolute()
                || keychain.chars().any(char::is_control)
            {
                return Err("Chemin absolu de trousseau synthétique requis.".into());
            }
            if write {
                let quoted = format!(
                    "\"{}\"",
                    keychain.replace('\\', "\\\\").replace('"', "\\\"")
                );
                input = input.map(|input| format!("{} {quoted}\n", input.trim_end()));
            } else {
                args.push(keychain.into());
            }
        } else if ci {
            return Err(
                "Une commande de coffre CI sans trousseau temporaire explicite est refusée.".into(),
            );
        }
    }
    Ok((args, input))
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

pub async fn load(database: &Database) -> Result<Document> {
    match database.call(read_encrypted).await? {
        None => Ok(Document::default()),
        Some(encrypted) => Ok(serde_json::from_str(&protect(&encrypted, true).await?)?),
    }
}

pub async fn encrypt_tokens(
    tokens: &Document,
    protect: &mut impl AsyncFnMut(&str, bool) -> Result<String>,
) -> Result<Option<String>> {
    match tokens_plaintext(tokens)? {
        Some(plain) => Ok(Some(protect(&plain, false).await?)),
        None => Ok(None),
    }
}

pub fn tokens_plaintext(tokens: &Document) -> Result<Option<String>> {
    let mut populated = false;
    for key in tokens.keys() {
        populated |= tokens.has_text(key)?;
    }
    populated
        .then(|| serde_json::to_string(tokens).map_err(Into::into))
        .transpose()
}

pub async fn save(database: &Database, tokens: &Document) -> Result<()> {
    let encrypted = encrypt_tokens(tokens, &mut protect).await?;
    database
        .call(move |store| save_encrypted(store, encrypted.as_deref()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{Value, json};

    #[test]
    fn ci_keyring_reads_writes_and_cleanup_never_fall_back_to_the_default_keychain() {
        let path = std::env::temp_dir().join("fixture with spaces.keychain-db");
        let path = path.to_str().unwrap();
        let lookup = [
            "find-generic-password",
            "-s",
            "synthetic-service",
            "-a",
            "token-key",
            "-w",
        ];
        let delete = [
            "delete-generic-password",
            "-s",
            "synthetic-service",
            "-a",
            "token-key",
        ];
        for args in [&lookup[..], &delete[..]] {
            assert!(scope_macos_keyring("security", args, None, None, true).is_err());
            let (scoped, _) =
                scope_macos_keyring("security", args, None, Some(path), true).unwrap();
            assert_eq!(scoped.last().unwrap(), path);
            let explicit: Vec<_> = scoped.iter().map(String::as_str).collect();
            let (unchanged, _) =
                scope_macos_keyring("security", &explicit, None, Some(path), true).unwrap();
            assert_eq!(scoped, unchanged);
        }
        let input = "add-generic-password -s synthetic-service -a token-key -w synthetic-key\n";
        assert!(scope_macos_keyring("security", &["-i"], Some(input), None, true).is_err());
        let (_, scoped) =
            scope_macos_keyring("security", &["-i"], Some(input), Some(path), true).unwrap();
        let (_, repeated) =
            scope_macos_keyring("security", &["-i"], scoped.as_deref(), Some(path), true).unwrap();
        assert_eq!(repeated, scoped);
        assert!(scope_macos_keyring("security", &lookup, None, Some("relative"), true).is_err());
        assert!(scope_macos_keyring("security", &lookup, None, None, false).is_ok());
    }

    #[tokio::test]
    async fn prototype_keyrings_use_only_their_validated_service_and_keep_ciphertexts_isolated() {
        for platform in [KeyringPlatform::Linux, KeyringPlatform::Macos] {
            let service = format!("{PROTOTYPE_SERVICE_PREFIX}{}", "ab".repeat(32));
            let mut key: Option<String> = None;
            let mut execute =
                async |_: &str, args: &[&str], input: Option<&str>| -> Result<(i32, String)> {
                    assert!(!args.contains(&"atelier-kanban"));
                    if let Some(input) = input {
                        let generated = match platform {
                            KeyringPlatform::Linux => {
                                assert!(args.contains(&service.as_str()));
                                input
                            }
                            KeyringPlatform::Macos => {
                                assert_eq!(args, ["-i"]);
                                assert!(input.starts_with(&format!(
                                    "add-generic-password -s {service} -a token-key -w "
                                )));
                                input.split_whitespace().last().unwrap()
                            }
                        };
                        unhex::<32>(generated)?;
                        assert!(!args.contains(&generated));
                        key = Some(generated.into());
                        Ok((0, String::new()))
                    } else {
                        assert!(args.contains(&service.as_str()));
                        Ok(match &key {
                            Some(key) => (0, key.clone()),
                            None => (
                                if matches!(platform, KeyringPlatform::Linux) {
                                    1
                                } else {
                                    44
                                },
                                String::new(),
                            ),
                        })
                    }
                };
            let encrypted =
                keyring_protect_scoped(platform, &service, "synthetic", false, &mut execute)
                    .await
                    .unwrap();
            assert_eq!(
                keyring_protect_scoped(platform, &service, &encrypted, true, &mut execute)
                    .await
                    .unwrap(),
                "synthetic"
            );
            let mut foreign =
                async |_: &str, _: &[&str], _: Option<&str>| -> Result<(i32, String)> {
                    Ok((0, "11".repeat(32)))
                };
            assert!(
                keyring_protect_scoped(
                    platform,
                    &format!("{PROTOTYPE_SERVICE_PREFIX}{}", "cd".repeat(32)),
                    &encrypted,
                    true,
                    &mut foreign
                )
                .await
                .is_err()
            );
            for invalid in [
                "atelier-kanban",
                "atelier-kanban-prototype-abc",
                "atelier-kanban-prototype-a\ncommand",
            ] {
                let mut forbidden =
                    async |_: &str, _: &[&str], _: Option<&str>| -> Result<(i32, String)> {
                        panic!("invalid service must not reach a keyring")
                    };
                assert!(
                    keyring_protect_scoped(platform, invalid, "synthetic", false, &mut forbidden)
                        .await
                        .is_err()
                );
            }
        }
    }

    #[tokio::test]
    async fn token_documents_preserve_unicode_and_reject_invalid_token_types() {
        let tokens: Document = serde_json::from_str(
            r#"{"jira":"synthetic-\u00e9","connector:\u00e8:token":"\u00e9"}"#,
        )
        .unwrap();
        let mut capture = async |plain: &str, decrypt: bool| -> Result<String> {
            assert!(!decrypt);
            Ok(plain.to_owned())
        };
        let plain = encrypt_tokens(&tokens, &mut capture)
            .await
            .unwrap()
            .unwrap();
        let decoded: Document = serde_json::from_str(&plain).unwrap();
        let mut key = String::from("connector:");
        key.push('è');
        key.push_str(":token");
        assert_eq!(
            serde_json::from_str::<String>(decoded.raw(&key).unwrap().get()).unwrap(),
            "é".to_owned()
        );
        assert!(decoded.has_text(&key).unwrap());
        for input in [
            r#"{"jira":{}}"#,
            r#"{"jira":[]}"#,
            r#"{"jira":true}"#,
            r#"{"jira":"valid","other":{}}"#,
        ] {
            let tokens = serde_json::from_str(input).unwrap();
            let mut refuse = async |_: &str, _: bool| -> Result<String> {
                panic!("Invalid credentials must be rejected before encryption")
            };
            assert!(encrypt_tokens(&tokens, &mut refuse).await.is_err());
        }
        assert!(tokens_plaintext(&Document::default()).unwrap().is_none());
        #[cfg(windows)]
        {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("synthetic-vault.sqlite");
            let database = Database::open(&path).unwrap();
            save(&database, &tokens).await.unwrap();
            let encrypted = database.call(read_encrypted).await.unwrap().unwrap();
            assert!(!encrypted.contains("synthetic-"));
            database.close().await.unwrap();
            let database = Database::open(&path).unwrap();
            assert_eq!(
                serde_json::to_string(&load(&database).await.unwrap()).unwrap(),
                plain
            );
            database.close().await.unwrap();
        }
    }

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
        assert_eq!(load(&database).await.unwrap().keys().count(), 0);
        database
            .call(|store| save_encrypted(store, Some("synthetic-previous")))
            .await
            .unwrap();
        let mut unavailable =
            async |_: &str, _: bool| -> Result<String> { Err("unavailable".into()) };
        assert!(
            encrypt_tokens(
                &serde_json::from_str(r#"{"jira":"synthetic-replacement"}"#).unwrap(),
                &mut unavailable
            )
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
            database
                .call(|store| store.settings::<serde_json::Value>())
                .await
                .unwrap(),
            json!({})
        );
        assert!(
            encrypt_tokens(
                &serde_json::from_str(
                    r#"{"jira":"","tempo":null,"\u00e9":"","disabled":false,"zero":0}"#
                )
                .unwrap(),
                &mut unavailable
            )
            .await
            .unwrap()
            .is_none()
        );
        save(&database, &Document::default()).await.unwrap();
        assert_eq!(load(&database).await.unwrap().keys().count(), 0);
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
