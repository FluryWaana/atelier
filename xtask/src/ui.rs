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

async fn start_server(data: &Path, port: u16) -> Result<(Process, String)> {
    let executable = root()
        .join("target/debug")
        .join(format!("atelier{}", std::env::consts::EXE_SUFFIX));
    let mut process = Process(
        command(executable)
            .arg("serve-prototype")
            .arg("--data-dir")
            .arg(data)
            .arg("--assets")
            .arg(root().join(".artifacts/rust-ui"))
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
        if let Ok(element) = client.find(Locator::Css(".card-title")).await
            && element.text().await? == title
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

pub async fn test() -> Result<()> {
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
    let result = scenario(&client, &http).await;
    let closed = client.close().await;
    result?;
    closed?;
    println!(
        "Parcours {browser} réussi : WASM sous CSP, création, redémarrage du processus, relecture et rotation de session."
    );
    Ok(())
}

async fn scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    let data = tempfile::tempdir()?;
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let (mut server, token) = start_server(data.path(), port).await?;
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
    std::fs::write(
        root().join(".artifacts/rust-ui-created.png"),
        client.screenshot().await?,
    )?;
    stop_server(&mut server, port, &token).await?;
    let (mut restarted, next_token) = start_server(data.path(), port).await?;
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
    client
        .goto(&format!("{address}/#session={next_token}"))
        .await
        .map_err(|_| "Navigation après redémarrage refusée.")?;
    wait_title(client, title).await?;
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
