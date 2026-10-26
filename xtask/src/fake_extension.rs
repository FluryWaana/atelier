use atelier_core::application::extensions::{
    self, Manifest, Method, PROTOCOL, Reply, Request, Status,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    time::Duration,
};

fn ticket_status(request: &Request) -> Result<String, Box<dyn std::error::Error>> {
    let Some(root) = request
        .settings
        .get("fixtureRoot")
        .and_then(Value::as_str)
        .filter(|root| !root.is_empty())
    else {
        return Ok("Done".into());
    };
    let root = std::fs::canonicalize(root)?;
    if std::fs::read_to_string(root.join(".extension-fixture"))? != "atelier-r04-synthetic-only" {
        return Err("Synthetic fixture marker required".into());
    }
    match std::fs::read_to_string(root.join("synthetic-ticket-state")) {
        Ok(state) if state == "Closed" => Ok(state),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("Done".into()),
        _ => Err("Unexpected synthetic ticket state".into()),
    }
}
fn ticket(status: &str) -> Value {
    json!({"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket","key":"SYN-1","title":"Updated synthetic connector ticket","description":"Updated synthetic description.","url":"https://synthetic.example.test/tenant/tickets/1","status":status})
}

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
        json!({"id":"synthetic-extension","name":"Synthetic extension","description":"No account; guarded local synthetic effects only","sourceLocale":"en-US","methods":["check","ticketsSearch","ticketsRead","ticketsTransitions","ticketsTransition","timePreview","timeSend","timeReconcile","action","reconcile"],"fields":[{"id":"fault","label":"Synthetic fault","kind":"text"},{"id":"fixtureRoot","label":"Marked synthetic directory","kind":"text","credentialScope":true},{"id":"token","label":"Synthetic token","kind":"password"}],"tabs":[{"id":"effects","label":"Synthetic effects","description":"Marked local fixture only","actions":[{"id":"synthetic-write","label":"Synthetic write","effect":"remote"}]}],"translations":{}}),
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
        } else if request.method == Method::TicketsRead {
            if request.input["instance"] != "https://synthetic.example.test/tenant"
                || request.input["id"] != "synthetic-ticket"
            {
                return Err("Unexpected synthetic ticket scope".into());
            }
            ticket(&ticket_status(&request)?)
        } else if request.method == Method::TicketsTransitions {
            if request.input["instance"] != "https://synthetic.example.test/tenant"
                || request.input["id"] != "synthetic-ticket"
            {
                return Err("Unexpected synthetic transition scope".into());
            }
            let status = ticket_status(&request)?;
            json!({"ticket":request.input,"status":status,"transitions":if status=="Done" {json!([{"id":"close","name":"Close synthetic ticket","status":"Closed","requiresFields":false}])}else{json!([])}})
        } else if request.method == Method::TimePreview {
            json!({"source":request.input,"instance":"https://synthetic.example.test/time","account":"synthetic-time-account","reference":"SYN-1"})
        } else {
            json!({"available":true,"privateHome":true})
        },
    };
    if request.method != Method::Describe {
        extensions::effect(&manifest, &request)?;
    }
    if request.method == Method::Action
        || (request.method == Method::Reconcile && request.input["method"] == "action")
    {
        let root = request
            .settings
            .get("fixtureRoot")
            .and_then(Value::as_str)
            .ok_or("Marked synthetic root required")?;
        let root = std::fs::canonicalize(root)?;
        if std::fs::read_to_string(root.join(".extension-fixture"))? != "atelier-r04-synthetic-only"
        {
            return Err("Synthetic fixture marker required".into());
        }
        let id = request
            .operation_id
            .as_deref()
            .ok_or("Synthetic operation ID required")?;
        if !extensions::identifier(id) {
            return Err("Invalid synthetic operation ID".into());
        }
        let marker = root.join(format!("{id}.effect"));
        if request.method == Method::Action {
            if request.action.as_deref() != Some("synthetic-write") {
                return Err("Unexpected synthetic action".into());
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)?;
            file.write_all(b"one synthetic effect")?;
            file.sync_all()?;
            if fault == "lost-write" {
                std::io::stdout().write_all(b"{lost synthetic response")?;
                return Ok(());
            }
        } else if request.input["method"] != "action"
            || request.input["action"] != "synthetic-write"
        {
            return Err("Unexpected synthetic reconciliation scope".into());
        }
        let applied = std::fs::read(&marker)? == b"one synthetic effect";
        reply.output = json!({"receipt":{"operationId":id,"outcome":if applied {"applied"} else {"unknown"},"reference":format!("synthetic:{id}"),"observedAt":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?}});
    }
    if request.method == Method::TicketsTransition
        || request.method == Method::Reconcile && request.input["method"] == "ticketsTransition"
    {
        let root = request
            .settings
            .get("fixtureRoot")
            .and_then(Value::as_str)
            .ok_or("Marked synthetic root required")?;
        let root = std::fs::canonicalize(root)?;
        if std::fs::read_to_string(root.join(".extension-fixture"))? != "atelier-r04-synthetic-only"
        {
            return Err("Synthetic fixture marker required".into());
        }
        let id = request
            .operation_id
            .as_deref()
            .filter(|id| extensions::identifier(id))
            .ok_or("Synthetic operation ID required")?;
        let input = if request.method == Method::TicketsTransition {
            request.input.clone()
        } else {
            request.input["input"].clone()
        };
        let payload = extensions::ticket_transition_payload(&input)?;
        if payload.ticket.instance != "https://synthetic.example.test/tenant"
            || payload.ticket.id != "synthetic-ticket"
            || payload.transition.id != "close"
            || payload.transition.status != "Closed"
        {
            return Err("Unexpected synthetic transition payload".into());
        }
        let marker = root.join(format!("{id}.ticket-effect"));
        if request.method == Method::TicketsTransition {
            if ticket_status(&request)? != payload.from_status {
                return Err("Synthetic transition changed before dispatch".into());
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)?;
            serde_json::to_writer(&mut file, &input)?;
            file.sync_all()?;
            let mut state = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("synthetic-ticket-state"))?;
            state.write_all(b"Closed")?;
            state.sync_all()?;
            if fault == "lost-write" {
                std::io::stdout().write_all(b"{lost synthetic transition response")?;
                return Ok(());
            }
        }
        let applied = serde_json::from_slice::<Value>(&std::fs::read(&marker)?)? == input
            && ticket_status(&request)? == payload.transition.status;
        reply.output = json!({"ticket":ticket(&ticket_status(&request)?),"receipt":{"operationId":id,"outcome":if applied {"applied"}else{"unknown"},"reference":format!("synthetic-transition:{id}"),"observedAt":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?}});
    }
    if matches!(request.method, Method::TimeSend | Method::TimeReconcile) {
        let root = request
            .settings
            .get("fixtureRoot")
            .and_then(Value::as_str)
            .ok_or("Marked synthetic root required")?;
        let root = std::fs::canonicalize(root)?;
        if std::fs::read_to_string(root.join(".extension-fixture"))? != "atelier-r04-synthetic-only"
        {
            return Err("Synthetic fixture marker required".into());
        }
        let id = request
            .operation_id
            .as_deref()
            .filter(|id| extensions::identifier(id))
            .ok_or("Synthetic operation ID required")?;
        let marker = root.join(format!("{id}.time-effect"));
        let proposal = if request.method == Method::TimeSend {
            request.input.clone()
        } else {
            if request.input["method"] != "timeSend" || !request.input["action"].is_null() {
                return Err("Unexpected synthetic time reconciliation scope".into());
            }
            request.input["input"].clone()
        };
        extensions::time_proposal(&proposal)?;
        if request.method == Method::TimeSend {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)?;
            serde_json::to_writer(&mut file, &proposal)?;
            file.sync_all()?;
            if fault == "lost-write" {
                std::io::stdout().write_all(b"{lost synthetic time response")?;
                return Ok(());
            }
        }
        let observed: Value = serde_json::from_slice(&std::fs::read(&marker)?)?;
        reply.output = json!({"time":observed,"receipt":{"operationId":id,"outcome":if observed==proposal {"applied"}else{"unknown"},"reference":format!("synthetic-time:{id}"),"observedAt":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?}});
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
