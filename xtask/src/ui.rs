use crate::{Result, command, root, run};
use atelier_app::infrastructure::{local_http::LocalHttp, session};
use fantoccini::{Client, ClientBuilder, Locator};
use serde_json::json;
use std::{
    path::Path,
    process::{Child, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}

async fn start_server(data: &Path, port: u16, assets: &Path) -> Result<(Process, String)> {
    let executable = root()
        .join("target/debug")
        .join(format!("atelier{}", std::env::consts::EXE_SUFFIX));
    let mut process = Process(
        command(executable)
            .arg("serve-prototype")
            .arg("--data-dir")
            .arg(data)
            .arg("--assets")
            .arg(assets)
            .arg("--port")
            .arg(port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let http = LocalHttp::new()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if process.0.try_wait()?.is_some() {
            return Err("Le serveur de test s’est arrêté au démarrage.".into());
        }
        if http
            .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
                session::read_optional(data)
            })
            .await?
            .is_some()
        {
            return Ok((process, session::read(data)?));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("Délai de démarrage du serveur dépassé.".into())
}

async fn stop_server(process: &mut Process, port: u16, token: &str) -> Result<()> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse()?);
    headers.insert("x-local-request", "1".parse()?);
    headers.insert("content-type", "application/json".parse()?);
    let response = LocalHttp::new()?
        .request(
            &format!("http://127.0.0.1:{port}/api/prototype/stop"),
            reqwest::Method::POST,
            headers,
            Some("{}"),
            Duration::from_secs(5),
        )
        .await?;
    if !response.status.is_success() {
        return Err("Arrêt du serveur de test refusé.".into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(status) = process.0.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err("Le serveur a échoué pendant l’arrêt.".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("Le serveur ne s’est pas arrêté proprement.".into())
}

async fn wait_title(client: &Client, title: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if client.execute("return Array.from(document.querySelectorAll('.card-title')).some(node => node.textContent === arguments[0]);", vec![json!(title)]).await? == json!(true)
        {
            return Ok(());
        }
        if let Ok(error) = client.find(Locator::Css("#error")).await
            && !error.text().await?.is_empty()
        {
            return Err("L’interface a signalé une erreur.".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("La tâche n’apparaît pas dans le navigateur.".into())
}

pub async fn test(legacy: bool) -> Result<()> {
    run(command("cargo").args(["build", "--locked", "-p", "atelier-app"]))?;
    let browser = std::env::var("ATELIER_TEST_BROWSER")
        .unwrap_or(if cfg!(windows) { "edge" } else { "chrome" }.into());
    let default_driver = match browser.as_str() {
        "edge" => "msedgedriver",
        "chrome" => "chromedriver",
        "firefox" => "geckodriver",
        "safari" => "safaridriver",
        _ => return Err("Navigateur de test inconnu.".into()),
    };
    let driver_program = std::env::var_os("ATELIER_WEBDRIVER").unwrap_or(default_driver.into());
    let driver_port = free_port()?;
    let mut driver_command = command(driver_program);
    if matches!(browser.as_str(), "edge" | "chrome") {
        driver_command.arg(format!("--port={driver_port}"));
    } else {
        driver_command.args(["--port", &driver_port.to_string()]);
    }
    let mut driver = Process(
        driver_command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let endpoint = format!("http://127.0.0.1:{driver_port}");
    let deadline = Instant::now() + Duration::from_secs(15);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    loop {
        if driver.0.try_wait()?.is_some() {
            return Err("WebDriver s’est arrêté au démarrage.".into());
        }
        if http.get(format!("{endpoint}/status")).send().await.is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("WebDriver ne répond pas.".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut capabilities = serde_json::Map::new();
    match browser.as_str() {
        "edge" | "chrome" => {
            capabilities.insert(
                "browserName".into(),
                json!(if browser == "edge" {
                    "MicrosoftEdge"
                } else {
                    "chrome"
                }),
            );
            capabilities.insert(if browser == "edge" { "ms:edgeOptions" } else { "goog:chromeOptions" }.into(), json!({"args":["--headless=new", "--disable-gpu", "--no-first-run", "--window-size=1280,900"]}));
        }
        "firefox" => {
            capabilities.insert("browserName".into(), json!("firefox"));
            capabilities.insert("moz:firefoxOptions".into(), json!({"args":["-headless"]}));
        }
        "safari" => {
            capabilities.insert("browserName".into(), json!("safari"));
        }
        _ => unreachable!(),
    }
    let client = ClientBuilder::rustls()?
        .capabilities(capabilities)
        .connect(&endpoint)
        .await?;
    let result = if legacy {
        legacy_scenario(&client, &http).await
    } else {
        scenario(&client, &http).await
    };
    if result.is_err()
        && let Ok(screenshot) = client.screenshot().await
    {
        let _ = std::fs::write(
            root().join(if legacy {
                ".artifacts/rust-legacy-ui-failed.png"
            } else {
                ".artifacts/rust-ui-failed.png"
            }),
            screenshot,
        );
    }
    let closed = client.close().await;
    result?;
    closed?;
    if legacy {
        println!(
            "Interface historique sur Rust, {browser} : création/édition, sauvegarde/restauration, refus explicite des fonctions non portées et redémarrage réussis."
        );
    } else {
        println!(
            "Parcours {browser} réussi : WASM sous CSP, création, redémarrage du processus, relecture et rotation de session."
        );
    }
    Ok(())
}

async fn scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    let data = tempfile::tempdir()?;
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let assets = root().join(".artifacts/rust-ui");
    let (mut server, token) = start_server(data.path(), port, &assets).await?;
    let repo = data.path().join("repositories/board-fixture");
    std::fs::create_dir_all(&repo)?;
    let project: serde_json::Value = http
        .post(format!("{address}/api/projects"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Projet synthétique", "repo":repo}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let queued: serde_json::Value = http.post(format!("{address}/api/cards"))
        .bearer_auth(&token).header("x-local-request", "1")
        .json(&json!({"title":"Attente projet", "description":"Une description à retrouver", "status":"review", "repo":repo}))
        .send().await?.error_for_status()?.json().await?;
    let archived: serde_json::Value = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Archive synthétique", "repo":repo}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    http.post(format!(
        "{address}/api/cards/{}/archive",
        archived["id"].as_str().ok_or("Carte sans id")?
    ))
    .bearer_auth(&token)
    .header("x-local-request", "1")
    .json(&json!({"archived":true}))
    .send()
    .await?
    .error_for_status()?;
    client
        .goto(&format!("{address}/#session={token}"))
        .await
        .map_err(|_| "Navigation de test refusée.")?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("#task-title"))
        .await?;
    if client.current_url().await?.fragment().is_some() {
        return Err("Le jeton n’a pas été retiré de l’adresse.".into());
    }
    let title = "Tâche synthétique é 🔧 <script>";
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys(title)
        .await?;
    client
        .find(Locator::Css("#create-task"))
        .await?
        .click()
        .await?;
    wait_title(client, title).await?;
    wait_dom(client, "return document.querySelectorAll('.kanban-column').length === 5 && document.querySelectorAll('article.card').length === 2 && document.querySelector('[data-status=review] article.card')?.dataset.id === arguments[0] && !document.querySelector('.card-title script');", vec![queued["id"].clone()]).await?;
    client
        .find(Locator::Css("#board-search"))
        .await?
        .send_keys("DESCRIPTION À RETROUVER")
        .await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 1 && document.querySelector('article.card').dataset.id === arguments[0];", vec![queued["id"].clone()]).await?;
    client
        .find(Locator::Css("#board-search"))
        .await?
        .clear()
        .await?;
    // WebDriver clear does not consistently emit the input event across browsers.
    client.execute("document.querySelector('#board-search').dispatchEvent(new Event('input', {bubbles:true})); const select=document.querySelector('#board-project'); select.value=arguments[0]; select.dispatchEvent(new Event('change', {bubbles:true}));", vec![project["repo"].clone()]).await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 1 && document.querySelector('article.card').dataset.id === arguments[0];", vec![queued["id"].clone()]).await?;
    client
        .find(Locator::Css("#show-archives"))
        .await?
        .click()
        .await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 1 && document.querySelector('article.card').dataset.id === arguments[0];", vec![archived["id"].clone()]).await?;
    client
        .find(Locator::Css("#show-archives"))
        .await?
        .click()
        .await?;
    client.execute("const select=document.querySelector('#board-project'); select.value=''; select.dispatchEvent(new Event('change', {bubbles:true}));", vec![]).await?;
    wait_title(client, title).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-created.png"),
        client.screenshot().await?,
    )?;
    let response = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .header("content-type", "application/json")
        .body(r#"{"title":"\ud83d"}"#)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err("Création UTF-16 refusée.".into());
    }
    let atypical: atelier_core::document::Document = response.json().await?;
    let atypical_id = atypical
        .get::<String>("id")?
        .ok_or("Identifiant UTF-16 absent.")?;
    stop_server(&mut server, port, &token).await?;
    let (mut restarted, next_token) = start_server(data.path(), port, &assets).await?;
    if session::same_secret(&token, &next_token) {
        return Err("Session non renouvelée.".into());
    }
    if http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .status()
        != 401
    {
        return Err("Ancienne session encore acceptée.".into());
    }
    // A fragment-only navigation can retain the old DOM; require a new document after restart.
    client.goto("about:blank").await?;
    client
        .goto(&format!("{address}/#session={next_token}"))
        .await
        .map_err(|_| "Navigation après redémarrage refusée.")?;
    wait_title(client, title).await?;
    let units = client.execute("const card = Array.from(document.querySelectorAll('article.card')).find(node => node.dataset.id === arguments[0]); const title = card?.querySelector('.card-title').textContent; return title ? Array.from({length:title.length}, (_, i) => title.charCodeAt(i)) : [];", vec![json!(atypical_id)]).await?;
    if units != json!([0xd83d]) {
        return Err(format!("Le DOM a altéré le titre UTF-16 après redémarrage : {units}").into());
    }
    if !client
        .find(Locator::Css("#error"))
        .await?
        .text()
        .await?
        .is_empty()
    {
        return Err("Erreur après redémarrage.".into());
    }
    std::fs::write(
        root().join(".artifacts/rust-ui-reopened.png"),
        client.screenshot().await?,
    )?;
    stop_server(&mut restarted, port, &next_token).await?;
    Ok(())
}

async fn wait_dom(client: &Client, script: &str, arguments: Vec<serde_json::Value>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if client.execute(script, arguments.clone()).await? == json!(true) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("État DOM attendu non atteint : {script}").into())
}

async fn legacy_scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    let data = tempfile::tempdir()?;
    let assets = root().join(".artifacts/rust-legacy-ui");
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let (mut server, token) = start_server(data.path(), port, &assets).await?;
    for name in atelier_app::http::LEGACY_ASSETS {
        let path = if *name == "index.html" { "" } else { name };
        let response = http.get(format!("{address}/{path}")).send().await?;
        if response.status() != 200 || !response.headers().contains_key("content-security-policy") {
            return Err(format!("Asset historique indisponible sous CSP : {name}").into());
        }
    }
    for path in ["/api/agents", "/api/connectors", "/api/updates/check"] {
        let response = http
            .get(format!("{address}{path}"))
            .bearer_auth(&token)
            .send()
            .await?;
        if response.status() != 503
            || !response.json::<serde_json::Value>().await?["error"]
                .as_str()
                .is_some_and(|message| message.contains("prototype Rust"))
        {
            return Err("Une fonction non portée doit annoncer son indisponibilité.".into());
        }
    }
    client
        .goto(&format!("{address}/#session={token}&view=board"))
        .await?;
    wait_dom(client, "return document.querySelector('#live-connection')?.classList.contains('connected') === true;", vec![]).await?;
    if client
        .current_url()
        .await?
        .fragment()
        .is_some_and(|fragment| fragment.contains("session="))
    {
        return Err("Session historique restée dans l’URL.".into());
    }
    client
        .find(Locator::Css("#create-button"))
        .await?
        .click()
        .await?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("#new-dialog[open]"))
        .await?;
    let title = "Tâche historique é 🔧 <img src=x>";
    client
        .find(Locator::Css("#new-form [name=title]"))
        .await?
        .send_keys(title)
        .await?;
    client
        .find(Locator::Css("#new-form button.primary"))
        .await?
        .click()
        .await?;
    let title_matches = "return document.querySelector('#task-title')?.textContent === arguments[0] && !document.querySelector('#task-title img');";
    wait_dom(client, title_matches, vec![json!(title)]).await?;
    let task_fragment = client
        .current_url()
        .await?
        .fragment()
        .ok_or("Adresse de tâche absente.")?
        .to_owned();
    std::fs::write(
        root().join(".artifacts/rust-legacy-ui-created.png"),
        client.screenshot().await?,
    )?;
    client
        .find(Locator::Css("#backups-button"))
        .await?
        .click()
        .await?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("#backups-dialog[open]"))
        .await?;
    client
        .find(Locator::Css("#create-backup"))
        .await?
        .click()
        .await?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("[data-restore-backup]"))
        .await?;
    let name = client
        .execute(
            "return document.querySelector('[data-restore-backup]').dataset.restoreBackup;",
            vec![],
        )
        .await?;
    let name = name.as_str().ok_or("Nom de sauvegarde absent du DOM.")?;
    client
        .find(Locator::Css("#backups-dialog [data-close]"))
        .await?
        .click()
        .await?;
    let field = client.find(Locator::Css("#card-form [name=title]")).await?;
    field.clear().await?;
    field.send_keys("Modification synthétique").await?;
    client
        .find(Locator::Css("#card-form button[type=submit]"))
        .await?
        .click()
        .await?;
    wait_dom(
        client,
        title_matches,
        vec![json!("Modification synthétique")],
    )
    .await?;
    client
        .find(Locator::Css("#backups-button"))
        .await?
        .click()
        .await?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("#backups-dialog[open]"))
        .await?;
    client
        .find(Locator::Css(&format!("[data-restore-backup='{name}']")))
        .await?
        .click()
        .await?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("#confirm-dialog[open]"))
        .await?;
    client
        .find(Locator::Css("#confirm-action"))
        .await?
        .click()
        .await?;
    wait_dom(
        client,
        "return document.querySelectorAll('[data-restore-backup]').length === 2;",
        vec![],
    )
    .await?;
    client.refresh().await?;
    wait_dom(client, title_matches, vec![json!(title)]).await?;
    std::fs::write(
        root().join(".artifacts/rust-legacy-ui-restored.png"),
        client.screenshot().await?,
    )?;
    stop_server(&mut server, port, &token).await?;
    let (mut restarted, next_token) = start_server(data.path(), port, &assets).await?;
    if session::same_secret(&token, &next_token)
        || http
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
            .send()
            .await?
            .status()
            != 401
    {
        return Err("La session historique doit être renouvelée au redémarrage.".into());
    }
    client.goto("about:blank").await?;
    client
        .goto(&format!("{address}/#session={next_token}&{task_fragment}"))
        .await?;
    wait_dom(client, title_matches, vec![json!(title)]).await?;
    stop_server(&mut restarted, port, &next_token).await?;
    Ok(())
}
