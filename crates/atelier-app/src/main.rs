use atelier_app::{Result, http};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("serve-prototype") {
        return Err("Usage : atelier serve-prototype --data-dir DOSSIER_VIDE --assets DOSSIER_UI [--port 4318]".into());
    }
    let (mut directory, mut assets, mut port) = (None, None, 4318u16);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("Valeur d’argument absente.")?;
        match arg.as_str() {
            "--data-dir" if directory.is_none() => directory = Some(PathBuf::from(value)),
            "--assets" if assets.is_none() => assets = Some(PathBuf::from(value)),
            "--port" => port = value.parse()?,
            _ => return Err("Argument inconnu ou répété.".into()),
        }
    }
    let directory = directory.ok_or("Dossier de test obligatoire.")?;
    let assets = assets.ok_or("Dossier des assets obligatoire.")?;
    if !assets.join("index.html").is_file() {
        return Err("Construis l’interface avec cargo xtask build-ui.".into());
    }
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!(
        "Prototype Rust sur http://{} ; données de test uniquement.",
        listener.local_addr()?
    );
    http::serve(listener, directory, assets).await
}
