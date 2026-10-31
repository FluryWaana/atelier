use crate::{Result, command, root, run};
use atelier_app::infrastructure::{local_http::LocalHttp, session};
use fantoccini::{Client, ClientBuilder, Locator};
use serde_json::json;
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    process::{Child, Stdio},
    time::{Duration, Instant},
};

trait SyntheticResponse {
    fn checked_status(self) -> Pin<Box<dyn Future<Output = Result<reqwest::Response>>>>;
}
impl SyntheticResponse for reqwest::Response {
    fn checked_status(self) -> Pin<Box<dyn Future<Output = Result<Self>>>> {
        // Keep the many sequential journey futures small on Windows' native thread stack.
        Box::pin(async move {
            if self.status().is_success() {
                return Ok(self);
            }
            let status = self.status();
            let path = self.url().path().to_owned();
            let message = self
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|value| value["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "Réponse sans erreur publique JSON.".into());
            Err(format!(
                "Requête synthétique {path} refusée ({status}) : {}",
                message.chars().take(2000).collect::<String>()
            )
            .into())
        })
    }
}

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

async fn start_server(
    executable: &Path,
    data: &Path,
    port: u16,
    assets: &Path,
) -> Result<(Process, String)> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let search = synthetic_search_path(&search)?;
    let mut server = command(executable);
    server.env_clear().env("PATH", search);
    for key in [
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "GNOME_KEYRING_CONTROL",
        "XDG_RUNTIME_DIR",
        "CI",
        "ATELIER_RUST_FINAL_CAMPAIGN",
        "ATELIER_TEST_DISPOSABLE_KEYRING",
        "ATELIER_TEST_KEYCHAIN",
        "RUNNER_TEMP",
    ] {
        if let Some(value) = std::env::var_os(key) {
            server.env(key, value);
        }
    }
    for key in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ] {
        server.env(key, data);
    }
    let mut process = Process(
        server
            .arg("serve")
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

fn synthetic_search_path(search: &std::ffi::OsStr) -> Result<std::ffi::OsString> {
    let paths = std::env::split_paths(search).filter(|directory| {
        directory.is_absolute()
            && !atelier_core::domain::agent_catalog::AGENTS
                .iter()
                .filter_map(|agent| agent.executable)
                .any(|name| {
                    ["", ".exe", ".com", ".cmd", ".bat"]
                        .iter()
                        .any(|extension| directory.join(format!("{name}{extension}")).exists())
                })
    });
    Ok(std::env::join_paths(paths)?)
}

async fn stop_server(process: &mut Process, port: u16, token: &str) -> Result<()> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse()?);
    headers.insert("x-local-request", "1".parse()?);
    headers.insert("content-type", "application/json".parse()?);
    let response = LocalHttp::new()?
        .request(
            &format!("http://127.0.0.1:{port}/api/shutdown"),
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

pub async fn test(prepared_assets: bool) -> Result<()> {
    let requested_journey = std::env::var("ATELIER_TEST_JOURNEY").unwrap_or("all".into());
    if !matches!(
        requested_journey.as_str(),
        "all" | "application" | "diagnostics" | "jira" | "updates"
    ) {
        return Err("Parcours navigateur inconnu.".into());
    }
    let browser = std::env::var("ATELIER_TEST_BROWSER")
        .unwrap_or(if cfg!(windows) { "edge" } else { "chrome" }.into());
    let default_driver = match browser.as_str() {
        "edge" => "msedgedriver",
        "chrome" => "chromedriver",
        "firefox" => "geckodriver",
        "safari" => "safaridriver",
        _ => return Err("Navigateur de test inconnu.".into()),
    };
    if prepared_assets {
        super::validate_ui_assets(&ui_assets())?;
    } else {
        super::build_ui(false)?;
    }
    if std::env::var_os("ATELIER_TEST_SERVER").is_none() {
        run(command("cargo").env("CARGO_PROFILE_DEV_DEBUG", "0").args([
            "build",
            "--locked",
            "-p",
            "atelier-app",
        ]))?;
    }
    run(command("cargo").args([
        "build",
        "--locked",
        "-p",
        "xtask",
        "--bin",
        "native-fixture",
    ]))?;
    let driver_program = std::env::var_os("ATELIER_WEBDRIVER").unwrap_or(default_driver.into());
    let driver_port = free_port()?;
    let mut driver_command = command(driver_program);
    if matches!(browser.as_str(), "edge" | "chrome") {
        driver_command.arg(format!("--port={driver_port}"));
    } else {
        driver_command.args(["--port", &driver_port.to_string()]);
    }
    if browser == "safari" {
        driver_command.arg("--diagnose");
    }
    let driver_log = tempfile::NamedTempFile::new()?;
    let mut driver = Process(
        driver_command
            .stdout(Stdio::from(driver_log.as_file().try_clone()?))
            .stderr(Stdio::from(driver_log.as_file().try_clone()?))
            .spawn()?,
    );
    let endpoint = format!("http://127.0.0.1:{driver_port}");
    let deadline = Instant::now() + Duration::from_secs(15);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .retry(reqwest::retry::never())
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
            let mut options = json!({"args":["--headless=new", "--disable-gpu", "--no-first-run", "--window-size=1280,900"]});
            if let Ok(binary) = std::env::var("ATELIER_TEST_BROWSER_BINARY") {
                options["binary"] = json!(binary);
            }
            capabilities.insert(
                if browser == "edge" {
                    "ms:edgeOptions"
                } else {
                    "goog:chromeOptions"
                }
                .into(),
                options,
            );
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
    let connected = ClientBuilder::rustls()?
        .capabilities(capabilities)
        .connect(&endpoint)
        .await;
    if connected.is_err() {
        let artifacts = root().join(".artifacts");
        std::fs::create_dir_all(&artifacts)?;
        let path = artifacts.join(format!(
            "rust-ui-{browser}-{}-startup.log",
            std::process::id()
        ));
        // No application session or authenticated journey exists at this point.
        match driver_log.persist_noclobber(&path) {
            Ok(_) => eprintln!("Journal du démarrage WebDriver : {}", path.display()),
            Err(error) => eprintln!("Journal du démarrage WebDriver non conservé : {error}"),
        }
    }
    let client = connected?;
    // The two-second client above is only for driver readiness, not multi-step native operations.
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()?;
    let journeys = [
        (
            "application",
            Box::pin(scenario(&client, &http)) as Pin<Box<dyn Future<Output = Result<()>> + '_>>,
        ),
        (
            "diagnostics",
            Box::pin(diagnostics_scenario(&client, &http)),
        ),
        ("jira", Box::pin(jira_recovery_scenario(&client, &http))),
        ("updates", Box::pin(updates_scenario(&client, &http))),
    ];
    let mut failures = Vec::new();
    for (name, journey) in journeys {
        if requested_journey != "all" && requested_journey != name {
            println!("Parcours synthétique {name} non exécuté (sélection {requested_journey}).");
            continue;
        }
        match journey.await {
            Ok(()) => println!("Parcours synthétique {name} réussi."),
            Err(error) => {
                eprintln!("Parcours synthétique {name} en échec : {error}");
                if let Ok(screenshot) = client.screenshot().await {
                    let path = root().join(format!(
                        ".artifacts/rust-ui-{browser}-{name}-{}-failed.png",
                        std::process::id()
                    ));
                    let _ = std::fs::write(&path, screenshot);
                    eprintln!("Capture du parcours {name} : {}", path.display());
                }
                failures.push(format!("{name}: {error}"));
            }
        }
    }
    let closed = client.close().await;
    if !failures.is_empty() {
        return Err(failures.join("\n").into());
    }
    closed?;
    println!(
        "Sélection navigateur {browser}/{requested_journey} réussie ; les parcours non exécutés restent non qualifiés."
    );
    Ok(())
}

async fn diagnostics_scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    let data = tempfile::tempdir()?;
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let (_server, token) = start_server(&ui_server(), data.path(), port, &ui_assets()).await?;
    client.goto(&format!("{address}/#session={token}")).await?;
    wait_dom(
        client,
        "return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    profiles_scenario(client, http, &address, &token, data.path()).await?;
    let fixture = crate::target()
        .join("debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    http.post(format!("{address}/api/agent-providers")).bearer_auth(&token).header("x-local-request", "1")
        .json(&json!({"id":"diagnostic-command","name":"Diagnostic synthétique","type":"command","auth":"cli","command":fixture,"analyzeArgs":["invalid-synthetic-command"],"expected":null}))
        .send().await?.checked_status().await?;
    let card: atelier_core::contracts::Card = http
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic diagnostic failure","repo":""}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    client.goto(&format!("{address}/tasks/{}", card.id)).await?;
    task_tab(client, "activity").await?;
    wait_dom(client, "return [...document.querySelector('#run-agent')?.options || []].some(option => option.value === 'diagnostic-command');", vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent');select.value='diagnostic-command';select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    client
        .find(Locator::Css("#brainstorm-message"))
        .await?
        .send_keys("Synthetic failure")
        .await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#runs-heading')?.parentElement.textContent.includes('Échec de l’exécution') && document.querySelector('#check-run-agent')?.hidden === false && document.querySelector('#check-run-agent')?.disabled === false;",vec![]).await?;
    click(client, "#check-run-agent").await?;
    wait_dom(client,"return document.querySelector('#agents-dialog')?.matches(':modal') && document.querySelector('#profile-id')?.value === 'diagnostic-command';",vec![]).await?;
    wait_dom(
        client,
        "return document.activeElement?.id === 'agent-diagnostic-heading';",
        vec![],
    )
    .await?;
    for width in [1440, 1024, 390] {
        client.set_window_rect(0, 0, width, 900).await?;
        click(client, "#close-agents").await?;
        click(client, "#check-run-agent").await?;
        wait_dom(
            client,
            "return document.activeElement?.id === 'agent-diagnostic-heading';",
            vec![],
        )
        .await?;
        std::fs::write(
            root().join(format!(".artifacts/diagnostic-agent-{width}.png")),
            client.screenshot().await?,
        )?;
    }
    client.set_window_rect(0, 0, 1440, 1000).await?;
    client
        .find(Locator::Css("#profile-name"))
        .await?
        .send_keys(" brouillon conservé")
        .await?;
    click(client, "#close-agents").await?;
    click(client, "#cancel-profile-action").await?;
    wait_dom(
        client,
        "return document.querySelector('#profile-name')?.value.endsWith(' brouillon conservé');",
        vec![],
    )
    .await?;
    click(client, "#close-agents").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return !document.querySelector('#agents-dialog') && document.activeElement?.id === 'check-run-agent';",vec![]).await?;
    http.post(format!("{address}/api/agent-providers")).bearer_auth(&token).header("x-local-request", "1")
        .json(&json!({"id":"diagnostic-missing","name":"Programme synthétique absent","type":"command","auth":"cli","command":data.path().join("missing-agent.exe"),"expected":null}))
        .send().await?.checked_status().await?;
    wait_dom(client, "return [...document.querySelector('#run-agent')?.options || []].some(option => option.value === 'diagnostic-missing');", vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent');select.value='diagnostic-missing';select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    client
        .find(Locator::Css("#brainstorm-message"))
        .await?
        .send_keys("Keep this draft")
        .await?;
    click(client, "#start-run").await?;
    wait_dom(client, "return document.querySelector('#check-run-agent')?.disabled === false && document.querySelector('#brainstorm-message')?.value === 'Keep this draft';", vec![]).await?;
    click(client, "#check-run-agent").await?;
    wait_dom(client, "return document.querySelector('#agents-dialog')?.matches(':modal') && document.querySelector('#profile-id')?.value === 'diagnostic-missing';", vec![]).await?;
    click(client, "#close-agents").await?;
    wait_dom(client, "return !document.querySelector('#agents-dialog') && document.querySelector('#brainstorm-message')?.value === 'Keep this draft' && document.activeElement?.id === 'check-run-agent';", vec![]).await?;
    client
        .find(Locator::Css("#brainstorm-message"))
        .await?
        .clear()
        .await?;
    click(client, "#close-editor").await?;
    Ok(())
}

async fn scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    let data = tempfile::tempdir()?;
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let assets = ui_assets();
    let (mut server, token) = start_server(&ui_server(), data.path(), port, &assets).await?;
    let repo = data.path().join("repositories/board-fixture");
    std::fs::create_dir_all(&repo)?;
    let project: serde_json::Value = http
        .post(format!("{address}/api/projects"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Projet synthétique", "repo":repo}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let queued: serde_json::Value = http.post(format!("{address}/api/cards"))
        .bearer_auth(&token).header("x-local-request", "1")
        .json(&json!({"title":"Attente projet", "description":"Une description à retrouver", "status":"review", "repo":repo}))
        .send().await?.checked_status().await?.json().await?;
    let archived: serde_json::Value = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Archive synthétique", "repo":repo}))
        .send()
        .await?
        .checked_status()
        .await?
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
    .checked_status()
    .await?;
    client
        .goto(&format!("{address}/#session={token}"))
        .await
        .map_err(|error| {
            format!(
                "Navigation de test refusée : {}",
                error.to_string().replace(&token, "[session]")
            )
        })?;
    client
        .wait()
        .at_most(Duration::from_secs(15))
        .for_element(Locator::Css("#task-title"))
        .await?;
    if client.current_url().await?.fragment().is_some() {
        return Err("Le jeton n’a pas été retiré de l’adresse.".into());
    }
    wait_dom(
        client,
        "return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    wait_dom(
        client,
        "return document.querySelector('#live-status')?.dataset.connected === 'true' && document.querySelector('#live-status').hidden && !document.querySelector('.board-meta') && !document.querySelector('#activity');",
        vec![],
    )
    .await?;
    click(client, "#command-button").await?;
    wait_dom(client, "return document.querySelector('#command-dialog')?.matches(':modal') && document.activeElement?.id === 'command-query' && document.querySelector('#command-dialog').getAttribute('aria-labelledby') === 'command-title';", vec![]).await?;
    let query = client.find(Locator::Css("#command-query")).await?;
    query.send_keys("\u{e015}").await?;
    wait_dom(client, "return document.querySelectorAll('.command-result')[1]?.getAttribute('aria-current') === 'true';", vec![]).await?;
    query.send_keys("synthetic-no-search-match-92817").await?;
    wait_dom(client, "return document.querySelectorAll('.command-result').length === 0 && document.querySelector('#command-results .empty')?.hidden === false;", vec![]).await?;
    query.send_keys("\u{e007}").await?;
    wait_dom(
        client,
        "return document.querySelector('#command-dialog')?.open === true;",
        vec![],
    )
    .await?;
    query.send_keys("\u{e00c}").await?;
    wait_dom(client, "return !document.querySelector('#command-dialog') && document.activeElement?.id === 'command-button';", vec![]).await?;
    click(client, "#command-button").await?;
    wait_dom(client, "return document.activeElement?.id === 'command-query' && document.querySelector('#command-query')?.value === '' && document.querySelectorAll('.command-result').length >= 3;", vec![]).await?;
    click(client, "#command-dialog .modal-close").await?;
    wait_dom(client, "return !document.querySelector('#command-dialog') && document.activeElement?.id === 'command-button';", vec![]).await?;
    wait_dom(client,"return document.querySelector('#overview-button')?.getAttribute('aria-current') === 'page';",vec![]).await?;
    wait_dom(client, "const nav = document.querySelector('.sidebar'); const overview = document.querySelector('#overview-button'); return innerWidth <= 800 || (overview.getBoundingClientRect().height > 0 && (innerHeight < 600 || nav.scrollHeight <= nav.clientHeight + 1));", vec![]).await?;
    if client
        .execute("return matchMedia('(min-width: 801px)').matches;", vec![])
        .await?
        == json!(true)
    {
        let expanded_width = client
            .execute(
                "return document.querySelector('.sidebar').getBoundingClientRect().width;",
                vec![],
            )
            .await?;
        click(client, "#toggle-navigation").await?;
        wait_dom(client, "const nav = document.querySelector('.sidebar'); const main = document.querySelector('main'); return document.querySelector('#toggle-navigation').getAttribute('aria-expanded') === 'false' && nav.getBoundingClientRect().width < arguments[0] && Math.abs(main.getBoundingClientRect().left - nav.getBoundingClientRect().right) < 1 && getComputedStyle(document.querySelector('#overview-button .nav-label')).position === 'absolute' && document.querySelector('#overview-button').title.length > 0;", vec![expanded_width.clone()]).await?;
        click(client, "#overview-button").await?;
        wait_dom(client, "return location.pathname === '/overview' && document.querySelector('.sidebar').classList.contains('sidebar-collapsed');", vec![]).await?;
        click(client, "#toggle-navigation").await?;
        wait_dom(client, "return document.querySelector('#toggle-navigation').getAttribute('aria-expanded') === 'true' && document.querySelector('.sidebar').getBoundingClientRect().width === arguments[0] && getComputedStyle(document.querySelector('#overview-button .nav-label')).position !== 'absolute';", vec![expanded_width]).await?;
    } else {
        wait_dom(client, "return document.querySelector('#toggle-navigation').getAttribute('aria-expanded') === 'false' && document.querySelector('.sidebar').getBoundingClientRect().height < 100;", vec![]).await?;
        click(client, "#toggle-navigation").await?;
        wait_dom(client, "return document.querySelector('#overview-button').getBoundingClientRect().height >= 44 && document.querySelector('#manage-settings').getBoundingClientRect().height >= 44;", vec![]).await?;
    }
    click(client, "#overview-button").await?;
    wait_dom(client, "return location.pathname === '/overview';", vec![]).await?;
    new_task(client).await?;
    let title = "Tâche synthétique é 🔧 <script>";
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys(title)
        .await?;
    click(client, "#create-task").await?;
    created_task(client, title).await?;
    wait_dom(client, "return document.querySelectorAll('#prototype-board .column').length === 5 && document.querySelectorAll('article.card').length === 2 && document.querySelector('[data-status=review] article.card')?.dataset.id === arguments[0] && !document.querySelector('.card-title script');", vec![queued["id"].clone()]).await?;
    client
        .find(Locator::Css("#board-search"))
        .await?
        .send_keys("synthetic-no-task-match-onboarding")
        .await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 0 && document.querySelector('#welcome')?.hidden === true && document.querySelector('#create-button')?.hidden === false;", vec![]).await?;
    clear_input(client, "#board-search").await?;
    client
        .find(Locator::Css("#board-search"))
        .await?
        .send_keys("DESCRIPTION À RETROUVER")
        .await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 1 && document.querySelector('article.card').dataset.id === arguments[0];", vec![queued["id"].clone()]).await?;
    clear_input(client, "#board-search").await?;
    // WebDriver clear does not consistently emit the input event across browsers.
    wait_dom(
        client,
        "const select = document.querySelector('#board-project'); const search = document.querySelector('#board-search'); const a = getComputedStyle(select); const b = getComputedStyle(search); return ['backgroundColor', 'borderRadius', 'fontSize', 'minHeight'].every(key => a[key] === b[key]) && (!CSS.supports('appearance', 'base-select') || (a.appearance === 'base-select' && a.display === 'flex' && a.flexWrap === 'nowrap' && getComputedStyle(select, '::picker(select)').appearance === 'base-select'));",
        vec![],
    )
    .await?;
    client.execute("document.querySelector('#board-search').dispatchEvent(new Event('input', {bubbles:true})); const select=document.querySelector('#board-project'); select.value=arguments[0]; select.dispatchEvent(new Event('change', {bubbles:true}));", vec![project["repo"].clone()]).await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 1 && document.querySelector('article.card').dataset.id === arguments[0];", vec![queued["id"].clone()]).await?;
    click(client, "#show-archives").await?;
    wait_dom(client, "return location.pathname === '/archives' && document.querySelectorAll('#workspace-archives .archive-row').length === 1 && document.querySelector('#workspace-archives .card-open')?.id === 'card-open-'+arguments[0];", vec![archived["id"].clone()]).await?;
    show_board(client).await?;
    client.execute("const select=document.querySelector('#board-project'); select.value=''; select.dispatchEvent(new Event('change', {bubbles:true}));", vec![]).await?;
    wait_title(client, title).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-created.png"),
        client.screenshot().await?,
    )?;
    let original_id = client
        .execute(
            "return document.querySelector('[data-status=todo] article.card').dataset.id;",
            vec![],
        )
        .await?;
    click(
        client,
        &format!(
            "#card-actions-toggle-{}",
            original_id.as_str().ok_or("Carte sans id")?
        ),
    )
    .await?;
    wait_dom(
        client,
        "return document.getElementById('card-actions-'+arguments[0])?.matches(':popover-open');",
        vec![original_id.clone()],
    )
    .await?;
    click(
        client,
        &format!(
            "#card-actions-{} button.danger:not([hidden])",
            original_id.as_str().ok_or("Carte sans id")?
        ),
    )
    .await?;
    wait_dom(client,"return document.querySelector('#confirm-dialog')?.open && document.querySelectorAll('article.card').length === 2;",vec![]).await?;
    click(
        client,
        "#confirm-dialog .form-actions button:not(#confirm-action)",
    )
    .await?;
    wait_dom(client,"return !document.querySelector('#confirm-dialog') && document.querySelectorAll('article.card').length === 2 && !!document.getElementById('card-open-'+arguments[0]);",vec![original_id.clone()]).await?;
    click(client, "[data-status=todo] .card-open").await?;
    wait_dom(client, "return location.pathname.startsWith('/tasks/') && !!document.querySelector('#task-editor') && document.activeElement?.id === 'editor-heading';", vec![]).await?;
    if client.current_url().await?.path()
        != format!("/tasks/{}", original_id.as_str().ok_or("Carte sans id")?)
    {
        return Err("Adresse de tâche absente.".into());
    }
    wait_dom(client,"return document.querySelectorAll('.criterion-text').length === 0 && document.querySelector('#save-task')?.disabled === true && document.querySelector('#add-criterion')?.textContent.trim() === 'Ajouter un critère';",vec![]).await?;
    client.refresh().await?;
    wait_dom(client, "return (location.pathname.startsWith('/tasks/') && !!document.querySelector('#task-editor')) && document.querySelector('#edit-title')?.value === arguments[0];", vec![json!(title)]).await?;
    client
        .find(Locator::Css("#edit-title"))
        .await?
        .send_keys("\u{e00c}")
        .await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.activeElement?.classList.contains('card-open') === true;", vec![]).await?;
    if client.current_url().await?.path() != "/board" {
        return Err("Adresse de tâche conservée après fermeture.".into());
    }
    click(client, "[data-status=todo] .card-open").await?;
    let title = "Tâche modifiée é 🔧 <script>";
    clear_input(client, "#edit-title").await?;
    client
        .find(Locator::Css("#edit-title"))
        .await?
        .send_keys(title)
        .await?;
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .send_keys("Description enregistrée")
        .await?;
    click(client, "#add-criterion").await?;
    client
        .find(Locator::Css(".criterion-text"))
        .await?
        .send_keys("Vérification humaine synthétique")
        .await?;
    click(client, ".criterion-done").await?;
    // Switch mounted panels with the keyboard while the form contains an unsaved draft.
    task_tab(client, "details").await?;
    client
        .find(Locator::Css("#task-tab-details"))
        .await?
        .send_keys("\u{e014}")
        .await?;
    wait_dom(client,"return document.activeElement?.id==='task-tab-activity' && document.querySelector('#task-tab-activity')?.getAttribute('aria-selected')==='true' && document.querySelector('#task-panel-details')?.hidden===true && document.querySelector('#task-panel-activity')?.hidden===false;",vec![]).await?;
    client
        .find(Locator::Css("#task-tab-activity"))
        .await?
        .send_keys("\u{e010}")
        .await?;
    wait_dom(client,"return document.activeElement?.id==='task-tab-files' && document.querySelector('#task-tab-files')?.getAttribute('aria-selected')==='true';",vec![]).await?;
    client
        .find(Locator::Css("#task-tab-files"))
        .await?
        .send_keys("\u{e012}")
        .await?;
    wait_dom(client,"return document.activeElement?.id==='task-tab-diff' && document.querySelector('#task-tab-diff')?.getAttribute('aria-selected')==='true';",vec![]).await?;
    client
        .find(Locator::Css("#task-tab-diff"))
        .await?
        .send_keys("\u{e011}")
        .await?;
    wait_dom(client,"return document.activeElement?.id==='task-tab-details' && document.querySelector('#task-panel-details')?.hidden===false && document.querySelector('#edit-title')?.value===arguments[0] && document.querySelector('#edit-description')?.value==='Description enregistrée' && document.querySelector('.criterion-done')?.checked===true && document.querySelector('#save-task')?.disabled===false;",vec![json!(title)]).await?;
    click(client, "#manage-settings").await?;
    wait_dom(client,"return location.pathname.startsWith('/tasks/') && document.querySelector('#confirm-editor')?.offsetParent !== null && document.querySelector('#edit-title')?.value === arguments[0] && !document.querySelector('#settings-page');",vec![json!(title)]).await?;
    click(client, "#cancel-confirmation").await?;
    client.back().await?;
    wait_dom(client,"return location.pathname.startsWith('/tasks/') && document.querySelector('#confirm-editor')?.offsetParent !== null && document.querySelector('#edit-title')?.value === arguments[0];",vec![json!(title)]).await?;
    click(client, "#cancel-confirmation").await?;
    clear_input(client, "#edit-title").await?;
    task_tab(client, "activity").await?;
    if client
        .execute(
            "return document.querySelector('#edit-title').form.checkValidity();",
            vec![],
        )
        .await?
        != json!(false)
    {
        return Err("Un titre vide doit rester invalide dans un panneau masqué.".into());
    }
    wait_dom(client,"return document.querySelector('#task-tab-details')?.getAttribute('aria-selected')==='true' && document.querySelector('#task-panel-details')?.hidden===false && document.activeElement?.id==='edit-title' && document.querySelector('#edit-description')?.value==='Description enregistrée';",vec![]).await?;
    client
        .find(Locator::Css("#edit-title"))
        .await?
        .send_keys(title)
        .await?;
    click(
        client,
        &format!(
            "[data-dependency='{}']",
            queued["id"].as_str().ok_or("Dépendance sans id")?
        ),
    )
    .await?;
    client
        .find(Locator::Css("#edit-title"))
        .await?
        .send_keys("\u{e00c}")
        .await?;
    wait_dom(
        client,
        "return document.querySelector('#confirm-editor')?.textContent.includes('Abandonner');",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(
        client,
        "return document.querySelector('#edit-title')?.value === arguments[0];",
        vec![json!(title)],
    )
    .await?;
    click(client, "#save-task").await?;
    saved_task(client).await?;
    wait_title(client, title).await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelector('[data-status=todo] .card-description')?.textContent === 'Description enregistrée';", vec![]).await?;
    click(client, "[data-status=todo] .card-open").await?;
    // Exercise a real HTTP validation error and retain the draft, without changing server fixtures.
    client.execute("const select=document.querySelector('#edit-status'); select.add(new Option('Invalide', 'invalid')); select.value='invalid'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#save-task").await?;
    wait_dom(client, "return document.querySelector('#editor-error')?.textContent === 'Statut invalide.' && document.querySelector('#edit-title')?.value === arguments[0];", vec![json!(title)]).await?;
    client.execute("const select=document.querySelector('#edit-status'); select.value='doing'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#save-task").await?;
    saved_task(client).await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelector('[data-status=doing] article.card')?.dataset.id === arguments[0];", vec![original_id.clone()]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if !board.runs.is_empty() {
        return Err("Un déplacement manuel a lancé un agent.".into());
    }
    click(client, "[data-status=doing] .card-open").await?;
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .send_keys(" brouillon local")
        .await?;
    let external = http.patch(format!("{address}/api/cards/{}", original_id.as_str().ok_or("Carte sans id")?))
        .bearer_auth(&token).header("x-local-request", "1")
        .json(&json!({"description":"Modification externe", "expected":{"description":"Description enregistrée"}}))
        .send().await?;
    if !external.status().is_success() {
        let status = external.status();
        let reason = external.text().await?.replace(&token, "[session]");
        let current: serde_json::Value = http
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
            .send()
            .await?
            .checked_status()
            .await?
            .json()
            .await?;
        let description = current["cards"]
            .as_array()
            .and_then(|cards| cards.iter().find(|card| card["id"] == original_id))
            .and_then(|card| card["description"].as_str())
            .map(|description| description.encode_utf16().take(128).collect::<Vec<_>>());
        return Err(format!("Modification externe synthétique refusée ({status}) : {reason}; description actuelle UTF-16 : {description:?}").into());
    }
    wait_dom(client, "return document.querySelector('[data-status=doing] .card-description')?.textContent === 'Modification externe' && document.querySelector('#edit-description')?.value === 'Description enregistrée brouillon local';", vec![]).await?;
    click(client, "#save-task").await?;
    wait_dom(client, "return document.querySelector('#editor-error')?.textContent.includes('changé ailleurs') && document.querySelector('#edit-description')?.value === 'Description enregistrée brouillon local';", vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#confirm-editor").await?;
    click(client, "[data-status=doing] .card-open").await?;
    wait_dom(
        client,
        "return document.querySelector('#edit-description')?.value === 'Modification externe';",
        vec![],
    )
    .await?;
    clear_input(client, "#edit-description").await?;
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .send_keys("Description enregistrée")
        .await?;
    click(client, "#save-task").await?;
    saved_task(client).await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelector('[data-status=doing] .card-description')?.textContent === 'Description enregistrée';", vec![]).await?;
    click(client, "[data-status=doing] .card-open").await?;
    click(client, "#duplicate-task").await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length === 3;", vec![]).await?;
    let copy_id = client
        .execute(
            "return document.querySelector('[data-status=todo] article.card').dataset.id;",
            vec![],
        )
        .await?;
    let copy_selector = format!(
        "article.card[data-id='{}'] .card-open",
        copy_id.as_str().ok_or("Copie sans id")?
    );
    click(client, &copy_selector).await?;
    wait_dom(client, "return document.querySelector('.criterion-text')?.value === 'Vérification humaine synthétique' && document.querySelector('.criterion-done')?.checked === false && document.querySelectorAll('[data-dependency]:checked').length === 0;", vec![]).await?;
    click(client, "#archive-task").await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length === 2;", vec![]).await?;
    click(client, "[data-status=review] .card-open").await?;
    click(client, "#delete-task").await?;
    click(client, "#confirm-editor").await?;
    wait_dom(client, "return document.querySelector('#editor-error')?.textContent.includes('Bloquée par') && (location.pathname.startsWith('/tasks/') && !!document.querySelector('#task-editor'));", vec![]).await?;
    click(client, "#cancel-confirmation").await?;
    click(client, "#close-editor").await?;
    click(client, "#show-archives").await?;
    click(
        client,
        &format!(
            "#workspace-archives #card-open-{}",
            copy_id.as_str().ok_or("Copie sansid")?
        ),
    )
    .await?;
    wait_dom(client, "return document.querySelector('#edit-title')?.disabled === true || document.querySelector('#edit-title')?.matches(':disabled') === true;", vec![]).await?;
    click(client, "#archive-task").await?;
    wait_dom(client, "return location.pathname === '/archives' && !document.querySelector('#task-editor') && document.querySelectorAll('#workspace-archives .archive-row').length === 1;", vec![]).await?;
    show_board(client).await?;
    click(client, &copy_selector).await?;
    click(client, "#delete-task").await?;
    click(client, "#cancel-confirmation").await?;
    click(client, "#delete-task").await?;
    click(client, "#confirm-editor").await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length === 2;", vec![]).await?;
    stop_server(&mut server, port, &token).await?;
    let (mut restarted, next_token) =
        start_server(&ui_server(), data.path(), port, &assets).await?;
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
    show_board(client).await?;
    wait_title(client, title).await?;
    wait_dom(client, "return document.querySelector('[data-status=doing] article.card')?.dataset.id === arguments[0] && document.querySelector('[data-status=doing] .card-description')?.textContent === 'Description enregistrée' && !document.querySelector('.card-title script');", vec![original_id]).await?;
    click(client, "[data-status=doing] .card-open").await?;
    wait_dom(client, "return document.querySelector('.criterion-text')?.value === 'Vérification humaine synthétique' && document.querySelector('.criterion-done')?.checked === true && document.querySelectorAll('[data-dependency]:checked').length === 1;", vec![]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-editor.png"),
        client.screenshot().await?,
    )?;
    click(client, "#make-template").await?;
    client
        .find(Locator::Css("#template-name"))
        .await?
        .send_keys("Modèle synthétique")
        .await?;
    click(client, "#save-template").await?;
    wait_dom(
        client,
        "return document.querySelector('#editor-notice')?.textContent === 'Modèle enregistré.';",
        vec![],
    )
    .await?;
    click(client, "#close-editor").await?;
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
    new_task(client).await?;
    expand_control(client, "#task-template").await?;
    client.execute("const select=document.querySelector('#task-template'); select.selectedIndex=1; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys("Tâche issue du modèle")
        .await?;
    click(client, "#create-task").await?;
    created_task(client, "Tâche issue du modèle").await?;
    click(client, "[data-status=todo] .card-open").await?;
    wait_dom(client, "return document.querySelector('#edit-description')?.value === 'Description enregistrée' && document.querySelector('.criterion-text')?.value === 'Vérification humaine synthétique' && document.querySelector('.criterion-done')?.checked === false && document.querySelectorAll('[data-dependency]:checked').length === 0;", vec![]).await?;
    click(client, "#close-editor").await?;
    new_task(client).await?;
    expand_control(client, "#task-template").await?;
    client.execute("const select=document.querySelector('#task-template');select.selectedIndex=1;select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#remove-template").await?;
    click(client, "#confirm-remove-template").await?;
    wait_dom(client, "return document.querySelector('#task-template')?.options.length === 1 && document.querySelectorAll('article.card').length === 3;", vec![]).await?;
    click(client, "#new-dialog .ui-modal-header [data-close]").await?;
    live_scenario(client, http, &address, &next_token).await?;
    projects_scenario(client, http, &address, &next_token, data.path()).await?;
    kanban_columns_scenario(client, http, &address, &next_token).await?;
    settings_scenario(client, http, &address, &next_token, data.path()).await?;
    backups_scenario(client, http, &address, &next_token).await?;
    language_scenario(client).await?;
    files_scenario(client, http, &address, &next_token, data.path()).await?;
    profiles_scenario(client, http, &address, &next_token, data.path()).await?;
    client
        .goto(&format!("{address}/tasks/missing-synthetic-task"))
        .await?;
    wait_dom(client, "return document.querySelector('#error')?.textContent.includes('introuvable') && !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length > 0;", vec![]).await?;
    stop_server(&mut restarted, port, &next_token).await?;
    Ok(())
}

async fn updates_scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    use atelier_app::infrastructure::{
        installation::InstallationMode, installed, package, session, update_package, updates,
    };
    use atelier_core::application::updates::Phase;
    let temporary = tempfile::tempdir()?;
    let assets = ui_assets();
    let binary = ui_server();
    update_package::generate_synthetic(&temporary.path().join("keys"))?;
    let current = env!("CARGO_PKG_VERSION");
    let parts = package::version_parts(current)?;
    let proposed = format!(
        "{}.{}.{}",
        parts[0],
        parts[1],
        parts[2]
            .checked_add(1)
            .ok_or("Version de fixture excessive.")?
    );
    crate::notices::generate(&temporary.path().join("third-party-notices.txt")).await?;
    println!(
        "Updates : octets du binaire synthétique={}, avis={}; limite du binaire=134217728.",
        std::fs::metadata(&binary)?.len(),
        std::fs::metadata(temporary.path().join("third-party-notices.txt"))?.len(),
    );
    let (original, original_signature) =
        crate::update_flow::archive(temporary.path(), "original", &binary, &assets, current)?;
    let (candidate, candidate_signature) = crate::update_flow::archive(
        temporary.path(),
        "version-mismatch",
        &binary,
        &assets,
        &proposed,
    )?;
    let installation = temporary.path().join("installed");
    installed::install(
        &original,
        &original_signature,
        &temporary.path().join("keys/synthetic-update-public.json"),
        &installation,
        InstallationMode::Portable,
        package::native_target()?,
    )?;
    let installed = installed::open(&installation, package::native_target()?)?
        .ok_or("Installation navigateur absente.")?;
    let data = temporary.path().join("synthetic-data");
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let (_server, _) = start_server(
        &installed.active_executable()?,
        &data,
        port,
        &installed.slot_path(&installed.current().active)?.join("ui"),
    )
    .await?;
    let result=async {
        let token=session::read(&data)?;
        client.goto(&format!("{address}/#session={token}")).await.map_err(|error|format!("Updates : ouverture initiale : {error:?}").replace(&token,"[session]").chars().take(2000).collect::<String>())?;
        wait_dom(client,"return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        click(client,"#check-updates").await?;
        wait_dom(client,"return location.pathname === '/updates';",vec![]).await?;
        wait_dom(client,"return document.querySelector('#updates-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        if client.execute("return document.querySelector('#update-archive, #update-signature, #approve-update') === null;",vec![]).await? != json!(true) {
            return Err("Les anciens champs de mise à jour sont encore affichés.".into());
        }
        // Pause the page before the exclusive fixture reservation, including its agent probes.
        client.goto("about:blank").await?;
        let mut prepared=false;
        for _ in 0..75 {
            let response=http.post(format!("{address}/api/updates/preview")).bearer_auth(&token).header("x-local-request","1").json(&json!({"archive":candidate,"signature":candidate_signature})).send().await?;
            if response.status().is_success(){prepared=true;break;}
            let status=response.status();let message:serde_json::Value=response.json().await?;
            if status!=reqwest::StatusCode::CONFLICT || message["error"]!=json!("message.a_change_is_in_progress_try_again_shortly"){return Err(format!("Préparation synthétique refusée ({status}): {message}").into());}
            // This refusal happens before reservation, so this exact fixture request has no effect to duplicate.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        if !prepared{return Err("Préparation synthétique bloquée par une admission occupée.".into());}
        client.goto(&format!("{address}/updates")).await?;
        wait_dom(client,"return !!document.querySelector('#reload-updates');",vec![]).await?;
        click(client,"#reload-updates").await?;
        let preparation_deadline=Instant::now()+Duration::from_secs(120);
        loop {
            let phase=updates::status(Some(&installation))?.operation.map(|operation|operation.state);
            if phase==Some(Phase::Prepared) { break; }
            if phase.is_some_and(|phase|phase!=Phase::Preparing) {
                return Err(format!("Préparation navigateur interrompue : {phase:?}.").into());
            }
            if Instant::now()>=preparation_deadline {
                return Err(format!("Préparation navigateur trop longue : {phase:?}.").into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        wait_dom(client,"return document.querySelector('#update-phase')?.textContent === 'Version prête à confirmer' && document.querySelector('#updates-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        client.refresh().await?;
        wait_dom(client,"return location.pathname === '/updates' && document.querySelector('#update-phase')?.textContent === 'Version prête à confirmer' && document.querySelector('#updates-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        click(client,"#settings-tab-general").await?;
        let saved_agent=client.execute("const select=document.querySelector('#settings-agent'); select.value=select.value === 'claude' ? 'codex' : 'claude'; select.dispatchEvent(new Event('change',{bubbles:true})); return select.value;",vec![]).await?;
        click(client,"#settings-tab-updates").await?;
        if client.execute("return document.querySelector('#apply-update')?.matches(':disabled');",vec![]).await?!=json!(true){return Err("La mise à jour n’a pas protégé l’autre brouillon.".into());}
        click(client,"#settings-tab-general").await?;click(client,"#save-settings").await?;
        wait_dom(client,"return document.querySelector('#settings-form')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        let saved_state:serde_json::Value=http.get(format!("{address}/api/state")).bearer_auth(&token).send().await?.checked_status().await?.json().await?;
        if saved_state["settings"]["agent"]!=saved_agent {
            return Err(format!("Réglage non enregistré avant activation : {:?}",client.execute("return {error:document.querySelector('#settings-error')?.textContent,notice:document.querySelector('#settings-notice')?.textContent,agent:document.querySelector('#settings-agent')?.value};",vec![]).await?).into());
        }
        wait_dom(client,"return document.querySelector('#save-settings')?.disabled === true;",vec![]).await?;click(client,"#settings-tab-updates").await?;
        click(client,"#reload-updates").await?;
        wait_dom(client,"return document.querySelector('#updates-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        click(client,"#apply-update").await.map_err(|error|format!("Updates : clic natif d’activation : {error:?}").replace(&token,"[session]").chars().take(2000).collect::<String>())?;
        let deadline=Instant::now()+Duration::from_secs(120);
        loop {
            let state=updates::status(Some(&installation))?.operation.ok_or("Journal navigateur absent.")?.state;
            if state==Phase::RolledBack { break; }
            if !state.pending() { return Err("La mauvaise version n’a pas déclenché une récupération navigateur.".into()); }
            if Instant::now()>=deadline { return Err("Récupération navigateur trop longue.".into()); }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let next=session::read(&data)?;if next==token{return Err("La session n’a pas été renouvelée après récupération.".into());}
        // Reopening must mount a fresh page: a fragment-only navigation retains the old session.
        client.goto("about:blank").await.map_err(|error|format!("Updates : fermeture de l’ancien document : {error:?}").replace(&token,"[session]").replace(&next,"[session]").chars().take(2000).collect::<String>())?;
        client.goto(&format!("{address}/#session={next}")).await.map_err(|error|format!("Updates : réouverture après récupération : {error:?}").replace(&token,"[session]").replace(&next,"[session]").chars().take(2000).collect::<String>())?;
        wait_dom(client,"return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        click(client,"#check-updates").await.map_err(|error|format!("Updates : ouverture de la page après récupération : {error:?}").replace(&token,"[session]").replace(&next,"[session]").chars().take(2000).collect::<String>())?;
        wait_dom(client,"return location.pathname === '/updates' && document.querySelector('#update-phase')?.textContent === 'Version précédente rétablie' && document.querySelector('#updates-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
        let state:serde_json::Value=http.get(format!("{address}/api/state")).bearer_auth(&next).send().await?.checked_status().await?.json().await?;
        if state["settings"]["agent"]!=saved_agent{return Err("La récupération a perdu le réglage enregistré.".into());}
        std::fs::write(root().join(".artifacts/rust-ui-update-recovered.png"),client.screenshot().await?)?;
        Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    if result.is_err() {
        let phase = updates::status(Some(&installation))
            .ok()
            .and_then(|status| status.operation.map(|operation| operation.state));
        eprintln!("Updates : phase du journal avant nettoyage synthétique : {phase:?}");
    }
    let stopped = crate::update_flow::stop_fixture(&installation, &data, port).await;
    result?;
    stopped?;
    Ok(())
}

async fn jira_recovery_scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    use atelier_app::infrastructure::{prototype::PrototypeDirectory, store::Documents};
    use atelier_core::document::Document;
    let data = tempfile::tempdir()?;
    {
        let root = PrototypeDirectory::open(data.path())?;
        let database = root.database()?;
        let brainstorm_repo = data.path().join("repositories").join("brainstorm");
        std::fs::create_dir_all(&brainstorm_repo)?;
        run(command("git")
            .current_dir(&brainstorm_repo)
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .args(["init", "-b", "main"]))?;
        std::fs::write(
            brainstorm_repo.join(".run-fixture"),
            "atelier-r03-synthetic-only",
        )?;
        run(command("git")
            .current_dir(&brainstorm_repo)
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .args(["add", "--", ".run-fixture"]))?;
        run(command("git")
            .current_dir(&brainstorm_repo)
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .args([
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
                "-c",
                "core.hooksPath=NUL",
                "commit",
                "-qm",
                "Synthetic brainstorm baseline",
            ]))?;
        let brainstorm_verification = format!(
            "\"{}\" verify success",
            crate::target()
                .join("debug")
                .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX))
                .display()
        );
        database.call(move |store| {
            // Match the application boundary, including Windows short-path aliases.
            let brainstorm_repo = atelier_core::application::task_creation::CreationContext::repository(store, &serde_json::value::to_raw_value(&brainstorm_repo)?)?;
            store.put(Documents::Runs,&json!({"id":"usage-fixture","cardId":"jira-refresh","agentType":"codex","agent":"synthetic-usage-profile","model":"synthetic-usage-model","status":"completed","startedAt":"2026-10-25T10:00:00Z","usage":{"inputTokens":11,"cachedInputTokens":null,"outputTokens":2}}))?;
            // Report fixture only: no agent execution or remote identity proof is implied.
            store.put(Documents::Cards,&json!({"id":"review-report","title":"Synthetic review report","status":"todo","repo":"","checklist":[{"text":"Synthetic review criterion","done":false}]}))?;
            // Export preview/recovery fixtures only: no remote worklog is implied.
            store.put(Documents::Cards,&json!({"id":"time-export","title":"Synthetic time export","description":"Keep local export content","status":"todo","repo":"","ticket":{"connector":"jira","instance":"https://synthetic-recovery.example.test","id":"123","key":"SYN-123"}}))?;
            for (suffix,state) in [("1","draft"),("2","sending")] {
                let id=format!("00000000-0000-4000-8000-00000000000{suffix}");
                let entry=json!({"id":id,"cardId":"time-export","seconds":3600,"date":"2026-10-25","startTime":"09:30:00","description":"Synthetic human time","exportState":state});
                let mut snapshot=entry.clone();snapshot["exportState"]=json!("draft");
                let preview=json!({"entry":snapshot,"cardId":"time-export","ticket":{"connector":"jira","instance":"https://synthetic-recovery.example.test","id":"123","key":"SYN-123"},"jiraUrl":"https://synthetic-recovery.example.test","jiraRevision":1,"tempo":{"base":"https://api.tempo.io/4","revision":0},"payload":{"issueId":123,"authorAccountId":"synthetic-account","timeSpentSeconds":3600,"startDate":"2026-10-25","startTime":"09:30:00","description":format!("Synthetic human time\n[atelier:{id}]")}});
                let mut entry=entry;
                if state=="draft" {entry["tempoExportPreview"]=preview;} else {entry["tempoExport"]=json!({"id":"00000000-0000-4000-8000-000000000003","state":"sending","preview":preview,"worklogId":null});}
                store.put(Documents::Times,&entry)?;
            }
            // Preview fixture only; no native Git operation or remote creation is implied.
            store.put(Documents::Cards,&json!({"id":"commit-draft","title":"Synthetic commit draft","description":"Human commit content","status":"todo","repo":"/synthetic/source","worktree":{"path":"/synthetic/not-created","source":"/synthetic/source","branch":"task/commit","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}))?;
            store.put(Documents::Cards,&json!({
                "id":"publication-branch","title":"Synthetic branch task","description":"Human branch content","status":"todo","repo":"","forgePublicationPreview":{
                    "configuration":{
                        "kind":"github","base":"https://github-synthetic.example.test"
                    },"revision":0,"repository":"owner/repo","worktree":{
                        "path":"/synthetic/not-created","source":"/synthetic/source","branch":"task/first","base":"main","baseCommit":"b".repeat(40),"state":"ready"
                    },"target":"main","origin":"https://github-synthetic.example.test/owner/repo.git","remoteSource":null,"localChanges":true,"sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"title":"Synthetic first publication","body":"Keep first publication draft","draft":true
                }
            }))?;
            // Approval controls only: missing synthetic profile/worktree must never dispatch an agent.
            let approval_project:atelier_core::document::Document=serde_json::from_str(&json!({"id":"publication-approval-project","name":"Synthetic approval controls","repo":"/synthetic/approval-source","automationEnabled":true,"autoFix":false,"testCommand":"synthetic-disabled","prTarget":"main"}).to_string())?;
            store.put(Documents::Projects,&approval_project)?;
            let mut approval:atelier_core::contracts::Card=serde_json::from_str(&json!({"id":"publication-approval","title":"Synthetic publication approval","agent":"synthetic-unavailable","repo":"/synthetic/approval-source","status":"todo","worktree":{"path":"/synthetic/not-created","source":"/synthetic/approval-source","branch":"task/first","base":"main","baseCommit":"b".repeat(40),"state":"ready"}}).to_string())?;
            let worktree=approval.fields.get::<atelier_core::contracts::Worktree>("worktree")?.ok_or("Synthetic approval worktree missing")?;
            let profile=serde_json::from_str(r#"{"id":"synthetic-unavailable","type":"command","name":"Synthetic unavailable profile"}"#)?;
            let flow=atelier_core::application::automation::Flow{
                id:"00000000-0000-4000-8000-000000000007".into(),state:atelier_core::application::automation::State::Waiting,step:atelier_core::application::automation::Step::Publication,project:approval_project,requirements:atelier_core::application::preparation::requirements(&approval)?,worktree:worktree.clone(),agent:"synthetic-unavailable".into(),profile,model:String::new(),effort:String::new(),instructions:String::new(),run_id:None,review_id:None,verification_id:None,publication_plan:None,commit_plan:None,review_requested:false,correction_indexes:vec![],cycles:0,auto_fix:false,started_at:"2026-10-26T00:00:00Z".into(),updated_at:"2026-10-26T00:00:00Z".into(),message:"Synthetic approval fixture only; no verification performed".into()
            };
            approval.fields.set("automation",&flow)?;
            approval.fields.set("forgePublicationPreview",&json!({"configuration":{"kind":"github","base":"https://github-synthetic.example.test"},"revision":0,"repository":"owner/repo","worktree":worktree,"target":"main","origin":"https://github-synthetic.example.test/owner/repo.git","remoteSource":null,"localChanges":false,"sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"title":"Synthetic approved publication","body":"Synthetic approval fixture","draft":true}))?;
            store.put(Documents::Cards,&approval)?;
            store.put(Documents::Runs,&json!({
                "id":"review-report-run","cardId":"review-report","kind":"pr-review","purpose":"review","status":"completed","startedAt":"2026-10-25T10:00:00Z","reviewStaleReason":atelier_app::infrastructure::reviews::UNCHECKED,"reviewContext":{
                    "requirements":{
                        "criteria":["Synthetic review criterion"]
                    },"worktree":{
                        "path":"/synthetic/not-created","source":"/synthetic/source","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"
                    },"kind":"github","revision":0,"repository":"owner/repo","number":7,"source":"b".repeat(40),"target":"a".repeat(40),"base":"a".repeat(40)
                },"review":{
                    "summary":"Synthetic review limits: browser and service not executed","findings":[{
                        "priority":"P2","title":"Synthetic finding <script>ignored</script>","file":"src/lib.rs","line":12,"problem":"Synthetic evidence","suggestion":"Synthetic correction"
                    }
                    ],"criteria":[{
                        "index":0,"status":"unverifiable","evidence":"Synthetic UI not executed"
                    }
                    ]
                }
            }))?;
            // This independent journey shares the deliberate uncertain Jira fixture:
            // configure its project before startup, rather than bypassing the global guard.
            store.put(Documents::Projects,&json!({"id":"brainstorm-project","name":"Synthetic brainstorm","repo":brainstorm_repo,"testCommand":brainstorm_verification,"automationEnabled":true,"autoFix":false,"prTarget":"main"}))?;
            store.put(Documents::Cards,&json!({"id":"brainstorm-session","title":"Synthetic brainstorm idea","description":"Keep original idea","instructions":"","mode":"analyze","agent":"codex","status":"todo","brainstormSession":true,"repo":brainstorm_repo}))?;
            store.put(Documents::Runs,&json!({"id":"brainstorm-result","cardId":"brainstorm-session","status":"completed","purpose":"brainstorm","startedAt":"2026-10-25T10:00:00Z","userMessage":"Découpe cette idée","brainstormAnswer":{"message":"Synthetic structured proposal","questions":[{"question":"Synthetic choice?","options":["Local only","Shared"]}],"tickets":[{"title":"Synthetic converted task","description":"Converted description","criteria":["Manual validation remains required"],"questions":["Keep open question"]}]}}))?;
            store.save_settings(&json!({"jiraConnection":{"mode":"cloud","url":"https://synthetic-recovery.example.test","email":"synthetic@example.test","query":"project = SYN","revision":1}}))?;
            store.put(Documents::Cards,&json!({"id":"jira-creation","title":"Synthetic Jira creation","description":"Keep local creation content","instructions":"","agent":"codex","mode":"analyze","status":"todo","checklist":[],"jiraCreation":{"id":"synthetic-creation-operation","state":"sending","base":"https://synthetic-recovery.example.test","projectKey":"SYN","issueType":{"id":"1","name":"Task","subtask":false},"revision":1,"startedAt":"2026-10-25T10:00:00Z","result":null}}))?;
            store.put(Documents::Cards,&json!({
                "id":"jira-recovery","title":"Synthetic Jira recovery","description":"Local content","instructions":"","agent":"codex","mode":"analyze","status":"todo","checklist":[],"ticket":{
                    "connector":"jira","instance":"https://synthetic-recovery.example.test","id":"1","key":"SYN-1","status":"Open","url":"https://synthetic-recovery.example.test/browse/SYN-1"
                },"jiraTransition":{
                    "id":"synthetic-operation","state":"sending","base":"https://synthetic-recovery.example.test","issueId":"1","revision":1,"from":{
                        "id":"1","name":"Open"
                    },"transition":{
                        "id":"11","name":"Complete","target":{
                            "id":"2","name":"Done"
                        },"requiresFields":false
                    },"startedAt":"2026-10-25T10:00:00Z","observed":null
                }
            }))?;
            let mut card:atelier_core::contracts::Card=serde_json::from_str(&json!({"id":"jira-refresh","title":"Local refresh title","description":"Local refresh description","instructions":"Keep refresh instructions","agent":"codex","mode":"analyze","status":"todo","checklist":[{"text":"Keep refresh criterion","done":true}],"ticket":{"connector":"jira","instance":"https://synthetic-recovery.example.test","id":"2","key":"SYN-2","status":"Open"}}).to_string())?;
            card.fields.set("pullRequest",&json!({"revision":0,"kind":"github","repository":"owner/repo","number":7,"title":"Synthetic linked PR","url":"https://github-synthetic.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task/synthetic","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"fork":false,"baseUrl":"https://github-synthetic.example.test","checkedAt":"2026-10-25T10:00:00Z"}))?;
            store.put(Documents::Cards,&card)?;
            atelier_app::infrastructure::jira_refresh::reserve(store,&atelier_core::contracts::JiraRefreshRequest{revision:1,expected:card},"synthetic-preview")?;
            atelier_app::infrastructure::jira_refresh::ready(store,"jira-refresh","synthetic-preview",atelier_core::contracts::JiraIssueDetail{connection_revision:1,description:"Remote refresh description é😀".into(),issue:atelier_core::contracts::JiraIssue{id:"2".into(),key:"SYN-2".into(),title:"Remote refresh title".into(),status:"Open".into(),status_id:"1".into(),url:"https://synthetic-recovery.example.test/browse/SYN-2".into()}})?;
            // Prebuilt read snapshots exercise local batch approval, not a successful remote read.
            for index in 1..=2 {
                let id=format!("jira-batch-{index}");
                let issue=format!("{}",index+20);
                let card:atelier_core::contracts::Card=serde_json::from_str(&json!({"id":id,"title":format!("Local batch {index}"),"agent":"codex","description":"Keep batch description","instructions":"Keep batch instructions","status":"review","checklist":[{"text":"Keep batch criterion","done":true}],"ticket":{"connector":"jira","instance":"https://synthetic-recovery.example.test","id":issue,"key":format!("SYN-{}",index+20)}}).to_string())?;
                store.put(Documents::Cards,&card)?;
                atelier_app::infrastructure::jira_refresh::reserve(store,&atelier_core::contracts::JiraRefreshRequest{revision:1,expected:card},&id)?;
                atelier_app::infrastructure::jira_refresh::ready(store,&id,&id,atelier_core::contracts::JiraIssueDetail{connection_revision:1,description:"Batch remote description".into(),issue:atelier_core::contracts::JiraIssue{id:issue,key:format!("SYN-{}",index+20),title:format!("Remote batch {index}"),status:"Open".into(),status_id:"1".into(),url:format!("https://synthetic-recovery.example.test/browse/SYN-{}",index+20)}})?;
            }
            Ok(())
        }).await?;
        database.close().await?;
    }
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let (mut server, token) = start_server(&ui_server(), data.path(), port, &ui_assets()).await?;
    client
        .goto(&format!("{address}/tasks/time-export#session={token}"))
        .await?;
    task_tab(client, "details").await?;
    tempo_export_scenario(client, http, &address, &token).await?;
    client
        .goto(&format!("{address}/tasks/jira-recovery"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return (location.pathname.startsWith('/tasks/') && !!document.querySelector('#task-editor')) && document.querySelector('#load-jira-transitions')?.disabled === true && document.querySelector('#reconcile-jira-transition')?.disabled === false && document.querySelector('#dismiss-jira-transition')?.disabled === true;",vec![]).await?;
    click(client, "#dismiss-jira-ack").await?;
    wait_dom(
        client,
        "return document.querySelector('#dismiss-jira-transition')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#reconcile-jira-transition").await?;
    // No credential is stored: the reconciliation must fail before any remote request.
    wait_dom(client,"return document.querySelector('#jira-task-heading')?.parentElement.querySelector(':scope > .error')?.textContent.includes('Renseigne le jeton Jira') && document.querySelector('#dismiss-jira-transition')?.disabled === true;",vec![]).await?;
    let response = http
        .delete(format!("{address}/api/cards/jira-recovery"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"confirmed":true}))
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::CONFLICT {
        return Err("La suppression a accepté une transition Jira incertaine.".into());
    }
    let response = http
        .post(format!("{address}/api/backups"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::CONFLICT {
        return Err("La sauvegarde a accepté une transition Jira incertaine.".into());
    }
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let card = board
        .cards
        .iter()
        .find(|card| card.id == "jira-recovery")
        .ok_or("Carte Jira synthétique absente.")?;
    let intent = card
        .fields
        .get::<Document>("jiraTransition")?
        .ok_or("Intention Jira synthétique absente.")?;
    if !intent.string_equals("state", "uncertain")? {
        return Err("La vérification sans identifiant a perdu l’incertitude Jira.".into());
    }
    client
        .goto(&format!("{address}/tasks/jira-creation"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#jira-create-heading')?.parentElement.textContent.includes('Création Jira incertaine') && document.querySelector('#close-jira-creation')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css("#jira-created-key"))
        .await?
        .send_keys("SYN-9")
        .await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(
        client,
        "return document.querySelector('#jira-created-key')?.value === 'SYN-9';",
        vec![],
    )
    .await?;
    click(client, "#resolve-jira-creation").await?;
    // The absent synthetic credential prevents all remote I/O, even during reconciliation.
    wait_dom(client,"return document.querySelector('#jira-create-error')?.textContent.includes('Renseigne le jeton Jira') && document.querySelector('#jira-created-key')?.value === 'SYN-9' && document.querySelector('#resolve-jira-creation')?.disabled === false;",vec![]).await?;
    click(client, "#close-jira-creation-ack").await?;
    click(client, "#close-jira-creation").await?;
    wait_dom(client,"return document.querySelector('#jira-create-notice')?.textContent.includes('Aucun ticket distant supprimé') && document.querySelector('#jira-created-key')?.value === '' && document.querySelector('#jira-create-project')?.matches(':disabled') === true;",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/jira-creation"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#jira-create-heading')?.parentElement.textContent.includes('Incertitude Jira close') && document.querySelector('#edit-description')?.value === 'Keep local creation content';",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/jira-refresh"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#edit-title')?.value === 'Local refresh title' && document.querySelector('#apply-jira-refresh')?.disabled === true && document.querySelector('#confirm-jira-refresh')?.disabled === false;",vec![]).await?;
    click(client, "#confirm-jira-refresh").await?;
    click(client, "#apply-jira-refresh").await?;
    wait_dom(client,"return document.querySelector('#edit-title')?.value === 'Remote refresh title' && document.querySelector('#edit-description')?.value === 'Remote refresh description é😀' && document.querySelector('#edit-instructions')?.value === 'Keep refresh instructions' && document.querySelector('.criterion-done')?.checked === true && document.querySelector('#close-editor')?.disabled === false && document.querySelector('#jira-refresh-heading')?.parentElement.textContent.includes('Carte synchronisée');",vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#manage-settings").await?;
    click(client, "#settings-tab-tickets").await?;
    choose_integration(client, "ticketProvider", "jira").await?;
    show_board(client).await?;
    click(client, "#sync-button").await?;
    wait_dom(client,"return document.querySelector('#settings-panel-tickets')?.hidden === false && !document.querySelector('#jira-batch-heading');",vec![]).await?;
    click(client, "#overview-button").await?;
    client
        .goto(&format!("{address}/tasks/jira-refresh"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(
        client,
        "return document.querySelector('#edit-title')?.value === 'Remote refresh title';",
        vec![],
    )
    .await?;
    time_task_scenario(client, http, &address, &token).await?;
    forge_task_scenario(client, &address).await?;
    brainstorm_scenario(client, http, &address, &token, data.path()).await?;
    stop_server(&mut server, port, &token).await?;
    Ok(())
}

async fn tempo_export_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    let first = "[data-time-id='00000000-0000-4000-8000-000000000001']";
    let second = "[data-time-id='00000000-0000-4000-8000-000000000002']";
    wait_dom(client,"return document.querySelector('.send-time-export')?.disabled === true && document.querySelector('.reconcile-time-export')?.offsetParent !== null;",vec![]).await?;
    click(client, &format!("{first} .confirm-time-export")).await?;
    click(client, &format!("{first} .send-time-export")).await?;
    // No Tempo credential is stored: refuse before any Jira or Tempo HTTP request.
    wait_dom(client,"const entry=document.querySelector(arguments[0]);return entry?.querySelector('.time-export-error')?.textContent.includes('Renseigne le jeton Tempo') && entry?.querySelector('.send-time-export')?.disabled === true;",vec![json!(first)]).await?;
    client
        .find(Locator::Css("#time-description"))
        .await?
        .send_keys("Keep local time draft")
        .await?;
    click(client, &format!("{second} .reconcile-time-export")).await?;
    wait_dom(client,"return document.querySelector(arguments[0])?.querySelector('.time-export-error')?.textContent.includes('Renseigne le jeton Tempo') && document.querySelector('#time-description')?.value === 'Keep local time draft';",vec![json!(second)]).await?;
    let response=http.patch(format!("{address}/api/cards/time-export")).bearer_auth(token).header("x-local-request","1").json(&json!({"description":"Must be refused","expected":{"description":"Keep local export content"}})).send().await?;
    if response.status() != reqwest::StatusCode::CONFLICT {
        return Err("Une mutation a accepté un export Tempo incertain.".into());
    }
    clear_input(client, "#time-description").await?;
    click(client, &format!("{second} .open-time-export-reset")).await?;
    wait_dom(client,"return document.querySelector('[data-time-id=\"00000000-0000-4000-8000-000000000002\"] .reset-time-export')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css(&format!("{second} .time-export-evidence")))
        .await?
        .send_keys("Synthetic manual decision only; no remote effect qualified")
        .await?;
    wait_dom(client,"return document.querySelector(arguments[0])?.querySelector('.time-export-evidence')?.closest('dialog')?.matches(':modal') && location.pathname === '/tasks/time-export';",vec![json!(second)]).await?;
    wait_dom(client,"return document.querySelector('[data-time-id=\"00000000-0000-4000-8000-000000000002\"] .time-export-evidence')?.value.includes('Synthetic manual decision') && document.querySelector('[data-time-id=\"00000000-0000-4000-8000-000000000002\"] .reset-time-export')?.disabled === true;",vec![]).await?;
    click(client, &format!("{second} .time-export-risk")).await?;
    click(client, &format!("{second} .reset-time-export")).await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('.time-export-notice')).some(element=>element.textContent.includes('historique incertain est conservé')) && document.querySelector('#edit-description')?.value === 'Keep local export content';",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let entry = board
        .times
        .iter()
        .find(|entry| {
            entry
                .string_equals("id", "00000000-0000-4000-8000-000000000002")
                .unwrap_or(false)
        })
        .ok_or("Temps synthétique absent.")?;
    if !entry.string_equals("exportState", "draft")?
        || entry
            .get::<Vec<serde_json::Value>>("tempoExportHistory")?
            .is_none_or(|history| history.len() != 1)
        || entry.raw("tempoId").is_some()
    {
        return Err(
            "La décision manuelle a perdu l’historique ou inventé une preuve Tempo.".into(),
        );
    }
    Ok(())
}

async fn forge_task_scenario(client: &Client, address: &str) -> Result<()> {
    // The link is seeded explicitly; this journey proves local UI guards, not remote identity.
    client
        .goto(&format!("{address}/tasks/jira-refresh"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#forge-task-heading')?.parentElement.textContent.includes('Synthetic linked PR') && document.querySelector('#detach-task-forge')?.disabled === true && document.querySelector('#edit-title')?.readOnly === true && document.querySelector('#edit-description')?.readOnly === true;",vec![]).await?;
    client
        .find(Locator::Css("#forge-content-title"))
        .await?
        .send_keys(" Human content draft")
        .await?;
    click(client, "#close-editor").await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(client,"return document.querySelector('#forge-content-title')?.value.endsWith(' Human content draft') && document.querySelector('#save-forge-content')?.disabled === true;",vec![]).await?;
    click(client, "#reset-forge-content").await?;
    let instructions = editable(client, "#edit-instructions")
        .await?
        .prop("value")
        .await?
        .ok_or("Synthetic instructions absent")?;
    editable(client, "#edit-instructions")
        .await?
        .send_keys(" Synthetic checks draft")
        .await?;
    wait_dom(
        client,
        "return document.querySelector('#read-forge-checks')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#read-forge-checks").await?;
    // A metadata read remains usable with a local draft; no forge connection means no remote request.
    wait_dom(client,"return document.querySelector('#forge-checks-error')?.textContent.length > 0 && document.querySelector('#edit-instructions')?.value.endsWith(' Synthetic checks draft');",vec![]).await?;
    editable(client, "#edit-instructions")
        .await?
        .clear()
        .await?;
    editable(client, "#edit-instructions")
        .await?
        .send_keys(&instructions)
        .await?;
    clear_input(client, "#task-forge-repository").await?;
    client
        .find(Locator::Css("#task-forge-repository"))
        .await?
        .send_keys("owner/draft")
        .await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(
        client,
        "return document.querySelector('#task-forge-repository')?.value === 'owner/draft';",
        vec![],
    )
    .await?;
    click(client, "#cancel-task-forge").await?;
    wait_dom(
        client,
        "return document.querySelector('#task-forge-repository')?.value === 'owner/repo';",
        vec![],
    )
    .await?;
    wait_dom(
        client,
        "return document.querySelector('#change-forge-draft')?.disabled === true;",
        vec![],
    )
    .await?;
    click(client, "#confirm-forge-draft").await?;
    wait_dom(
        client,
        "return document.querySelector('#change-forge-draft')?.disabled === false;",
        vec![],
    )
    .await?;
    // No forge connection is configured: refusal must happen before any remote I/O.
    click(client, "#change-forge-draft").await?;
    wait_dom(client,"return document.querySelector('#forge-task-error')?.textContent.length > 0 && document.querySelector('#change-forge-draft')?.disabled === true && document.querySelector('#edit-title')?.value === 'Remote refresh title';",vec![]).await?;
    click(client, "#preview-git-push").await?;
    wait_dom(client,"return document.querySelector('#git-push-error')?.textContent.length > 0 && document.querySelector('#preview-git-push')?.disabled === false && document.querySelector('#edit-title')?.value === 'Remote refresh title';",vec![]).await?;
    click(client, "#confirm-detach-forge").await?;
    click(client, "#detach-task-forge").await?;
    wait_dom(client,"return document.querySelector('#forge-task-notice')?.textContent.includes('Aucune PR/MR distante modifiée') && document.querySelector('#edit-title')?.value === 'Remote refresh title' && document.querySelector('.criterion-done')?.checked === true;",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/jira-refresh"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#forge-task-heading') && !document.querySelector('#forge-task-heading')?.parentElement.querySelector('a') && document.querySelector('#edit-title')?.value === 'Remote refresh title';",vec![]).await?;
    client
        .find(Locator::Css("#publication-repository"))
        .await?
        .send_keys("owner/repo")
        .await?;
    wait_dom(
        client,
        "return document.querySelector('#task-forge-kind')?.matches(':disabled') === true;",
        vec![],
    )
    .await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    click(client, "#preview-publication").await?;
    // Missing synthetic credentials must refuse before Git or any remote request.
    wait_dom(client,"return document.querySelector('#publication-error')?.textContent.length > 0 && document.querySelector('#publication-repository')?.value === 'owner/repo' && document.querySelector('#edit-title')?.value === 'Remote refresh title';",vec![]).await?;
    click(client, "#cancel-publication").await?;
    wait_dom(client,"return document.querySelector('#publication-repository')?.value === '' && document.querySelector('#task-forge-kind')?.matches(':disabled') === false;",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/publication-branch"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#confirm-publication-push')?.matches(':disabled') === false && document.querySelector('#create-publication')?.offsetParent === null && document.querySelector('#publication-title')?.value === 'Synthetic first publication';",vec![]).await?;
    click(client, "#confirm-publication-push").await?;
    wait_dom(
        client,
        "return document.querySelector('#push-publication-branch')?.disabled === false;",
        vec![],
    )
    .await?;
    clear_input(client, "#publication-title").await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .send_keys("Changed publication")
        .await?;
    clear_input(client, "#publication-title").await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .send_keys("Synthetic first publication")
        .await?;
    wait_dom(client,"return document.querySelector('#confirm-publication-push')?.checked === false && document.querySelector('#push-publication-branch')?.disabled === true;",vec![]).await?;
    click(client, "#confirm-publication-push").await?;
    click(client, "#push-publication-branch").await?;
    // No forge connection exists, so this refuses before Git or network I/O.
    wait_dom(client,"return document.querySelector('#publication-error')?.textContent.length > 0 && document.querySelector('#publication-body')?.value === 'Keep first publication draft' && document.querySelector('#edit-description')?.value === 'Human branch content';",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/publication-approval"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(client,"return document.querySelector('#approve-automation')?.disabled === false && document.querySelector('#publish-workflow')?.disabled === true;",vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#approve-automation").await?;
    wait_dom(
        client,
        "return document.querySelector('#publish-workflow')?.disabled === true;",
        vec![],
    )
    .await?;
    click(client, "#approve-workflow-branch").await?;
    wait_dom(
        client,
        "return document.querySelector('#publish-workflow')?.disabled === false;",
        vec![],
    )
    .await?;
    task_tab(client, "details").await?;
    clear_input(client, "#publication-title").await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .send_keys("Changed approval draft")
        .await?;
    wait_dom(client,"return document.querySelector('#approve-automation')?.checked === false && document.querySelector('#approve-workflow-branch')?.checked === false;",vec![]).await?;
    clear_input(client, "#publication-title").await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .send_keys("Synthetic approved publication")
        .await?;
    wait_dom(client,"return document.querySelector('#publish-workflow')?.disabled === true && document.querySelector('#approve-automation')?.checked === false && document.querySelector('#approve-workflow-branch')?.checked === false;",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/commit-draft"))
        .await?;
    task_tab(client, "details").await?;
    wait_dom(
        client,
        "return document.querySelector('#commit-author-name')?.matches(':disabled') === false;",
        vec![],
    )
    .await?;
    click(client, "#commit-author-name").await?;
    client
        .find(Locator::Css("#commit-author-name"))
        .await?
        .send_keys("Synthetic author")
        .await?;
    client
        .find(Locator::Css("#commit-message"))
        .await?
        .send_keys("Keep synthetic commit draft")
        .await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    task_tab(client, "details").await?;
    click(client, "#load-commit-files").await?;
    // Invalid synthetic association refuses before a Git mutation; this is not a native commit proof.
    wait_dom(client,"return document.querySelector('#local-commit-error')?.textContent.length > 0 && document.querySelector('#commit-author-name')?.value === 'Synthetic author' && document.querySelector('#commit-message')?.value === 'Keep synthetic commit draft' && document.querySelector('#edit-description')?.value === 'Human commit content';",vec![]).await?;
    Ok(())
}

async fn brainstorm_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    client
        .goto(&format!("{address}/tasks/review-report"))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return document.querySelector('#review-summary')?.textContent.includes('Synthetic review limits') && document.querySelector('.review-criterion')?.textContent.includes('Synthetic review criterion') && document.querySelector('#review-freshness')?.textContent.includes('non revalidée') && document.querySelector('.criterion-done')?.checked === false && !document.querySelector('#review-result script');",vec![]).await?;
    click(client, "#validate-review").await?;
    // No PR is linked: fail locally before credential lookup or a remote request.
    wait_dom(client,"return document.querySelector('#review-result')?.textContent.includes('Liaison de PR/MR retirée') && document.querySelector('#review-freshness')?.textContent.includes('non revalidée') && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    click(client, ".review-selection").await?;
    wait_dom(client,"return document.querySelector('.review-selection')?.checked === true && document.querySelector('#correct-review')?.disabled === true;",vec![]).await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(
        client,
        "return document.querySelector('.review-selection')?.checked === true;",
        vec![],
    )
    .await?;
    click(client, ".review-selection").await?;
    // The completed answer is an explicit fixture, not evidence of a native agent run.
    client
        .goto(&format!("{address}/tasks/brainstorm-session"))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Synthetic structured proposal') && document.querySelector('#convert-brainstorm')?.disabled === true;",vec![]).await?;
    click(client, ".brainstorm-option").await?;
    task_tab(client, "details").await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return document.querySelector('#brainstorm-message')?.value.includes('Synthetic choice? : Local only');",vec![]).await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    clear_input(client, "#brainstorm-message").await?;
    click(client, ".brainstorm-proposal").await?;
    client.execute("const select=document.querySelector('#brainstorm-project');select.value='brainstorm-project';select.dispatchEvent(new Event('change',{bubbles:true}));return true;",vec![]).await?;
    click(client, "#confirm-brainstorm-convert").await?;
    click(client, "#convert-brainstorm").await?;
    wait_dom(client,"return document.querySelector('.brainstorm-proposal')?.disabled === true && document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Propositions converties');",vec![]).await?;
    client
        .goto(&format!("{address}/tasks/brainstorm-session"))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(
        client,
        "return document.querySelector('.brainstorm-proposal')?.disabled === true;",
        vec![],
    )
    .await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let converted: Vec<_> = board
        .cards
        .iter()
        .filter(|card| card.title.clone() == "Synthetic converted task")
        .collect();
    if converted.len() != 1 || !converted[0].fields.string_equals("status", "todo")? {
        return Err("La conversion de brainstorm est absente ou dupliquée.".into());
    }
    let project = board
        .projects
        .iter()
        .find(|project| {
            project
                .string_equals("id", "brainstorm-project")
                .unwrap_or(false)
        })
        .ok_or("Projet synthétique absent après conversion.")?;
    if !atelier_core::domain::projects::same_repository(
        &project
            .get::<String>("repo")?
            .ok_or("Dépôt du projet synthétique absent.")?,
        &converted[0]
            .fields
            .get::<String>("repo")?
            .ok_or("Dépôt de la tâche convertie absent.")?,
        cfg!(windows),
    ) {
        return Err("Le projet synthétique et la tâche convertie doivent utiliser le même chemin de dépôt normalisé.".into());
    }
    let fixture = crate::target()
        .join("debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"preparation-command","name":"Synthetic preparation command","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-command","preparation"],"editArgs":["run-command","preparation"],"expected":null})).send().await?.checked_status().await?;
    http.post(format!("{address}/api/cards/{}/worktree", converted[0].id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"branch":"atelier/synthetic-preparation","base":"main","fetchOrigin":false}))
        .send()
        .await?
        .checked_status()
        .await?;
    client
        .goto(&format!("{address}/tasks/{}", converted[0].id))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return document.querySelector('#task-editor')?.textContent.includes('Keep open question') && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='preparation-command');",vec![]).await?;
    client.execute("for(const [id,value] of [['run-agent','preparation-command'],['run-purpose','preparation']]){const select=document.getElementById(id);select.value=value;select.dispatchEvent(new Event('change',{bubbles:true}));}return true;",vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#preparation-heading')?.parentElement.textContent.includes('Synthetic prepared criterion') && document.querySelector('#stop-run')?.disabled === true && document.querySelector('#apply-preparation')?.disabled === true;",vec![]).await?;
    click(client, "#confirm-preparation").await?;
    click(client, "#apply-preparation").await?;
    wait_dom(client,"return document.querySelector('.criterion-text')?.value === 'Synthetic prepared criterion' && document.querySelector('.criterion-done')?.checked === false && document.querySelector('#preparation-notice')?.textContent.includes('Préparation appliquée') && document.querySelector('#save-task')?.disabled === true;",vec![]).await?;
    task_tab(client, "details").await?;
    editable(client, "#edit-model")
        .await?
        .send_keys("synthetic invalid model")
        .await?;
    click(client, "#save-task").await?;
    wait_dom(client,"return document.querySelector('#editor-error')?.textContent.includes('Modèle invalide') && document.querySelector('#edit-model')?.value === 'synthetic invalid model';",vec![]).await?;
    task_tab(client, "details").await?;
    editable(client, "#edit-model").await?.clear().await?;
    task_tab(client, "details").await?;
    editable(client, "#edit-model")
        .await?
        .send_keys("synthetic-task-model")
        .await?;
    click(client, "#save-task").await?;
    saved_task(client).await?;
    wait_dom(
        client,
        "return !document.querySelector('#task-editor');",
        vec![],
    )
    .await?;
    client
        .goto(&format!("{address}/tasks/{}", converted[0].id))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return document.querySelector('#edit-model')?.value === 'synthetic-task-model' && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    let verification_ready = wait_dom(
        client,
        "return document.querySelector('#start-verification')?.disabled === false;",
        vec![],
    )
    .await;
    if let Err(error) = verification_ready {
        let state: atelier_core::contracts::Board = http
            .get(format!("{address}/api/state"))
            .bearer_auth(token)
            .send()
            .await?
            .checked_status()
            .await?
            .json()
            .await?;
        let card = state.cards.iter().find(|card| card.id == converted[0].id);
        let controls = client.execute("return Object.fromEntries(['save-task','create-worktree','verify-worktree','enqueue-task','start-verification','run-agent','run-mode'].map(id=>{const el=document.getElementById(id);return [id,el?{disabled:el.disabled,value:el.value}:null]}));", vec![]).await?;
        return Err(format!(
            "{error}. Verification task: {card:?}. Projects: {:?}. Controls: {controls:?}",
            state.projects
        )
        .into());
    }
    task_tab(client, "activity").await?;
    click(client, "#start-verification").await?;
    wait_dom(client,"return document.querySelector('#verification-result')?.textContent.includes('Vérification réussie sur cet instantané') && document.querySelector('#stop-run')?.disabled === true && document.querySelector('.criterion-done')?.checked === false && !document.querySelector('.service-log script');",vec![]).await?;
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"queue-command","name":"Synthetic queued command","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-command","success"],"editArgs":["run-command","success"],"expected":null})).send().await?.checked_status().await?;
    http.patch(format!("{address}/api/cards/{}", converted[0].id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"agent":"queue-command","expected":{"agent":"codex"}}))
        .send()
        .await?
        .checked_status()
        .await?;
    client
        .goto(&format!("{address}/tasks/{}", converted[0].id))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(
        client,
        "return document.querySelector('#edit-agent')?.value === 'queue-command' && document.querySelector('#enqueue-task')?.disabled === false;",
        vec![],
    )
    .await?;
    task_tab(client, "activity").await?;
    click(client, "#enqueue-task").await?;
    wait_dom(
        client,
        "return document.querySelector('#close-editor')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#close-editor").await?;
    show_board(client).await?;
    click(client, "#agents").await?;
    wait_dom(
        client,
        "return document.querySelector('#run-queue summary')?.textContent.includes('(1)');",
        vec![],
    )
    .await?;
    click(client, "#run-queue summary").await?;
    click(client, "#confirm-resume-queue").await?;
    click(client, "#resume-queue").await?;
    wait_dom(
        client,
        "return document.querySelector('#run-queue summary')?.textContent.includes('(0)');",
        vec![],
    )
    .await?;
    client
        .goto(&format!("{address}/tasks/{}", converted[0].id))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return document.querySelector('#approve-automation')?.disabled === false && document.querySelector('#automation')?.textContent.includes('Synthetic queued command');",vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#approve-automation").await?;
    click(client, "#start-automation").await?;
    wait_dom(client,"return document.querySelector('#automation')?.textContent.includes('Publication à confirmer') && document.querySelector('#edit-status')?.value === 'standby' && document.querySelector('#stop-run')?.disabled === true && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#approve-automation").await?;
    click(client, "#start-automation").await?;
    wait_dom(client,"return document.querySelector('#automation-error')?.textContent.includes('Publie ou lie une PR/MR') && document.querySelector('#approve-automation')?.checked === false;",vec![]).await?;
    let before: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let task = before
        .cards
        .iter()
        .find(|card| card.id == converted[0].id)
        .ok_or("Synthetic workflow card missing")?;
    let worktree = task
        .fields
        .get::<atelier_core::contracts::Worktree>("worktree")?
        .ok_or("Synthetic workflow worktree missing")?;
    let directory = std::fs::canonicalize(&worktree.path)?;
    let fixture_root = std::fs::canonicalize(data)?;
    if !directory.starts_with(&fixture_root)
        || std::fs::read_to_string(directory.join(".run-fixture"))? != "atelier-r03-synthetic-only"
    {
        return Err("Synthetic workflow directory scope required".into());
    }
    let metadata = std::fs::read_to_string(directory.join(".git"))?;
    let git_dir = Path::new(
        metadata
            .trim()
            .strip_prefix("gitdir: ")
            .ok_or("Synthetic worktree metadata missing")?,
    );
    let git_dir = std::fs::canonicalize(if git_dir.is_absolute() {
        git_dir.to_owned()
    } else {
        directory.join(git_dir)
    })?;
    if !git_dir.starts_with(&fixture_root) {
        return Err("Synthetic Git metadata outside fixture".into());
    }
    let index = std::fs::read(git_dir.join("index"))?;
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("workflow-synthetic-change.txt"))?;
        file.write_all(b"Synthetic workflow commit, no user data.\n")?;
        file.sync_all()?;
    }
    task_tab(client, "details").await?;
    click(client, "#load-commit-files").await?;
    wait_dom(client,"return document.querySelector('[data-commit-path=\"workflow-synthetic-change.txt\"]') !== null;",vec![]).await?;
    click(
        client,
        "[data-commit-path=\"workflow-synthetic-change.txt\"]",
    )
    .await?;
    for (selector, text) in [
        ("#commit-author-name", "Synthetic workflow author"),
        ("#commit-author-email", "synthetic@example.invalid"),
        ("#commit-message", "feat: record synthetic workflow content"),
    ] {
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(text)
            .await?;
    }
    click(client, "#preview-local-commit").await?;
    wait_dom(client,"return document.querySelector('#confirm-local-commit')?.matches(':disabled') === false && document.querySelector('#workflow-local-commit')?.offsetParent !== null;",vec![]).await?;
    click(client, "#confirm-local-commit").await?;
    click(client, "#workflow-local-commit").await?;
    wait_dom(client,"return document.querySelector('#automation')?.textContent.includes('Publication à confirmer') && document.querySelector('#edit-status')?.value === 'standby' && document.querySelector('#commit-author-name')?.value === '' && document.querySelector('#stop-run')?.disabled === true && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    let committed: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let task = committed
        .cards
        .iter()
        .find(|card| card.id == converted[0].id)
        .ok_or("Committed workflow card missing")?;
    let effect = task
        .fields
        .get::<atelier_core::contracts::LocalCommitIntent>("localCommit")?
        .ok_or("Workflow commit effect missing")?;
    let flow = task
        .fields
        .get::<atelier_core::application::automation::Flow>("automation")?
        .ok_or("Workflow missing after commit")?;
    let verified = committed
        .runs
        .iter()
        .find(|run| run.get::<String>("id").ok().flatten() == flow.verification_id)
        .ok_or("Workflow verification run missing")?;
    if effect.state != atelier_core::contracts::ForgeWriteState::Confirmed
        || effect.commit.is_none()
        || effect.observed != effect.commit
        || !verified.string_equals("verificationState", "passed")?
        || std::fs::read(git_dir.join("index"))? != index
        || std::fs::read(directory.join("workflow-synthetic-change.txt"))?.as_slice()
            != b"Synthetic workflow commit, no user data.\n"
    {
        return Err(
            "Synthetic workflow commit or verification incomplete, or human index changed".into(),
        );
    }
    task_tab(client, "activity").await?;
    click(client, "#enqueue-workflow").await?;
    wait_dom(
        client,
        "return document.querySelector('#close-editor')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#close-editor").await?;
    show_board(client).await?;
    click(client, "#agents").await?;
    wait_dom(client,"return document.querySelector('#run-queue summary')?.textContent.includes('(1)') && document.querySelector('#run-queue .remove-queued-task')?.disabled === true;",vec![]).await?;
    let queued: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let queue = queued
        .settings
        .get::<atelier_core::contracts::RunQueue>("runQueue")?
        .ok_or("Workflow queue missing")?;
    if queue.active.as_ref().map(|active| active.run_id.as_str()) != Some(flow.id.as_str())
        || queued.runs.len() != committed.runs.len()
    {
        return Err("Queue replaced or relaunched the waiting workflow".into());
    }
    client
        .goto(&format!("{address}/tasks/{}", converted[0].id))
        .await?;
    task_tab(client, "activity").await?;
    click(client, "#stop-automation").await?;
    wait_dom(client,"return document.querySelector('#automation')?.textContent.includes('Parcours arrêté') && document.querySelector('#stop-automation')?.hidden === true;",vec![]).await?;
    client.refresh().await?;
    wait_dom(client,"return document.querySelector('#automation')?.textContent.includes('Parcours arrêté') && document.querySelector('#approve-automation')?.checked === false;",vec![]).await?;
    click(client, "#close-editor").await?;
    show_board(client).await?;
    click(client, "#agents").await?;
    click(client, "#run-queue summary").await?;
    wait_dom(
        client,
        "return document.querySelector('#run-queue .remove-queued-task')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#run-queue .remove-queued-task").await?;
    wait_dom(
        client,
        "return document.querySelector('#run-queue summary')?.textContent.includes('(0)');",
        vec![],
    )
    .await?;
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"brainstorm-command","name":"Synthetic brainstorm command","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-command","brainstorm"],"editArgs":["run-command","brainstorm"],"expected":null})).send().await?.checked_status().await?;
    http.post(format!("{address}/api/cards/brainstorm-session/worktree"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"branch":"atelier/synthetic-brainstorm","base":"main","fetchOrigin":false}))
        .send()
        .await?
        .checked_status()
        .await?;
    client
        .goto(&format!("{address}/tasks/brainstorm-session"))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='brainstorm-command');",vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent');select.value='brainstorm-command';select.dispatchEvent(new Event('change',{bubbles:true}));return true;",vec![]).await?;
    for message in ["First synthetic question", "Second synthetic question"] {
        client
            .find(Locator::Css("#brainstorm-message"))
            .await?
            .send_keys(message)
            .await?;
        task_tab(client, "activity").await?;
        click(client, "#start-run").await?;
        wait_dom(client,"return document.querySelector('#brainstorm-message')?.value==='' && document.querySelector('#stop-run')?.disabled===true && document.querySelector('#runs-heading')?.parentElement.textContent.includes('Exécution terminée') && document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Synthetic agent conversation reply');",vec![]).await?;
    }
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"empty-brainstorm-command","name":"Synthetic empty brainstorm","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-empty-brainstorm","synthetic-only"],"editArgs":["run-empty-brainstorm","synthetic-only"],"expected":null})).send().await?.checked_status().await?;
    let empty: atelier_core::contracts::Card = http
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic idea without repo","repo":""}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    client
        .goto(&format!("{address}/tasks/{}", empty.id))
        .await?;
    task_tab(client, "activity").await?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='empty-brainstorm-command') && document.querySelector('#task-editor')?.textContent.includes('Conversation sans dépôt');",vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent');select.value='empty-brainstorm-command';select.dispatchEvent(new Event('change',{bubbles:true}));return true;",vec![]).await?;
    client
        .find(Locator::Css("#brainstorm-message"))
        .await?
        .send_keys("Explore this synthetic idea")
        .await?;
    task_tab(client, "activity").await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#brainstorm-message')?.value==='' && document.querySelector('#stop-run')?.disabled===true && document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Synthetic repository-free reply');",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let record = board
        .runs
        .iter()
        .find(|run| run.string_equals("cardId", &empty.id).unwrap_or(false))
        .ok_or("Exécution de brainstorm sans dépôt absente.")?;
    let scratch = record
        .get::<String>("repo")?
        .ok_or("Dossier temporaire absent.")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Path::new(&scratch).exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if Path::new(&scratch).exists() {
        return Err("Le dossier temporaire du brainstorm est encore présent après l’arrêt.".into());
    }
    Ok(())
}

async fn time_task_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    for (selector, value) in [
        ("#time-duration", "00:01:30"),
        ("#time-description", "Synthetic human work"),
    ] {
        client.find(Locator::Css(selector)).await?.clear().await?;
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(value)
            .await?;
    }
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(
        client,
        "return document.querySelector('#time-description')?.value === 'Synthetic human work';",
        vec![],
    )
    .await?;
    click(client, "#save-time").await?;
    wait_dom(client,"return document.querySelectorAll('[data-time-id]').length === 1 && document.querySelector('#time-description')?.value === '' && document.querySelector('#close-editor')?.disabled === false;",vec![]).await?;
    click(client, "[data-time-id] .edit-time").await?;
    clear_input(client, "#time-duration").await?;
    client
        .find(Locator::Css("#time-duration"))
        .await?
        .send_keys("00:02:00")
        .await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let entry = board.times.first().ok_or("Temps synthétique absent.")?;
    let id = entry
        .get::<String>("id")?
        .ok_or("Identifiant temps absent.")?;
    let date = entry
        .get::<String>("date")?
        .ok_or("Date du temps absente.")?;
    let mut update = serde_json::to_value(entry)?;
    update["expected"] = serde_json::to_value(entry)?;
    update["description"] = json!("Concurrent synthetic note");
    http.patch(format!("{address}/api/time/{id}"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&update)
        .send()
        .await?
        .checked_status()
        .await?;
    click(client, "#save-time").await?;
    wait_dom(client,"return document.querySelector('#time-error')?.textContent.includes('a changé') && document.querySelector('#time-duration')?.value === '00:02:00';",vec![]).await?;
    click(client, "#reset-time-draft").await?;
    click(client, "#discard-time-draft").await?;
    click(client, "[data-time-id] .edit-time").await?;
    wait_dom(client,"return document.querySelector('#time-description')?.value === 'Concurrent synthetic note';",vec![]).await?;
    clear_input(client, "#time-duration").await?;
    client
        .find(Locator::Css("#time-duration"))
        .await?
        .send_keys("00:02:00")
        .await?;
    click(client, "#save-time").await?;
    wait_dom(client,"return document.querySelector('[data-time-id]')?.textContent.includes('00:02:00') && document.querySelector('#time-description')?.value === '';",vec![]).await?;
    client
        .find(Locator::Css("#time-description"))
        .await?
        .send_keys("Synthetic timer")
        .await?;
    click(client, "#start-human-timer").await?;
    wait_dom(client,"return document.querySelector('#human-timer-in-task')?.hidden === false && document.querySelectorAll('[data-time-id]').length === 2 && document.querySelector('#close-editor')?.disabled === false;",vec![]).await?;
    click(client, "#close-editor").await?;
    new_task(client).await?;
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys("Unsaved synthetic idea")
        .await?;
    click(client, "#new-dialog .ui-modal-header [data-close]").await?;
    click(client, "#open-timer-task").await?;
    wait_dom(client,"return (location.pathname.startsWith('/tasks/') && !!document.querySelector('#task-editor')) && document.querySelector('#task-title')?.value==='Unsaved synthetic idea';",vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#card-open-jira-recovery").await?;
    editable(client, "#edit-instructions")
        .await?
        .send_keys("Unsaved synthetic task draft")
        .await?;
    click(client, "#open-timer-task-in-task").await?;
    wait_dom(client,"return document.querySelector('#edit-instructions')?.value==='Unsaved synthetic task draft' && document.querySelector('#cancel-confirmation')?.offsetParent !== null;",vec![]).await?;
    click(client, "#cancel-confirmation").await?;
    click(client, "#open-timer-task-in-task").await?;
    click(client, "#confirm-editor").await?;
    wait_dom(client,"return document.querySelector('#edit-title')?.value==='Remote refresh title' && document.querySelector('#task-title')?.value==='Unsaved synthetic idea';",vec![]).await?;
    click(client, "#stop-human-timer-in-task").await?;
    wait_dom(
        client,
        "return document.querySelector('#human-timer-in-task')?.hidden === true;",
        vec![],
    )
    .await?;
    click(client, "[data-time-id] .delete-time").await?;
    click(client, "#confirm-delete-time").await?;
    wait_dom(client,"return document.querySelectorAll('[data-time-id]').length === 1 && document.querySelector('#close-editor')?.disabled === false;",vec![]).await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#task-title')?.value==='Unsaved synthetic idea';",
        vec![],
    )
    .await?;
    new_task(client).await?;
    clear_input(client, "#task-title").await?;
    click(client, "#new-dialog .ui-modal-header [data-close]").await?;
    click(client, "#view-time-report").await?;
    wait_dom(client,"return (location.pathname === '/time' && !!document.querySelector('#time-report')) && document.querySelector('#report-human-total')?.textContent.includes('02:02:00') && document.querySelectorAll('.time-report-row').length===3;",vec![]).await?;
    let set_dates = "for(const id of ['report-from','report-to']){const input=document.getElementById(id);input.value=arguments[0];input.dispatchEvent(new Event('input',{bubbles:true}));}";
    client.execute(set_dates, vec![json!(date)]).await?;
    wait_dom(client,"return document.querySelector('#report-human-total')?.textContent.includes('00:02:00') && document.querySelectorAll('.time-report-row').length===1;",vec![]).await?;
    client.refresh().await?;
    wait_dom(client,"return (location.pathname === '/time' && !!document.querySelector('#time-report')) && document.querySelector('#report-from')?.value==='' && document.querySelector('#report-to')?.value==='' && document.querySelector('#report-human-total')?.textContent.includes('02:02:00') && document.querySelectorAll('.time-report-row').length===3;",vec![]).await?;
    client.execute(set_dates, vec![json!(date)]).await?;
    client.execute("const source=document.querySelector('#report-source');source.value='agent';source.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.querySelectorAll('.time-report-row').length===0 && document.querySelector('#download-time-csv')?.disabled===true;",vec![]).await?;
    client.execute(set_dates, vec![json!("")]).await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('#agent-usage-report tbody tr')).some(row=>row.textContent.includes('synthetic-usage-model')&&row.cells[2].textContent.includes('11 (1/1)')&&row.cells[3].textContent==='Indisponible');",vec![]).await?;
    client.execute(set_dates, vec![json!(date)]).await?;
    client.execute("const source=document.querySelector('#report-source');source.value='human';source.dispatchEvent(new Event('change',{bubbles:true}));window.syntheticCsvHref='';window.syntheticCsvBlob=null;window.syntheticCreateObjectURL=URL.createObjectURL;URL.createObjectURL=function(blob){window.syntheticCsvBlob=blob;return window.syntheticCreateObjectURL.call(this,blob);};window.syntheticAnchorClick=HTMLAnchorElement.prototype.click;HTMLAnchorElement.prototype.click=function(){if(this.download==='atelier-time.csv'){window.syntheticCsvHref=this.href;return;}return window.syntheticAnchorClick.call(this);};",vec![]).await?;
    wait_dom(client,"return document.querySelector('#report-human-total')?.textContent.includes('00:02:00') && document.querySelectorAll('.time-report-row').length===1;",vec![]).await?;
    click(client, "#download-time-csv").await?;
    let csv=client.execute_async("const done=arguments[arguments.length-1];window.syntheticCsvBlob.text().then(text=>done(text)).catch(()=>done(null));",vec![]).await?;
    client.execute("HTMLAnchorElement.prototype.click=window.syntheticAnchorClick;URL.createObjectURL=window.syntheticCreateObjectURL;delete window.syntheticAnchorClick;delete window.syntheticCreateObjectURL;delete window.syntheticCsvBlob;",vec![]).await?;
    let csv = csv.as_str().ok_or("CSV synthétique absent.")?;
    if !csv.contains("Concurrent synthetic note")
        || !csv.contains("\"120\"")
        || csv.contains("Synthetic timer")
        || csv.contains("Synthetic human time")
    {
        return Err("Le CSV ne correspond pas à la sélection synthétique arrêtée.".into());
    }
    let before: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let before_count = before["times"]
        .as_array()
        .ok_or("Historique des temps absent.")?
        .len();
    click(client, "#add-time").await?;
    wait_dom(client,"return document.querySelector('#time-dialog')?.open && document.querySelector('#time-card')?.disabled === false;",vec![]).await?;
    client.execute("const select=document.querySelector('#time-card');select.value='jira-refresh';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    client.execute("const input=document.querySelector('#time-date');input.value=arguments[0];input.dispatchEvent(new Event('input',{bubbles:true}));",vec![json!(date)]).await?;
    clear_input(client, "#time-duration").await?;
    client
        .find(Locator::Css("#time-duration"))
        .await?
        .send_keys("00:00:05")
        .await?;
    client
        .find(Locator::Css("#time-description"))
        .await?
        .send_keys("Synthetic global time draft")
        .await?;
    click(client, "#close-time-editor").await?;
    wait_dom(client,"return document.querySelector('#time-dialog')?.open && document.querySelector('#discard-time-editor')?.offsetParent !== null && document.querySelector('#time-description')?.value === 'Synthetic global time draft';",vec![]).await?;
    click(client, "#keep-time-editor").await?;
    click(client, "#close-time-editor").await?;
    click(client, "#discard-time-editor").await?;
    wait_dom(
        client,
        "return !document.querySelector('#time-dialog') && location.pathname === '/time';",
        vec![],
    )
    .await?;
    let unchanged: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if unchanged["times"] != before["times"] {
        return Err("Abandonner la saisie globale a modifié les temps enregistrés.".into());
    }
    click(client, "#add-time").await?;
    wait_dom(client,"return document.querySelector('#time-dialog')?.open && document.querySelector('#time-card')?.disabled === false;",vec![]).await?;
    client.execute("const select=document.querySelector('#time-card');select.value='jira-refresh';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    client.execute("const input=document.querySelector('#time-date');input.value=arguments[0];input.dispatchEvent(new Event('input',{bubbles:true}));",vec![json!(date)]).await?;
    clear_input(client, "#time-duration").await?;
    client
        .find(Locator::Css("#time-duration"))
        .await?
        .send_keys("00:00:05")
        .await?;
    client
        .find(Locator::Css("#time-description"))
        .await?
        .send_keys("Synthetic global time saved")
        .await?;
    click(client, "#save-time").await?;
    wait_dom(client,"return !document.querySelector('#time-dialog') && document.querySelector('#report-human-total')?.textContent === '00:02:05' && document.querySelectorAll('.time-report-row').length === 2;",vec![]).await?;
    let saved: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if saved["times"].as_array().map(Vec::len) != Some(before_count + 1) {
        return Err("La saisie globale n’a pas produit une seule entrée.".into());
    }
    let row=client.execute("return Array.from(document.querySelectorAll('.time-report-row')).findIndex(row=>row.querySelector('.entry-description')?.textContent === 'Synthetic global time saved');",vec![]).await?.as_i64().filter(|index|*index>=0).ok_or("Saisie globale absente du rapport.")?;
    click(
        client,
        &format!(
            "#time-report .time-report-row:nth-child({}) .delete-report-time",
            row + 1
        ),
    )
    .await?;
    click(client, "#confirm-report-delete-time").await?;
    wait_dom(client,"return document.querySelector('#report-human-total')?.textContent === '00:02:00' && document.querySelectorAll('.time-report-row').length === 1;",vec![]).await?;
    let removed: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if removed["times"] != before["times"] {
        return Err("Le retrait global n’a pas préservé les autres saisies.".into());
    }
    click(client, "#close-time-report").await?;
    wait_dom(
        client,
        "return location.pathname === '/overview' && !document.querySelector('#time-report');",
        vec![],
    )
    .await?;
    Ok(())
}

async fn profiles_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    wait_dom(client, "return !document.querySelector('#diagnostics-button') && !document.querySelector('#diagnostics-dialog');", vec![]).await?;
    let source = crate::target()
        .join("debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    let native = data.join("agents-ui-native");
    std::fs::create_dir(&native)?;
    let fixture = native.join(format!("synthetic-agent{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(source, &fixture)?;
    std::fs::write(
        native.join(".agent-run-fixture"),
        "atelier-r03-synthetic-only",
    )?;
    let config = data.join("agents-ui-config");
    std::fs::create_dir_all(&config)?;
    let request = |method, path: &str| {
        http.request(method, format!("{address}{path}"))
            .bearer_auth(token)
            .header("x-local-request", "1")
    };
    click(client, "#command-button").await?;
    client
        .find(Locator::Css("#command-query"))
        .await?
        .send_keys("Agents")
        .await?;
    wait_dom(client,"return [...document.querySelectorAll('#command-results .command-result')].some(item => item.textContent.includes('Installation et connexion'));",vec![]).await?;
    client
        .find(Locator::Css("#command-query"))
        .await?
        .send_keys("\u{e007}")
        .await?;
    wait_dom(client,"return document.querySelector('#agents-dialog')?.matches(':modal') && !document.querySelector('#command-dialog');",vec![]).await?;
    wait_dom(client, "return document.querySelector('#agents-dialog')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('[data-profile]').length >= 2;", vec![]).await?;
    click(client, "#new-profile").await?;
    wait_dom(client, "return /^profile-[0-9]+$/.test(document.querySelector('#profile-id')?.value) && !document.querySelector('#profile-advanced')?.open;", vec![]).await?;
    client
        .find(Locator::Css("#profile-name"))
        .await?
        .send_keys("Brouillon à annuler")
        .await?;
    click(client, "#cancel-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client, "return document.querySelector('#profile-name')?.value === '' && document.querySelector('#save-profile')?.disabled;", vec![]).await?;
    click(client, "#profile-advanced > summary").await?;
    clear_input(client, "#profile-id").await?;
    for (selector, text) in [
        ("#profile-id", "ui-api"),
        ("#profile-name", "Profil synthétique <b>é😀</b>"),
    ] {
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(text)
            .await?;
    }
    client
        .find(Locator::Css("#profile-command"))
        .await?
        .send_keys(&fixture.to_string_lossy())
        .await?;
    client
        .find(Locator::Css("#profile-configDir"))
        .await?
        .send_keys(&config.to_string_lossy())
        .await?;
    client.execute("const select=document.querySelector('#profile-auth'); select.value='api'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    client
        .find(Locator::Css("#profile-key"))
        .await?
        .send_keys("synthetic-ui-key")
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client, "return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('#profile-key')?.value === '' && document.querySelector('#profile-key-state')?.textContent.includes('Secret enregistré') && document.querySelector('[data-profile=ui-api]')?.textContent.includes('<b>é😀</b>') && !document.querySelector('[data-profile=ui-api] b');", vec![]).await?;
    wait_dom(
        client,
        "return document.querySelector('#diagnose-profile')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-version')?.textContent === '0.0.7' && document.querySelector('#agent-diagnostic-auth')?.textContent === 'Clé présente, validité non vérifiée.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    click(client, "#replace-profile-key").await?;
    client
        .find(Locator::Css("#profile-key"))
        .await?
        .send_keys("synthetic-ui-replacement-key")
        .await?;
    click(client, "#cancel-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client, "return document.querySelector('#profile-key')?.value === '' && document.querySelector('#profile-key-state')?.textContent.includes('Secret enregistré') && document.querySelector('#save-profile')?.disabled;", vec![]).await?;
    let catalog = request(reqwest::Method::GET, "/api/agent-providers")
        .send()
        .await?
        .checked_status()
        .await?
        .text()
        .await?;
    if catalog.contains("synthetic-ui-key") {
        return Err("La clé synthétique a été exposée dans le catalogue.".into());
    }
    click(client, "#new-profile").await?;
    let second_id = client
        .execute(
            "return document.querySelector('#profile-id').value;",
            vec![],
        )
        .await?;
    client
        .find(Locator::Css("#profile-name"))
        .await?
        .send_keys("Second profil API synthétique")
        .await?;
    client.execute("const select=document.querySelector('#profile-auth'); select.value='api'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    client
        .find(Locator::Css("#profile-key"))
        .await?
        .send_keys("synthetic-second-profile-key")
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client, "return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && [...document.querySelectorAll('[data-profile]')].some(row => row.dataset.profile === arguments[0]);", vec![second_id.clone()]).await?;
    let separate: Vec<serde_json::Value> = request(reqwest::Method::GET, "/api/agent-providers")
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let first = separate
        .iter()
        .find(|profile| profile["id"] == "ui-api")
        .ok_or("Premier profil absent.")?;
    let second = separate
        .iter()
        .find(|profile| profile["id"] == second_id)
        .ok_or("Second profil absent.")?;
    if first["type"] != second["type"]
        || first["apiKeySet"] != true
        || second["apiKeySet"] != true
        || serde_json::to_string(&separate)?.contains("synthetic-second-profile-key")
    {
        return Err("Les profils API doivent rester distincts sans exposer leurs clés.".into());
    }
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(
        client,
        "return document.querySelector('#profiles-notice')?.textContent === 'Profil retiré.';",
        vec![],
    )
    .await?;
    click(client, "[data-profile=ui-api]").await?;
    let profiles: Vec<serde_json::Value> = serde_json::from_str(&catalog)?;
    let original = profiles
        .iter()
        .find(|profile| profile["id"] == "ui-api")
        .ok_or("Profil absent.")?
        .clone();
    let mut external = original.clone();
    external["expected"] = original;
    external["name"] = json!("Nom modifié ailleurs");
    request(reqwest::Method::POST, "/api/agent-providers")
        .json(&external)
        .send()
        .await?
        .checked_status()
        .await?;
    clear_input(client, "#profile-name").await?;
    client
        .find(Locator::Css("#profile-name"))
        .await?
        .send_keys("Brouillon local conservé")
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client, "return document.querySelector('#profiles-error')?.textContent.includes('changé ailleurs') && document.querySelector('#profile-name')?.value === 'Brouillon local conservé';",vec![]).await?;
    click(client, "#reload-profile").await?;
    click(client, "#cancel-profile-action").await?;
    wait_dom(
        client,
        "return document.querySelector('#profile-name')?.value === 'Brouillon local conservé';",
        vec![],
    )
    .await?;
    click(client, "#reload-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return document.querySelector('#profile-name')?.value === 'Nom modifié ailleurs' && document.querySelector('#profiles-error')?.textContent === '' && document.querySelector('#profile-key')?.value === '';",vec![]).await?;
    click(client, "#profile-clear-key").await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('#profile-key-state')?.textContent === 'Aucun secret enregistré.';",vec![]).await?;
    wait_dom(
        client,
        "return document.querySelector('#diagnose-profile')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-auth')?.textContent === 'Aucune clé API enregistrée.' && document.querySelector('#agent-check-next-step')?.textContent.includes('Ajoute une clé API') && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    client.execute("const select=document.querySelector('#profile-auth'); select.value='cli'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    wait_dom(
        client,
        "return document.querySelector('#diagnose-profile')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-auth')?.textContent === 'Connexion CLI présente, accès au service non vérifié.' && !document.querySelector('#agents-dialog')?.textContent.includes('private-output-must-not-be-returned');",vec![]).await?;
    wait_dom(client,"return document.querySelector('#agent-account-quotas')?.textContent.includes('synthetic-plan') && document.querySelector('.agent-quota-value')?.textContent === '75.0 % restants' && !document.querySelector('#agent-account-quotas')?.textContent.includes('private-account-output') && !document.querySelector('#agent-account-quotas')?.textContent.includes('private-quota-output');",vec![]).await?;
    let shifted_clock = client
        .execute(
            "window.__atelierQuotaClock=window.Date.now;const now=window.Date.now();window.Date.now=()=>now+601000;return {delta:window.Date.now()-now,sameClock:Date===window.Date};",
            vec![],
        )
        .await?;
    let expired_quota = if shifted_clock["delta"].as_f64() == Some(601_000.0) {
        wait_dom(client,"return document.querySelector('.agent-quota-value')?.textContent === 'À actualiser' && document.querySelector('.agent-quota-window progress')?.hidden;",vec![]).await
    } else {
        Err(format!("Horloge de quota synthétique non décalée : {shifted_clock}").into())
    };
    client
        .execute(
            "window.Date.now=window.__atelierQuotaClock;delete window.__atelierQuotaClock;",
            vec![],
        )
        .await?;
    expired_quota.map_err(|error| format!("{error}; horloge synthétique : {shifted_clock}"))?;
    let board: atelier_core::contracts::Board = request(reqwest::Method::GET, "/api/state")
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let previous = board
        .settings
        .get::<String>("agent")?
        .ok_or("Agent par défaut absent.")?;
    request(reqwest::Method::PUT, "/api/settings")
        .json(&json!({"agent":"ui-api","expected":{"agent":previous}}))
        .send()
        .await?
        .checked_status()
        .await?;
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return document.querySelector('#profiles-error')?.textContent.includes('fournisseur est utilisé') && !!document.querySelector('[data-profile=ui-api]');",vec![]).await?;
    request(reqwest::Method::PUT, "/api/settings")
        .json(&json!({"agent":previous,"expected":{"agent":"ui-api"}}))
        .send()
        .await?
        .checked_status()
        .await?;
    click(client, "#remove-profile").await?;
    click(client, "#cancel-profile-action").await?;
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil retiré.' && !document.querySelector('[data-profile=ui-api]');",vec![]).await?;
    click(client, "#new-profile").await?;
    client.execute("const select=document.querySelector('#profile-type'); select.value='command'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    clear_input(client, "#profile-id").await?;
    for (selector, text) in [
        ("#profile-id", "ui-command"),
        ("#profile-name", "Commande synthétique"),
        ("#profile-analyzeArgs", "invalid-json"),
    ] {
        client.find(Locator::Css(selector)).await?.clear().await?;
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(text)
            .await?;
    }
    client
        .find(Locator::Css("#profile-command"))
        .await?
        .send_keys(&fixture.to_string_lossy())
        .await?;
    clear_input(client, "#profile-statusArgs").await?;
    client
        .find(Locator::Css("#profile-statusArgs"))
        .await?
        .send_keys(r#"["agent-status","present"]"#)
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-error')?.textContent.includes('liste JSON') && document.querySelector('#profile-analyzeArgs')?.value === 'invalid-json';",vec![]).await?;
    clear_input(client, "#profile-analyzeArgs").await?;
    client
        .find(Locator::Css("#profile-analyzeArgs"))
        .await?
        .send_keys(r#"["--analyze","{model}"]"#)
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && !!document.querySelector('[data-profile=ui-command]');",vec![]).await?;
    click(client, "#close-agents").await?;
    client.refresh().await?;
    wait_dom(
        client,
        "return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    click(client, "#manage-agents").await?;
    wait_dom(client,"return document.querySelector('[data-profile=ui-command]') && document.querySelector('#agents-dialog')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    click(client, "[data-profile=ui-command]").await?;
    wait_dom(client,"return document.querySelector('#profile-analyzeArgs')?.value === '[\"--analyze\",\"{model}\"]' && document.querySelector('#profile-versionArgs')?.value === '[\"--version\"]';",vec![]).await?;
    wait_dom(
        client,
        "return document.querySelector('#diagnose-profile')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-auth')?.textContent === 'Connexion CLI présente, accès au service non vérifié.' && document.querySelector('#agent-diagnostic-result')?.textContent.includes('synthetic@example.invalid') && !document.querySelector('#agent-diagnostic-result')?.textContent.includes('private-output-must-not-be-returned');",vec![]).await?;
    for (selector, arguments, expected) in [
        (
            "#profile-statusArgs",
            r#"["agent-status","absent"]"#,
            "Aucune connexion CLI.",
        ),
        (
            "#profile-statusArgs",
            r#"["agent-status","invalid"]"#,
            "Diagnostic de connexion indisponible.",
        ),
        (
            "#profile-versionArgs",
            r#"["agent-version","flood"]"#,
            "Lecture de version impossible ou interrompue.",
        ),
    ] {
        client.find(Locator::Css(selector)).await?.clear().await?;
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(arguments)
            .await?;
        click(client, "#save-profile").await?;
        wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
        wait_dom(
            client,
            "return document.querySelector('#diagnose-profile')?.disabled === false;",
            vec![],
        )
        .await?;
        click(client, "#diagnose-profile").await?;
        wait_dom(client,"return document.querySelector('#agent-diagnostic-result')?.textContent.includes(arguments[0]) && !document.querySelector('#agent-diagnostic-result')?.textContent.includes('private-output-must-not-be-returned') && document.querySelector('#diagnose-profile')?.disabled === false;",vec![json!(expected)]).await?;
    }
    std::fs::write(
        root().join(".artifacts/rust-ui-agent-profiles.png"),
        client.screenshot().await?,
    )?;
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return !document.querySelector('[data-profile=ui-command]') && document.querySelector('#profiles-notice')?.textContent === 'Profil retiré.';",vec![]).await?;
    click(client, "#new-profile").await?;
    client.execute("const select=document.querySelector('#profile-type'); select.value='acp'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    clear_input(client, "#profile-id").await?;
    for (selector, value) in [
        ("#profile-id", "ui-acp".to_owned()),
        ("#profile-name", "Agent ACP synthétique".to_owned()),
        ("#profile-command", fixture.to_string_lossy().into_owned()),
    ] {
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(&value)
            .await?;
    }
    let arguments = client.find(Locator::Css("#profile-protocolArgs")).await?;
    arguments.clear().await?;
    arguments.send_keys(r#"["acp"]"#).await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('[data-profile=ui-acp]') && document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.';",vec![]).await?;
    wait_dom(
        client,
        "return document.querySelector('#diagnose-profile')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-version')?.textContent === '0.0.7' && document.querySelector('#agent-diagnostic-auth')?.textContent === 'Connexion non vérifiée.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return !document.querySelector('[data-profile=ui-acp]') && document.querySelector('#profiles-notice')?.textContent === 'Profil retiré.';",vec![]).await?;
    click(client, "#new-profile").await?;
    client
        .find(Locator::Css("#profile-name"))
        .await?
        .send_keys("Brouillon à abandonner")
        .await?;
    click(client, "#close-agents").await?;
    click(client, "#cancel-profile-action").await?;
    click(client, "#close-agents").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return !document.querySelector('#agents-dialog') && document.activeElement?.id === 'manage-agents';",vec![]).await?;
    Ok(())
}

async fn live_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    show_board(client).await?;
    let card: atelier_core::contracts::Card = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Ajout depuis une autre fenêtre"}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    wait_title(client, "Ajout depuis une autre fenêtre").await?;
    http.patch(format!("{address}/api/cards/{}", card.id)).bearer_auth(token).header("x-local-request", "1")
        .json(&json!({"title":"Modification depuis une autre fenêtre", "status":"standby", "expected":{"title":"Ajout depuis une autre fenêtre", "status":"todo"}}))
        .send().await?.checked_status().await?;
    wait_dom(client, "return Array.from(document.querySelectorAll('[data-status=standby] article.card')).some(card => card.dataset.id === arguments[0] && card.querySelector('.card-title').textContent === 'Modification depuis une autre fenêtre');", vec![json!(card.id)]).await?;
    http.delete(format!("{address}/api/cards/{}", card.id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await?
        .checked_status()
        .await?;
    wait_dom(client, "return !Array.from(document.querySelectorAll('article.card')).some(card => card.dataset.id === arguments[0]);", vec![json!(card.id)]).await?;
    Ok(())
}

async fn projects_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    let repo = data.join("repositories/project-ui");
    std::fs::create_dir_all(repo.join("client"))?;
    std::fs::write(
        repo.join("client/package.json"),
        r#"{"scripts":{"dev":"vite","predev":"never execute during detection"},"devDependencies":{"vite":"synthetic"}}"#,
    )?;
    click(client, "#manage-projects").await?;
    click(client, "#add-project").await?;
    client
        .find(Locator::Css("#project-create-name"))
        .await?
        .send_keys("Projet depuis l’interface")
        .await?;
    client
        .execute(
            "document.querySelector('#project-form').requestSubmit();",
            vec![],
        )
        .await?;
    wait_dom(client, "return document.querySelector('#project-directory').getAttribute('aria-invalid') === 'true' && document.querySelector('#project-directory-field-error').textContent.trim().length > 0 && document.activeElement.id === 'project-directory';", vec![]).await?;

    client
        .find(Locator::Css("#project-directory"))
        .await?
        .send_keys(&repo.to_string_lossy())
        .await?;
    wait_dom(client,"return document.querySelector('#project-create-access')?.hidden === false && document.querySelector('#project-create-access .authorize-project-repository') && !document.querySelector('#project-edit-access')?.matches(':not([hidden])');",vec![]).await?;
    click(client, "#project-form button[type=submit]").await?;
    wait_dom(client, "return document.querySelectorAll('#project-list .project-choice').length === 2 && !document.querySelector('#project-create-dialog');", vec![]).await?;
    let id = client.execute("return Array.from(document.querySelectorAll('#project-list [data-project-id]')).find(button=>button.querySelector('strong').textContent === 'Projet depuis l’interface').dataset.projectId;", vec![]).await?;
    click(
        client,
        &format!(
            "[data-project-id='{}']",
            id.as_str().ok_or("Projet sans id")?
        ),
    )
    .await?;
    wait_dom(client, "return document.querySelector('#save-project')?.disabled === true && document.querySelector('#add-project')?.disabled === false;", vec![]).await?;
    click(client, "#reset-project").await?;
    wait_dom(client, "return document.querySelector('#project-settings-panel')?.hidden === true && !document.querySelector('#project-confirmation-dialog');", vec![]).await?;
    click(
        client,
        &format!(
            "[data-project-id='{}']",
            id.as_str().ok_or("Projet sans id")?
        ),
    )
    .await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys(" brouillon à annuler")
        .await?;
    click(client, "#add-project").await?;
    wait_dom(
        client,
        "return document.querySelector('#project-confirmation-dialog')?.open === true;",
        vec![],
    )
    .await?;
    click(client, "#cancel-project-confirmation").await?;
    wait_dom(client, "return document.querySelector('#project-name')?.value.endsWith(' brouillon à annuler') && document.querySelector('#add-project')?.disabled === false;", vec![]).await?;
    click(client, "#reset-project").await?;
    click(client, "#confirm-project").await?;
    wait_dom(client, "return document.querySelector('#project-settings-panel')?.hidden === true && document.querySelector('#add-project')?.disabled === false && Array.from(document.querySelectorAll('#project-list .project-choice')).every(button=>!button.disabled);", vec![]).await?;
    click(client, "#add-project").await?;
    wait_dom(
        client,
        "return document.querySelector('#project-create-dialog')?.open === true;",
        vec![],
    )
    .await?;
    click(client, "#project-create-dialog [data-close]").await?;
    click(client, "#close-projects").await?;
    wait_dom(client, "return location.pathname === '/overview';", vec![]).await?;
    click(client, "#manage-projects").await?;
    click(
        client,
        &format!(
            "[data-project-id='{}']",
            id.as_str().ok_or("Projet sans id")?
        ),
    )
    .await?;
    wait_dom(
        client,
        "return document.querySelector('#project-name')?.value === 'Projet depuis l’interface';",
        vec![],
    )
    .await?;
    clear_input(client, "#project-name").await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys("Projet renommé")
        .await?;
    client
        .find(Locator::Css("#project-branch"))
        .await?
        .send_keys("main")
        .await?;
    wait_dom(client, "return document.querySelectorAll('#projects-page button#close-projects').length === 1 && !!(document.querySelector('#projects-page').compareDocumentPosition(document.querySelector('main > footer')) & Node.DOCUMENT_POSITION_FOLLOWING) && Array.from(document.querySelectorAll('#project-list .project-choice')).every(button=>button.querySelector('.project-choice-action')?.textContent.includes('Configurer')) && !document.querySelector('.project-detail-heading button:not(.project-open-tasks)') && document.querySelector('#close-projects')?.textContent === 'Tous les projets';", vec![]).await?;
    click(client, "#project-tab-agent").await?;
    client
        .find(Locator::Css("#project-rules"))
        .await?
        .send_keys("Consignes synthétiques")
        .await?;
    client.execute("const select=document.querySelector('#project-agent'); select.value='claude'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;

    click(client, "#project-tab-general").await?;
    client.execute("window.__syntheticCopy = null; Object.defineProperty(navigator, 'clipboard', {configurable:true, value:{writeText:async value=>{window.__syntheticCopy=value;}}});", vec![]).await?;
    click(
        client,
        "#project-repo + .ui-input-buttons [data-input-action=copy]",
    )
    .await?;
    wait_dom(client, "return window.__syntheticCopy === document.querySelector('#project-repo').value && document.querySelector('#project-repo').closest('.ui-input-actions').querySelector('[role=status]').textContent.trim().length > 0;", vec![]).await?;
    client.execute("Object.defineProperty(navigator, 'clipboard', {configurable:true, value:{writeText:async()=>{throw Error('Synthetic clipboard denied');}}});",vec![]).await?;
    click(
        client,
        "#project-repo + .ui-input-buttons [data-input-action=copy]",
    )
    .await?;
    wait_dom(client, "return document.querySelector('#project-repo').closest('.ui-input-actions').querySelector('[role=status]').textContent.includes('impossible');", vec![]).await?;
    client
        .execute(
            "delete navigator.clipboard; delete window.__syntheticCopy;",
            vec![],
        )
        .await?;
    click(client, "#project-tab-launch").await?;
    click(client, "#detect-front").await?;
    wait_dom(client, "return document.querySelectorAll('.launch-candidate[data-role=front]').length === 1 && !document.querySelector('#project-name')?.disabled && document.querySelector('#project-settings-panel fieldset')?.disabled === false;", vec![]).await?;
    click(client, ".launch-candidate[data-role=front]").await?;
    wait_dom(client, "return document.querySelector('#project-front-directory')?.value === 'client' && document.querySelector('#project-front-command')?.value.includes('--port {port}');", vec![]).await?;
    clear_input(client, "#project-front-port").await?;
    client
        .find(Locator::Css("#project-front-port"))
        .await?
        .send_keys("4300")
        .await?;
    client
        .find(Locator::Css("#project-back-command"))
        .await?
        .send_keys("synthetic-back --port {port}")
        .await?;
    click(client, "#project-tab-automation").await?;
    client
        .find(Locator::Css("#project-linked"))
        .await?
        .send_keys("missing-synthetic-project")
        .await?;
    wait_dom(client, "return document.querySelector('#project-linked').getAttribute('aria-expanded') === 'true' && document.querySelectorAll('#project-linked-listbox [role=option]').length === 0;", vec![]).await?;
    client
        .find(Locator::Css("#project-linked"))
        .await?
        .send_keys("\u{e00c}")
        .await?;
    wait_dom(client, "return document.querySelector('#project-linked').getAttribute('aria-expanded') === 'false' && document.querySelector('#project-linked').dataset.selectedValue === '';", vec![]).await?;

    client
        .execute("document.querySelector('#project-linked').focus(); document.querySelector('.ui-combobox-toggle').click();", vec![])
        .await?;
    wait_dom(
        client,
        "return document.querySelectorAll('#project-linked-listbox [role=option]').length > 1;",
        vec![],
    )
    .await?;
    client
        .execute(
            "document.querySelectorAll('#project-linked-listbox [role=option]')[1].click();",
            vec![],
        )
        .await?;
    client
        .find(Locator::Css("#project-tab-automation"))
        .await?
        .send_keys("\u{e011}")
        .await?;
    wait_dom(client, "return document.activeElement?.id === 'project-tab-general' && document.querySelector('#project-panel-general')?.hidden === false && document.querySelector('#project-panel-launch')?.hidden === true && document.querySelector('#project-name')?.value === 'Projet renommé';", vec![]).await?;
    click(client, "#project-tab-agent").await?;
    wait_dom(client, "return document.querySelector('#project-rules')?.value === 'Consignes synthétiques' && document.querySelector('#project-agent')?.value === 'claude';", vec![]).await?;
    click(client, "#project-tab-general").await?;
    clear_input(client, "#project-name").await?;
    click(client, "#project-tab-agent").await?;
    click(client, "#save-project").await?;
    wait_dom(client, "return document.querySelector('#project-tab-general')?.getAttribute('aria-selected') === 'true' && document.querySelector('#project-name')?.validity.valueMissing && document.querySelector('#save-project')?.disabled === false;", vec![]).await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys("Projet renommé")
        .await?;
    click(client, "#project-tab-agent").await?;
    click(client, "#save-project").await?;
    wait_dom(client, "return document.querySelector('#save-project')?.disabled && document.querySelector('#add-project')?.disabled === false && Array.from(document.querySelectorAll('#project-list strong')).some(element=>element.textContent === 'Projet renommé');", vec![]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-project-editor.png"),
        client.screenshot().await?,
    )?;
    click(client, "#add-project").await?;
    client
        .find(Locator::Css("#project-create-name"))
        .await?
        .send_keys("Doublon refusé")
        .await?;
    client
        .find(Locator::Css("#project-directory"))
        .await?
        .send_keys(&repo.to_string_lossy())
        .await?;
    for _ in 0..2 {
        click(client, "#project-form button[type=submit]").await?;
        wait_dom(client, "const error=document.querySelector('#project-form > .error')?.textContent;return (error?.includes('déjà') || error === 'Une modification est en cours. Réessaie dans un instant.') && document.querySelector('#project-create-name')?.value === 'Doublon refusé';", vec![]).await?;
        if client
            .execute(
                "return document.querySelector('#project-form > .error').textContent.includes('déjà');",
                vec![],
            )
            .await?
            == json!(true)
        {
            break;
        }
        // This explicit admission refusal guarantees no write; verify it before a second user click.
        let state: serde_json::Value = http
            .get(format!("{address}/api/state"))
            .bearer_auth(token)
            .send()
            .await?
            .checked_status()
            .await?
            .json()
            .await?;
        if state["projects"].as_array().is_none_or(|projects| {
            projects.len() != 2
                || projects
                    .iter()
                    .any(|project| project["name"] == "Doublon refusé")
        }) {
            return Err("Le refus de concurrence a modifié les projets.".into());
        }
    }
    wait_dom(client, "return document.querySelector('#project-form > .error')?.textContent.includes('déjà') && document.querySelector('#project-create-name')?.value === 'Doublon refusé';", vec![]).await?;
    click(client, "#project-create-dialog [data-close]").await?;
    click(client, "#project-confirmation-dialog #confirm-project").await?;
    wait_dom(
        client,
        "return !document.querySelector('#project-create-dialog');",
        vec![],
    )
    .await?;
    click(
        client,
        &format!(
            "#project-list [data-project-id='{}']",
            id.as_str().ok_or("Projet sans id")?
        ),
    )
    .await?;
    click(client, ".project-open-tasks").await?;
    wait_dom(client, "return location.pathname === '/overview' && document.querySelector('#board-project')?.selectedOptions[0]?.textContent === 'Projet renommé';", vec![]).await?;
    new_task(client).await?;
    client.execute("const select=document.querySelector('#new-project');select.value=Array.from(select.options).find(option=>option.textContent === 'Projet renommé').value;select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys("Tâche du nouveau projet")
        .await?;
    click(client, "#create-task").await?;
    created_task(client, "Tâche du nouveau projet").await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let project = board
        .projects
        .iter()
        .find(|project| project.get::<String>("id").ok().flatten().as_deref() == id.as_str())
        .ok_or("Projet créé absent")?;
    if !project.string_equals("name", "Projet renommé")?
        || !project.string_equals("projectRules", "Consignes synthétiques")?
        || !project.string_equals("baseBranch", "main")?
        || !project.string_equals("frontDirectory", "client")?
        || project.get::<u16>("frontPort")? != Some(4300)
        || !project.string_equals("backCommand", "synthetic-back --port {port}")?
        || project
            .get::<String>("linkedProjectId")?
            .unwrap_or_default()
            .is_empty()
    {
        return Err("Réglages du projet non enregistrés.".into());
    }
    let card = board
        .cards
        .iter()
        .find(|card| card.title == "Tâche du nouveau projet")
        .ok_or("Tâche du projet absente")?;
    if card.fields.get::<String>("repo")? != project.get::<String>("repo")? {
        return Err("La tâche n’est pas associée au projet sélectionné.".into());
    }
    if !card.fields.string_equals("agent", "claude")? {
        return Err("La tâche n’hérite pas de l’agent du projet.".into());
    }
    let card_id = card.id.clone();
    click(client, "#manage-projects").await?;
    click(
        client,
        &format!(
            "[data-project-id='{}']",
            id.as_str().ok_or("Projet sans id")?
        ),
    )
    .await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys(" brouillon")
        .await?;
    click(client, "#manage-settings").await?;
    wait_dom(client, "return document.querySelector('#project-confirmation-dialog')?.open === true && location.pathname === '/projects';", vec![]).await?;
    click(client, "#cancel-project-confirmation").await?;
    http.patch(format!(
        "{address}/api/projects/{}",
        id.as_str().ok_or("Projet sans id")?
    ))
    .bearer_auth(token)
    .header("x-local-request", "1")
    .json(&json!({"name":"Nom externe", "expected":{"name":"Projet renommé"}}))
    .send()
    .await?
    .checked_status()
    .await?;
    click(client, "#save-project").await?;
    wait_dom(client,"return document.querySelector('#project-error')?.textContent.includes('changé ailleurs') && document.querySelector('#project-name')?.value === 'Projet renommé brouillon';",vec![]).await?;
    click(client, "#close-projects").await?;
    click(client, "#confirm-project").await?;
    click(client, "#close-projects").await?;
    click(client, "#manage-projects").await?;
    click(
        client,
        &format!(
            "[data-project-id='{}']",
            id.as_str().ok_or("Projet sans id")?
        ),
    )
    .await?;
    wait_dom(
        client,
        "return document.querySelector('#project-name')?.value === 'Nom externe';",
        vec![],
    )
    .await?;
    click(client, "#remove-project").await?;
    click(client, "#cancel-project-confirmation").await?;
    click(client, "#remove-project").await?;
    wait_dom(client, "return document.querySelector('#project-confirmation-dialog')?.open === true && document.querySelector('#confirm-project')?.disabled === true;", vec![]).await?;
    client
        .find(Locator::Css("#project-confirmation-name"))
        .await?
        .send_keys("Nom externe")
        .await?;
    click(client, "#archive-project-tasks").await?;
    click(client, "#confirm-project").await?;
    wait_dom(client, "return document.querySelectorAll('#project-list .project-choice').length === 1 && document.querySelector('#project-settings-panel')?.hidden === true && document.querySelector('#close-projects')?.disabled === false;", vec![]).await?;
    click(client, "#close-projects").await?;
    click(client, "#show-archives").await?;
    wait_dom(client, "return location.pathname === '/archives' && Array.from(document.querySelectorAll('#workspace-archives .archive-row')).some(card => card.dataset.id === arguments[0]);", vec![json!(card_id)]).await?;
    if !repo.is_dir() {
        return Err("Le retrait du projet a supprimé son dossier.".into());
    }
    std::fs::write(
        root().join(".artifacts/rust-ui-projects.png"),
        client.screenshot().await?,
    )?;
    Ok(())
}

async fn kanban_columns_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    show_board(client).await?;
    click(client, "#configure-columns").await?;
    wait_dom(
        client,
        "return document.querySelector('#kanban-settings')?.matches(':modal');",
        vec![],
    )
    .await?;
    client.execute("const select=document.querySelector('#kanban-project');select.value=Array.from(select.options).find(option=>option.value).value;select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    let project = client
        .execute(
            "return document.querySelector('#kanban-project').value;",
            vec![],
        )
        .await?;
    client
        .find(Locator::Css("#kanban-name-todo"))
        .await?
        .send_keys("Prêt à démarrer")
        .await?;
    client.execute("document.querySelector('#kanban-name-todo').closest('li').querySelectorAll('button')[1].click();",vec![]).await?;
    click(client, "#save-kanban-columns").await?;
    wait_dom(client,"return document.querySelector('#save-kanban-columns')?.disabled && document.querySelector('#kanban-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    click(client, "#close-kanban-settings").await?;
    wait_dom(client,"return document.querySelector('#prototype-board .column')?.dataset.status === 'doing' && document.querySelector('#prototype-board [data-status=todo] .column-heading')?.textContent.includes('Prêt à démarrer');",vec![]).await?;
    let repo = client
        .execute(
            "return document.querySelector('#board-project').value;",
            vec![],
        )
        .await?;
    client.refresh().await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('#board-project option')).some(option=>option.value===arguments[0]);",vec![repo.clone()]).await?;
    client.execute("const select=document.querySelector('#board-project');select.value=arguments[0];select.dispatchEvent(new Event('change',{bubbles:true}));",vec![repo]).await?;
    wait_dom(client,"return document.querySelector('#prototype-board [data-status=todo] .column-heading')?.textContent.includes('Prêt à démarrer');",vec![]).await?;
    click(client, "#configure-columns").await?;
    client
        .find(Locator::Css("#kanban-name-todo"))
        .await?
        .send_keys(" brouillon")
        .await?;
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let expected = state["projects"]
        .as_array()
        .and_then(|projects| projects.iter().find(|entry| entry["id"] == project))
        .ok_or("Projet Kanban synthétique absent")?["kanban"]
        .clone();
    http.put(format!("{address}/api/projects/{}/kanban",project.as_str().ok_or("Identifiant projet invalide")?)).bearer_auth(token).header("x-local-request","1").json(&json!({"configuration":atelier_core::application::kanban::Configuration::default(),"expected":expected,"jiraRevision":null})).send().await?.checked_status().await?;
    click(client, "#save-kanban-columns").await?;
    wait_dom(client,"return document.querySelector('#kanban-name-todo')?.value === 'Prêt à démarrer brouillon' && document.querySelector('#kanban-settings > .error')?.textContent.includes('modifiées ailleurs') && document.querySelector('#kanban-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    click(client, "#reset-kanban-columns").await?;
    click(client, "#close-kanban-settings").await?;
    wait_dom(client,"return document.querySelector('#prototype-board .column')?.dataset.status === 'todo' && document.activeElement?.id === 'configure-columns';",vec![]).await?;
    Ok(())
}

async fn choose_integration(client: &Client, setting: &str, provider: &str) -> Result<()> {
    wait_dom(client,"return document.querySelector('#'+arguments[0]) && !document.querySelector('#'+arguments[0]).matches(':disabled');",vec![json!(setting)]).await?;
    if client.execute("return document.querySelector('#'+arguments[0]).value === arguments[1] && document.querySelector('#save-'+arguments[0]).disabled;",vec![json!(setting),json!(provider)]).await? == json!(true) {
        return Ok(());
    }
    client.execute("const select=document.querySelector('#'+arguments[0]);select.value=arguments[1];select.dispatchEvent(new Event('change',{bubbles:true}));",vec![json!(setting),json!(provider)]).await?;
    click(client, &format!("#save-{setting}")).await?;
    wait_dom(client,"return document.querySelector('#'+arguments[0])?.value===arguments[1] && document.querySelector('#save-'+arguments[0])?.disabled && !document.querySelector('#'+arguments[0])?.matches(':disabled');",vec![json!(setting),json!(provider)]).await?;
    Ok(())
}

async fn settings_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    let _ = data;
    click(client, "#manage-settings").await?;
    wait_dom(client,"return !document.querySelector('#settings-repo') && !document.querySelector('#settings-branch') && !document.querySelector('#settings-jira-enabled') && !document.querySelector('#jira-batch-heading');",vec![]).await?;
    client.execute("const select=document.querySelector('#settings-agent'); select.value='claude'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#save-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-notice')?.textContent === 'Paramètres enregistrés.' && document.querySelector('#save-settings')?.disabled === true;",vec![]).await?;
    client.refresh().await?;
    wait_dom(client,"return document.querySelector('#settings-agent')?.value === 'claude' && document.querySelector('#save-settings')?.disabled === true;",vec![]).await?;
    client.execute("const select=document.querySelector('#settings-agent'); select.value='codex'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#overview-button").await?;
    click(client, "#keep-settings").await?;
    click(client, "#overview-button").await?;
    click(client, "#discard-settings").await?;
    click(client, "#manage-settings").await?;
    wait_dom(
        client,
        "return document.querySelector('#settings-agent')?.value === 'claude';",
        vec![],
    )
    .await?;
    click(client, "#settings-tab-tickets").await?;
    wait_dom(client,"return document.querySelector('#ticketProvider')?.value === 'local' && !document.querySelector('#settings-tab-jira') && !document.querySelector('#jira-url') && !document.querySelector('#tempo-token');",vec![]).await?;
    click(client, "#settings-tab-tickets").await?;
    client.execute("const select=document.querySelector('#ticketProvider');select.value='jira';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.querySelector('#jira-url')?.offsetParent !== null && document.querySelector('#save-ticketProvider')?.disabled === false;",vec![]).await?;
    let preview_state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if preview_state["settings"]["ticketProvider"] != json!("local") {
        return Err("L’aperçu Jira a enregistré le choix sans confirmation.".into());
    }
    client.execute("const select=document.querySelector('#ticketProvider');select.value='local';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return !document.querySelector('#jira-url') && document.querySelector('#save-ticketProvider')?.disabled === true;",vec![]).await?;
    choose_integration(client, "ticketProvider", "jira").await?;
    wait_dom(
        client,
        "return document.querySelector('#jira-query')?.matches(':disabled') === false;",
        vec![],
    )
    .await?;
    for (selector, value) in [
        ("#jira-url", "https://synthetic-one.example.test"),
        ("#jira-email", "synthetic@example.test"),
        ("#jira-token", "synthetic-jira-one"),
        ("#jira-query", "project = SYN"),
    ] {
        client.find(Locator::Css(selector)).await?.clear().await?;
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(value)
            .await?;
    }
    click(client, "#overview-button").await?;
    click(client, "#keep-settings").await?;
    wait_dom(client,"return document.querySelector('#jira-token')?.value === 'synthetic-jira-one' && document.querySelector('#jira-query')?.value === 'project = SYN';",vec![]).await?;
    client
        .find(Locator::Css("#settings-tab-tickets"))
        .await?
        .send_keys("\u{e014}")
        .await?;
    wait_dom(client,"return document.activeElement?.id === 'settings-tab-forges' && document.querySelector('#settings-panel-tickets')?.hidden === true;",vec![]).await?;
    client
        .find(Locator::Css("#settings-tab-forges"))
        .await?
        .send_keys("\u{e012}")
        .await?;
    wait_dom(client,"return document.activeElement?.id === 'settings-tab-tickets' && document.querySelector('#jira-token')?.value === 'synthetic-jira-one' && document.querySelector('#jira-query')?.value === 'project = SYN';",vec![]).await?;
    click(client, "#save-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-notice')?.textContent === 'Connexion Jira enregistrée.' && document.querySelector('#jira-token')?.value === '' && document.querySelector('#search-jira')?.disabled === false;",vec![]).await?;
    let saved: serde_json::Value = http
        .get(format!("{address}/api/jira/config"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if serde_json::to_string(&saved)?.contains("synthetic-jira-one") {
        return Err("Le jeton Jira apparaît dans la réponse publique.".into());
    }
    clear_input(client, "#jira-query").await?;
    client
        .find(Locator::Css("#jira-query"))
        .await?
        .send_keys("project = LOCAL")
        .await?;
    http.put(format!("{address}/api/jira/config")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":saved["configuration"]["revision"],"mode":"cloud","url":"https://synthetic-one.example.test","email":"synthetic@example.test","query":"project = REMOTE","token":"synthetic-jira-rotated"})).send().await?.checked_status().await?;
    click(client, "#save-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-error')?.textContent.includes('a changé') && document.querySelector('#jira-query')?.value === 'project = LOCAL';",vec![]).await?;
    click(client, "#reload-jira").await?;
    click(client, "#confirm-reload-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-query')?.value === 'project = REMOTE' && document.querySelector('#jira-token')?.value === '' && document.querySelector('#search-jira')?.disabled === false;",vec![]).await?;
    wait_dom(client,"return document.querySelector('#jira-connection-editor')?.open === false && document.querySelector('#jira-connection-summary')?.textContent.includes('https://synthetic-one.example.test') && document.querySelector('#jira-query')?.offsetParent !== null;",vec![]).await?;
    click(client, "#edit-jira-connection").await?;
    wait_dom(
        client,
        "return document.querySelector('#jira-clear-token')?.tagName === 'BUTTON';",
        vec![],
    )
    .await?;
    click(client, "#jira-clear-token").await?;
    wait_dom(client,"return document.querySelector('#jira-clear-token')?.getAttribute('aria-pressed') === 'true' && document.querySelector('#jira-token')?.disabled === true;",vec![]).await?;
    click(client, "#jira-clear-token").await?;
    wait_dom(client,"return document.querySelector('#jira-clear-token')?.getAttribute('aria-pressed') === 'false' && document.querySelector('#jira-token')?.disabled === false;",vec![]).await?;
    click(client, "#jira-clear-token").await?;
    click(client, "#save-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-notice')?.textContent === 'Connexion Jira enregistrée.' && document.querySelector('#search-jira')?.disabled === true && document.querySelector('#save-jira')?.disabled === true;",vec![]).await?;
    click(client, "#settings-tab-forges").await?;
    wait_dom(
        client,
        "return document.querySelector('#forge-base')?.matches(':disabled') === false;",
        vec![],
    )
    .await?;
    for (selector, value) in [
        ("#forge-base", "https://github-synthetic.example.test"),
        ("#forge-token", "synthetic-forge-ui"),
    ] {
        client.find(Locator::Css(selector)).await?.clear().await?;
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(value)
            .await?;
    }
    click(client, "#overview-button").await?;
    click(client, "#keep-settings").await?;
    wait_dom(
        client,
        "return document.querySelector('#forge-token')?.value === 'synthetic-forge-ui';",
        vec![],
    )
    .await?;

    click(
        client,
        "#forge-token + .ui-input-buttons [data-input-action=reveal]",
    )
    .await?;
    wait_dom(client, "return document.querySelector('#forge-token').type === 'text' && document.querySelector('#forge-token').value === 'synthetic-forge-ui';", vec![]).await?;
    click(
        client,
        "#forge-token + .ui-input-buttons [data-input-action=reveal]",
    )
    .await?;
    wait_dom(
        client,
        "return document.querySelector('#forge-token').type === 'password';",
        vec![],
    )
    .await?;
    click(client, "#save-forge").await?;
    wait_dom(client,"return document.querySelector('#forge-token')?.value === '' && document.querySelector('#forge-notice')?.textContent.includes('Aucun accès distant vérifié') && document.querySelector('#save-forge')?.disabled === true;",vec![]).await?;
    wait_dom(client, "return !document.querySelector('#forge-repository') && !document.querySelector('#read-forge-pr') && !document.querySelector('#list-forge-prs');", vec![]).await?;
    let forges: atelier_core::contracts::ForgeConnections = http
        .get(format!("{address}/api/forges"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if serde_json::to_string(&forges)?.contains("synthetic-forge-ui") {
        return Err("Le jeton de forge apparaît dans la réponse publique.".into());
    }
    clear_input(client, "#forge-base").await?;
    client
        .find(Locator::Css("#forge-base"))
        .await?
        .send_keys("https://local-forge-draft.example.test")
        .await?;
    http.put(format!("{address}/api/forges")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":forges.revision,"configuration":{"kind":"github","base":"https://github-synthetic.example.test"},"token":"synthetic-forge-rotation"})).send().await?.checked_status().await?;
    click(client, "#save-forge").await?;
    wait_dom(client,"return document.querySelector('#forge-error')?.textContent.includes('ont changé') && document.querySelector('#forge-base')?.value === 'https://local-forge-draft.example.test';",vec![]).await?;
    click(client, "#reload-forges").await?;
    click(client, "#confirm-reload-forges").await?;
    wait_dom(client,"return document.querySelector('#forge-base')?.value === 'https://github-synthetic.example.test' && document.querySelector('#forge-token')?.value === '' && document.querySelector('#save-forge')?.disabled === true;",vec![]).await?;
    wait_dom(client, "return document.querySelector('#forge-clear-token')?.tagName === 'BUTTON' && document.querySelector('#forge-clear-token')?.closest('.forge-actions') === document.querySelector('#save-forge')?.closest('.forge-actions') && document.querySelector('#reload-forges')?.closest('.forge-actions') === document.querySelector('#save-forge')?.closest('.forge-actions');", vec![]).await?;
    click(client, "#forge-clear-token").await?;
    wait_dom(client, "return document.querySelector('#forge-clear-token')?.hidden === true && document.querySelector('#forge-notice')?.textContent === 'Jeton retiré.' && document.querySelector('#save-forge')?.disabled === true;", vec![]).await?;
    let removed: atelier_core::contracts::ForgeConnections = http
        .get(format!("{address}/api/forges"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if removed.connections.iter().any(|connection| {
        connection.configuration.kind == atelier_core::contracts::ForgeKind::Github
            && connection.token_set
    }) {
        return Err("Le bouton de retrait a conservé le jeton GitHub.".into());
    }
    // Jira changed above: read the current scope before entering a new token.
    click(client, "#settings-tab-time").await?;
    client.execute("const select=document.querySelector('#timeProvider');select.value='tempo';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.querySelector('#tempo-token')?.offsetParent !== null && document.querySelector('#save-timeProvider')?.disabled === false;",vec![]).await?;
    let time_preview: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if time_preview["settings"]["timeProvider"] != json!("local") {
        return Err("L’aperçu Tempo a enregistré le choix sans confirmation.".into());
    }
    client.execute("const select=document.querySelector('#timeProvider');select.value='local';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(
        client,
        "return !document.querySelector('#tempo-token');",
        vec![],
    )
    .await?;
    choose_integration(client, "timeProvider", "tempo").await?;
    click(client, "#reload-tempo").await?;
    wait_dom(
        client,
        "return document.querySelector('#tempo-settings')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    client
        .find(Locator::Css("#tempo-token"))
        .await?
        .send_keys("synthetic-tempo-ui-token")
        .await?;
    click(client, "#overview-button").await?;
    click(client, "#keep-settings").await?;
    wait_dom(
        client,
        "return document.querySelector('#tempo-token')?.value === 'synthetic-tempo-ui-token';",
        vec![],
    )
    .await?;
    click(client, "#save-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-token')?.value === '' && document.querySelector('#tempo-notice')?.textContent.includes('Aucun accès distant ni export vérifié') && document.querySelector('#save-tempo')?.disabled === true;",vec![]).await?;
    let tempo: atelier_core::contracts::TempoConnection = http
        .get(format!("{address}/api/tempo/config"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if serde_json::to_string(&tempo)?.contains("synthetic-tempo-ui-token") || !tempo.token_set {
        return Err(
            "Le secret Tempo est absent du coffre ou exposé dans la réponse publique.".into(),
        );
    }
    client
        .find(Locator::Css("#tempo-token"))
        .await?
        .send_keys("synthetic-tempo-local-draft")
        .await?;
    http.put(format!("{address}/api/tempo/config")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":tempo.configuration.revision,"expectedJiraRevision":tempo.jira_revision,"base":tempo.configuration.base,"token":"synthetic-tempo-external-rotation","clearToken":false})).send().await?.checked_status().await?;
    click(client, "#save-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-error')?.textContent.includes('a changé') && document.querySelector('#tempo-token')?.value === 'synthetic-tempo-local-draft';",vec![]).await?;
    click(client, "#reload-tempo").await?;
    click(client, "#confirm-reload-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-token')?.value === '' && document.querySelector('#save-tempo')?.disabled === true;",vec![]).await?;
    click(client, "#tempo-clear-token").await?;
    click(client, "#save-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-notice')?.textContent.includes('Aucun accès distant ni export vérifié') && document.querySelector('#tempo-clear-token')?.checked === false && document.querySelector('#save-tempo')?.disabled === true;",vec![]).await?;
    connectors_scenario(client, http, address, token).await?;
    click(client, "#settings-tab-general").await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-settings.png"),
        client.screenshot().await?,
    )?;
    client
        .find(Locator::Css("#settings-tab-general"))
        .await?
        .send_keys("\u{e00c}")
        .await?;
    wait_dom(client,"return !document.querySelector('#settings-dialog') && document.activeElement?.id === 'manage-settings';",vec![]).await?;
    Ok(())
}

async fn connectors_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    click(client, "#settings-tab-tickets").await?;
    wait_dom(client, "return document.querySelector('#settings-panel-tickets')?.hidden === false && document.querySelector('#settings-module-configuration')?.hidden === true && !document.querySelector('#prepare-azure-tickets') && !document.querySelector('#connector-module') && !document.querySelector('#probe-connector') && !document.querySelector('#install-connector');", vec![]).await?;
    for path in ["probe", "install"] {
        let response = http
            .post(format!("{address}/api/extensions/{path}"))
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::NOT_FOUND
            && response.status() != reqwest::StatusCode::METHOD_NOT_ALLOWED
        {
            return Err("L’installation de connecteurs natifs reste accessible.".into());
        }
    }
    Ok(())
}
async fn backups_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    show_board(client).await?;
    let post = |path: &str| {
        http.post(format!("{address}{path}"))
            .bearer_auth(token)
            .header("x-local-request", "1")
    };
    let card: atelier_core::contracts::Card = post("/api/cards")
        .json(&json!({"title":"État avant sauvegarde"}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    wait_title(client, "État avant sauvegarde").await?;
    click(client, "#manage-backups").await?;
    wait_dom(
        client,
        "return document.querySelector('.backup-list')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    let count = client
        .execute(
            "return document.querySelectorAll('[data-backup]').length;",
            vec![],
        )
        .await?
        .as_u64()
        .ok_or("Liste des sauvegardes absente")?;
    let timer: serde_json::Value = post("/api/time").json(&json!({"cardId":card.id,"start":true,"seconds":60,"date":"2026-10-24","startTime":"09:00:00","description":"Chronomètre synthétique"})).send().await?.checked_status().await?.json().await?;
    click(client, "#create-backup").await?;
    wait_dom(client,"return document.querySelector('#backups-error')?.textContent.includes('chronomètres') && document.querySelectorAll('[data-backup]').length === arguments[0] && document.querySelector('#create-backup')?.disabled === false;",vec![json!(count)]).await?;
    post(&format!(
        "/api/time/{}/stop",
        timer["id"].as_str().ok_or("Chronomètre absent")?
    ))
    .json(&json!({}))
    .send()
    .await?
    .checked_status()
    .await?;
    click(client, "#create-backup").await?;
    wait_dom(client,"return document.querySelector('#backups-notice')?.textContent === 'Sauvegarde créée.' && document.querySelectorAll('[data-backup]').length === arguments[0] && document.querySelector('#create-backup')?.disabled === false;",vec![json!(count+1)]).await?;
    let name = client
        .execute(
            "return document.querySelector('[data-backup]').dataset.backup;",
            vec![],
        )
        .await?;
    let selector = format!(
        "[data-backup='{}']",
        name.as_str().ok_or("Sauvegarde absente")?
    );
    http.patch(format!("{address}/api/cards/{}", card.id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(
            &json!({"title":"État après sauvegarde", "expected":{"title":"État avant sauvegarde"}}),
        )
        .send()
        .await?
        .checked_status()
        .await?;
    wait_title(client, "État après sauvegarde").await?;
    click(client, &selector).await?;
    click(client, "#cancel-restore").await?;
    wait_title(client, "État après sauvegarde").await?;
    click(client, &selector).await?;
    click(client, "#restore-backup").await?;
    wait_dom(client,"return document.querySelector('#backups-notice')?.textContent.startsWith('Restauration terminée.') && document.querySelectorAll('[data-backup]').length === arguments[0] && document.querySelector('#create-backup')?.disabled === false;",vec![json!(count+2)]).await?;
    wait_title(client, "État avant sauvegarde").await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-backups.png"),
        client.screenshot().await?,
    )?;
    // The newest backup is the automatically saved state from before restoration.
    click(client, "[data-backup]").await?;
    click(client, "#restore-backup").await?;
    wait_dom(client,"return document.querySelector('#backups-notice')?.textContent.startsWith('Restauration terminée.') && document.querySelectorAll('[data-backup]').length === arguments[0] && document.querySelector('#create-backup')?.disabled === false;",vec![json!(count+3)]).await?;
    wait_title(client, "État après sauvegarde").await?;
    click(client, "#close-backups").await?;
    wait_dom(client,"return !document.querySelector('#backups-dialog') && document.activeElement?.id === 'manage-backups';",vec![]).await?;
    client.refresh().await?;
    wait_title(client, "État après sauvegarde").await?;
    Ok(())
}

async fn language_scenario(client: &Client) -> Result<()> {
    click(client, "#manage-settings").await?;
    client.execute("const select=document.querySelector('#settings-agent'); select.value=select.value === 'claude' ? 'codex' : 'claude'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    client.execute("const select=document.querySelector('#interface-language'); select.value='en-US'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.documentElement.lang === 'en-US' && document.querySelector('#workspace-title')?.textContent === 'Settings' && document.querySelector('#save-settings')?.disabled === false && document.querySelector('#save-settings')?.textContent === 'Save general settings';",vec![]).await?;
    click(client, "#overview-button").await?;
    wait_dom(client,"return document.querySelector('#settings-page dialog.ui-confirmation[open] .ui-confirmation-body > p')?.textContent === 'Discard unsaved changes?';",vec![]).await?;
    click(client, "#discard-settings").await?;
    show_board(client).await?;
    wait_dom(client,"return document.querySelector('#workspace-title')?.textContent === 'Task board' && document.querySelector('#board-search')?.placeholder === 'Title, ticket or description…' && Array.from(document.querySelectorAll('#prototype-board .column-heading')).map(node=>Array.from(node.childNodes).filter(child=>child.nodeType===Node.TEXT_NODE).map(child=>child.textContent).join('').trim()).join('|') === 'To do|In progress|To review|Done|On hold';",vec![]).await?;
    wait_title(client, "État après sauvegarde").await?;
    click(client, ".card-open").await?;
    wait_dom(client,"return document.querySelector('#editor-heading')?.textContent === 'État après sauvegarde' && document.querySelector('#criteria-heading')?.textContent === 'Acceptance criteria' && document.querySelector('#edit-status option')?.textContent === 'To do';",vec![]).await?;
    click(client, "#add-criterion").await?;
    wait_dom(client,"return document.querySelector('.criterion .sr-only')?.textContent === 'Criterion 1' && document.querySelector('.criterion button')?.getAttribute('aria-label') === 'Remove criterion 1';",vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#confirm-editor").await?;
    client.refresh().await?;
    wait_dom(client,"return document.documentElement.lang === 'en-US' && document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false' && document.querySelector('#manage-settings')?.textContent === 'Settings';",vec![]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-english.png"),
        client.screenshot().await?,
    )?;
    click(client, "#manage-backups").await?;
    wait_dom(client,"return document.querySelector('#backups-title')?.textContent === 'Backups and restore' && document.querySelector('#create-backup')?.disabled === false;",vec![]).await?;
    click(client, "#close-backups").await?;
    click(client, "#manage-settings").await?;
    client.execute("const select=document.querySelector('#interface-language'); select.value='fr-FR'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.documentElement.lang === 'fr-FR' && document.querySelector('#workspace-title')?.textContent === 'Paramètres';",vec![]).await?;
    click(client, "#overview-button").await?;
    Ok(())
}

async fn files_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    show_board(client).await?;
    let repo = data.join("repositories/files-ui");
    std::fs::create_dir_all(repo.join("src"))?;
    run(command("git")
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args(["init", "-b", "main"]))?;
    std::fs::write(repo.join(".gitignore"), "ignored.txt\nnode_modules/\n")?;
    std::fs::create_dir_all(repo.join("node_modules"))?;
    std::fs::write(
        repo.join("node_modules/keep.txt"),
        "synthetic shared dependency",
    )?;
    std::fs::write(repo.join("src/tracked.txt"), "tracked synthetic file\n")?;
    run(command("git")
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args(["add", "--", ".gitignore", "src/tracked.txt"]))?;
    run(command("git")
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args([
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
            "-c",
            "core.hooksPath=NUL",
            "commit",
            "-qm",
            "Synthetic file baseline",
        ]))?;
    let service_repo = data.join("repositories/files-ui-service");
    run(command("git")
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args(["-c", "core.hooksPath=NUL", "clone", "--no-hardlinks", "--"])
        .arg(&repo)
        .arg(&service_repo))?;
    run(command("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "--local", "remote.origin.url"])
        .arg(&service_repo))?;
    let project: serde_json::Value = http
        .post(format!("{address}/api/projects"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Files and services fixture", "repo":repo}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let project_id = project["id"].as_str().ok_or("Projet synthétique absent.")?;
    let fixture_command = format!(
        "\"{}\" service-listener {{port}}",
        crate::target()
            .join("debug")
            .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX))
            .display()
    );
    http.patch(format!("{address}/api/projects/{project_id}")).bearer_auth(token).header("x-local-request", "1").json(&json!({"frontRepo":service_repo,"backRepo":service_repo,"frontCommand":fixture_command,"backCommand":fixture_command})).send().await?.checked_status().await?;
    std::fs::write(repo.join("src/tracked.txt"), "modified synthetic file\n")?;
    let text = "<script>window.syntheticPreviewExecuted=true</script>\nTexte é 🔧";
    std::fs::write(repo.join("new file é.txt"), text)?;
    std::fs::write(
        repo.join("ignored.txt"),
        "synthetic private ignored content",
    )?;
    std::fs::write(repo.join("binary.bin"), [1, 0, 2])?;
    for index in 0..205 {
        std::fs::write(repo.join(format!("src/file-{index:03}.txt")), "synthetic")?;
    }
    let card: atelier_core::contracts::Card = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Explorateur synthétique","repo":repo}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let endpoint = format!("{address}/api/cards/{}/file", card.id);
    let unauthorized = http
        .get(format!("{address}/api/cards/{}/files", card.id))
        .send()
        .await?;
    if unauthorized.status() != 401 {
        return Err(format!(
            "La lecture sans session doit être refusée par HTTP 401, reçu {}.",
            unauthorized.status()
        )
        .into());
    }
    if unauthorized.json::<serde_json::Value>().await?["error"]
        != "message.session_expired_or_missing_reopen_atelier_using_its_launcher"
    {
        return Err(
            "Le refus de session de l’explorateur ne donne pas le diagnostic attendu.".into(),
        );
    }
    for path in ["ignored.txt", ".git/config", "../outside"] {
        let mut url = reqwest::Url::parse(&endpoint)?;
        url.query_pairs_mut().append_pair("path", path);
        if http.get(url).bearer_auth(token).send().await?.status() != 400 {
            return Err("L’explorateur a accepté un chemin interdit.".into());
        }
    }
    wait_title(client, "Explorateur synthétique").await?;
    click(client, &format!("[data-id='{}'] .card-open", card.id)).await?;
    task_tab(client, "files").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('.file-entry').length === 200 && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    click(client, "#next-files").await?;
    wait_dom(client,"return document.querySelectorAll('.file-entry').length > 0 && document.querySelectorAll('.file-entry').length < 200 && document.querySelector('#next-files')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .send_keys("é")
        .await?;
    wait_dom(
        client,
        "return document.querySelectorAll('.file-entry').length === 1;",
        vec![],
    )
    .await?;
    click(client, ".file-entry").await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('#file-content .code-line code')).map(line=>line.textContent).join('\\n') === arguments[0] && !window.syntheticPreviewExecuted;",vec![json!(text)]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-files.png"),
        client.screenshot().await?,
    )?;
    std::fs::write(repo.join("new file é.txt"), "Contenu modifié sur disque")?;
    click(client, "#refresh-files").await?;
    wait_dom(
        client,
        "return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    click(client, ".file-entry").await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('#file-content .code-line code')).map(line=>line.textContent).join('\\n') === 'Contenu modifié sur disque';",vec![]).await?;
    clear_input(client, "#file-search").await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .send_keys("binary")
        .await?;
    click(client, ".file-entry").await?;
    wait_dom(client,"return document.querySelector('#file-unavailable')?.textContent.includes('binaire') && document.querySelector('#file-content')?.hidden === true;",vec![]).await?;
    task_tab(client, "diff").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    clear_input(client, "#file-search").await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .send_keys("tracked.txt")
        .await?;
    wait_dom(client,"return document.querySelectorAll('.file-entry').length === 1 && document.querySelector('.file-entry')?.textContent.startsWith('M · ');",vec![]).await?;
    click(client, ".file-entry").await?;
    wait_dom(client,"return document.querySelector('#file-content')?.textContent.includes('-tracked synthetic file') && document.querySelector('#file-content')?.textContent.includes('+modified synthetic file');",vec![]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-diff.png"),
        client.screenshot().await?,
    )?;
    std::fs::write(repo.join("src/tracked.txt"), "tracked synthetic file\n")?;
    click(client, "#refresh-files").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('.file-entry').length === 0 && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    task_tab(client, "details").await?;
    clear_input(client, "#worktree-base").await?;
    task_tab(client, "details").await?;
    client
        .find(Locator::Css("#worktree-base"))
        .await?
        .send_keys("main")
        .await?;
    click(client, "#worktree-fetch").await?;
    client.execute("const select=document.querySelector('#worktree-fetch-forge'); select.value='github'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    client
        .find(Locator::Css("#worktree-fetch-repository"))
        .await?
        .send_keys("../repo")
        .await?;
    click(client, "#create-worktree").await?;
    // Invalid scope fails before any credential lookup or network access.
    wait_dom(client, "return document.querySelector('#worktree-error')?.textContent.length > 0 && document.querySelector('#worktree-fetch-repository')?.value === '../repo' && document.querySelector('#create-worktree')?.matches(':disabled') === false;", vec![]).await?;
    client.execute("const select=document.querySelector('#worktree-fetch-forge'); select.value='local'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#create-worktree").await?;
    wait_dom(client,"return document.querySelector('#worktree-state')?.textContent === 'Worktree prêt.' && document.querySelector('#worktree-error')?.textContent === '' && document.querySelector('#verify-worktree')?.disabled === false;",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let worktree = board
        .cards
        .iter()
        .find(|value| value.id == card.id)
        .ok_or("Tâche synthétique absente.")?
        .fields
        .get::<atelier_core::contracts::Worktree>("worktree")?
        .ok_or("Worktree synthétique absent.")?;
    let services = board
        .cards
        .iter()
        .find(|value| value.id == card.id)
        .ok_or("Tâche synthétique absente.")?
        .fields
        .get::<atelier_core::application::worktrees::ServiceWorktrees>("serviceWorktrees")?
        .ok_or("Worktrees de service absents.")?;
    if services.get("front") != services.get("back") || services.len() != 2 {
        return Err("Les services d’un même dépôt doivent partager le worktree.".into());
    }
    wait_dom(client, "return Array.from(document.querySelectorAll('.service-worktree')).filter(element=>!element.hidden).length === 2 && Array.from(document.querySelectorAll('.service-worktree [role=status]')).every(element=>element.textContent === 'Worktree prêt.');", vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#start-services").await?;
    wait_dom(client, "return document.querySelectorAll('.service-state').length === 2 && Array.from(document.querySelectorAll('.service-state')).every(service=>service.querySelector('.service-status')?.textContent === 'Processus actif' && service.querySelector('.service-log')?.textContent.includes('synthetic-service-ready'));", vec![]).await?;
    wait_dom(client,"return document.querySelectorAll('.service-endpoints a').length === 2 && Array.from(document.querySelectorAll('.service-endpoints a')).every(link=>link.href.startsWith('http://127.0.0.1:'));",vec![]).await?;
    let before: atelier_core::contracts::ServiceSession = http
        .get(format!("{address}/api/cards/{}/services", card.id))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    for service in &before.services {
        let endpoint = service
            .endpoints
            .iter()
            .find(|endpoint| Some(endpoint.port) == service.port)
            .ok_or("Port synthétique non détecté.")?;
        let url = endpoint
            .url
            .as_ref()
            .ok_or("Adresse synthétique absente.")?;
        if http
            .get(url)
            .send()
            .await
            .map_err(|error| {
                format!(
                    "HTTP du service synthétique {} (PID {:?}, port {:?}) : {error:?}",
                    service.id, service.pid, service.port
                )
            })?
            .checked_status()
            .await?
            .text()
            .await?
            != "Atelier synthetic service é😀"
        {
            return Err("Le port détecté ne répond pas comme le service synthétique.".into());
        }
    }
    click(client, ".restart-service").await?;
    wait_dom(client,"return document.querySelector('.restart-service')?.disabled === false && document.querySelector('#services-error')?.textContent === '' && Array.from(document.querySelectorAll('.service-status')).every(element=>element.textContent === 'Processus actif');",vec![]).await?;
    let restarted: atelier_core::contracts::ServiceSession = http
        .get(format!("{address}/api/cards/{}/services", card.id))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    if before.services[0].pid == restarted.services[0].pid
        || before.services[1].pid != restarted.services[1].pid
    {
        return Err(
            "La relance individuelle a conservé son processus ou interrompu son voisin.".into(),
        );
    }
    click(client, ".stop-service").await?;
    wait_dom(client,"return document.querySelector('.service-status')?.textContent === 'Arrêté' && document.querySelectorAll('.service-status')[1]?.textContent === 'Processus actif';",vec![]).await?;
    for path in [
        "/api/extensions",
        "/api/agent-providers/cli-update",
        "/api/backups",
    ] {
        http.get(format!("{address}{path}"))
            .bearer_auth(token)
            .send()
            .await?
            .checked_status()
            .await?;
    }
    for path in [
        format!("{address}/api/cards/{}/worktree/remove", card.id),
        format!("{address}/api/backups"),
        format!("{address}/api/backups/restore"),
        format!("{address}/api/extensions/synthetic-guard/operations"),
        format!("{address}/api/extensions/synthetic-guard/operations/synthetic-operation/send"),
        format!("{address}/api/extensions/synthetic-guard/tickets/reset/preview"),
        format!("{address}/api/extensions/synthetic-guard/tickets/reset/apply"),
    ]
    .into_iter()
    .chain(
        cfg!(windows).then(|| format!("{address}/api/agent-providers/synthetic-guard/cli-update")),
    ) {
        let response = http
            .post(&path)
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({"confirmed":true,"discardChanges":true}))
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::CONFLICT {
            return Err(format!(
                "Opération {path} avec services actifs : HTTP {} au lieu de 409, {}",
                response.status(),
                response.text().await?
            )
            .into());
        }
    }
    if http
        .delete(format!("{address}/api/projects/{project_id}"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await?
        .status()
        != reqwest::StatusCode::CONFLICT
    {
        return Err("Un projet actif peut être retiré.".into());
    }
    for method in [
        "repositoriesPreview",
        "repositoriesRefresh",
        "repositoriesReview",
    ] {
        let response=http.post(format!("{address}/api/extensions/synthetic-guard/invoke"))
            .bearer_auth(token).header("x-local-request","1")
            .json(&json!({"expectedRevision":1,"method":method,"action":null,"input":{},"confirmed":true})).send().await?;
        if response.status() != reqwest::StatusCode::BAD_REQUEST
            || response.json::<serde_json::Value>().await?["error"]
                != "message.stop_agents_and_services_before_this_module_repository_operation"
        {
            return Err("Une opération de dépôt extensible n’a pas été bloquée avant de charger son profil avec un service actif.".into());
        }
    }
    // Run commands alongside the remaining service, then prove cancellation leaves it alive.
    let run_fixture = crate::target()
        .join("debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request", "1").json(&json!({"id":"ui-run-command","name":"Commande de parcours","type":"command","auth":"cli","command":run_fixture,"analyzeArgs":["run-command","success"],"editArgs":["run-command","hang"],"expected":null})).send().await?.checked_status().await?;
    let run_marker = Path::new(&worktree.path).join(".run-fixture");
    std::fs::write(&run_marker, "atelier-r03-synthetic-only")?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options || []).some(option=>option.value === 'ui-run-command');",vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent'); select.value='ui-run-command'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#runs-heading')?.parentElement.textContent.includes('Exécution terminée') && document.querySelector('#runs-heading')?.parentElement.querySelector('pre.service-log > code')?.textContent.includes('Commande synthétique é😀') && window.syntheticRunExecuted !== true;",vec![]).await?;
    client.execute("const select=document.querySelector('#run-mode'); select.value='edit'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    task_tab(client, "activity").await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#stop-run')?.disabled === false && document.querySelector('#runs-heading')?.parentElement.textContent.includes('Processus actif');",vec![]).await?;
    for path in [
        format!("{address}/api/cards/{}/worktree/remove", card.id),
        format!("{address}/api/backups"),
    ] {
        if http
            .post(path)
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({"confirmed":true,"discardChanges":true}))
            .send()
            .await?
            .status()
            != reqwest::StatusCode::CONFLICT
        {
            return Err("Une opération destructive a accepté un agent actif.".into());
        }
    }
    for path in [
        format!("{address}/api/cards/{}", card.id),
        format!("{address}/api/projects/{project_id}"),
        format!("{address}/api/agent-providers/ui-run-command"),
    ] {
        if http
            .delete(path)
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({"confirmed":true}))
            .send()
            .await?
            .status()
            != reqwest::StatusCode::CONFLICT
        {
            return Err("Une suppression a accepté un agent actif.".into());
        }
    }
    click(client, "#stop-run").await?;
    wait_dom(client,"return document.querySelector('#runs-heading')?.parentElement.textContent.includes('Exécution arrêtée') && document.querySelectorAll('.service-status')[1]?.textContent === 'Processus actif';",vec![]).await?;
    std::fs::remove_file(run_marker)?;
    let agent_marker = Path::new(&worktree.path).join(".agent-run-fixture");
    std::fs::write(&agent_marker, "atelier-r03-synthetic-only")?;
    for kind in ["codex", "claude"] {
        let agent_id = format!("ui-session-{kind}");
        let fixture = crate::target()
            .join("debug")
            .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
        let mut body = json!({"id":agent_id,"name":format!("Session synthétique {kind}"),"type":kind,"auth":"cli","command":fixture,"expected":null});
        if kind == "codex" {
            body["auth"] = json!("api");
            body["apiKey"] = json!("synthetic-run-key");
        }
        http.post(format!("{address}/api/agent-providers"))
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&body)
            .send()
            .await?
            .checked_status()
            .await?;
        wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options || []).some(option=>option.value === arguments[0]);",vec![json!(agent_id)]).await?;
        client.execute("const select=document.querySelector('#run-agent'); select.value=arguments[0]; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![json!(agent_id)]).await?;
        task_tab(client, "activity").await?;
        click(client, "#start-run").await?;
        wait_dom(client,"return document.querySelector('.agent-interaction')?.textContent.includes('echo synthetic') && document.querySelector('.approve-interaction')?.disabled === false;",vec![]).await?;
        click(client, ".approve-interaction").await?;
        wait_dom(client,"return document.querySelector('.agent-question')?.type === 'password' && document.querySelector('.approve-interaction')?.disabled === false;",vec![]).await?;
        client
            .find(Locator::Css(".agent-question"))
            .await?
            .send_keys("synthetic-private-answer")
            .await?;
        click(client, "#close-editor").await?;
        wait_dom(client,"return document.querySelector('#confirm-editor')?.textContent.includes('Abandonner') && document.querySelector('#confirm-editor')?.offsetParent !== null;",vec![]).await?;
        click(client, "#cancel-confirmation").await?;
        wait_dom(client,"return document.querySelector('.agent-question')?.value === 'synthetic-private-answer';",vec![]).await?;
        click(client, ".approve-interaction").await?;
        wait_dom(client,"const panel=document.querySelector('#runs-heading')?.parentElement; return panel?.textContent.includes('Exécution terminée') && panel.textContent.includes('Public synthetic') && !panel.textContent.includes('synthetic-run-key') && !panel.textContent.includes('synthetic-private-answer') && !panel.textContent.includes('private-synthetic-reasoning') && !document.querySelector('.agent-interaction');",vec![]).await?;
    }
    std::fs::remove_file(agent_marker)?;
    click(client, "#stop-services").await?;
    wait_dom(client, "return document.querySelectorAll('.service-status').length === 2 && Array.from(document.querySelectorAll('.service-status')).every(element=>element.textContent === 'Arrêté') && document.querySelector('#start-services')?.disabled === false;", vec![]).await?;
    // Invalid bodies cannot launch anything; after the real services stop,
    // these requests must reach validation rather than the active-service guard.
    for path in [
        "/api/agent-providers/synthetic-guard/cli-update",
        "/api/extensions/synthetic-guard/operations",
        "/api/extensions/synthetic-guard/tickets/reset/preview",
        "/api/extensions/synthetic-guard/tickets/reset/apply",
    ] {
        let response = http
            .post(format!("{address}{path}"))
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await?;
        let expected = if !cfg!(windows) && path.ends_with("/cli-update") {
            reqwest::StatusCode::NOT_FOUND
        } else {
            reqwest::StatusCode::BAD_REQUEST
        };
        if response.status() != expected {
            return Err(format!(
                "Après arrêt des services, {path} n’a pas rejoint sa validation sans effet : {}",
                response.status()
            )
            .into());
        }
    }
    if std::fs::read_to_string(Path::new(&worktree.path).join("node_modules/keep.txt"))?
        != "synthetic shared dependency"
    {
        return Err("Les dépendances sources ne sont pas partagées.".into());
    }
    std::fs::write(
        Path::new(&worktree.path).join("src/tracked.txt"),
        "worktree synthetic modification\n",
    )?;
    task_tab(client, "files").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    task_tab(client, "diff").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('.file-entry').length === 1 && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    click(client, ".file-entry").await?;
    wait_dom(client,"return document.querySelector('#file-content')?.textContent.includes('+worktree synthetic modification');",vec![]).await?;
    if std::fs::read_to_string(repo.join("src/tracked.txt"))? != "tracked synthetic file\n" {
        return Err("Le worktree a modifié le dépôt source.".into());
    }
    task_tab(client, "details").await?;
    local_commit_scenario(client, http, address, token, &card.id, &worktree).await?;
    click(client, "#remove-worktree").await?;
    click(client, "#confirm-remove-worktree").await?;
    wait_dom(client,"return document.querySelector('#worktree-error')?.textContent.includes('modifiés') && document.querySelector('#confirm-remove-worktree')?.disabled === false;",vec![]).await?;
    if !Path::new(&worktree.path).join("src/tracked.txt").exists() {
        return Err("Le retrait non confirmé a perdu les modifications.".into());
    }
    if !Path::new(&worktree.path)
        .join("node_modules/keep.txt")
        .is_file()
    {
        return Err("Le retrait refusé n’a pas restauré les liens de dépendances.".into());
    }
    click(client, "#discard-worktree-files").await?;
    click(client, "#confirm-remove-worktree").await?;
    wait_dom(client,"return document.querySelector('#worktree-notice')?.textContent === 'Worktree retiré. La branche Git est conservée.' && document.querySelector('#create-worktree')?.matches(':disabled') === false;",vec![]).await?;
    if Path::new(&worktree.path).exists() {
        return Err("Le retrait du worktree n’a pas terminé.".into());
    }
    if services
        .values()
        .any(|worktree| Path::new(&worktree.path).exists())
    {
        return Err("Un worktree de service subsiste après le retrait.".into());
    }
    if std::fs::read_to_string(service_repo.join("src/tracked.txt"))? != "tracked synthetic file\n"
    {
        return Err("Le retrait a modifié le dépôt source du service.".into());
    }
    if std::fs::read_to_string(repo.join("node_modules/keep.txt"))? != "synthetic shared dependency"
    {
        return Err("Le retrait du worktree a modifié les dépendances sources.".into());
    }
    click(client, "#close-editor").await?;
    Ok(())
}

async fn local_commit_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    card_id: &str,
    worktree: &atelier_core::contracts::Worktree,
) -> Result<()> {
    let directory = Path::new(&worktree.path);
    let metadata = std::fs::read_to_string(directory.join(".git"))?;
    let metadata = metadata
        .trim()
        .strip_prefix("gitdir: ")
        .ok_or("Métadonnées synthétiques absentes.")?;
    let index_path = Path::new(metadata).join("index");
    let index_before = std::fs::read(&index_path)?;
    std::fs::write(
        directory.join("human-unselected.txt"),
        "Keep human unselected content\n",
    )?;
    task_tab(client, "details").await?;
    click(client, "#load-commit-files").await?;
    wait_dom(client,"return document.querySelector('#local-commit')?.getAttribute('aria-busy') === 'false' && !!document.querySelector('[data-commit-path=\"src/tracked.txt\"]') && !!document.querySelector('[data-commit-path=\"human-unselected.txt\"]');",vec![]).await?;
    click(client, "[data-commit-path='src/tracked.txt']").await?;
    for (selector, value) in [
        ("#commit-author-name", "Synthetic author"),
        ("#commit-author-email", "synthetic@example.invalid"),
        ("#commit-message", "Synthetic browser commit"),
    ] {
        client
            .find(Locator::Css(selector))
            .await?
            .send_keys(value)
            .await?;
    }
    click(client, "#preview-local-commit").await?;
    wait_dom(client,"return document.querySelector('#local-commit-notice')?.textContent === 'Aperçu préparé. Aucun commit créé.' && document.querySelector('#local-commit .file-preview')?.textContent.includes('+worktree synthetic modification') && !document.querySelector('#local-commit .file-preview')?.textContent.includes('Keep human unselected content') && document.querySelector('#create-local-commit')?.disabled === true;",vec![]).await?;
    click(client, "#confirm-local-commit").await?;
    clear_input(client, "#commit-message").await?;
    client
        .find(Locator::Css("#commit-message"))
        .await?
        .send_keys("Changed synthetic message")
        .await?;
    clear_input(client, "#commit-message").await?;
    client
        .find(Locator::Css("#commit-message"))
        .await?
        .send_keys("Synthetic browser commit")
        .await?;
    wait_dom(client,"return document.querySelector('#confirm-local-commit')?.checked === false && document.querySelector('#create-local-commit')?.disabled === true;",vec![]).await?;
    click(client, "#confirm-local-commit").await?;
    // Refuse the exact reviewed tree when a human has changed the file since preview.
    std::fs::write(
        directory.join("src/tracked.txt"),
        "human changed after preview\n",
    )?;
    click(client, "#create-local-commit").await?;
    wait_dom(client,"return document.querySelector('#local-commit-error')?.textContent.includes('Prépare un nouvel aperçu') && document.querySelector('#commit-message')?.value === 'Synthetic browser commit' && document.querySelector('#local-commit')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    if std::fs::read(&index_path)? != index_before {
        return Err("Le refus du commit a modifié l’index humain.".into());
    }
    click(client, "#preview-local-commit").await?;
    wait_dom(client,"return document.querySelector('#local-commit .file-preview')?.textContent.includes('+human changed after preview') && document.querySelector('#confirm-local-commit')?.checked === false;",vec![]).await?;
    click(client, "#confirm-local-commit").await?;
    let unrelated: serde_json::Value = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Actualisation indépendante du commit","repo":worktree.source}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('article.card')).some(card=>card.dataset.id===arguments[0]) && document.querySelector('#confirm-local-commit')?.checked === true && document.querySelector('#create-local-commit')?.disabled === false;",vec![unrelated["id"].clone()]).await?;
    click(client, "#create-local-commit").await?;
    wait_dom(client,"return document.querySelector('#local-commit-notice')?.textContent.includes('Commit observé sur la branche locale') && document.querySelector('#commit-message')?.value === '' && document.querySelector('#edit-title')?.matches(':disabled') === false;",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let card = board
        .cards
        .iter()
        .find(|card| card.id == card_id)
        .ok_or("Tâche synthétique absente.")?;
    let intent = card
        .fields
        .get::<atelier_core::contracts::LocalCommitIntent>("localCommit")?
        .ok_or("Intention de commit absente.")?;
    if intent.state != atelier_core::contracts::ForgeWriteState::Confirmed
        || intent.commit != intent.observed
    {
        return Err("Le commit synthétique n’est pas confirmé sur sa branche.".into());
    }
    let output = command("git")
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args([
            "--no-pager",
            "--no-replace-objects",
            "show",
            "HEAD:src/tracked.txt",
        ])
        .output()?;
    if !output.status.success()
        || output.stdout != b"human changed after preview\n"
        || std::fs::read(&index_path)? != index_before
        || std::fs::read(directory.join("human-unselected.txt"))?
            != b"Keep human unselected content\n"
    {
        return Err(
            "Le commit navigateur ne conserve pas le contenu sélectionné ou les données humaines."
                .into(),
        );
    }
    Ok(())
}

async fn task_tab(client: &Client, tab: &str) -> Result<()> {
    if !matches!(tab, "details" | "activity" | "diff" | "files") {
        return Err("Onglet de tâche inconnu dans le parcours.".into());
    }
    let path = client.current_url().await?.path().to_owned();
    click(client, &format!("#task-tab-{tab}"))
        .await
        .map_err(|error| format!("Onglet {tab} sur {path} : {error}"))?;
    wait_dom(client, "const tab=document.getElementById('task-tab-'+arguments[0]); const panel=document.getElementById(tab?.getAttribute('aria-controls')); return tab?.getAttribute('aria-selected')==='true' && panel?.hidden===false && panel.getAttribute('aria-labelledby')===tab.id && Array.from(document.querySelectorAll('#task-editor .task-panel')).filter(panel=>!panel.hidden).length===1;", vec![json!(tab)]).await
}
async fn show_board(client: &Client) -> Result<()> {
    if client.current_url().await?.path() != "/board" {
        click(client, "#overview-button").await?;
        click(client, "[data-view=board]").await?;
    }
    wait_dom(client,"return location.pathname === '/board' && document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",vec![]).await
}

async fn new_task(client: &Client) -> Result<()> {
    show_board(client).await?;
    click(
        client,
        "#create-button:not([hidden]), #welcome:not([hidden]) #welcome-create",
    )
    .await?;
    wait_dom(client,"return document.querySelector('#new-dialog')?.open && document.activeElement?.id === 'task-title';",vec![]).await?;
    // Exercise local creation explicitly even when the only saved project is proposed by default.
    client.execute("const select=document.querySelector('#new-project');select.value='';select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    Ok(())
}

async fn created_task(client: &Client, title: &str) -> Result<()> {
    wait_dom(client,"return location.pathname.startsWith('/tasks/') && !!document.querySelector('#task-editor') && document.querySelector('#edit-title')?.value === arguments[0] && document.querySelector('#new-dialog')?.open === false;",vec![json!(title)]).await?;
    click(client, "#close-editor").await?;
    show_board(client).await?;
    wait_title(client, title).await
}

async fn saved_task(client: &Client) -> Result<()> {
    wait_dom(client,"return location.pathname.startsWith('/tasks/') && document.querySelector('#save-task')?.disabled === true && document.querySelector('#editor-notice')?.textContent === 'Carte enregistrée.';",vec![]).await?;
    click(client, "#close-editor").await?;
    show_board(client).await
}

async fn expand_control(client: &Client, selector: &str) -> Result<()> {
    for _ in 0..16 {
        if client.execute("const element=document.querySelector(arguments[0]);const container=element?.matches('summary')?element.parentElement?.parentElement:element;return !!container?.closest('details:not([open])');",vec![json!(selector)]).await? != json!(true) { return Ok(()); }
        client
            .find(Locator::Css(&format!(
                "details:not([open]):has({selector}) > summary"
            )))
            .await?
            .click()
            .await?;
    }
    Err("Trop de panneaux repliés dans le parcours synthétique.".into())
}

async fn editable(client: &Client, selector: &str) -> Result<fantoccini::elements::Element> {
    expand_control(client, selector).await?;
    Ok(client.find(Locator::Css(selector)).await?)
}

async fn clear_input(client: &Client, selector: &str) -> Result<()> {
    editable(client, selector).await?.clear().await?;
    // WebDriver clear does not consistently notify input listeners across browsers.
    client.execute("document.querySelector(arguments[0]).dispatchEvent(new Event('input',{bubbles:true}));",vec![json!(selector)]).await?;
    Ok(())
}

async fn click(client: &Client, selector: &str) -> Result<()> {
    expand_control(client, selector).await?;
    wait_dom(client, "const element=document.querySelector(arguments[0]);return !!element && element.getClientRects().length > 0 && getComputedStyle(element).visibility === 'visible' && !element.matches(':disabled');", vec![json!(selector)]).await.map_err(|error|format!("Contrôle synthétique {selector} indisponible : {error}"))?;
    // Center controls inside scrolling dialogs; retain WebDriver's native click and hit testing.
    client.execute("document.querySelector(arguments[0])?.scrollIntoView({block:'center', inline:'nearest', behavior:'instant'});", vec![json!(selector)]).await?;
    client
        .find(Locator::Css(selector))
        .await?
        .click()
        .await
        .map_err(|error| format!("Clic natif sur {selector} refusé : {error}"))?;
    inspect_surface(client).await?;
    Ok(())
}

fn wait_dom<'a>(
    client: &'a Client,
    script: &'a str,
    arguments: Vec<serde_json::Value>,
) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
    // Keep repeated WebDriver waits and their failure diagnostics off Windows' thread stack.
    Box::pin(async move {
        // Allow local Git and compiled fixtures to finish on a single low-priority CPU.
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if client.execute(script, arguments.clone()).await? == json!(true) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let state = client
        .execute(
            "return Object.fromEntries(['file-error','file-search','next-files','previous-files','refresh-files','project-error','save-project','editor-error','task-editor','duplicate-task','close-editor','forge-error','connectors-error','connectors-notice','connectors-settings','forge-base','save-forge','approve-connector-import','import-connector-tickets'].map(id=>{const element=document.getElementById(id);return [id,element ? {text:element.matches('[role=alert],[role=status]') ? element.textContent : null,busy:element.getAttribute('aria-busy'),disabled:element.disabled,fieldsetDisabled:element.closest('fieldset')?.disabled,valid:element.validity?.valid,validation:element.validationMessage,checked:element.checked,value:(id==='forge-base'||id==='file-search') ? element.value : null} : null];}));",
            vec![],
        )
        .await;
        let journals = client.execute("return Array.from(document.querySelectorAll('[data-connector-journal-card]')).map(section=>({card:section.dataset.connectorJournalCard,busy:section.getAttribute('aria-busy'),errors:Array.from(section.querySelectorAll('p.error')).map(error=>error.textContent),operations:Array.from(section.querySelectorAll('[data-connector-operation]')).map(operation=>({id:operation.dataset.connectorOperation,state:operation.querySelector('.connector-effect-state')?.textContent}))}));", vec![]).await;
        let services = client.execute("return {error:document.querySelector('#services-error')?.textContent,states:Array.from(document.querySelectorAll('.service-state')).map(service=>({status:service.querySelector('.service-status')?.textContent,ready:service.querySelector('.service-log')?.textContent.includes('synthetic-service-ready'),errors:Array.from(service.querySelectorAll('.error')).map(error=>error.textContent.slice(0,2000))}))};",vec![]).await;
        let settings = client.execute("return {timeControls:Array.from(document.querySelectorAll('.confirm-time-export,.send-time-export')).map(e=>({tag:e.tagName,classes:e.className,disabled:e.disabled,fieldset:e.closest('fieldset')?.disabled})),saveDisabled:document.querySelector('#save-settings')?.disabled,discardHidden:document.querySelector('#discard-settings')?.closest('dialog')?.open === false,focused:document.activeElement?.id};",vec![]).await;
        let export_guards=client.execute("return Array.from(document.querySelectorAll('.tempo-export')).map(el=>({busy:el.getAttribute('aria-busy'),blocked:el.dataset.editBlocked,otherDraft:el.dataset.otherDraft,archived:el.dataset.archived,checkDisabled:el.querySelector('.confirm-time-export')?.disabled,fieldsetDisabled:el.querySelector('.confirm-time-export')?.closest('fieldset')?.disabled}));",vec![]).await;
        let behavior=client.execute("return {localCommit:{busy:document.querySelector('#local-commit')?.getAttribute('aria-busy'),notice:document.querySelector('#local-commit-notice')?.textContent,error:document.querySelector('#local-commit-error')?.textContent,approved:document.querySelector('#confirm-local-commit')?.checked,disabled:document.querySelector('#create-local-commit')?.disabled,preview:document.querySelector('#local-commit .file-preview')?.textContent.slice(0,1000)},verification:{result:document.querySelector('#verification-result')?.textContent,stopDisabled:document.querySelector('#stop-run')?.disabled,criterionChecked:document.querySelector('.criterion-done')?.checked,criteriaCount:document.querySelectorAll('.criterion-done').length},search:document.querySelector('#board-search')?.value,repo:document.querySelector('#board-project')?.value,cards:Array.from(document.querySelectorAll('article.card')).map(el=>({id:el.dataset.id,description:el.querySelector('.card-description')?.textContent})),tempoErrors:Array.from(document.querySelectorAll('.time-export-error')).map(el=>el.textContent),tempoChecks:Array.from(document.querySelectorAll('.confirm-time-export')).map(el=>el.checked),update:{phase:document.querySelector('#update-phase')?.textContent,busy:document.querySelector('#updates-settings')?.getAttribute('aria-busy'),error:document.querySelector('#updates-error')?.textContent,disabled:document.querySelector('#apply-update')?.matches(':disabled'),fieldset:document.querySelector('#apply-update')?.closest('fieldset')?.disabled,settingsError:document.querySelector('#settings-error')?.textContent,agent:document.querySelector('#settings-agent')?.value,agents:Array.from(document.querySelectorAll('#settings-agent option')).map(el=>el.value)}};",vec![]).await;
        Err(
        format!("État DOM attendu non atteint : {script}; diagnostic synthétique : {state:?}; journaux : {journals:?}; services : {services:?}; paramètres : {settings:?}; export guards: {export_guards:?}; behavior: {behavior:?}")
            .into(),
    )
    })
}

async fn inspect_surface(client: &Client) -> Result<()> {
    if std::env::var_os("ATELIER_UI_INSPECT").is_none() {
        return Ok(());
    }
    let surface = client.execute(r#"const visible = el => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
const ids = Array.from(document.querySelectorAll('[id]')).map(el=>el.id);
const controls = Array.from(document.querySelectorAll('input:not([type=hidden]),select,textarea')).filter(visible);
const buttons = Array.from(document.querySelectorAll('button')).filter(visible);
const navigation = innerWidth > 800 ? Array.from(document.querySelectorAll('.sidebar:not(.sidebar-collapsed) .nav-item')).filter(visible) : [];
const firstIcon = navigation[0]?.querySelector('svg')?.getBoundingClientRect().left;
const dialogs = Array.from(document.querySelectorAll('dialog[open]'));
return {path:location.pathname, dialogs:dialogs.map(el=>el.id), tabs:Array.from(document.querySelectorAll('[role=tab][aria-selected=true]')).filter(visible).map(el=>el.id),
duplicates:ids.filter((id,index)=>ids.indexOf(id)!==index), unnamed:controls.filter(el=>!el.labels?.length && !el.getAttribute('aria-label') && !el.getAttribute('aria-labelledby')).map(el=>el.id||el.className),
unsharedButtons:buttons.filter(el=>!el.classList.contains('ui-button')).map(el=>el.id||el.className),
unnamedButtons:buttons.filter(el=>!el.textContent.trim() && !el.getAttribute('aria-label')?.trim() && !document.getElementById(el.getAttribute('aria-labelledby'))?.textContent.trim()).map(el=>el.id||el.className),
busyEnabledButtons:buttons.filter(el=>el.getAttribute('aria-busy')==='true' && !el.matches(':disabled')).map(el=>el.id||el.className),
misalignedNavigation:navigation.filter(el=>Math.abs(el.querySelector('svg').getBoundingClientRect().left-firstIcon)>1).map(el=>el.id),
framedCardTitles:Array.from(document.querySelectorAll('.card-title.card-open')).filter(visible).filter(el=>{const style=getComputedStyle(el);return style.textAlign!=='left' || style.paddingLeft!=='0px' || style.backgroundColor!=='rgba(0, 0, 0, 0)';}).map(el=>el.id),
unsharedCards:Array.from(document.querySelectorAll('article:not(.brainstorm-message)')).filter(visible).filter(el=>!el.classList.contains('ui-card')).map(el=>el.id||el.className),
overlappingCardActions:Array.from(document.querySelectorAll('.ui-card.card .ui-card-header')).filter(visible).filter(el=>{const label=el.querySelector('.card-label'),actions=el.querySelector('.card-actions');return label && actions && label.getBoundingClientRect().right>actions.getBoundingClientRect().left;}).map(el=>el.closest('.card').dataset.id),
detachedCardMenus:Array.from(document.querySelectorAll('.ui-card.card .card-action-menu:popover-open')).filter(el=>{const menu=el.getBoundingClientRect(),button=el.closest('.card-actions').querySelector('button').getBoundingClientRect();return menu.left<0 || menu.top<0 || menu.right>innerWidth || menu.bottom>innerHeight || Math.min(Math.abs(menu.top-button.bottom),Math.abs(menu.bottom-button.top))>12;}).map(el=>el.id),
unlabelledDialogs:dialogs.filter(el=>!document.getElementById(el.getAttribute('aria-labelledby'))?.textContent.trim()).map(el=>el.id),
brokenDescriptions:controls.flatMap(el=>(el.getAttribute('aria-describedby')||'').split(/\s+/).filter(Boolean).filter(id=>!document.getElementById(id)).map(id=>el.id+':'+id)),
overflow:document.documentElement.scrollWidth>innerWidth};"#,vec![]).await?;
    let directory = root().join(".artifacts/ui-surfaces");
    std::fs::create_dir_all(&directory)?;
    let key = serde_json::to_string(&json!([
        surface["path"],
        surface["dialogs"],
        surface["tabs"]
    ]))?;
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    let fresh = SEEN
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "UI inventory lock failed")?
        .insert(key);
    if fresh {
        use std::io::Write;
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("surfaces.jsonl"))?;
        writeln!(log, "{}", serde_json::to_string(&surface)?)?;
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if index < 300 {
            std::fs::write(
                directory.join(format!("surface-{index:03}.png")),
                client.screenshot().await?,
            )?;
        }
    }
    if [
        "duplicates",
        "unnamed",
        "unlabelledDialogs",
        "brokenDescriptions",
        "unsharedButtons",
        "unnamedButtons",
        "busyEnabledButtons",
        "misalignedNavigation",
        "framedCardTitles",
        "unsharedCards",
        "overlappingCardActions",
        "detachedCardMenus",
    ]
    .iter()
    .any(|key| {
        surface[*key]
            .as_array()
            .is_some_and(|values| !values.is_empty())
    }) || surface["overflow"] == json!(true)
    {
        return Err(format!("UI accessibility/layout check failed: {}", surface).into());
    }
    Ok(())
}

fn ui_assets() -> std::path::PathBuf {
    std::env::var_os("ATELIER_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root().join(".artifacts/rust-ui"))
}

fn ui_server() -> std::path::PathBuf {
    std::env::var_os("ATELIER_TEST_SERVER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::target()
                .join("debug")
                .join(format!("atelier{}", std::env::consts::EXE_SUFFIX))
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn synthetic_path_excludes_installed_agents_and_relative_directories() {
        let root = tempfile::tempdir().unwrap();
        let git = root.path().join("git");
        let agents = root.path().join("agents");
        std::fs::create_dir(&git).unwrap();
        std::fs::create_dir(&agents).unwrap();
        for filename in ["codex", "claude.exe", "gemini.cmd", "copilot.bat"] {
            let file = agents.join(filename);
            std::fs::write(&file, b"never execute").unwrap();
            let path =
                std::env::join_paths([std::path::Path::new("relative"), &agents, &git]).unwrap();
            let filtered = super::synthetic_search_path(&path).unwrap();
            assert_eq!(
                std::env::split_paths(&filtered).collect::<Vec<_>>(),
                vec![git.clone()]
            );
            std::fs::remove_file(file).unwrap();
        }
    }
}
