use atelier_app::{
    Result, http,
    infrastructure::{
        installation::{self, InstallationMode, Platform},
        installed, launcher, package, updates,
    },
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

const USAGE: &str = "Atelier Kanban Rust\natelier [launch] [--data-dir DOSSIER] [--assets DOSSIER_UI] [--port 4317] [--no-browser]\natelier stop [--data-dir DOSSIER] [--port 4317]\natelier install --archive PAQUET --signature SIGNATURE --trust CLE_PUBLIQUE --destination NOUVEAU_DOSSIER [--mode portable|user|system]\natelier update-preview --archive PAQUET --signature SIGNATURE --data-dir DONNEES_RUST [--port 4317]\natelier import-data --source DONNEES_RUST --destination DOSSIER_VIDE --confirm\natelier update-status\natelier update-apply|update-cancel|update-recover --operation IDENTIFIANT --confirm\natelier serve --data-dir DOSSIER --assets DOSSIER_UI [--port 4317]\natelier serve-prototype --data-dir DOSSIER_SYNTHETIQUE --assets DOSSIER_UI [--port 4318]";

#[derive(Debug, PartialEq)]
enum Action {
    Launch,
    Stop,
    Serve,
    Prototype,
    Help,
    Version,
}
#[derive(Debug)]
struct Options {
    action: Action,
    data: Option<PathBuf>,
    assets: Option<PathBuf>,
    port: u16,
    browser: bool,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Options> {
    let mut args = args.into_iter().peekable();
    let action = match args.peek().and_then(|value| value.to_str()) {
        Some("launch") => {
            args.next();
            Action::Launch
        }
        Some("stop") => {
            args.next();
            Action::Stop
        }
        Some("serve") => {
            args.next();
            Action::Serve
        }
        Some("serve-prototype") => {
            args.next();
            Action::Prototype
        }
        Some("--help" | "-h") => {
            args.next();
            Action::Help
        }
        Some("--version") => {
            args.next();
            Action::Version
        }
        Some(value) if !value.starts_with('-') => return Err("Commande Atelier inconnue.".into()),
        _ => Action::Launch,
    };
    let mut options = Options {
        port: if action == Action::Prototype {
            4318
        } else {
            4317
        },
        action,
        data: None,
        assets: None,
        browser: true,
    };
    let mut port = false;
    while let Some(arg) = args.next() {
        if matches!(options.action, Action::Help | Action::Version) {
            return Err("Arguments inattendus.".into());
        }
        let arg = arg.to_str().ok_or("Option Atelier invalide.")?;
        if arg == "--no-browser" && options.action == Action::Launch && options.browser {
            options.browser = false;
            continue;
        }
        let value = args
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("Valeur d’argument absente.")?;
        match arg {
            "--data-dir" if options.data.is_none() => options.data = Some(value.into()),
            "--assets" if options.assets.is_none() && options.action != Action::Stop => {
                options.assets = Some(value.into())
            }
            "--port" if !port => {
                options.port = value.to_str().ok_or("Port Atelier invalide.")?.parse()?;
                port = true;
            }
            _ => return Err("Argument inconnu ou répété.".into()),
        }
    }
    if options.port == 0
        || (matches!(options.action, Action::Serve | Action::Prototype)
            && (options.data.is_none() || options.assets.is_none()))
    {
        return Err("Dossiers de service ou port Atelier invalides.".into());
    }
    Ok(options)
}

fn data_directory(root: &Path) -> Result<PathBuf> {
    if installation::mode(root)? == InstallationMode::Portable {
        return Ok(root.join("data"));
    }
    let (platform, home) = if cfg!(windows) {
        (Platform::Windows, "USERPROFILE")
    } else if cfg!(target_os = "macos") {
        (Platform::Macos, "HOME")
    } else {
        (Platform::Linux, "HOME")
    };
    let home = std::env::var_os(home)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Dossier personnel absolu indisponible pour cette installation.")?;
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    installation::data_directory(root, platform, &home, local.as_deref(), xdg.as_deref())
}

fn main() -> Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("tray-helper")) {
        return atelier_app::infrastructure::tray::command(std::env::args_os().skip(2));
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(application())
}

async fn application() -> Result<()> {
    let mut command = std::env::args_os().skip(1);
    let first = command.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("verify-installation")) {
        if command.next().is_some() {
            return Err("La vérification de l’installation ne reçoit aucun argument.".into());
        }
        let executable = std::env::current_exe()?;
        let target = atelier_app::infrastructure::package::native_target()?;
        let installation =
            installed::locate(&executable, target)?.ok_or("Installation Rust absente.")?;
        return installed::verify_all(installation.root(), target);
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("import-data")) {
        return atelier_app::infrastructure::portable_import::command(command);
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("choose-folder")) {
        if command.next().is_some() {
            return Err("Le sélecteur ne reçoit aucun argument ni dossier de données.".into());
        }
        return atelier_app::infrastructure::folders::command().await;
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("connector-azure")) {
        if command.next().is_some() {
            return Err("Le connecteur Azure reçoit uniquement sa requête sur stdin.".into());
        }
        return atelier_app::infrastructure::azure_tickets::command().await;
    }
    if let Some(name) = first
        .as_ref()
        .and_then(|value| value.to_str())
        .filter(|name| {
            [
                "update-preview",
                "update-status",
                "update-apply",
                "update-cancel",
                "update-recover",
            ]
            .contains(name)
        })
    {
        return updates::command(name, command).await;
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("update-worker")) {
        return updates::worker_command(command, false).await;
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("recover-update")) {
        return updates::worker_command(command, true).await;
    }
    if first.as_deref() == Some(std::ffi::OsStr::new("install")) {
        installed::install_command(command)?;
        println!("Installation Rust créée. Aucun dossier de données importé.");
        return Ok(());
    }
    let options = parse(std::env::args_os().skip(1))?;
    if options.action == Action::Help {
        println!("{USAGE}");
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    if options.action == Action::Launch {
        updates::recover_if_needed(&executable).await?;
    }
    let installation = installed::locate(&executable, package::native_target()?)?;
    if options.action == Action::Version {
        println!(
            "Atelier Kanban Rust {}",
            installation
                .as_ref()
                .map(|installation| installation.current().active.version.as_str())
                .unwrap_or(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }
    let root = if let Some(installation) = &installation {
        installation.root()
    } else {
        executable
            .parent()
            .ok_or("Dossier applicatif introuvable.")?
    };
    let effective = if let Some(installation) = &installation {
        installation.active_executable()?
    } else {
        executable.clone()
    };
    if matches!(options.action, Action::Serve | Action::Prototype)
        && std::fs::canonicalize(&effective)? != std::fs::canonicalize(&executable)?
    {
        return Err("Le service doit être lancé depuis la version active vérifiée.".into());
    }
    let data = match options.data {
        Some(path) => std::path::absolute(path)?,
        None => data_directory(root)?,
    };
    if data.to_str().is_none() {
        return Err("Le chemin des données doit être un texte Unicode valide.".into());
    }
    if options.action == Action::Stop {
        launcher::stop(&data, options.port).await?;
        println!("Atelier est arrêté.");
        return Ok(());
    }
    let default_assets = if let Some(installation) = &installation {
        installation
            .slot_path(&installation.current().active)?
            .join("ui")
    } else {
        root.join("ui")
    };
    let assets = std::path::absolute(options.assets.unwrap_or(default_assets))?;
    if !assets.join("index.html").is_file() {
        return Err("Interface Rust absente. Utilise un paquet complet ou construis les assets avec cargo xtask build-ui.".into());
    }
    if options.action == Action::Launch {
        launcher::start(&effective, &data, &assets, options.port, options.browser).await?;
        println!(
            "Atelier Kanban : http://127.0.0.1:{}\nUtilise atelier stop avec le même port et dossier pour arrêter le serveur.",
            options.port
        );
        return Ok(());
    }
    let listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, options.port)).await?;
    println!("Atelier Kanban Rust sur http://{}", listener.local_addr()?);
    if options.action == Action::Prototype {
        http::serve(listener, data, assets).await
    } else if installation.is_some() {
        http::serve_installed(listener, data, assets, root.to_owned()).await
    } else {
        http::serve_native(listener, data, assets).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(args: &[&str]) -> Result<Options> {
        parse(args.iter().map(|arg| OsString::from(*arg)))
    }
    #[test]
    fn launch_defaults_and_explicit_service_paths_do_not_trigger_io() {
        let empty = options(&[]).unwrap();
        assert_eq!(empty.action, Action::Launch);
        assert_eq!(empty.port, 4317);
        assert!(empty.browser);
        let launch = options(&[
            "launch",
            "--port",
            "1234",
            "--data-dir",
            "synthetic data",
            "--assets",
            "synthetic ui",
            "--no-browser",
        ])
        .unwrap();
        assert_eq!(launch.port, 1234);
        assert!(!launch.browser);
        assert_eq!(launch.data, Some(PathBuf::from("synthetic data")));
        assert_eq!(
            options(&[
                "serve-prototype",
                "--data-dir",
                "synthetic",
                "--assets",
                "ui"
            ])
            .unwrap()
            .port,
            4318
        );
        assert_eq!(options(&["stop"]).unwrap().action, Action::Stop);
        for args in [
            vec!["serve"],
            vec!["serve", "--data-dir", "synthetic"],
            vec!["launch", "--port", "0"],
            vec!["launch", "--port", "1", "--port", "2"],
            vec!["stop", "--no-browser"],
            vec!["stop", "--assets", "ui"],
            vec!["--help", "--port", "1"],
            vec!["launch", "--no-browser", "--no-browser"],
            vec!["--unknown", "value"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}
