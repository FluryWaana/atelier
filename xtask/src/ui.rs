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
    client
        .goto(&format!("{address}/?task=missing-synthetic-task"))
        .await?;
    wait_dom(client, "return document.querySelector('#error')?.textContent.includes('introuvable') && !document.querySelector('#task-editor') && document.querySelectorAll('article.card').length > 0;", vec![]).await?;
    stop_server(&mut restarted, port, &next_token).await?;
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
    std::fs::create_dir_all(&repo)?;
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
    std::fs::write(repo.join(".gitignore"), "ignored.txt\n")?;
    std::fs::write(repo.join("src/tracked.txt"), "tracked synthetic file\n")?;
    run(command("git")
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args(["add", "--", ".gitignore", "src/tracked.txt"]))?;
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
        if http
            .get(url)
            .bearer_auth(token)
            .send()
            .await?
            .status()
            != 400
        {
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
    click(client, "#close-editor").await?;
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
