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
    run(command("cargo").args([
        "build",
        "--locked",
        "-p",
        "xtask",
        "--bin",
        "native-fixture",
    ]))?;
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
            "Parcours {browser} réussi : tableau, filtres, cartes, critères/dépendances, conflits d’édition, projets, persistance et rotation de session sous CSP."
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
    wait_dom(
        client,
        "return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    wait_dom(
        client,
        "return document.querySelector('#live-status')?.dataset.connected === 'true';",
        vec![],
    )
    .await?;
    let title = "Tâche synthétique é 🔧 <script>";
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys(title)
        .await?;
    click(client, "#create-task").await?;
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
    click(client, "#show-archives").await?;
    wait_dom(client, "return document.querySelectorAll('article.card').length === 1 && document.querySelector('article.card').dataset.id === arguments[0];", vec![archived["id"].clone()]).await?;
    click(client, "#show-archives").await?;
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
    click(client, "[data-status=todo] .card-open").await?;
    wait_dom(client, "return document.querySelector('#task-editor')?.open === true && document.activeElement?.id === 'edit-title';", vec![]).await?;
    if !client
        .current_url()
        .await?
        .query_pairs()
        .any(|(key, value)| key == "task" && Some(value.as_ref()) == original_id.as_str())
    {
        return Err("Adresse de tâche absente.".into());
    }
    client.refresh().await?;
    wait_dom(client, "return document.querySelector('#task-editor')?.open === true && document.querySelector('#edit-title')?.value === arguments[0];", vec![json!(title)]).await?;
    client
        .find(Locator::Css("#edit-title"))
        .await?
        .send_keys("\u{e00c}")
        .await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.activeElement?.classList.contains('card-open') === true;", vec![]).await?;
    if client
        .current_url()
        .await?
        .query_pairs()
        .any(|(key, _)| key == "task")
    {
        return Err("Adresse de tâche conservée après fermeture.".into());
    }
    click(client, "[data-status=todo] .card-open").await?;
    let title = "Tâche modifiée é 🔧 <script>";
    client
        .find(Locator::Css("#edit-title"))
        .await?
        .clear()
        .await?;
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
    wait_title(client, title).await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelector('[data-status=todo] .card-description')?.textContent === 'Description enregistrée';", vec![]).await?;
    click(client, "[data-status=todo] .card-open").await?;
    // Exercise a real HTTP validation error and retain the draft, without changing server fixtures.
    client.execute("const select=document.querySelector('#edit-status'); select.add(new Option('Invalide', 'invalid')); select.value='invalid'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#save-task").await?;
    wait_dom(client, "return document.querySelector('#editor-error')?.textContent === 'Statut invalide.' && document.querySelector('#edit-title')?.value === arguments[0];", vec![json!(title)]).await?;
    client.execute("const select=document.querySelector('#edit-status'); select.value='doing'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#save-task").await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelector('[data-status=doing] article.card')?.dataset.id === arguments[0];", vec![original_id.clone()]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
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
    http.patch(format!("{address}/api/cards/{}", original_id.as_str().ok_or("Carte sans id")?))
        .bearer_auth(&token).header("x-local-request", "1")
        .json(&json!({"description":"Modification externe", "expected":{"description":"Description enregistrée"}}))
        .send().await?.error_for_status()?;
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
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .send_keys("Description enregistrée")
        .await?;
    click(client, "#save-task").await?;
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
    wait_dom(client, "return document.querySelector('#editor-error')?.textContent.includes('Bloquée par') && document.querySelector('#task-editor')?.open === true;", vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#show-archives").await?;
    click(client, &copy_selector).await?;
    wait_dom(client, "return document.querySelector('#edit-title')?.disabled === true || document.querySelector('#edit-title')?.matches(':disabled') === true;", vec![]).await?;
    click(client, "#archive-task").await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length === 1;", vec![]).await?;
    click(client, "#show-archives").await?;
    click(client, &copy_selector).await?;
    click(client, "#delete-task").await?;
    click(client, "#cancel-confirmation").await?;
    click(client, "#delete-task").await?;
    click(client, "#confirm-editor").await?;
    wait_dom(client, "return !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length === 2;", vec![]).await?;
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
    client.execute("const select=document.querySelector('#task-template'); select.selectedIndex=1; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys("Tâche issue du modèle")
        .await?;
    click(client, "#create-task").await?;
    wait_title(client, "Tâche issue du modèle").await?;
    click(client, "[data-status=todo] .card-open").await?;
    wait_dom(client, "return document.querySelector('#edit-description')?.value === 'Description enregistrée' && document.querySelector('.criterion-text')?.value === 'Vérification humaine synthétique' && document.querySelector('.criterion-done')?.checked === false && document.querySelectorAll('[data-dependency]:checked').length === 0;", vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#remove-template").await?;
    click(client, "#confirm-remove-template").await?;
    wait_dom(client, "return document.querySelector('#task-template')?.options.length === 1 && document.querySelectorAll('article.card').length === 3;", vec![]).await?;
    live_scenario(client, http, &address, &next_token).await?;
    projects_scenario(client, http, &address, &next_token, data.path()).await?;
    settings_scenario(client, http, &address, &next_token, data.path()).await?;
    backups_scenario(client, http, &address, &next_token).await?;
    language_scenario(client).await?;
    files_scenario(client, http, &address, &next_token, data.path()).await?;
    profiles_scenario(client, http, &address, &next_token, data.path()).await?;
    client
        .goto(&format!("{address}/?task=missing-synthetic-task"))
        .await?;
    wait_dom(client, "return document.querySelector('#error')?.textContent.includes('introuvable') && !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length > 0;", vec![]).await?;
    stop_server(&mut restarted, port, &next_token).await?;
    jira_recovery_scenario(client, http).await?;
    Ok(())
}

async fn jira_recovery_scenario(client: &Client, http: &reqwest::Client) -> Result<()> {
    use atelier_app::infrastructure::{prototype::PrototypeDirectory, store::Documents};
    use atelier_core::document::Document;
    let data = tempfile::tempdir()?;
    {
        let root = PrototypeDirectory::open(data.path())?;
        let database = root.database()?;
        let brainstorm_repo = root.path.join("repositories/brainstorm");
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
        database.call(move |store| {
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
            store.put(Documents::Cards,&json!({"id":"publication-branch","title":"Synthetic branch task","description":"Human branch content","status":"todo","repo":"","forgePublicationPreview":{"configuration":{"kind":"github","base":"https://github-synthetic.example.test"},"revision":0,"repository":"owner/repo","worktree":{"path":"/synthetic/not-created","source":"/synthetic/source","branch":"task/first","base":"main","baseCommit":"b".repeat(40),"state":"ready"},"target":"main","origin":"https://github-synthetic.example.test/owner/repo.git","remoteSource":null,"localChanges":true,"sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"title":"Synthetic first publication","body":"Keep first publication draft","draft":true}}))?;
            store.put(Documents::Runs,&json!({"id":"review-report-run","cardId":"review-report","kind":"pr-review","purpose":"review","status":"completed","startedAt":"2026-10-25T10:00:00Z","reviewStaleReason":atelier_app::infrastructure::reviews::UNCHECKED,"reviewContext":{"requirements":{"criteria":["Synthetic review criterion"]},"worktree":{"path":"/synthetic/not-created","source":"/synthetic/source","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"},"kind":"github","revision":0,"repository":"owner/repo","number":7,"source":"b".repeat(40),"target":"a".repeat(40),"base":"a".repeat(40)},"review":{"summary":"Synthetic review limits: browser and service not executed","findings":[{"priority":"P2","title":"Synthetic finding <script>ignored</script>","file":"src/lib.rs","line":12,"problem":"Synthetic evidence","suggestion":"Synthetic correction"}],"criteria":[{"index":0,"status":"unverifiable","evidence":"Synthetic UI not executed"}]}}))?;
            store.put(Documents::Projects,&json!({"id":"brainstorm-project","name":"Synthetic brainstorm","repo":brainstorm_repo}))?;
            store.put(Documents::Cards,&json!({"id":"brainstorm-session","title":"Synthetic brainstorm idea","description":"Keep original idea","instructions":"","mode":"analyze","agent":"codex","status":"todo","brainstormSession":true,"repo":brainstorm_repo}))?;
            store.put(Documents::Runs,&json!({"id":"brainstorm-result","cardId":"brainstorm-session","status":"completed","purpose":"brainstorm","startedAt":"2026-10-25T10:00:00Z","userMessage":"Découpe cette idée","brainstormAnswer":{"message":"Synthetic structured proposal","questions":[{"question":"Synthetic choice?","options":["Local only","Shared"]}],"tickets":[{"title":"Synthetic converted task","description":"Converted description","criteria":["Manual validation remains required"],"questions":["Keep open question"]}]}}))?;
            store.save_settings(&json!({"jiraConnection":{"mode":"cloud","url":"https://synthetic-recovery.example.test","email":"synthetic@example.test","query":"project = SYN","revision":1}}))?;
            store.put(Documents::Cards,&json!({"id":"jira-creation","title":"Synthetic Jira creation","description":"Keep local creation content","instructions":"","agent":"codex","mode":"analyze","status":"todo","checklist":[],"jiraCreation":{"id":"synthetic-creation-operation","state":"sending","base":"https://synthetic-recovery.example.test","projectKey":"SYN","issueType":{"id":"1","name":"Task","subtask":false},"revision":1,"startedAt":"2026-10-25T10:00:00Z","result":null}}))?;
            store.put(Documents::Cards,&json!({"id":"jira-recovery","title":"Synthetic Jira recovery","description":"Local content","instructions":"","agent":"codex","mode":"analyze","status":"todo","checklist":[],"ticket":{"connector":"jira","instance":"https://synthetic-recovery.example.test","id":"1","key":"SYN-1","status":"Open","url":"https://synthetic-recovery.example.test/browse/SYN-1"},"jiraTransition":{"id":"synthetic-operation","state":"sending","base":"https://synthetic-recovery.example.test","issueId":"1","revision":1,"from":{"id":"1","name":"Open"},"transition":{"id":"11","name":"Complete","target":{"id":"2","name":"Done"},"requiresFields":false},"startedAt":"2026-10-25T10:00:00Z","observed":null}}))?;
            let mut card:atelier_core::contracts::Card=serde_json::from_str(&json!({"id":"jira-refresh","title":"Local refresh title","description":"Local refresh description","instructions":"Keep refresh instructions","agent":"codex","mode":"analyze","status":"todo","checklist":[{"text":"Keep refresh criterion","done":true}],"ticket":{"connector":"jira","instance":"https://synthetic-recovery.example.test","id":"2","key":"SYN-2","status":"Open"}}).to_string())?;
            card.fields.set("pullRequest",&json!({"revision":0,"kind":"github","repository":"owner/repo","number":7,"title":"Synthetic linked PR","url":"https://github-synthetic.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task/synthetic","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"fork":false,"baseUrl":"https://github-synthetic.example.test","checkedAt":"2026-10-25T10:00:00Z"}))?;
            store.put(Documents::Cards,&card)?;
            atelier_app::infrastructure::jira_refresh::reserve(store,&atelier_core::contracts::JiraRefreshRequest{revision:1,expected:card},"synthetic-preview")?;
            atelier_app::infrastructure::jira_refresh::ready(store,"jira-refresh","synthetic-preview",atelier_core::contracts::JiraIssueDetail{connection_revision:1,description:"Remote refresh description é😀".into(),issue:atelier_core::contracts::JiraIssue{id:"2".into(),key:"SYN-2".into(),title:"Remote refresh title".into(),status:"Open".into(),url:"https://synthetic-recovery.example.test/browse/SYN-2".into()}})?;
            Ok(())
        }).await?;
        database.close().await?;
    }
    let port = free_port()?;
    let address = format!("http://127.0.0.1:{port}");
    let (mut server, token) =
        start_server(data.path(), port, &root().join(".artifacts/rust-ui")).await?;
    client
        .goto(&format!("{address}/?task=time-export#session={token}"))
        .await?;
    tempo_export_scenario(client, http, &address, &token).await?;
    client
        .goto(&format!("{address}/?task=jira-recovery#session={token}"))
        .await?;
    wait_dom(client,"return document.querySelector('#task-editor')?.open && document.querySelector('#load-jira-transitions')?.disabled === true && document.querySelector('#reconcile-jira-transition')?.disabled === false && document.querySelector('#dismiss-jira-transition')?.disabled === true;",vec![]).await?;
    click(client, "#dismiss-jira-ack").await?;
    wait_dom(
        client,
        "return document.querySelector('#dismiss-jira-transition')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#reconcile-jira-transition").await?;
    // No credential is stored: the reconciliation must fail before any remote request.
    wait_dom(client,"return document.querySelector('#jira-task-heading')?.parentElement.querySelector('.error')?.textContent.includes('Renseigne le jeton Jira') && document.querySelector('#dismiss-jira-transition')?.disabled === true;",vec![]).await?;
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
        .error_for_status()?
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
        .goto(&format!("{address}/?task=jira-creation"))
        .await?;
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
    wait_dom(client,"return document.querySelector('#jira-create-notice')?.textContent.includes('Aucun ticket distant supprimé') && document.querySelector('#jira-created-key')?.value === '' && document.querySelector('#jira-create-project')?.disabled === false;",vec![]).await?;
    client
        .goto(&format!("{address}/?task=jira-creation"))
        .await?;
    wait_dom(client,"return document.querySelector('#jira-create-heading')?.parentElement.textContent.includes('Incertitude Jira close') && document.querySelector('#edit-description')?.value === 'Keep local creation content';",vec![]).await?;
    client
        .goto(&format!("{address}/?task=jira-refresh"))
        .await?;
    wait_dom(client,"return document.querySelector('#edit-title')?.value === 'Local refresh title' && document.querySelector('#apply-jira-refresh')?.disabled === true && document.querySelector('#confirm-jira-refresh')?.disabled === false;",vec![]).await?;
    click(client, "#confirm-jira-refresh").await?;
    click(client, "#apply-jira-refresh").await?;
    wait_dom(client,"return document.querySelector('#edit-title')?.value === 'Remote refresh title' && document.querySelector('#edit-description')?.value === 'Remote refresh description é😀' && document.querySelector('#edit-instructions')?.value === 'Keep refresh instructions' && document.querySelector('.criterion-done')?.checked === true && document.querySelector('#close-editor')?.disabled === false && document.querySelector('#jira-refresh-heading')?.parentElement.textContent.includes('Carte synchronisée');",vec![]).await?;
    time_task_scenario(client, http, &address, &token).await?;
    forge_task_scenario(client, &address).await?;
    brainstorm_scenario(client, http, &address, &token).await?;
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
    wait_dom(client,"return document.querySelector('.time-export-error')?.textContent.includes('Renseigne le jeton Tempo') && document.querySelector('.send-time-export')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css("#time-description"))
        .await?
        .send_keys("Keep local time draft")
        .await?;
    click(client, &format!("{second} .reconcile-time-export")).await?;
    wait_dom(client,"return document.querySelectorAll('.time-export-error')[1]?.textContent.includes('Renseigne le jeton Tempo') && document.querySelector('#time-description')?.value === 'Keep local time draft';",vec![]).await?;
    let response=http.patch(format!("{address}/api/cards/time-export")).bearer_auth(token).header("x-local-request","1").json(&json!({"description":"Must be refused","expected":{"description":"Keep local export content"}})).send().await?;
    if response.status() != reqwest::StatusCode::CONFLICT {
        return Err("Une mutation a accepté un export Tempo incertain.".into());
    }
    client
        .find(Locator::Css("#time-description"))
        .await?
        .clear()
        .await?;
    click(client, &format!("{second} .open-time-export-reset")).await?;
    wait_dom(client,"return document.querySelector('[data-time-id=\"00000000-0000-4000-8000-000000000002\"] .reset-time-export')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css(&format!("{second} .time-export-evidence")))
        .await?
        .send_keys("Synthetic manual decision only; no remote effect qualified")
        .await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    wait_dom(client,"return document.querySelector('[data-time-id=\"00000000-0000-4000-8000-000000000002\"] .time-export-evidence')?.value.includes('Synthetic manual decision') && document.querySelector('[data-time-id=\"00000000-0000-4000-8000-000000000002\"] .reset-time-export')?.disabled === true;",vec![]).await?;
    click(client, &format!("{second} .time-export-risk")).await?;
    click(client, &format!("{second} .reset-time-export")).await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('.time-export-notice')).some(element=>element.textContent.includes('historique incertain est conservé')) && document.querySelector('#edit-description')?.value === 'Keep local export content';",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
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
        .goto(&format!("{address}/?task=jira-refresh"))
        .await?;
    wait_dom(client,"return document.querySelector('#forge-task-heading')?.parentElement.textContent.includes('Synthetic linked PR') && document.querySelector('#detach-task-forge')?.disabled === true;",vec![]).await?;
    let description = client
        .find(Locator::Css("#edit-description"))
        .await?
        .prop("value")
        .await?
        .ok_or("Synthetic description absent")?;
    client
        .find(Locator::Css("#edit-description"))
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
    wait_dom(client,"return document.querySelector('#forge-checks-error')?.textContent.length > 0 && document.querySelector('#edit-description')?.value.endsWith(' Synthetic checks draft');",vec![]).await?;
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#edit-description"))
        .await?
        .send_keys(&description)
        .await?;
    client
        .find(Locator::Css("#task-forge-repository"))
        .await?
        .clear()
        .await?;
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
        .goto(&format!("{address}/?task=jira-refresh"))
        .await?;
    wait_dom(client,"return document.querySelector('#forge-task-heading') && !document.querySelector('#forge-task-heading')?.parentElement.querySelector('a') && document.querySelector('#edit-title')?.value === 'Remote refresh title';",vec![]).await?;
    client
        .find(Locator::Css("#publication-repository"))
        .await?
        .send_keys("owner/repo")
        .await?;
    wait_dom(
        client,
        "return document.querySelector('#task-forge-kind')?.disabled === true;",
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
    wait_dom(client,"return document.querySelector('#publication-repository')?.value === '' && document.querySelector('#task-forge-kind')?.disabled === false;",vec![]).await?;
    client
        .goto(&format!("{address}/?task=publication-branch"))
        .await?;
    wait_dom(client,"return document.querySelector('#confirm-publication-push')?.disabled === false && document.querySelector('#create-publication')?.offsetParent === null && document.querySelector('#publication-title')?.value === 'Synthetic first publication';",vec![]).await?;
    click(client, "#confirm-publication-push").await?;
    wait_dom(
        client,
        "return document.querySelector('#push-publication-branch')?.disabled === false;",
        vec![],
    )
    .await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .send_keys("Changed publication")
        .await?;
    client
        .find(Locator::Css("#publication-title"))
        .await?
        .clear()
        .await?;
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
        .goto(&format!("{address}/?task=commit-draft"))
        .await?;
    wait_dom(
        client,
        "return document.querySelector('#commit-author-name')?.disabled === false;",
        vec![],
    )
    .await?;
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
) -> Result<()> {
    client
        .goto(&format!("{address}/?task=review-report"))
        .await?;
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
        .goto(&format!("{address}/?task=brainstorm-session"))
        .await?;
    wait_dom(client,"return document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Synthetic structured proposal') && document.querySelector('#convert-brainstorm')?.disabled === true;",vec![]).await?;
    click(client, ".brainstorm-option").await?;
    wait_dom(client,"return document.querySelector('#brainstorm-message')?.value.includes('Synthetic choice? : Local only');",vec![]).await?;
    click(client, "#close-editor").await?;
    wait_dom(
        client,
        "return document.querySelector('#cancel-confirmation')?.offsetParent !== null;",
        vec![],
    )
    .await?;
    click(client, "#cancel-confirmation").await?;
    client
        .find(Locator::Css("#brainstorm-message"))
        .await?
        .clear()
        .await?;
    click(client, ".brainstorm-proposal").await?;
    client.execute("const select=document.querySelector('#brainstorm-project');select.value='brainstorm-project';select.dispatchEvent(new Event('change',{bubbles:true}));return true;",vec![]).await?;
    click(client, "#confirm-brainstorm-convert").await?;
    click(client, "#convert-brainstorm").await?;
    wait_dom(client,"return document.querySelector('.brainstorm-proposal')?.disabled === true && document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Propositions converties');",vec![]).await?;
    client
        .goto(&format!("{address}/?task=brainstorm-session"))
        .await?;
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
        .error_for_status()?
        .json()
        .await?;
    let converted: Vec<_> = board
        .cards
        .iter()
        .filter(|card| String::from_utf16_lossy(&card.title.0) == "Synthetic converted task")
        .collect();
    if converted.len() != 1 || !converted[0].fields.string_equals("status", "todo")? {
        return Err("La conversion de brainstorm est absente ou dupliquée.".into());
    }
    client
        .goto(&format!("{address}/?task={}", converted[0].id))
        .await?;
    wait_dom(client,"return document.querySelector('#task-editor')?.textContent.includes('Keep open question') && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    let fixture = root()
        .join("target/debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"preparation-command","name":"Synthetic preparation command","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-command","preparation"],"editArgs":["run-command","preparation"],"expected":null})).send().await?.error_for_status()?;
    http.post(format!("{address}/api/cards/{}/worktree", converted[0].id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"branch":"atelier/synthetic-preparation","base":"main","fetchOrigin":false}))
        .send()
        .await?
        .error_for_status()?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='preparation-command');",vec![]).await?;
    client.execute("for(const [id,value] of [['run-agent','preparation-command'],['run-purpose','preparation']]){const select=document.getElementById(id);select.value=value;select.dispatchEvent(new Event('change',{bubbles:true}));}return true;",vec![]).await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#preparation-heading')?.parentElement.textContent.includes('Synthetic prepared criterion') && document.querySelector('#stop-run')?.disabled === true && document.querySelector('#apply-preparation')?.disabled === true;",vec![]).await?;
    click(client, "#confirm-preparation").await?;
    click(client, "#apply-preparation").await?;
    wait_dom(client,"return document.querySelector('.criterion-text')?.value === 'Synthetic prepared criterion' && document.querySelector('.criterion-done')?.checked === false && document.querySelector('#preparation-notice')?.textContent.includes('Préparation appliquée') && document.querySelector('#save-task')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css("#edit-model"))
        .await?
        .send_keys("synthetic invalid model")
        .await?;
    click(client, "#save-task").await?;
    wait_dom(client,"return document.querySelector('#editor-error')?.textContent.includes('Modèle invalide') && document.querySelector('#edit-model')?.value === 'synthetic invalid model';",vec![]).await?;
    client
        .find(Locator::Css("#edit-model"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#edit-model"))
        .await?
        .send_keys("synthetic-task-model")
        .await?;
    click(client, "#save-task").await?;
    wait_dom(
        client,
        "return !document.querySelector('#task-editor');",
        vec![],
    )
    .await?;
    client
        .goto(&format!("{address}/?task={}", converted[0].id))
        .await?;
    wait_dom(client,"return document.querySelector('#edit-model')?.value === 'synthetic-task-model' && document.querySelector('.criterion-done')?.checked === false;",vec![]).await?;
    let verification_command = format!("\"{}\" verify success", fixture.display());
    http.patch(format!("{address}/api/projects/brainstorm-project"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"testCommand":verification_command,"expected":{"testCommand":""}}))
        .send()
        .await?
        .error_for_status()?;
    wait_dom(
        client,
        "return document.querySelector('#start-verification')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#start-verification").await?;
    wait_dom(client,"return document.querySelector('#verification-result')?.textContent.includes('Vérification réussie sur cet instantané') && document.querySelector('#stop-run')?.disabled === true && document.querySelector('.criterion-done')?.checked === false && !document.querySelector('.service-log script');",vec![]).await?;
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"queue-command","name":"Synthetic queued command","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-command","success"],"editArgs":["run-command","success"],"expected":null})).send().await?.error_for_status()?;
    http.patch(format!("{address}/api/cards/{}", converted[0].id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"agent":"queue-command","expected":{"agent":"codex"}}))
        .send()
        .await?
        .error_for_status()?;
    wait_dom(
        client,
        "return document.querySelector('#enqueue-task')?.disabled === false;",
        vec![],
    )
    .await?;
    click(client, "#enqueue-task").await?;
    wait_dom(client,"return document.querySelector('#close-editor')?.disabled === false && document.querySelector('#run-queue summary')?.textContent.includes('(1)');",vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#run-queue summary").await?;
    click(client, "#confirm-resume-queue").await?;
    click(client, "#resume-queue").await?;
    wait_dom(
        client,
        "return document.querySelector('#run-queue summary')?.textContent.includes('(0)');",
        vec![],
    )
    .await?;
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"brainstorm-command","name":"Synthetic brainstorm command","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-command","brainstorm"],"editArgs":["run-command","brainstorm"],"expected":null})).send().await?.error_for_status()?;
    http.post(format!("{address}/api/cards/brainstorm-session/worktree"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"branch":"atelier/synthetic-brainstorm","base":"main","fetchOrigin":false}))
        .send()
        .await?
        .error_for_status()?;
    client
        .goto(&format!("{address}/?task=brainstorm-session"))
        .await?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='brainstorm-command');",vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent');select.value='brainstorm-command';select.dispatchEvent(new Event('change',{bubbles:true}));return true;",vec![]).await?;
    for message in ["First synthetic question", "Second synthetic question"] {
        client
            .find(Locator::Css("#brainstorm-message"))
            .await?
            .send_keys(message)
            .await?;
        click(client, "#start-run").await?;
        wait_dom(client,"return document.querySelector('#brainstorm-message')?.value==='' && document.querySelector('#stop-run')?.disabled===true && document.querySelector('#runs-heading')?.parentElement.textContent.includes('Exécution terminée') && document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Synthetic agent conversation reply');",vec![]).await?;
    }
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"empty-brainstorm-command","name":"Synthetic empty brainstorm","type":"command","auth":"cli","command":fixture,"analyzeArgs":["run-empty-brainstorm","synthetic-only"],"editArgs":["run-empty-brainstorm","synthetic-only"],"expected":null})).send().await?.error_for_status()?;
    let empty: atelier_core::contracts::Card = http
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic idea without repo","repo":""}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    client
        .goto(&format!("{address}/?task={}", empty.id))
        .await?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='empty-brainstorm-command') && document.querySelector('#task-editor')?.textContent.includes('Conversation sans dépôt');",vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent');select.value='empty-brainstorm-command';select.dispatchEvent(new Event('change',{bubbles:true}));return true;",vec![]).await?;
    client
        .find(Locator::Css("#brainstorm-message"))
        .await?
        .send_keys("Explore this synthetic idea")
        .await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#brainstorm-message')?.value==='' && document.querySelector('#stop-run')?.disabled===true && document.querySelector('#brainstorm-answer-heading')?.parentElement.textContent.includes('Synthetic repository-free reply');",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
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
    client
        .find(Locator::Css("#time-duration"))
        .await?
        .clear()
        .await?;
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
        .error_for_status()?
        .json()
        .await?;
    let entry = board.times.first().ok_or("Temps synthétique absent.")?;
    let id = entry
        .get::<String>("id")?
        .ok_or("Identifiant temps absent.")?;
    let mut update = serde_json::to_value(entry)?;
    update["expected"] = serde_json::to_value(entry)?;
    update["description"] = json!("Concurrent synthetic note");
    http.patch(format!("{address}/api/time/{id}"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&update)
        .send()
        .await?
        .error_for_status()?;
    click(client, "#save-time").await?;
    wait_dom(client,"return document.querySelector('#time-error')?.textContent.includes('a changé') && document.querySelector('#time-duration')?.value === '00:02:00';",vec![]).await?;
    click(client, "#reset-time-draft").await?;
    click(client, "#discard-time-draft").await?;
    click(client, "[data-time-id] .edit-time").await?;
    wait_dom(client,"return document.querySelector('#time-description')?.value === 'Concurrent synthetic note';",vec![]).await?;
    client
        .find(Locator::Css("#time-duration"))
        .await?
        .clear()
        .await?;
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
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys("Unsaved synthetic idea")
        .await?;
    click(client, "#open-timer-task").await?;
    wait_dom(client,"return document.querySelector('#task-editor')?.open && document.querySelector('#task-title')?.value==='Unsaved synthetic idea';",vec![]).await?;
    click(client, "#close-editor").await?;
    click(client, "#card-open-jira-recovery").await?;
    client
        .find(Locator::Css("#edit-instructions"))
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
    client
        .find(Locator::Css("#task-title"))
        .await?
        .clear()
        .await?;
    click(client, "#view-time-report").await?;
    wait_dom(client,"return document.querySelector('#time-report')?.open && document.querySelector('#report-human-total')?.textContent.includes('00:02:00') && document.querySelectorAll('.time-report-row').length===1;",vec![]).await?;
    client.refresh().await?;
    wait_dom(client,"return document.querySelector('#time-report')?.open && document.querySelectorAll('.time-report-row').length===1;",vec![]).await?;
    client.execute("const source=document.querySelector('#report-source');source.value='agent';source.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.querySelectorAll('.time-report-row').length===0 && document.querySelector('#download-time-csv')?.disabled===true;",vec![]).await?;
    wait_dom(client,"return Array.from(document.querySelectorAll('#agent-usage-report tbody tr')).some(row=>row.textContent.includes('synthetic-usage-model')&&row.cells[2].textContent.includes('11 (1/1)')&&row.cells[3].textContent==='Indisponible');",vec![]).await?;
    client.execute("const source=document.querySelector('#report-source');source.value='human';source.dispatchEvent(new Event('change',{bubbles:true}));window.syntheticCsvHref='';window.syntheticCsvBlob=null;window.syntheticCreateObjectURL=URL.createObjectURL;URL.createObjectURL=function(blob){window.syntheticCsvBlob=blob;return window.syntheticCreateObjectURL.call(this,blob);};window.syntheticAnchorClick=HTMLAnchorElement.prototype.click;HTMLAnchorElement.prototype.click=function(){if(this.download==='atelier-time.csv'){window.syntheticCsvHref=this.href;return;}return window.syntheticAnchorClick.call(this);};",vec![]).await?;
    click(client, "#download-time-csv").await?;
    let csv=client.execute_async("const done=arguments[arguments.length-1];window.syntheticCsvBlob.text().then(text=>done(text)).catch(()=>done(null));",vec![]).await?;
    client.execute("HTMLAnchorElement.prototype.click=window.syntheticAnchorClick;URL.createObjectURL=window.syntheticCreateObjectURL;delete window.syntheticAnchorClick;delete window.syntheticCreateObjectURL;delete window.syntheticCsvBlob;",vec![]).await?;
    let csv = csv.as_str().ok_or("CSV synthétique absent.")?;
    if !csv.contains("Concurrent synthetic note")
        || !csv.contains("\"120\"")
        || csv.contains("Synthetic timer")
    {
        return Err("Le CSV ne correspond pas à la sélection synthétique arrêtée.".into());
    }
    click(client, "#close-time-report").await?;
    wait_dom(client,"return !document.querySelector('#time-report') && document.activeElement?.id==='view-time-report';",vec![]).await?;
    Ok(())
}

async fn profiles_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    let fixture = root()
        .join("target/debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    let config = data.join("agents-ui-config");
    std::fs::create_dir_all(&config)?;
    let request = |method, path: &str| {
        http.request(method, format!("{address}{path}"))
            .bearer_auth(token)
            .header("x-local-request", "1")
    };
    click(client, "#manage-agents").await?;
    wait_dom(client, "return document.querySelector('#agents-dialog')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('[data-profile]').length >= 2;", vec![]).await?;
    click(client, "#new-profile").await?;
    for (selector, text) in [
        ("#profile-id", "ui-api"),
        ("#profile-name", "Profil synthétique <b>é😀</b>"),
        ("#profile-model", "synthetic/model"),
        ("#profile-effort", "invalid"),
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
    wait_dom(client, "return document.querySelector('#profiles-error')?.textContent.includes('Niveau de réflexion invalide') && document.querySelector('#profile-key')?.value === 'synthetic-ui-key';", vec![]).await?;
    client
        .find(Locator::Css("#profile-effort"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#profile-effort"))
        .await?
        .send_keys("high")
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client, "return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('#profile-key')?.value === '' && document.querySelector('#profile-key-state')?.textContent.includes('Secret enregistré') && document.querySelector('[data-profile=ui-api]')?.textContent.includes('<b>é😀</b>') && !document.querySelector('[data-profile=ui-api] b');", vec![]).await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-version')?.textContent === '0.0.7' && document.querySelector('#agent-diagnostic-auth')?.textContent === 'Clé présente, validité non vérifiée.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    let catalog = request(reqwest::Method::GET, "/api/agent-providers")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if catalog.contains("synthetic-ui-key") {
        return Err("La clé synthétique a été exposée dans le catalogue.".into());
    }
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
        .error_for_status()?;
    client
        .find(Locator::Css("#profile-name"))
        .await?
        .clear()
        .await?;
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
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-auth')?.textContent === 'Aucune clé API enregistrée.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    client.execute("const select=document.querySelector('#profile-auth'); select.value='cli'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('#diagnose-profile')?.disabled === false;",vec![]).await?;
    click(client, "#diagnose-profile").await?;
    wait_dom(client,"return document.querySelector('#agent-diagnostic-auth')?.textContent === 'Connexion CLI présente, accès au service non vérifié.' && !document.querySelector('#agents-dialog')?.textContent.includes('private-output-must-not-be-returned');",vec![]).await?;
    let board: atelier_core::contracts::Board = request(reqwest::Method::GET, "/api/state")
        .send()
        .await?
        .error_for_status()?
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
        .error_for_status()?;
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return document.querySelector('#profiles-error')?.textContent.includes('fournisseur est utilisé') && document.querySelector('[data-profile=ui-api]');",vec![]).await?;
    request(reqwest::Method::PUT, "/api/settings")
        .json(&json!({"agent":previous,"expected":{"agent":"ui-api"}}))
        .send()
        .await?
        .error_for_status()?;
    click(client, "#remove-profile").await?;
    click(client, "#cancel-profile-action").await?;
    click(client, "#remove-profile").await?;
    click(client, "#confirm-profile-action").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil retiré.' && !document.querySelector('[data-profile=ui-api]');",vec![]).await?;
    click(client, "#new-profile").await?;
    client.execute("const select=document.querySelector('#profile-type'); select.value='command'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
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
    client
        .find(Locator::Css("#profile-statusArgs"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#profile-statusArgs"))
        .await?
        .send_keys(r#"["agent-status","present"]"#)
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-error')?.textContent.includes('liste JSON') && document.querySelector('#profile-analyzeArgs')?.value === 'invalid-json';",vec![]).await?;
    client
        .find(Locator::Css("#profile-analyzeArgs"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#profile-analyzeArgs"))
        .await?
        .send_keys(r#"["--analyze","{model}"]"#)
        .await?;
    click(client, "#save-profile").await?;
    wait_dom(client,"return document.querySelector('#profiles-notice')?.textContent === 'Profil enregistré.' && document.querySelector('[data-profile=ui-command]');",vec![]).await?;
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
    let card: atelier_core::contracts::Card = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Ajout depuis une autre fenêtre"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    wait_title(client, "Ajout depuis une autre fenêtre").await?;
    http.patch(format!("{address}/api/cards/{}", card.id)).bearer_auth(token).header("x-local-request", "1")
        .json(&json!({"title":"Modification depuis une autre fenêtre", "status":"standby", "expected":{"title":"Ajout depuis une autre fenêtre", "status":"todo"}}))
        .send().await?.error_for_status()?;
    wait_dom(client, "return Array.from(document.querySelectorAll('[data-status=standby] article.card')).some(card => card.dataset.id === arguments[0] && card.querySelector('.card-title').textContent === 'Modification depuis une autre fenêtre');", vec![json!(card.id)]).await?;
    http.delete(format!("{address}/api/cards/{}", card.id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?;
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
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys("Projet depuis l’interface")
        .await?;
    client
        .find(Locator::Css("#project-repo"))
        .await?
        .send_keys(&repo.to_string_lossy())
        .await?;
    click(client, "#save-project").await?;
    wait_dom(client, "return document.querySelectorAll('.project-choice').length === 2 && document.querySelector('#new-project')?.disabled === false && document.querySelector('#project-error')?.textContent === '';", vec![]).await?;
    let id = client.execute("return document.querySelector('.project-choice[aria-pressed=true]').dataset.projectId;", vec![]).await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys("Projet renommé")
        .await?;
    client
        .find(Locator::Css("#project-rules"))
        .await?
        .send_keys("Consignes synthétiques")
        .await?;
    client
        .find(Locator::Css("#project-branch"))
        .await?
        .send_keys("main")
        .await?;
    client.execute("const select=document.querySelector('#project-agent'); select.value='claude'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#detect-front").await?;
    wait_dom(client, "return document.querySelectorAll('.launch-candidate[data-role=front]').length === 1 && !document.querySelector('#project-name')?.disabled && document.querySelector('#projects-dialog fieldset')?.disabled === false;", vec![]).await?;
    click(client, ".launch-candidate[data-role=front]").await?;
    wait_dom(client, "return document.querySelector('#project-front-directory')?.value === 'client' && document.querySelector('#project-front-command')?.value.includes('--port {port}');", vec![]).await?;
    client
        .find(Locator::Css("#project-front-port"))
        .await?
        .clear()
        .await?;
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
    client.execute("const select=document.querySelector('#project-linked'); select.value=Array.from(select.options).find(option=>option.value).value; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#save-project").await?;
    wait_dom(client, "return document.querySelector('.project-choice[aria-pressed=true] strong')?.textContent === 'Projet renommé' && document.querySelector('#new-project')?.disabled === false;", vec![]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-project-editor.png"),
        client.screenshot().await?,
    )?;
    click(client, "#new-project").await?;
    client
        .find(Locator::Css("#project-name"))
        .await?
        .send_keys("Doublon refusé")
        .await?;
    client
        .find(Locator::Css("#project-repo"))
        .await?
        .send_keys(&repo.to_string_lossy())
        .await?;
    click(client, "#save-project").await?;
    wait_dom(client, "return document.querySelector('#project-error')?.textContent.includes('déjà') && document.querySelector('#project-name')?.value === 'Doublon refusé';", vec![]).await?;
    click(client, "#reset-project").await?;
    click(client, "#confirm-project").await?;
    wait_dom(client, "return document.querySelector('#project-name')?.value === '' && document.querySelector('#project-error')?.textContent === '';", vec![]).await?;
    click(client, "#close-projects").await?;
    wait_dom(client, "return !document.querySelector('#projects-dialog') && document.querySelector('#board-project')?.selectedOptions[0]?.textContent === 'Projet renommé';", vec![]).await?;
    client
        .find(Locator::Css("#task-title"))
        .await?
        .send_keys("Tâche du nouveau projet")
        .await?;
    click(client, "#create-task").await?;
    wait_title(client, "Tâche du nouveau projet").await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
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
        .find(|card| card.title == "Tâche du nouveau projet".into())
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
    http.patch(format!(
        "{address}/api/projects/{}",
        id.as_str().ok_or("Projet sans id")?
    ))
    .bearer_auth(token)
    .header("x-local-request", "1")
    .json(&json!({"name":"Nom externe", "expected":{"name":"Projet renommé"}}))
    .send()
    .await?
    .error_for_status()?;
    click(client, "#save-project").await?;
    wait_dom(client,"return document.querySelector('#project-error')?.textContent.includes('changé ailleurs') && document.querySelector('#project-name')?.value === 'Projet renommé brouillon';",vec![]).await?;
    click(client, "#close-projects").await?;
    click(client, "#confirm-project").await?;
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
    click(client, "#archive-project-tasks").await?;
    click(client, "#confirm-project").await?;
    wait_dom(client, "return document.querySelectorAll('.project-choice').length === 1 && document.querySelector('#project-name')?.value === '' && document.querySelector('#close-projects')?.disabled === false;", vec![]).await?;
    click(client, "#close-projects").await?;
    click(client, "#show-archives").await?;
    wait_dom(client, "return Array.from(document.querySelectorAll('article.card')).some(card => card.dataset.id === arguments[0]);", vec![json!(card_id)]).await?;
    if !repo.is_dir() {
        return Err("Le retrait du projet a supprimé son dossier.".into());
    }
    std::fs::write(
        root().join(".artifacts/rust-ui-projects.png"),
        client.screenshot().await?,
    )?;
    Ok(())
}

async fn settings_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    let repo = data.join("repositories/settings-ui");
    std::fs::create_dir_all(&repo)?;
    click(client, "#manage-settings").await?;
    client
        .find(Locator::Css("#settings-repo"))
        .await?
        .send_keys(&repo.to_string_lossy())
        .await?;
    client
        .find(Locator::Css("#settings-branch"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#settings-branch"))
        .await?
        .send_keys("main")
        .await?;
    client.execute("const select=document.querySelector('#settings-agent'); select.value='claude'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#save-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-notice')?.textContent === 'Paramètres enregistrés.' && document.querySelector('#close-settings')?.disabled === false;",vec![]).await?;
    let card: atelier_core::contracts::Card = http
        .post(format!("{address}/api/cards"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Héritage des paramètres généraux"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if !card.fields.string_equals("agent", "claude")?
        || std::fs::canonicalize(
            card.fields
                .get::<String>("repo")?
                .ok_or("Dépôt hérité absent")?,
        )? != std::fs::canonicalize(&repo)?
    {
        return Err("Les nouvelles tâches n’héritent pas des préférences enregistrées.".into());
    }
    http.delete(format!("{address}/api/cards/{}", card.id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?;
    client
        .find(Locator::Css("#settings-branch"))
        .await?
        .send_keys("-local")
        .await?;
    http.put(format!("{address}/api/settings"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"baseBranch":"remote", "expected":{"baseBranch":"main"}}))
        .send()
        .await?
        .error_for_status()?;
    click(client, "#save-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-error')?.textContent.includes('changé ailleurs') && document.querySelector('#settings-branch')?.value === 'main-local';",vec![]).await?;
    click(client, "#close-settings").await?;
    click(client, "#keep-settings").await?;
    click(client, "#close-settings").await?;
    click(client, "#discard-settings").await?;
    click(client, "#manage-settings").await?;
    wait_dom(
        client,
        "return document.querySelector('#settings-branch')?.value === 'remote';",
        vec![],
    )
    .await?;
    client
        .find(Locator::Css("#settings-repo"))
        .await?
        .send_keys("/missing-synthetic-directory")
        .await?;
    click(client, "#save-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-error')?.textContent.length > 0 && document.querySelector('#settings-repo')?.value.endsWith('/missing-synthetic-directory') && document.querySelector('#close-settings')?.disabled === false;",vec![]).await?;
    click(client, "#close-settings").await?;
    click(client, "#discard-settings").await?;
    client.refresh().await?;
    wait_dom(
        client,
        "return document.querySelector('#prototype-board')?.getAttribute('aria-busy') === 'false';",
        vec![],
    )
    .await?;
    click(client, "#manage-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-branch')?.value === 'remote' && document.querySelector('#settings-agent')?.value === 'claude' && !document.querySelector('#settings-repo')?.value.includes('missing-synthetic-directory');",vec![]).await?;
    click(client, "#settings-tab-jira").await?;
    wait_dom(
        client,
        "return document.querySelector('#jira-query')?.disabled === false;",
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
    click(client, "#close-settings").await?;
    click(client, "#keep-settings").await?;
    wait_dom(client,"return document.querySelector('#jira-token')?.value === 'synthetic-jira-one' && document.querySelector('#jira-query')?.value === 'project = SYN';",vec![]).await?;
    client
        .find(Locator::Css("#settings-tab-jira"))
        .await?
        .send_keys("\u{e014}")
        .await?;
    wait_dom(client,"return document.activeElement?.id === 'settings-tab-forges' && document.querySelector('#settings-panel-jira')?.hidden === true;",vec![]).await?;
    client
        .find(Locator::Css("#settings-tab-forges"))
        .await?
        .send_keys("\u{e012}")
        .await?;
    wait_dom(client,"return document.activeElement?.id === 'settings-tab-jira' && document.querySelector('#jira-token')?.value === 'synthetic-jira-one' && document.querySelector('#jira-query')?.value === 'project = SYN';",vec![]).await?;
    click(client, "#save-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-notice')?.textContent === 'Connexion Jira enregistrée.' && document.querySelector('#jira-token')?.value === '' && document.querySelector('#search-jira')?.disabled === false;",vec![]).await?;
    let saved: serde_json::Value = http
        .get(format!("{address}/api/jira/config"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if serde_json::to_string(&saved)?.contains("synthetic-jira-one") {
        return Err("Le jeton Jira apparaît dans la réponse publique.".into());
    }
    client
        .find(Locator::Css("#jira-query"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#jira-query"))
        .await?
        .send_keys("project = LOCAL")
        .await?;
    http.put(format!("{address}/api/jira/config")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":saved["configuration"]["revision"],"mode":"cloud","url":"https://synthetic-one.example.test","email":"synthetic@example.test","query":"project = REMOTE","token":"synthetic-jira-rotated"})).send().await?.error_for_status()?;
    click(client, "#save-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-error')?.textContent.includes('a changé') && document.querySelector('#jira-query')?.value === 'project = LOCAL';",vec![]).await?;
    click(client, "#reload-jira").await?;
    click(client, "#confirm-reload-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-query')?.value === 'project = REMOTE' && document.querySelector('#jira-token')?.value === '' && document.querySelector('#search-jira')?.disabled === false;",vec![]).await?;
    click(client, "#jira-clear-token").await?;
    click(client, "#save-jira").await?;
    wait_dom(client,"return document.querySelector('#jira-notice')?.textContent === 'Connexion Jira enregistrée.' && document.querySelector('#search-jira')?.disabled === true && document.querySelector('#save-jira')?.disabled === true;",vec![]).await?;
    click(client, "#settings-tab-forges").await?;
    wait_dom(
        client,
        "return document.querySelector('#forge-base')?.disabled === false;",
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
    click(client, "#close-settings").await?;
    click(client, "#keep-settings").await?;
    wait_dom(
        client,
        "return document.querySelector('#forge-token')?.value === 'synthetic-forge-ui';",
        vec![],
    )
    .await?;
    click(client, "#save-forge").await?;
    wait_dom(client,"return document.querySelector('#forge-token')?.value === '' && document.querySelector('#forge-notice')?.textContent.includes('Aucun accès distant vérifié') && document.querySelector('#save-forge')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css("#forge-repository"))
        .await?
        .send_keys("../invalid")
        .await?;
    click(client, "#list-forge-prs").await?;
    // Invalid repository scope is rejected before any HTTP call to the forge.
    wait_dom(client,"return document.querySelector('#forge-error')?.textContent.includes('Dépôt ou numéro de PR invalide') && document.querySelector('#forge-repository')?.value === '../invalid' && document.querySelector('#list-forge-prs')?.disabled === false;",vec![]).await?;
    client
        .find(Locator::Css("#forge-repository"))
        .await?
        .clear()
        .await?;
    let forges: atelier_core::contracts::ForgeConnections = http
        .get(format!("{address}/api/forges"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if serde_json::to_string(&forges)?.contains("synthetic-forge-ui") {
        return Err("Le jeton de forge apparaît dans la réponse publique.".into());
    }
    client
        .find(Locator::Css("#forge-base"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#forge-base"))
        .await?
        .send_keys("https://local-forge-draft.example.test")
        .await?;
    http.put(format!("{address}/api/forges")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":forges.revision,"configuration":{"kind":"github","base":"https://github-synthetic.example.test"},"token":"synthetic-forge-rotation"})).send().await?.error_for_status()?;
    click(client, "#save-forge").await?;
    wait_dom(client,"return document.querySelector('#forge-error')?.textContent.includes('ont changé') && document.querySelector('#forge-base')?.value === 'https://local-forge-draft.example.test';",vec![]).await?;
    click(client, "#reload-forges").await?;
    click(client, "#confirm-reload-forges").await?;
    wait_dom(client,"return document.querySelector('#forge-base')?.value === 'https://github-synthetic.example.test' && document.querySelector('#forge-token')?.value === '' && document.querySelector('#save-forge')?.disabled === true;",vec![]).await?;
    click(client, "#forge-clear-token").await?;
    click(client, "#save-forge").await?;
    wait_dom(client,"return document.querySelector('#save-forge')?.disabled === true && document.querySelector('#forge-notice')?.textContent.includes('Aucun accès distant vérifié');",vec![]).await?;
    // Jira changed above: read the current scope before entering a new token.
    click(client, "#settings-tab-tempo").await?;
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
    click(client, "#close-settings").await?;
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
        .error_for_status()?
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
    http.put(format!("{address}/api/tempo/config")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":tempo.configuration.revision,"expectedJiraRevision":tempo.jira_revision,"base":tempo.configuration.base,"token":"synthetic-tempo-external-rotation","clearToken":false})).send().await?.error_for_status()?;
    click(client, "#save-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-error')?.textContent.includes('a changé') && document.querySelector('#tempo-token')?.value === 'synthetic-tempo-local-draft';",vec![]).await?;
    click(client, "#reload-tempo").await?;
    click(client, "#confirm-reload-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-token')?.value === '' && document.querySelector('#save-tempo')?.disabled === true;",vec![]).await?;
    click(client, "#tempo-clear-token").await?;
    click(client, "#save-tempo").await?;
    wait_dom(client,"return document.querySelector('#tempo-notice')?.textContent.includes('Aucun accès distant ni export vérifié') && document.querySelector('#tempo-clear-token')?.checked === false && document.querySelector('#save-tempo')?.disabled === true;",vec![]).await?;
    connectors_scenario(client, http, address, token, data).await?;
    click(client, "#settings-tab-general").await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-settings.png"),
        client.screenshot().await?,
    )?;
    client
        .find(Locator::Css("#settings-agent"))
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
    data: &Path,
) -> Result<()> {
    use atelier_core::application::extensions::Registry;
    click(client, "#settings-tab-connectors").await?;
    wait_dom(client,"return document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    let fixture = root()
        .join("target/debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    client
        .find(Locator::Css("#connector-module"))
        .await?
        .send_keys("synthetic-extension")
        .await?;
    client
        .find(Locator::Css("#connector-path"))
        .await?
        .send_keys(&fixture.to_string_lossy())
        .await?;
    click(client, "#connector-trusted").await?;
    click(client, "#probe-connector").await?;
    wait_dom(client,"return document.querySelector('#approve-connector')?.offsetParent !== null && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    click(client, "#approve-connector").await?;
    client
        .find(Locator::Css("#connector-module"))
        .await?
        .send_keys("-changed")
        .await?;
    wait_dom(client,"return document.querySelector('#approve-connector')?.checked === false && document.querySelector('#install-connector')?.disabled === true && document.querySelector('#connector-trusted')?.checked === false;",vec![]).await?;
    client
        .find(Locator::Css("#connector-module"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#connector-module"))
        .await?
        .send_keys("synthetic-extension")
        .await?;
    click(client, "#connector-trusted").await?;
    click(client, "#probe-connector").await?;
    wait_dom(client,"return document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false' && document.querySelector('#approve-connector')?.offsetParent !== null;",vec![]).await?;
    click(client, "#approve-connector").await?;
    click(client, "#install-connector").await?;
    wait_dom(client,"return !!document.querySelector('[data-connector=synthetic-extension]') && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    click(client, "[data-connector=synthetic-extension]").await?;
    client
        .find(Locator::Css("#connector-field-token"))
        .await?
        .send_keys("synthetic-extension-browser-token")
        .await?;
    click(client, "#settings-tab-general").await?;
    click(client, "#settings-tab-connectors").await?;
    wait_dom(client,"return document.querySelector('#connector-field-token')?.value === 'synthetic-extension-browser-token' && document.querySelector('#check-connector')?.disabled === true;",vec![]).await?;
    click(client, "#close-settings").await?;
    click(client, "#keep-settings").await?;
    click(client, "#save-connector").await?;
    wait_dom(client,"return document.querySelector('#connector-field-token')?.value === '' && document.querySelector('#save-connector')?.disabled === true;",vec![]).await?;
    let response = http
        .get(format!("{address}/api/extensions"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if response.contains("synthetic-extension-browser-token") {
        return Err("Un secret de connecteur a atteint la réponse publique.".into());
    }
    let registry: Registry = serde_json::from_str(&response)?;
    let connection = registry
        .modules
        .iter()
        .find(|connection| connection.installed.manifest.id == "synthetic-extension")
        .ok_or("Connecteur synthétique absent")?;
    if !connection.secret_fields.contains("token") {
        return Err("Le secret synthétique n’a pas été enregistré.".into());
    }
    click(client, "#check-connector").await?;
    wait_dom(client,"return document.querySelector('#connectors-settings')?.textContent.includes('Synthetic extension ready') && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    client
        .find(Locator::Css("#connector-field-token"))
        .await?
        .send_keys("synthetic-extension-browser-draft")
        .await?;
    http.put(format!("{address}/api/extensions/synthetic-extension")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":connection.installed.revision,"values":{"fault":"success"},"forgetValues":[],"secrets":{},"forgetSecrets":[]})).send().await?.error_for_status()?;
    click(client, "#save-connector").await?;
    wait_dom(client,"return document.querySelector('#connectors-error')?.textContent.length > 0 && document.querySelector('#connector-field-token')?.value === 'synthetic-extension-browser-draft' && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    click(client, "#reload-connectors").await?;
    click(client, "#confirm-reload-connectors").await?;
    wait_dom(client,"return document.querySelector('#connector-field-token')?.value === '' && document.querySelector('#connector-field-fault')?.value === 'success' && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    let repository = data.join("repositories/connector-ticket-import");
    std::fs::create_dir_all(&repository)?;
    let project: serde_json::Value = http
        .post(format!("{address}/api/projects"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Synthetic connector import","repo":repository}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let project_id = project["id"].as_str().ok_or("Projet synthétique absent")?;
    click(client, "#search-connector-tickets").await?;
    wait_dom(client,"return !!document.querySelector('[data-connector-ticket=synthetic-ticket]') && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false' && Array.from(document.querySelector('#connector-import-project')?.options ?? []).some(option=>option.value===arguments[0]);",vec![json!(project_id)]).await?;
    click(client, "[data-connector-ticket=synthetic-ticket]").await?;
    client.execute("const select=document.querySelector('#connector-import-project');select.value=arguments[0];select.dispatchEvent(new Event('change',{bubbles:true}));",vec![json!(project_id)]).await?;
    click(client, "#approve-connector-import").await?;
    http.patch(format!("{address}/api/projects/{project_id}"))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Updated synthetic connector import"}))
        .send()
        .await?
        .error_for_status()?;
    click(client, "#import-connector-tickets").await?;
    wait_dom(client,"return document.querySelector('#connectors-error')?.textContent.includes('projet a changé') && document.querySelector('[data-connector-ticket=synthetic-ticket]')?.checked === true && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    wait_dom(client,"return Array.from(document.querySelector('#connector-import-project')?.options ?? []).some(option=>option.value===arguments[0] && option.textContent==='Updated synthetic connector import');",vec![json!(project_id)]).await?;
    client.execute("const select=document.querySelector('#connector-import-project');select.value='';select.dispatchEvent(new Event('change',{bubbles:true}));select.value=arguments[0];select.dispatchEvent(new Event('change',{bubbles:true}));",vec![json!(project_id)]).await?;
    wait_dom(client,"return document.querySelector('#approve-connector-import')?.checked === false && document.querySelector('#import-connector-tickets')?.disabled === true;",vec![]).await?;
    click(client, "#approve-connector-import").await?;
    click(client, "#import-connector-tickets").await?;
    wait_dom(client,"return document.querySelector('#connectors-notice')?.textContent.includes('Import terminé') && document.querySelector('[data-connector-ticket=synthetic-ticket]')?.checked === false && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let imported = state["cards"]
        .as_array()
        .ok_or("Cartes absentes")?
        .iter()
        .find(|card| card["ticket"]["connector"] == "extension:synthetic-extension")
        .cloned()
        .ok_or("Ticket synthétique non importé")?;
    click(client, "#remove-connector").await?;
    click(client, "#confirm-remove-connector").await?;
    wait_dom(client,"return !document.querySelector('[data-connector=synthetic-extension]') && document.querySelector('#connectors-settings')?.getAttribute('aria-busy') === 'false';",vec![]).await?;
    let registry: Registry = http
        .get(format!("{address}/api/extensions"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if !registry.modules.is_empty() {
        return Err("Le connecteur synthétique n’a pas été retiré.".into());
    }
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if state["cards"]
        .as_array()
        .is_none_or(|cards| !cards.contains(&imported))
    {
        return Err("Le retrait du module a modifié la carte importée.".into());
    }
    Ok(())
}

async fn backups_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
) -> Result<()> {
    let post = |path: &str| {
        http.post(format!("{address}{path}"))
            .bearer_auth(token)
            .header("x-local-request", "1")
    };
    let card: atelier_core::contracts::Card = post("/api/cards")
        .json(&json!({"title":"État avant sauvegarde"}))
        .send()
        .await?
        .error_for_status()?
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
    let timer: serde_json::Value = post("/api/time").json(&json!({"cardId":card.id,"start":true,"seconds":60,"date":"2026-10-24","startTime":"09:00:00","description":"Chronomètre synthétique"})).send().await?.error_for_status()?.json().await?;
    click(client, "#create-backup").await?;
    wait_dom(client,"return document.querySelector('#backups-error')?.textContent.includes('chronomètres') && document.querySelectorAll('[data-backup]').length === arguments[0] && document.querySelector('#create-backup')?.disabled === false;",vec![json!(count)]).await?;
    post(&format!(
        "/api/time/{}/stop",
        timer["id"].as_str().ok_or("Chronomètre absent")?
    ))
    .json(&json!({}))
    .send()
    .await?
    .error_for_status()?;
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
        .error_for_status()?;
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
    let original = client
        .find(Locator::Css("#settings-branch"))
        .await?
        .prop("value")
        .await?
        .ok_or("Branche absente")?;
    client
        .find(Locator::Css("#settings-branch"))
        .await?
        .send_keys("-draft")
        .await?;
    client.execute("const select=document.querySelector('#interface-language'); select.value='en-US'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.documentElement.lang === 'en-US' && document.querySelector('#settings-title')?.textContent === 'Application settings' && document.querySelector('#settings-branch')?.value === arguments[0] && document.querySelector('#save-settings')?.textContent === 'Save';",vec![json!(format!("{original}-draft"))]).await?;
    click(client, "#close-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-dialog .confirmation p')?.textContent === 'Discard unsaved changes?';",vec![]).await?;
    click(client, "#discard-settings").await?;
    wait_dom(client,"return document.querySelector('h1')?.textContent === 'Your workspace' && document.querySelector('#board-search')?.placeholder === 'Title or description…' && Array.from(document.querySelectorAll('.column-heading h2')).map(node=>node.textContent).join('|') === 'To do|In progress|To review|Done|On hold';",vec![]).await?;
    wait_title(client, "État après sauvegarde").await?;
    click(client, ".card-open").await?;
    wait_dom(client,"return document.querySelector('#editor-heading')?.textContent === 'Task details' && document.querySelector('#criteria-heading')?.textContent === 'Acceptance criteria' && document.querySelector('#edit-status option')?.textContent === 'To do';",vec![]).await?;
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
    client
        .find(Locator::Css("#settings-repo"))
        .await?
        .send_keys("/missing-synthetic-directory")
        .await?;
    click(client, "#save-settings").await?;
    wait_dom(client,"return document.querySelector('#settings-error')?.textContent === 'The repository could not be found or accessed.' && document.querySelector('#close-settings')?.disabled === false;",vec![]).await?;
    click(client, "#close-settings").await?;
    click(client, "#discard-settings").await?;
    click(client, "#manage-settings").await?;
    client.execute("const select=document.querySelector('#interface-language'); select.value='fr-FR'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    wait_dom(client,"return document.documentElement.lang === 'fr-FR' && document.querySelector('#settings-title')?.textContent === 'Paramètres de l’application';",vec![]).await?;
    click(client, "#close-settings").await?;
    Ok(())
}

async fn files_scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
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
        .error_for_status()?
        .json()
        .await?;
    let project_id = project["id"].as_str().ok_or("Projet synthétique absent.")?;
    let fixture_command = format!(
        "\"{}\" service-listener {{port}}",
        root()
            .join("target/debug")
            .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX))
            .display()
    );
    http.patch(format!("{address}/api/projects/{project_id}")).bearer_auth(token).header("x-local-request", "1").json(&json!({"frontRepo":service_repo,"backRepo":service_repo,"frontCommand":fixture_command,"backCommand":fixture_command})).send().await?.error_for_status()?;
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
        .error_for_status()?
        .json()
        .await?;
    let endpoint = format!("{address}/api/cards/{}/file", card.id);
    if http
        .get(format!("{address}/api/cards/{}/files", card.id))
        .send()
        .await?
        .status()
        != 403
    {
        return Err("L’explorateur a accepté une lecture sans session.".into());
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
    click(client, "#browse-task-files").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('.file-choice').length === 200 && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    click(client, "#next-files").await?;
    wait_dom(client,"return document.querySelectorAll('.file-choice').length > 0 && document.querySelectorAll('.file-choice').length < 200 && document.querySelector('#next-files')?.disabled === true;",vec![]).await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .send_keys("é")
        .await?;
    wait_dom(
        client,
        "return document.querySelectorAll('.file-choice').length === 1;",
        vec![],
    )
    .await?;
    click(client, ".file-choice").await?;
    wait_dom(client,"return document.querySelector('#file-content')?.textContent === arguments[0] && !window.syntheticPreviewExecuted;",vec![json!(text)]).await?;
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
    click(client, ".file-choice").await?;
    wait_dom(client,"return document.querySelector('#file-content')?.textContent === 'Contenu modifié sur disque';",vec![]).await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .send_keys("binary")
        .await?;
    click(client, ".file-choice").await?;
    wait_dom(client,"return document.querySelector('#file-unavailable')?.textContent.includes('binaire') && document.querySelector('#file-content')?.hidden === true;",vec![]).await?;
    click(client, "#show-changes").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#file-search"))
        .await?
        .send_keys("tracked.txt")
        .await?;
    wait_dom(client,"return document.querySelectorAll('.file-choice').length === 1 && document.querySelector('.file-choice')?.textContent.startsWith('M · ');",vec![]).await?;
    click(client, ".file-choice").await?;
    wait_dom(client,"return document.querySelector('#file-content')?.textContent.includes('-tracked synthetic file') && document.querySelector('#file-content')?.textContent.includes('+modified synthetic file');",vec![]).await?;
    std::fs::write(
        root().join(".artifacts/rust-ui-diff.png"),
        client.screenshot().await?,
    )?;
    std::fs::write(repo.join("src/tracked.txt"), "tracked synthetic file\n")?;
    click(client, "#refresh-files").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('.file-choice').length === 0 && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    client
        .find(Locator::Css("#worktree-base"))
        .await?
        .clear()
        .await?;
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
    wait_dom(client, "return document.querySelector('#worktree-error')?.textContent.length > 0 && document.querySelector('#worktree-fetch-repository')?.value === '../repo' && document.querySelector('#create-worktree')?.disabled === false;", vec![]).await?;
    client.execute("const select=document.querySelector('#worktree-fetch-forge'); select.value='local'; select.dispatchEvent(new Event('change',{bubbles:true}));", vec![]).await?;
    click(client, "#create-worktree").await?;
    wait_dom(client,"return document.querySelector('#worktree-state')?.textContent === 'Worktree prêt.' && document.querySelector('#worktree-error')?.textContent === '' && document.querySelector('#verify-worktree')?.disabled === false;",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
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
    click(client, "#start-services").await?;
    wait_dom(client, "return document.querySelectorAll('.service-status').length === 2 && Array.from(document.querySelectorAll('.service-status')).every(element=>element.textContent === 'Processus actif') && Array.from(document.querySelectorAll('.service-log')).every(element=>element.textContent.includes('synthetic-service-ready'));", vec![]).await?;
    wait_dom(client,"return document.querySelectorAll('.service-endpoints a').length === 2 && Array.from(document.querySelectorAll('.service-endpoints a')).every(link=>link.href.startsWith('http://127.0.0.1:'));",vec![]).await?;
    let before: atelier_core::contracts::ServiceSession = http
        .get(format!("{address}/api/cards/{}/services", card.id))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
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
            .await?
            .error_for_status()?
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
        .error_for_status()?
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
            return Err("Une opération destructive a accepté des services actifs.".into());
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
    // Run commands alongside the remaining service, then prove cancellation leaves it alive.
    let run_fixture = root()
        .join("target/debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request", "1").json(&json!({"id":"ui-run-command","name":"Commande de parcours","type":"command","auth":"cli","command":run_fixture,"analyzeArgs":["run-command","success"],"editArgs":["run-command","hang"],"expected":null})).send().await?.error_for_status()?;
    let run_marker = Path::new(&worktree.path).join(".run-fixture");
    std::fs::write(&run_marker, "atelier-r03-synthetic-only")?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options || []).some(option=>option.value === 'ui-run-command');",vec![]).await?;
    client.execute("const select=document.querySelector('#run-agent'); select.value='ui-run-command'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#runs-heading')?.parentElement.textContent.includes('Exécution terminée') && document.querySelector('#runs-heading')?.parentElement.querySelector('code')?.textContent.includes('Commande synthétique é😀') && window.syntheticRunExecuted !== true;",vec![]).await?;
    client.execute("const select=document.querySelector('#run-mode'); select.value='edit'; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![]).await?;
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
        let fixture = root()
            .join("target/debug")
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
            .error_for_status()?;
        wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options || []).some(option=>option.value === arguments[0]);",vec![json!(agent_id)]).await?;
        client.execute("const select=document.querySelector('#run-agent'); select.value=arguments[0]; select.dispatchEvent(new Event('change',{bubbles:true}));",vec![json!(agent_id)]).await?;
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
    if std::fs::read_to_string(Path::new(&worktree.path).join("node_modules/keep.txt"))?
        != "synthetic shared dependency"
    {
        return Err("Les dépendances sources ne sont pas partagées.".into());
    }
    std::fs::write(
        Path::new(&worktree.path).join("src/tracked.txt"),
        "worktree synthetic modification\n",
    )?;
    click(client, "#browse-task-files").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    click(client, "#show-changes").await?;
    wait_dom(client,"return document.querySelector('.file-explorer')?.getAttribute('aria-busy') === 'false' && document.querySelectorAll('.file-choice').length === 1 && document.querySelector('#file-error')?.textContent === '';",vec![]).await?;
    click(client, ".file-choice").await?;
    wait_dom(client,"return document.querySelector('#file-content')?.textContent.includes('+worktree synthetic modification');",vec![]).await?;
    if std::fs::read_to_string(repo.join("src/tracked.txt"))? != "tracked synthetic file\n" {
        return Err("Le worktree a modifié le dépôt source.".into());
    }
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
    wait_dom(client,"return document.querySelector('#worktree-notice')?.textContent === 'Worktree retiré. La branche Git est conservée.' && document.querySelector('#create-worktree')?.disabled === false;",vec![]).await?;
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
    click(client, "#load-commit-files").await?;
    wait_dom(client,"return document.querySelector('#local-commit')?.getAttribute('aria-busy') === 'false' && document.querySelector('[data-commit-path=\"src/tracked.txt\"]') && document.querySelector('[data-commit-path=\"human-unselected.txt\"]');",vec![]).await?;
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
    client
        .find(Locator::Css("#commit-message"))
        .await?
        .clear()
        .await?;
    client
        .find(Locator::Css("#commit-message"))
        .await?
        .send_keys("Changed synthetic message")
        .await?;
    client
        .find(Locator::Css("#commit-message"))
        .await?
        .clear()
        .await?;
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
    click(client, "#create-local-commit").await?;
    wait_dom(client,"return document.querySelector('#local-commit-notice')?.textContent.includes('Commit observé sur la branche locale') && document.querySelector('#commit-message')?.value === '' && document.querySelector('#edit-title')?.disabled === false;",vec![]).await?;
    let board: atelier_core::contracts::Board = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
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

async fn click(client: &Client, selector: &str) -> Result<()> {
    // Center controls inside scrolling dialogs; retain WebDriver's native click and hit testing.
    client.execute("document.querySelector(arguments[0])?.scrollIntoView({block:'center', inline:'nearest', behavior:'instant'});", vec![json!(selector)]).await?;
    client.find(Locator::Css(selector)).await?.click().await?;
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
