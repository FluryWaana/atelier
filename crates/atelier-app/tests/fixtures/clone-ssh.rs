use std::{
    path::PathBuf,
    process::{Command, Stdio},
};
fn main() {
    let source = PathBuf::from(
        std::env::var_os("ATELIER_SYNTHETIC_SSH_REPOSITORY").expect("synthetic repository"),
    );
    assert_eq!(
        std::fs::read_to_string(source.join(".clone-fixture")).unwrap(),
        "synthetic-only"
    );
    let args = std::env::args().collect::<Vec<_>>();
    assert!(args.iter().any(|arg| arg == "git@clone.example.test"));
    assert!(args.last().unwrap().starts_with("git-upload-pack "));
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if name.starts_with("GIT_")
            && ![
                "GIT_CONFIG_GLOBAL",
                "GIT_CONFIG_SYSTEM",
                "GIT_CONFIG_NOSYSTEM",
                "GIT_PROTOCOL",
            ]
            .contains(&name.as_str())
        {
            command.env_remove(key);
        }
    }
    let status = command
        .arg("upload-pack")
        .arg("--strict")
        .arg(source.join(".git"))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
