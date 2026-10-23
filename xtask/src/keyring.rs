use crate::Result;
use atelier_app::infrastructure::{
    session,
    token_vault::{KeyringPlatform, execute_keyring, keyring_protect},
};

pub async fn test() -> Result<()> {
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
