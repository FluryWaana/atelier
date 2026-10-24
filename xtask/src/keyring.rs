use crate::Result;
use atelier_app::infrastructure::{
    session,
    token_vault::{KeyringPlatform, execute_keyring, keyring_protect},
};

pub async fn test(locked: bool) -> Result<()> {
    if locked
        && (!cfg!(target_os = "linux")
            || std::env::var("ATELIER_TEST_DISPOSABLE_KEYRING").as_deref() != Ok("1"))
    {
        return Err("Le contrôle verrouillé exige une session Linux jetable et ATELIER_TEST_DISPOSABLE_KEYRING=1.".into());
    }
    let platform = if cfg!(target_os = "linux") {
        KeyringPlatform::Linux
    } else if cfg!(target_os = "macos") {
        KeyringPlatform::Macos
    } else {
        return Err("Ce contrôle nécessite un trousseau Linux ou macOS natif.".into());
    };
    let service = format!("atelier-kanban-test-{}", session::new_token()?);
    // Rewrite only the fixed service identifier; never access the user's production key.
    let mut execute =
        async |command: &str, args: &[&str], input: Option<&str>| -> Result<(i32, String)> {
            let args: Vec<String> = args
                .iter()
                .map(|arg| {
                    if *arg == "atelier-kanban" {
                        service.clone()
                    } else {
                        (*arg).into()
                    }
                })
                .collect();
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let input =
                input.map(|input| input.replace("-s atelier-kanban ", &format!("-s {service} ")));
            execute_keyring(command, &args, input.as_deref()).await
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

#[cfg(target_os = "linux")]
async fn test_locked(
    encrypted: &str,
    plain: &str,
    execute: &mut impl AsyncFnMut(&str, &[&str], Option<&str>) -> Result<(i32, String)>,
) -> Result<()> {
    use zbus::{proxy::CacheProperties, zvariant::OwnedObjectPath};

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
    // Always unlock our disposable CI collection before the caller removes its test item.
    let (status, _) = execute_keyring(
        "gnome-keyring-daemon",
        &["--unlock", "--components=secrets"],
        Some("synthetic-ci-password"),
    )
    .await?;
    if status != 0 || collection.get_property::<bool>("Locked").await? {
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
