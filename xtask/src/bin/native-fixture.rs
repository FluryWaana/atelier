use std::{
    io::{self, Write},
    process::{Command, Stdio},
    time::Duration,
};

#[path = "../fake_agent.rs"]
mod fake_agent;
#[path = "../fake_extension.rs"]
mod fake_extension;

#[cfg(windows)]
#[path = "../../tests/support/windows.rs"]
mod windows;

#[cfg(windows)]
#[path = "../../tests/support/keyring_windows.rs"]
mod keyring_windows;

#[cfg(windows)]
#[path = "../../tests/support/tray.rs"]
mod tray;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "../../tests/support/tray_posix.rs"]
mod tray;

fn ready(value: impl std::fmt::Display) -> io::Result<()> {
    println!("{value}");
    io::stdout().flush()
}

fn no_inherited_credentials() -> Result<(), Box<dyn std::error::Error>> {
    for key in [
        "CODEX_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "JIRA_TOKEN",
        "NODE_OPTIONS",
    ] {
        if std::env::var_os(key).is_some() {
            return Err("Unexpected secret or injection in synthetic CLI environment".into());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => fake_extension::run(),
        ["review-json"] => {
            use std::io::Read;
            no_inherited_credentials()?;
            if std::fs::read_to_string(".review-fixture")?
                != "atelier-review-synthetic-only"
            {
                return Err("Synthetic review scope marker missing".into());
            }
            let mut prompt=Vec::new();std::io::stdin().take(256*1024+1).read_to_end(&mut prompt)?;
            if prompt.len()>256*1024{return Err("Synthetic review prompt too large".into());}
            let source = std::fs::read_to_string("src/lib.rs")?;
            if source != "pub fn synthetic_answer() -> u8 { 42 }\n" {
                return Err("Synthetic review input changed".into());
            }
            serde_json::to_writer(io::stdout().lock(), &serde_json::json!({
                "summary":"Lecture du fichier synthétique ; aucun test ni interface exécuté par cet agent.",
                "findings":[],
                "criteria":[{"index":0,"status":"covered","evidence":"src/lib.rs:1 définit synthetic_answer et retourne la constante 42 ; fichier effectivement lu par le processus synthétique."}]
            }))?;
            ready("")?;
            Ok(())
        }
        #[cfg(unix)]
        ["posix-detached-parent"] => {
            use std::io::{BufRead, BufReader};
            if std::fs::read_to_string(".agent-run-fixture")? != "atelier-r03-synthetic-only" {
                return Err("Synthetic process scope marker missing".into());
            }
            let mut child=Command::new(std::env::current_exe()?).arg("posix-detached-child")
                .stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()?;
            let mut line=String::new();
            BufReader::new(child.stdout.take().ok_or("Synthetic output missing")?).read_line(&mut line)?;
            if line.trim()!=format!("detached-ready {}",child.id()) {return Err("Synthetic detach not confirmed".into());}
            ready(line.trim())?;
            Ok(())
        }
        #[cfg(unix)]
        ["posix-detached-child"] => {
            use std::io::Read;
            if std::fs::read_to_string(".agent-run-fixture")? != "atelier-r03-synthetic-only" {
                return Err("Synthetic process scope marker missing".into());
            }
            // This child deliberately escapes its parent's group to demonstrate the POSIX limit.
            if unsafe{libc::setsid()}<0 {return Err(std::io::Error::last_os_error().into());}
            ready(format!("detached-ready {}",std::process::id()))?;
            let _=std::io::stdin().read(&mut [0u8;1])?;
            Ok(())
        }
        ["--version"] => {
            no_inherited_credentials()?;
            if std::fs::read_to_string(".cli-update-fixture").is_ok_and(|value|value=="atelier-r03-cli-update/synthetic-only") {
                let version=std::fs::read_to_string("installed-version")?;
                if !["0.0.7", "0.0.8"].contains(&version.as_str()){return Err("Synthetic installed version invalid".into());}
                ready(format!("synthetic-agent {version}"))?;
            } else {
                ready("synthetic-agent 0.0.7")?;
            }
            Ok(())
        }
        ["update"] => {
            no_inherited_credentials()?;
            if std::fs::read_to_string(".cli-update-fixture")? != "atelier-r03-cli-update/synthetic-only" {
                return Err("Synthetic update scope missing".into());
            }
            let mut marker=std::fs::OpenOptions::new().write(true).create_new(true).open("update-attempted")?;
            marker.write_all(b"one synthetic native update")?;marker.sync_all()?;
            match std::fs::read_to_string("update-mode")?.as_str() {
                "success"=>{std::fs::write("installed-version","0.0.8")?;ready("private-cli-update-output")?;Ok(())},
                "hang"=>{
                    let mut command=Command::new(std::env::current_exe()?);
                    #[cfg(windows)]
                    {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
                    let mut child=command.arg("idle").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
                    std::fs::write("update-child.pid",child.id().to_string())?;
                    child.wait()?;Ok(())
                },
                _=>Err("Synthetic native update refused".into()),
            }
        }
        ["login", "status"] => {
            if std::env::var("CODEX_HOME")?.ends_with("agents-ui-config") {
                eprintln!("Logged in using synthetic ChatGPT; private-output-must-not-be-returned");
                Ok(())
            } else { Err("Missing synthetic Codex configuration".into()) }
        }
        ["auth", "status"] | ["agent-status", "present"] => {
            ready(r#"{"loggedIn":true,"email":"synthetic@example.invalid","authMethod":"cli","accessToken":"private-output-must-not-be-returned"}"#)?;
            Ok(())
        }
        ["agent-status", "absent"] => { ready(r#"{"loggedIn":false}"#)?; Ok(()) }
        ["agent-status", "invalid"] => { ready("private-output-must-not-be-returned")?; Ok(()) }
        ["agent-version", "flood"] => { io::stdout().write_all(&vec![b'x';65_537])?; Ok(()) }
        ["service-listener", port] => {
            use std::{io::Read, net::{Ipv4Addr, TcpListener}};
            let port = port.parse::<u16>()?;
            if port == 0 { return Err("Synthetic service requires an assigned port".into()); }
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST,port))?;
            listener.set_nonblocking(true)?;
            ready(format!("synthetic-service-ready http://127.0.0.1:{port}/"))?;
            let deadline = std::time::Instant::now() + Duration::from_secs(120);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream,_)) => {
                        stream.set_nonblocking(false)?;
                        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                        let mut request = [0u8;1024];
                        if stream.read(&mut request).is_ok() {
                            let body = "Atelier synthetic service é😀";
                            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                            let _ = stream.write_all(response.as_bytes());
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        }
        #[cfg(windows)]
        ["keyring-account", phase] => keyring_windows::probe(&std::env::current_dir()?, phase),
        ["keyring-echo"] => {
            std::io::copy(&mut io::stdin(), &mut io::stdout())?;
            Ok(())
        }
        ["keyring-flood"] => {
            io::stderr().write_all(&vec![b'x'; 65_537])?;
            io::stderr().flush()?;
            std::thread::sleep(Duration::from_secs(60));
            Ok(())
        }
        ["app-server", ..] => fake_agent::run(true),
        ["-p", ..] => fake_agent::run(false),
        ["run-empty-brainstorm","synthetic-only"] => {
            let cwd=std::env::current_dir()?;
            if !cwd.file_name().and_then(|name|name.to_str()).is_some_and(|name|name.starts_with("atelier-brainstorm-")) || cwd.read_dir()?.next().is_some(){return Err("Empty synthetic conversation directory required".into());}
            let mut prompt=String::new();std::io::Read::read_to_string(&mut io::stdin(),&mut prompt)?;
            if !prompt.contains("Synthetic idea without repo") || !prompt.contains("Conversation sans dépôt"){return Err("Synthetic repository-free prompt required".into());}
            ready(r#"{"message":"Synthetic repository-free reply","questions":[],"tickets":[]}"#)?;Ok(())
        }
        ["verify", mode @ ("success" | "failure" | "modify" | "hang")] => {
            if std::fs::read_to_string(".run-fixture")? != "atelier-r03-synthetic-only" {return Err("Not a verification fixture".into());}
            ready("synthetic-verification <script>ignored</script>")?;
            match *mode {
                "failure"=>std::process::exit(7),
                "modify"=>std::fs::write(".run-fixture","synthetic changed source")?,
                "hang"=>loop{std::thread::sleep(Duration::from_secs(60));},
                _=>{},
            }
            Ok(())
        }
        ["run-command", mode @ ("success" | "hang" | "brainstorm" | "preparation")] => {
            if std::fs::read_to_string(".run-fixture")? != "atelier-r03-synthetic-only" { return Err("Not a run test fixture".into()); }
            let mut prompt = String::new();
            std::io::Read::read_to_string(&mut io::stdin(), &mut prompt)?;
            if prompt.is_empty() { return Err("Missing synthetic task input".into()); }
            if *mode=="preparation" {
                if !prompt.contains("Besoin enregistré") || !prompt.contains("Synthetic converted task"){return Err("Synthetic preparation prompt required".into());}
                ready(r#"{"criteria":["Synthetic prepared criterion"],"questions":["Synthetic preparation question"]}"#)?;return Ok(());
            }
            if *mode=="brainstorm" {
                if !prompt.contains("Données de la conversation") || !prompt.contains("Synthetic structured proposal") {return Err("Missing synthetic conversation history".into());}
                if prompt.contains("Second synthetic question") && !prompt.contains("Synthetic agent conversation reply") {return Err("Missing preceding synthetic answer".into());}
                ready(r#"{"message":"Synthetic agent conversation reply","questions":[],"tickets":[]}"#)?;
                return Ok(());
            }
            io::stdout().write_all(b"<script>window.syntheticRunExecuted=true</script>\n")?;
            io::stdout().write_all("Commande synthétique é😀\n".as_bytes())?;
            io::stdout().flush()?;
            if *mode == "hang" { loop { std::thread::sleep(Duration::from_secs(60)); } }
            Ok(())
        }
        ["process-streams"] => {
            let stderr = std::thread::spawn(|| -> io::Result<()> {
                io::stderr().write_all(&vec![b'e'; 200_000])
            });
            io::stdout().write_all(&vec![b'o'; 200_000])?;
            stderr.join().map_err(|_| "Synthetic stderr writer panicked")??;
            std::io::copy(&mut io::stdin(), &mut io::stdout())?;
            Ok(())
        }
        ["process-output", mode @ ("flood" | "hang" | "failure"), directory] => {
            use std::fs;
            let directory = fs::canonicalize(directory)?;
            if fs::read_to_string(directory.join(".process-fixture"))? != "atelier-m04-synthetic-only" {
                return Err("Not a process test fixture".into());
            }
            let mut command=Command::new(std::env::current_exe()?);
            #[cfg(windows)]
            { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
            let mut child = command.arg("idle")
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
            fs::write(directory.join("pid"), child.id().to_string())?;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !directory.join("go").exists() {
                if std::time::Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); return Err("Missing synthetic output gate".into()); }
                std::thread::sleep(Duration::from_millis(10));
            }
            match *mode {
                "failure" => { io::stderr().write_all(b"synthetic failure\n")?; std::process::exit(7); }
                "flood" => { io::stderr().write_all(&vec![b'e'; 32_000])?; }
                _ => {}
            }
            child.wait()?;
            Ok(())
        }
        ["idle"] => {
            ready("ready")?;
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        ["family"] => {
            ready("ready")?;
            let mut gate = String::new();
            io::stdin().read_line(&mut gate)?;
            if gate.trim() != "spawn" {
                return Err("Missing test gate".into());
            }
            let mut command = Command::new(std::env::current_exe()?);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .arg("idle")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            ready(child.id())?;
            child.wait()?;
            Ok(())
        }
        #[cfg(windows)]
        ["family-immediate", mode @ ("normal" | "detached" | "orphan" | "breakaway")] => {
            use std::os::windows::process::CommandExt;
            let flags = match *mode {
                "detached" => 0x00000008,
                "breakaway" => 0x01000000 | 0x08000000,
                _ => 0x08000000,
            };
            let spawned = Command::new(std::env::current_exe()?).arg("idle")
                .creation_flags(flags).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
            if *mode == "breakaway" {
                return match spawned {
                    Err(error) if error.raw_os_error() == Some(5) => ready("breakaway-refused").map_err(Into::into),
                    Err(error) => Err(error.into()),
                    Ok(mut child) => { let _ = child.kill(); let _ = child.wait(); Err("The child escaped its job".into()) }
                };
            }
            let mut child = spawned?;
            ready(child.id())?;
            if *mode != "orphan" { child.wait()?; }
            Ok(())
        }
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        ["tray"] => tray::probe(),
        #[cfg(windows)]
        ["worker", directory, pid, created] => {
            use std::fs;
            let directory = fs::canonicalize(directory)?;
            if fs::read_to_string(directory.join(".native-fixture"))?
                != "atelier-m01-synthetic-only"
            {
                return Err("Not a native test fixture".into());
            }
            if fs::canonicalize(std::env::current_exe()?)?
                != fs::canonicalize(directory.join("stage/worker.exe"))?
            {
                return Err("Worker must run from its staged copy".into());
            }
            for name in ["app.exe", "candidate.exe"] {
                if !fs::symlink_metadata(directory.join(name))?.is_file() {
                    return Err("Regular fixture executable required".into());
                }
            }
            let process = windows::open_process(pid.parse()?)?;
            if windows::creation_time(&process)? != created.parse::<u64>()? {
                return Err("Process identity mismatch".into());
            }
            ready("ready")?;
            if windows::wait(&process, 10_000) != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
                return Err("Process did not exit; replacement refused".into());
            }
            // shortcut: this is a synthetic feasibility probe, not the transactional M09 updater.
            fs::copy(directory.join("candidate.exe"), directory.join("app.exe"))?;
            Ok(())
        }
        _ => Err(
            "Native test fixture only: idle | family | worker DIRECTORY PID CREATION_TIME | tray | keyring-account PHASE"
                .into(),
        ),
    }
}
