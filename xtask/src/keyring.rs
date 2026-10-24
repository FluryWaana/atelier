use crate::Result;
use atelier_app::infrastructure::{
    session,
    token_vault::{KeyringPlatform, execute_keyring, keyring_protect},
};

pub async fn test(locked: bool) -> Result<()> {
    if locked
        && (!cfg!(any(target_os = "linux", target_os = "macos"))
            || std::env::var("ATELIER_TEST_DISPOSABLE_KEYRING").as_deref() != Ok("1"))
    {
        return Err("Le contrôle verrouillé exige une session Linux/macOS jetable et ATELIER_TEST_DISPOSABLE_KEYRING=1.".into());
    }
    let platform = if cfg!(target_os = "linux") {
        KeyringPlatform::Linux
    } else if cfg!(target_os = "macos") {
        KeyringPlatform::Macos
    } else {
        return Err("Ce contrôle nécessite un trousseau Linux ou macOS natif.".into());
    };
    let service = format!("atelier-kanban-test-{}", session::new_token()?);
    if locked && matches!(platform, KeyringPlatform::Macos) {
        return test_locked_macos(&service).await;
    }
    let mut execute =
        async |command: &str, args: &[&str], input: Option<&str>| -> Result<(i32, String)> {
            execute_fixture(&service, None, command, args, input).await
        };
    let result = async {
        let plain = "synthetic-native-é-🔑";
        let encrypted = keyring_protect(platform, plain, false, &mut execute).await?;
        if keyring_protect(platform, &encrypted, true, &mut execute).await? != plain {
            return Err::<String, atelier_app::Error>("Secret synthétique natif illisible.".into());
        }
        #[cfg(target_os = "linux")]
        if locked {
            test_locked(&encrypted, plain, &mut execute).await?;
        }
        Ok(encrypted)
    }
    .await;
    let (status, _) = match platform {
        KeyringPlatform::Linux => {
            execute_keyring(
                "secret-tool",
                &["clear", "application", &service, "purpose", "token-key"],
                None,
            )
            .await?
        }
        KeyringPlatform::Macos => {
            execute_keyring(
                "security",
                &["delete-generic-password", "-s", &service, "-a", "token-key"],
                None,
            )
            .await?
        }
    };
    let encrypted = result?;
    if status != 0 {
        return Err("Le nettoyage du secret synthétique a échoué.".into());
    }
    if keyring_protect(platform, &encrypted, true, &mut execute)
        .await
        .is_ok()
    {
        return Err("Le déchiffrement sans clé doit échouer.".into());
    }
    println!(
        "Trousseau natif : création, relecture et suppression du seul secret synthétique réussies."
    );
    Ok(())
}

fn security_argument(value: &str) -> Result<String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err("Argument interactif security invalide.".into());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

async fn execute_fixture(
    service: &str,
    keychain: Option<&str>,
    command: &str,
    args: &[&str],
    input: Option<&str>,
) -> Result<(i32, String)> {
    // Rewrite only the fixed fixture service and, on macOS, restrict access to our keychain.
    let mut args: Vec<String> = args
        .iter()
        .map(|arg| {
            if *arg == "atelier-kanban" {
                service.into()
            } else {
                (*arg).into()
            }
        })
        .collect();
    let mut input =
        input.map(|input| input.replace("-s atelier-kanban ", &format!("-s {service} ")));
    if let Some(keychain) = keychain {
        if let Some(input) = input.as_mut() {
            *input = format!("{} {}\n", input.trim_end(), security_argument(keychain)?);
        } else {
            args.push(keychain.into());
        }
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    execute_keyring(command, &args, input.as_deref()).await
}

async fn test_locked_macos(service: &str) -> Result<()> {
    let directory = tempfile::Builder::new()
        .prefix("atelier keyring é ")
        .tempdir()?;
    let path = directory.path().join("fixture.keychain-db");
    let keychain = path.to_str().ok_or("Chemin synthétique non UTF-8.")?;
    let quoted = security_argument(keychain)?;
    let password = session::new_token()?;
    let mut execute = async |command: &str, args: &[&str], input: Option<&str>| {
        execute_fixture(service, Some(keychain), command, args, input).await
    };
    let result = async {
        let (status, _) = execute_keyring(
            "security",
            &["-i"],
            Some(&format!("create-keychain -p {password} {quoted}\n")),
        )
        .await?;
        if status != 0 || !path.is_file() {
            return Err::<(), atelier_app::Error>(
                "Création du trousseau synthétique impossible.".into(),
            );
        }
        let plain = "synthetic-native-é-🔑";
        let encrypted = keyring_protect(KeyringPlatform::Macos, plain, false, &mut execute).await?;
        if keyring_protect(KeyringPlatform::Macos, &encrypted, true, &mut execute).await? != plain {
            return Err("Le trousseau synthétique doit être lisible avant verrouillage.".into());
        }
        let (status, _) = execute_keyring("security", &["lock-keychain", keychain], None).await?;
        if status != 0 {
            return Err("Le verrouillage du trousseau synthétique a échoué.".into());
        }
        for (value, decrypt) in [(&*encrypted, true), (plain, false)] {
            if keyring_protect(KeyringPlatform::Macos, value, decrypt, &mut execute)
                .await
                .is_ok()
            {
                return Err("Un trousseau verrouillé ne doit pas fournir de secret.".into());
            }
        }
        let (status, _) = execute_keyring(
            "security",
            &["-i"],
            Some(&format!("unlock-keychain -p {password} {quoted}\n")),
        )
        .await?;
        if status != 0
            || keyring_protect(KeyringPlatform::Macos, &encrypted, true, &mut execute).await?
                != plain
        {
            return Err("La clé originale n'est pas relue après déverrouillage.".into());
        }
        Ok(())
    }
    .await;
    // Delete only our owned keychain, including its search-list entry, on every outcome.
    let cleanup = execute_keyring("security", &["delete-keychain", keychain], None).await;
    result?;
    if cleanup?.0 != 0 {
        return Err("Le nettoyage du trousseau synthétique a échoué.".into());
    }
    println!("Trousseau macOS verrouillé : refus, clé préservée et trousseau jetable supprimé.");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn test_locked(
    encrypted: &str,
    plain: &str,
    execute: &mut impl AsyncFnMut(&str, &[&str], Option<&str>) -> Result<(i32, String)>,
) -> Result<()> {
    use zbus::{
        proxy::CacheProperties,
        zvariant::{OwnedObjectPath, OwnedValue, Value},
    };

    let connection = zbus::Connection::session().await?;
    let service = zbus::Proxy::new(
        &connection,
        "org.freedesktop.secrets",
        "/org/freedesktop/secrets",
        "org.freedesktop.Secret.Service",
    )
    .await?;
    let path: OwnedObjectPath = service.call("ReadAlias", &("default",)).await?;
    let collection = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(&connection)
        .destination("org.freedesktop.secrets")?
        .path(path.clone())?
        .interface("org.freedesktop.Secret.Collection")?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    if collection.get_property::<bool>("Locked").await? {
        return Err("Le trousseau synthétique doit être ouvert avant le contrôle.".into());
    }
    let items: Vec<OwnedObjectPath> = collection.get_property("Items").await?;
    let (_, session): (OwnedValue, OwnedObjectPath) = service
        .call("OpenSession", &("plain", Value::from("")))
        .await?;
    let control = zbus::Proxy::new(
        &connection,
        "org.freedesktop.secrets",
        "/org/freedesktop/secrets",
        "org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface",
    )
    .await?;
    let result = async {
        let (locked, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
            service.call("Lock", &(vec![path.clone()],)).await?;
        if !locked.contains(&path)
            || prompt.as_str() != "/"
            || !collection.get_property::<bool>("Locked").await?
        {
            return Err::<(), atelier_app::Error>(
                "Le verrouillage natif n'est pas démontré.".into(),
            );
        }
        for (value, decrypt) in [(encrypted, true), (plain, false)] {
            if keyring_protect(KeyringPlatform::Linux, value, decrypt, execute)
                .await
                .is_ok()
            {
                return Err("Un trousseau verrouillé ne doit pas fournir de secret.".into());
            }
            if !collection.get_property::<bool>("Locked").await? {
                return Err("Le trousseau a été déverrouillé pendant le refus attendu.".into());
            }
        }
        Ok(())
    }
    .await;
    // The test-only GNOME interface unlocks this daemon; --unlock starts another daemon.
    let master = (
        session,
        Vec::<u8>::new(),
        b"synthetic-ci-password".to_vec(),
        "text/plain",
    );
    control
        .call::<_, _, ()>("UnlockWithMasterPassword", &(path, master))
        .await?;
    if collection.get_property::<bool>("Locked").await? {
        return Err("Le déverrouillage du trousseau synthétique a échoué.".into());
    }
    result?;
    if keyring_protect(KeyringPlatform::Linux, encrypted, true, execute).await? != plain
        || collection
            .get_property::<Vec<OwnedObjectPath>>("Items")
            .await?
            != items
    {
        return Err("Le refus a modifié la clé ou les entrées du trousseau.".into());
    }
    println!("Trousseau verrouillé : refus du chiffrement/déchiffrement et clé préservée.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_security_arguments_quote_paths_without_creating_commands() {
        assert_eq!(
            security_argument("/tmp/clé avec espaces").unwrap(),
            "\"/tmp/clé avec espaces\""
        );
        assert_eq!(security_argument("a\\b\"c").unwrap(), "\"a\\\\b\\\"c\"");
        for value in ["a\nb", "a\rb", "a\0b"] {
            assert!(security_argument(value).is_err());
        }
    }
}
