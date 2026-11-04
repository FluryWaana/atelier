#[path = "agent-cli.rs"]
mod agent_cli;
#[path = "architecture.rs"]
mod architecture;
#[path = "command-runs.rs"]
mod command_runs;
#[path = "connector-repositories.rs"]
mod connector_repositories;
#[path = "extensions.rs"]
mod extensions;
#[path = "interactive-runs.rs"]
mod interactive_runs;
#[path = "keyring-execution.rs"]
mod keyring_execution;
#[path = "macos-git-cache.rs"]
mod macos_git_cache;
#[path = "posix-runtime.rs"]
mod posix_runtime;
#[path = "prepared-ui.rs"]
mod prepared_ui;
#[path = "release-source.rs"]
mod release_source;
#[path = "safari-bootstrap.rs"]
mod safari_bootstrap;
#[path = "translations.rs"]
mod translations;
#[path = "unix-release-source.rs"]
mod unix_release_source;
#[path = "verification-runs.rs"]
mod verification_runs;
#[cfg(windows)]
#[path = "support/windows.rs"]
mod windows;
#[path = "windows-native.rs"]
mod windows_native;
#[path = "windows-tray-contract.rs"]
mod windows_tray_contract;
#[path = "workflow-selection.rs"]
mod workflow_selection;
