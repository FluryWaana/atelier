use super::{Client, Result, SyntheticResponse, click, command, root, wait_dom};
use atelier_core::{
    application::extensions::{Method, Registry, RemoteOperation},
    contracts::Card,
};
use serde_json::json;
use std::path::{Path, PathBuf};
fn git(path: &Path, args: &[&str]) -> Result<String> {
    let mut process = command("git");
    process.env_clear().current_dir(path);
    for key in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            process.env(key, value);
        }
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = process
        .env("HOME", path)
        .env("USERPROFILE", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
        ])
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err("Git de fixture UI dépôts a échoué.".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub(super) async fn scenario(
    client: &Client,
    http: &reqwest::Client,
    address: &str,
    token: &str,
    data: &Path,
) -> Result<()> {
    let repo = data.join("repositories/connector-ui");
    std::fs::create_dir_all(repo.join("src"))?;
    git(&repo, &["init", "-b", "main"])?;
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn synthetic_answer() -> u8 { 0 }\n",
    )?;
    std::fs::write(
        repo.join(".review-fixture"),
        "atelier-review-synthetic-only",
    )?;
    git(&repo, &["add", "."])?;
    git(&repo, &["commit", "-qm", "Synthetic connector target"])?;
    let target = git(&repo, &["rev-parse", "HEAD"])?;
    let origin = "https://synthetic.example.test/owner/repo.git";
    git(&repo, &["remote", "add", "origin", origin])?;
    let card:Card=http.post(format!("{address}/api/cards")).bearer_auth(token).header("x-local-request","1").json(&json!({"title":"UI module repository","description":"Human content retained","repo":repo,"checklist":[{"text":"src/lib.rs returns 42","done":false}]})).send().await?.checked_status().await?.json().await?;
    let prepared: Card = http
        .post(format!("{address}/api/cards/{}/worktree", card.id))
        .bearer_auth(token)
        .header("x-local-request", "1")
        .json(&json!({"branch":"connector-ui-task","base":"main","fetchOrigin":false}))
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let worktree = prepared
        .fields
        .get::<atelier_core::contracts::Worktree>("worktree")?
        .ok_or("Worktree UI du module absent")?;
    let work = PathBuf::from(&worktree.path);
    std::fs::write(
        work.join("src/lib.rs"),
        "pub fn synthetic_answer() -> u8 { 42 }\n",
    )?;
    git(&work, &["add", "."])?;
    git(&work, &["commit", "-qm", "Synthetic connector source"])?;
    let source = git(&work, &["rev-parse", "HEAD"])?;
    std::fs::write(
        data.join(".extension-fixture"),
        "atelier-r04-synthetic-only",
    )?;
    std::fs::write(data.join("repository-observation.json"),json!({"reference":{"instance":"https://synthetic.example.test","repository":"owner/repo","origin":origin},"targetBranch":"main","targetCommit":target,"remoteSource":source,"pullRequest":null}).to_string())?;
    let registry: Registry = http
        .get(format!("{address}/api/extensions"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let profile = registry
        .modules
        .into_iter()
        .find(|profile| profile.installed.manifest.id == "synthetic-extension")
        .ok_or("Module UI absent")?;
    http.put(format!("{address}/api/extensions/synthetic-extension")).bearer_auth(token).header("x-local-request","1").json(&json!({"expectedRevision":profile.installed.revision,"values":{"fault":"repository-lost","fixture-root":data,"site":"https://synthetic.example.test"},"forgetValues":[],"secrets":{},"forgetSecrets":[]})).send().await?.checked_status().await?;
    click(client, "#reload-connectors").await?;
    wait_dom(
        client,
        "return document.querySelector('[data-connector=synthetic-extension]')?.disabled===false;",
        vec![],
    )
    .await?;
    click(client, "[data-connector=synthetic-extension]").await?;
    wait_dom(client,"return Array.from(document.querySelector('#connector-repository-card')?.options||[]).some(option=>option.value===arguments[0]);",vec![json!(card.id)]).await?;
    client.execute("const select=document.querySelector('#connector-repository-card');select.value=arguments[0];select.dispatchEvent(new Event('change',{bubbles:true}));",vec![json!(card.id)]).await?;
    wait_dom(client,"return document.querySelector('#connector-repository-title')?.value==='UI module repository'&&!document.querySelector('#preview-connector-repository')?.disabled;",vec![]).await?;
    click(client, "#preview-connector-repository").await?;
    wait_dom(client,"return !document.querySelector('#prepare-connector-repository')?.disabled&&document.querySelector('#connector-repository-card')?.parentElement.textContent.includes(arguments[0]);",vec![json!(source)]).await?;
    click(client, "#prepare-connector-repository").await?;
    wait_dom(client,"return Array.from(document.querySelector('#prepare-connector-repository')?.closest('fieldset')?.querySelectorAll('[role=status]')||[]).some(element=>element.textContent.startsWith('Aperçu d’envoi enregistré.'));",vec![]).await?;
    let operations: Vec<RemoteOperation> = http
        .get(format!(
            "{address}/api/extensions/synthetic-extension/operations"
        ))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let operation = operations
        .into_iter()
        .find(|operation| {
            operation.method == Method::RepositoriesPublish
                && operation.input["context"]["cardId"] == card.id
        })
        .ok_or("Intention UI dépôt absente")?;
    let scope = format!("[data-connector-operation=\"{}\"]", operation.id);
    wait_dom(client,"return document.querySelector('[data-connector-operation=\"'+arguments[0]+'\"] .confirm-connector-effect')!==null;",vec![json!(operation.id)]).await?;
    click(client, &format!("{scope} .confirm-connector-effect")).await?;
    click(client, &format!("{scope} .send-connector-effect")).await?;
    wait_dom(client,"return document.querySelector('[data-connector-operation=\"'+arguments[0]+'\"] .connector-effect-state')?.textContent==='Effet distant incertain';",vec![json!(operation.id)]).await?;
    let marker = data.join(format!("{}.repository-effect", operation.id));
    let before = std::fs::metadata(&marker)?.modified()?;
    click(client, &format!("{scope} .confirm-connector-effect")).await?;
    click(client, &format!("{scope} .send-connector-effect")).await?;
    wait_dom(client,"return document.querySelector('[data-connector-operation=\"'+arguments[0]+'\"] .connector-effect-state')?.textContent==='Rapproché par le module';",vec![json!(operation.id)]).await?;
    if std::fs::metadata(&marker)?.modified()? != before {
        return Err("Publication rejouée pendant rapprochement UI.".into());
    }
    let fixture = root()
        .join("target/debug")
        .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX));
    http.post(format!("{address}/api/agent-providers")).bearer_auth(token).header("x-local-request","1").json(&json!({"id":"module-review-command","name":"Synthetic module review","type":"command","auth":"cli","command":fixture,"analyzeArgs":["review-json"],"editArgs":["review-json"],"expected":null})).send().await?.checked_status().await?;
    click(client, "#overview-button").await?;
    client.goto(&format!("{address}/?task={}", card.id)).await?;
    wait_dom(client,"return Array.from(document.querySelector('#run-agent')?.options||[]).some(option=>option.value==='module-review-command');",vec![]).await?;
    client.execute("for(const [id,value] of [['run-agent','module-review-command'],['run-purpose','review']]){const select=document.getElementById(id);select.value=value;select.dispatchEvent(new Event('change',{bubbles:true}));}",vec![]).await?;
    click(client, "#start-run").await?;
    wait_dom(client,"return document.querySelector('#review-summary')?.textContent.includes('Lecture du fichier synthétique')&&!document.querySelector('#validate-review')?.disabled;",vec![]).await?;
    click(client, "#validate-review").await?;
    wait_dom(client,"return document.querySelector('#review-freshness')?.textContent.startsWith('Revalidée à :')&&document.querySelector('#review-result')?.textContent.includes(arguments[0])&&document.querySelector('#review-result')?.textContent.includes(arguments[1]);",vec![json!(source),json!(target)]).await?;
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(token)
        .send()
        .await?
        .checked_status()
        .await?
        .json()
        .await?;
    let retained = state["cards"]
        .as_array()
        .and_then(|cards| cards.iter().find(|value| value["id"] == card.id))
        .ok_or("Carte UI module perdue")?;
    if retained["checklist"][0]["done"] != false
        || retained["title"] != "UI module repository"
        || !retained["pullRequest"].is_null()
    {
        return Err("Review module UI a modifié un critère, titre ou liaison native.".into());
    }
    click(client, "#close-editor").await?;
    click(client, "#manage-settings").await?;
    click(client, "#settings-tab-connectors").await?;
    click(client, "#reload-connectors").await?;
    wait_dom(
        client,
        "return document.querySelector('[data-connector=synthetic-extension]')?.disabled===false;",
        vec![],
    )
    .await?;
    click(client, "[data-connector=synthetic-extension]").await?;
    Ok(())
}
