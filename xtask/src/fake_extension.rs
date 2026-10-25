use atelier_core::application::extensions::{
    self, Manifest, Method, PROTOCOL, Reply, Request, Status,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    time::Duration,
};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    std::io::stdin()
        .take((extensions::INPUT_LIMIT + 1) as u64)
        .read_to_end(&mut input)?;
    if input.len() > extensions::INPUT_LIMIT {
        return Err("Synthetic extension input bound exceeded".into());
    }
    let request: Request = serde_json::from_slice(&input)?;
    if request.module != "synthetic-extension" {
        return Err("Synthetic extension identity required".into());
    }
    for key in [
        "CODEX_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "JIRA_TOKEN",
        "TEMPO_TOKEN",
        "NODE_OPTIONS",
        "GIT_CONFIG_COUNT",
        "LC_SYNTHETIC_SECRET",
    ] {
        if std::env::var_os(key).is_some() {
            return Err("Unexpected inherited credential or injection".into());
        }
    }
    let cwd = std::env::current_dir()?;
    for (key, directory) in [("HOME", "home"), ("USERPROFILE", "home"), ("TEMP", "temp")] {
        let supplied =
            std::env::var_os(key).ok_or("Private synthetic extension environment required")?;
        if std::fs::canonicalize(supplied)? != std::fs::canonicalize(cwd.join(directory))? {
            return Err("Private synthetic extension environment required".into());
        }
    }
    let fault = request
        .settings
        .get("fault")
        .and_then(Value::as_str)
        .unwrap_or("success");
    let manifest: Manifest = serde_json::from_value(
        json!({"id":"synthetic-extension","name":"Synthetic extension","description":"No account or remote effect","sourceLocale":"en-US","methods":["check","ticketsSearch"],"fields":[{"id":"fault","label":"Synthetic fault","kind":"text"},{"id":"token","label":"Synthetic token","kind":"password"}],"tabs":[],"translations":{}}),
    )?;
    let mut reply = Reply {
        protocol: PROTOCOL.into(),
        request_id: request.request_id.clone(),
        module: request.module.clone(),
        method: request.method,
        status: Status::Ok,
        message: "Synthetic extension ready".into(),
        output: if request.method == Method::Describe {
            serde_json::to_value(&manifest)?
        } else if request.method == Method::TicketsSearch {
            json!({"items":[{"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket","key":"SYN-1","title":"Synthetic connector ticket","description":"No remote account or paid agent.","url":"https://synthetic.example.test/tenant/tickets/1","status":"Open"}],"next":null})
        } else {
            json!({"available":true,"privateHome":true})
        },
    };
    if request.method != Method::Describe {
        extensions::effect(&manifest, &request)?;
    }
    match fault {
        "secret" => reply.output = json!({"nested":[{"value":request.secrets.get("token")}]}),
        "secret-key" => {
            let mut object = serde_json::Map::new();
            object.insert(
                request
                    .secrets
                    .get("token")
                    .ok_or("Synthetic token required")?
                    .clone(),
                Value::Null,
            );
            reply.output = Value::Object(object);
        }
        "stderr" => {
            eprintln!(
                "{}",
                request
                    .secrets
                    .get("token")
                    .ok_or("Synthetic token required")?
            );
            return Err("Synthetic nonzero exit".into());
        }
        "identity" => reply.request_id = "request-other".into(),
        "flood" => {
            std::io::stdout().write_all(&vec![b'x'; extensions::OUTPUT_LIMIT + 1])?;
            return Ok(());
        }
        "hang" => {
            let root = request.input["fixtureRoot"]
                .as_str()
                .ok_or("Synthetic fixture root required")?;
            let root = std::fs::canonicalize(root)?;
            if std::fs::read_to_string(root.join(".extension-fixture"))?
                != "atelier-r04-synthetic-only"
            {
                return Err("Synthetic fixture marker required".into());
            }
            let mut command = std::process::Command::new(std::env::current_exe()?);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .arg("idle")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            std::fs::write(root.join("extension-child.pid"), child.id().to_string())?;
            std::thread::sleep(Duration::from_secs(120));
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        _ => {}
    }
    let mut bytes = serde_json::to_vec(&reply)?;
    if fault == "extra" {
        bytes.extend_from_slice(b"\n{}");
    }
    if fault == "partial" {
        bytes.pop();
    }
    std::io::stdout().write_all(&bytes)?;
    std::io::stdout().flush()?;
    Ok(())
}
