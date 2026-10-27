# Inventaire des sources Rust

Généré par `cargo xtask inventory`. Vérification sans écriture : `cargo xtask inventory --check`.

Cartographie lexicale des fichiers actuels, sans analyse des appels ni preuve de compilation, de couverture ou de réussite. Les références fonctionnelles de l’ancien projet se consultent dans Git au commit `7f48996` ; elles ne font pas partie du projet actif.

Périmètre : `crates/`, `xtask/`, `scripts/`, `.github/`, `.cargo/` et les fichiers racine Cargo, toolchain, README, AGENTS, LICENSE et règles Git. Les segments `target`, `data`, `.artifacts`, `.git` et `runtime` sont exclus ; les liens et points de réanalyse sont refusés. Aucune donnée utilisateur ni contenu de source n’est lu.

Le plan et les capacités restent décrits dans [rust-migration-plan.md](rust-migration-plan.md) et [rust-rebuild-features.md](rust-rebuild-features.md).

## Répartition

| Périmètre | Fichiers |
| --- | ---: |
| crates/atelier-core | 66 |
| crates/atelier-app | 184 |
| crates/atelier-ui | 53 |
| xtask | 100 |
| scripts | 4 |
| .github | 1 |
| .cargo | 1 |
| racine | 9 |

Total : 418 fichiers.

## Fichiers

| Fichier | Nature |
| --- | --- |
| [.cargo/config.toml](../.cargo/config.toml) | Configuration |
| [.gitattributes](../.gitattributes) | Configuration |
| [.github/workflows/rust.yml](../.github/workflows/rust.yml) | Configuration |
| [.gitignore](../.gitignore) | Configuration |
| [AGENTS.md](../AGENTS.md) | Ressource / documentation |
| [Cargo.lock](../Cargo.lock) | Configuration |
| [Cargo.toml](../Cargo.toml) | Configuration |
| [LICENSE](../LICENSE) | Ressource / documentation |
| [Preview.cmd](../Preview.cmd) | Ressource / documentation |
| [README.md](../README.md) | Ressource / documentation |
| [crates/atelier-app/Cargo.toml](../crates/atelier-app/Cargo.toml) | Configuration |
| [crates/atelier-app/examples/connector-demo.rs](../crates/atelier-app/examples/connector-demo.rs) | Test / support |
| [crates/atelier-app/src/http.rs](../crates/atelier-app/src/http.rs) | Source Rust |
| [crates/atelier-app/src/http/agent_cli.rs](../crates/atelier-app/src/http/agent_cli.rs) | Source Rust |
| [crates/atelier-app/src/http/agent_providers.rs](../crates/atelier-app/src/http/agent_providers.rs) | Source Rust |
| [crates/atelier-app/src/http/assets.rs](../crates/atelier-app/src/http/assets.rs) | Source Rust |
| [crates/atelier-app/src/http/automation.rs](../crates/atelier-app/src/http/automation.rs) | Source Rust |
| [crates/atelier-app/src/http/backups.rs](../crates/atelier-app/src/http/backups.rs) | Source Rust |
| [crates/atelier-app/src/http/brainstorm.rs](../crates/atelier-app/src/http/brainstorm.rs) | Source Rust |
| [crates/atelier-app/src/http/card_lifecycle.rs](../crates/atelier-app/src/http/card_lifecycle.rs) | Source Rust |
| [crates/atelier-app/src/http/commits.rs](../crates/atelier-app/src/http/commits.rs) | Source Rust |
| [crates/atelier-app/src/http/connector_effects.rs](../crates/atelier-app/src/http/connector_effects.rs) | Source Rust |
| [crates/atelier-app/src/http/connector_repositories.rs](../crates/atelier-app/src/http/connector_repositories.rs) | Source Rust |
| [crates/atelier-app/src/http/connector_ticket_reset.rs](../crates/atelier-app/src/http/connector_ticket_reset.rs) | Source Rust |
| [crates/atelier-app/src/http/connectors.rs](../crates/atelier-app/src/http/connectors.rs) | Source Rust |
| [crates/atelier-app/src/http/dev_reload.js](../crates/atelier-app/src/http/dev_reload.js) | Ressource / documentation |
| [crates/atelier-app/src/http/events.rs](../crates/atelier-app/src/http/events.rs) | Source Rust |
| [crates/atelier-app/src/http/files.rs](../crates/atelier-app/src/http/files.rs) | Source Rust |
| [crates/atelier-app/src/http/folders.rs](../crates/atelier-app/src/http/folders.rs) | Source Rust |
| [crates/atelier-app/src/http/forge.rs](../crates/atelier-app/src/http/forge.rs) | Source Rust |
| [crates/atelier-app/src/http/forge_draft.rs](../crates/atelier-app/src/http/forge_draft.rs) | Source Rust |
| [crates/atelier-app/src/http/forge_publication.rs](../crates/atelier-app/src/http/forge_publication.rs) | Source Rust |
| [crates/atelier-app/src/http/git_push.rs](../crates/atelier-app/src/http/git_push.rs) | Source Rust |
| [crates/atelier-app/src/http/jira.rs](../crates/atelier-app/src/http/jira.rs) | Source Rust |
| [crates/atelier-app/src/http/jira_creation.rs](../crates/atelier-app/src/http/jira_creation.rs) | Source Rust |
| [crates/atelier-app/src/http/jira_refresh.rs](../crates/atelier-app/src/http/jira_refresh.rs) | Source Rust |
| [crates/atelier-app/src/http/jira_transitions.rs](../crates/atelier-app/src/http/jira_transitions.rs) | Source Rust |
| [crates/atelier-app/src/http/mutations.rs](../crates/atelier-app/src/http/mutations.rs) | Source Rust |
| [crates/atelier-app/src/http/preparation.rs](../crates/atelier-app/src/http/preparation.rs) | Source Rust |
| [crates/atelier-app/src/http/projects.rs](../crates/atelier-app/src/http/projects.rs) | Source Rust |
| [crates/atelier-app/src/http/queue.rs](../crates/atelier-app/src/http/queue.rs) | Source Rust |
| [crates/atelier-app/src/http/repository_access.rs](../crates/atelier-app/src/http/repository_access.rs) | Source Rust |
| [crates/atelier-app/src/http/responses.rs](../crates/atelier-app/src/http/responses.rs) | Source Rust |
| [crates/atelier-app/src/http/reviews.rs](../crates/atelier-app/src/http/reviews.rs) | Source Rust |
| [crates/atelier-app/src/http/runs.rs](../crates/atelier-app/src/http/runs.rs) | Source Rust |
| [crates/atelier-app/src/http/services.rs](../crates/atelier-app/src/http/services.rs) | Source Rust |
| [crates/atelier-app/src/http/settings.rs](../crates/atelier-app/src/http/settings.rs) | Source Rust |
| [crates/atelier-app/src/http/task_creation.rs](../crates/atelier-app/src/http/task_creation.rs) | Source Rust |
| [crates/atelier-app/src/http/task_editing.rs](../crates/atelier-app/src/http/task_editing.rs) | Source Rust |
| [crates/atelier-app/src/http/task_templates.rs](../crates/atelier-app/src/http/task_templates.rs) | Source Rust |
| [crates/atelier-app/src/http/tempo.rs](../crates/atelier-app/src/http/tempo.rs) | Source Rust |
| [crates/atelier-app/src/http/tempo_export.rs](../crates/atelier-app/src/http/tempo_export.rs) | Source Rust |
| [crates/atelier-app/src/http/time_tracking.rs](../crates/atelier-app/src/http/time_tracking.rs) | Source Rust |
| [crates/atelier-app/src/http/updates.rs](../crates/atelier-app/src/http/updates.rs) | Source Rust |
| [crates/atelier-app/src/http/workflow_publication.rs](../crates/atelier-app/src/http/workflow_publication.rs) | Source Rust |
| [crates/atelier-app/src/http/worktrees.rs](../crates/atelier-app/src/http/worktrees.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/agent_account.rs](../crates/atelier-app/src/infrastructure/agent_account.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/agent_cli.rs](../crates/atelier-app/src/infrastructure/agent_cli.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/agent_context.rs](../crates/atelier-app/src/infrastructure/agent_context.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/agent_diagnostics.rs](../crates/atelier-app/src/infrastructure/agent_diagnostics.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/agent_providers.rs](../crates/atelier-app/src/infrastructure/agent_providers.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/automation.rs](../crates/atelier-app/src/infrastructure/automation.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/azure_tickets.rs](../crates/atelier-app/src/infrastructure/azure_tickets.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/backups.rs](../crates/atelier-app/src/infrastructure/backups.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/brainstorm.rs](../crates/atelier-app/src/infrastructure/brainstorm.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/checked_file.rs](../crates/atelier-app/src/infrastructure/checked_file.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/claude_account.rs](../crates/atelier-app/src/infrastructure/claude_account.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/command_runs.rs](../crates/atelier-app/src/infrastructure/command_runs.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/commits.rs](../crates/atelier-app/src/infrastructure/commits.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connector_effects.rs](../crates/atelier-app/src/infrastructure/connector_effects.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connector_repositories.rs](../crates/atelier-app/src/infrastructure/connector_repositories.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connector_ticket_reset.rs](../crates/atelier-app/src/infrastructure/connector_ticket_reset.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connector_ticket_transitions.rs](../crates/atelier-app/src/infrastructure/connector_ticket_transitions.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connector_tickets.rs](../crates/atelier-app/src/infrastructure/connector_tickets.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connector_time.rs](../crates/atelier-app/src/infrastructure/connector_time.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/connectors.rs](../crates/atelier-app/src/infrastructure/connectors.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/data_lock.rs](../crates/atelier-app/src/infrastructure/data_lock.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/database.rs](../crates/atelier-app/src/infrastructure/database.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/dependencies.rs](../crates/atelier-app/src/infrastructure/dependencies.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/dev_servers.rs](../crates/atelier-app/src/infrastructure/dev_servers.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/directory_link.rs](../crates/atelier-app/src/infrastructure/directory_link.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/extensions.rs](../crates/atelier-app/src/infrastructure/extensions.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/files.rs](../crates/atelier-app/src/infrastructure/files.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/folders.rs](../crates/atelier-app/src/infrastructure/folders.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/forge.rs](../crates/atelier-app/src/infrastructure/forge.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/forge/checks.rs](../crates/atelier-app/src/infrastructure/forge/checks.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/forge_association.rs](../crates/atelier-app/src/infrastructure/forge_association.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/forge_draft.rs](../crates/atelier-app/src/infrastructure/forge_draft.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/forge_publication.rs](../crates/atelier-app/src/infrastructure/forge_publication.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/git_push.rs](../crates/atelier-app/src/infrastructure/git_push.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/git_transport.rs](../crates/atelier-app/src/infrastructure/git_transport.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/installation.rs](../crates/atelier-app/src/infrastructure/installation.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/installed.rs](../crates/atelier-app/src/infrastructure/installed.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/interactive_runs.rs](../crates/atelier-app/src/infrastructure/interactive_runs.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/jira.rs](../crates/atelier-app/src/infrastructure/jira.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/jira_creation.rs](../crates/atelier-app/src/infrastructure/jira_creation.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/jira_import.rs](../crates/atelier-app/src/infrastructure/jira_import.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/jira_refresh.rs](../crates/atelier-app/src/infrastructure/jira_refresh.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/jira_transition.rs](../crates/atelier-app/src/infrastructure/jira_transition.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/launch_detection.rs](../crates/atelier-app/src/infrastructure/launch_detection.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/launch_ports.rs](../crates/atelier-app/src/infrastructure/launch_ports.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/launcher.rs](../crates/atelier-app/src/infrastructure/launcher.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/local_commit.rs](../crates/atelier-app/src/infrastructure/local_commit.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/local_http.rs](../crates/atelier-app/src/infrastructure/local_http.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/mod.rs](../crates/atelier-app/src/infrastructure/mod.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/package.rs](../crates/atelier-app/src/infrastructure/package.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/package_archive.rs](../crates/atelier-app/src/infrastructure/package_archive.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/package_maintenance.rs](../crates/atelier-app/src/infrastructure/package_maintenance.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/portable_import.rs](../crates/atelier-app/src/infrastructure/portable_import.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/preparation.rs](../crates/atelier-app/src/infrastructure/preparation.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/process_inspection.rs](../crates/atelier-app/src/infrastructure/process_inspection.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/process_inspection/unix.rs](../crates/atelier-app/src/infrastructure/process_inspection/unix.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/process_tree.rs](../crates/atelier-app/src/infrastructure/process_tree.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/process_tree/unix.rs](../crates/atelier-app/src/infrastructure/process_tree/unix.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/process_tree/windows.rs](../crates/atelier-app/src/infrastructure/process_tree/windows.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/projects.rs](../crates/atelier-app/src/infrastructure/projects.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/prototype.rs](../crates/atelier-app/src/infrastructure/prototype.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/publish.rs](../crates/atelier-app/src/infrastructure/publish.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/queue.rs](../crates/atelier-app/src/infrastructure/queue.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/repository_access.rs](../crates/atelier-app/src/infrastructure/repository_access.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/reviews.rs](../crates/atelier-app/src/infrastructure/reviews.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/runs.rs](../crates/atelier-app/src/infrastructure/runs.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/service_launch.rs](../crates/atelier-app/src/infrastructure/service_launch.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/session.rs](../crates/atelier-app/src/infrastructure/session.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/settings.rs](../crates/atelier-app/src/infrastructure/settings.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/store.rs](../crates/atelier-app/src/infrastructure/store.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/synthetic_tls.rs](../crates/atelier-app/src/infrastructure/synthetic_tls.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/system_package.rs](../crates/atelier-app/src/infrastructure/system_package.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/task_creation.rs](../crates/atelier-app/src/infrastructure/task_creation.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/task_editing.rs](../crates/atelier-app/src/infrastructure/task_editing.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/task_templates.rs](../crates/atelier-app/src/infrastructure/task_templates.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/tempo.rs](../crates/atelier-app/src/infrastructure/tempo.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/tempo/client.rs](../crates/atelier-app/src/infrastructure/tempo/client.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/tempo_export.rs](../crates/atelier-app/src/infrastructure/tempo_export.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/time_tracking.rs](../crates/atelier-app/src/infrastructure/time_tracking.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/token_vault.rs](../crates/atelier-app/src/infrastructure/token_vault.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/tray.rs](../crates/atelier-app/src/infrastructure/tray.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/update_package.rs](../crates/atelier-app/src/infrastructure/update_package.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/updates.rs](../crates/atelier-app/src/infrastructure/updates.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/verification.rs](../crates/atelier-app/src/infrastructure/verification.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/windows_registration.rs](../crates/atelier-app/src/infrastructure/windows_registration.rs) | Source Rust |
| [crates/atelier-app/src/infrastructure/worktrees.rs](../crates/atelier-app/src/infrastructure/worktrees.rs) | Source Rust |
| [crates/atelier-app/src/lib.rs](../crates/atelier-app/src/lib.rs) | Source Rust |
| [crates/atelier-app/src/main.rs](../crates/atelier-app/src/main.rs) | Source Rust |
| [crates/atelier-app/tests/agent-cli-catalog.rs](../crates/atelier-app/tests/agent-cli-catalog.rs) | Test / support |
| [crates/atelier-app/tests/azure-tickets.rs](../crates/atelier-app/tests/azure-tickets.rs) | Test / support |
| [crates/atelier-app/tests/brainstorm.rs](../crates/atelier-app/tests/brainstorm.rs) | Test / support |
| [crates/atelier-app/tests/claude-account.rs](../crates/atelier-app/tests/claude-account.rs) | Test / support |
| [crates/atelier-app/tests/connector-effects.rs](../crates/atelier-app/tests/connector-effects.rs) | Test / support |
| [crates/atelier-app/tests/connector-time.rs](../crates/atelier-app/tests/connector-time.rs) | Test / support |
| [crates/atelier-app/tests/connector-worker-guards.rs](../crates/atelier-app/tests/connector-worker-guards.rs) | Test / support |
| [crates/atelier-app/tests/document-storage.rs](../crates/atelier-app/tests/document-storage.rs) | Test / support |
| [crates/atelier-app/tests/files.rs](../crates/atelier-app/tests/files.rs) | Test / support |
| [crates/atelier-app/tests/fixtures/rust-jira-url-reference.json](../crates/atelier-app/tests/fixtures/rust-jira-url-reference.json) | Test / support |
| [crates/atelier-app/tests/fixtures/rust-time-reference.json](../crates/atelier-app/tests/fixtures/rust-time-reference.json) | Test / support |
| [crates/atelier-app/tests/forge-association.rs](../crates/atelier-app/tests/forge-association.rs) | Test / support |
| [crates/atelier-app/tests/forge-checks.rs](../crates/atelier-app/tests/forge-checks.rs) | Test / support |
| [crates/atelier-app/tests/forge-content.rs](../crates/atelier-app/tests/forge-content.rs) | Test / support |
| [crates/atelier-app/tests/forge-draft.rs](../crates/atelier-app/tests/forge-draft.rs) | Test / support |
| [crates/atelier-app/tests/forge-lists.rs](../crates/atelier-app/tests/forge-lists.rs) | Test / support |
| [crates/atelier-app/tests/forge-publication-storage.rs](../crates/atelier-app/tests/forge-publication-storage.rs) | Test / support |
| [crates/atelier-app/tests/forge-publication.rs](../crates/atelier-app/tests/forge-publication.rs) | Test / support |
| [crates/atelier-app/tests/forge.rs](../crates/atelier-app/tests/forge.rs) | Test / support |
| [crates/atelier-app/tests/git-push.rs](../crates/atelier-app/tests/git-push.rs) | Test / support |
| [crates/atelier-app/tests/git-transport.rs](../crates/atelier-app/tests/git-transport.rs) | Test / support |
| [crates/atelier-app/tests/jira-creation.rs](../crates/atelier-app/tests/jira-creation.rs) | Test / support |
| [crates/atelier-app/tests/jira-http.rs](../crates/atelier-app/tests/jira-http.rs) | Test / support |
| [crates/atelier-app/tests/jira-import.rs](../crates/atelier-app/tests/jira-import.rs) | Test / support |
| [crates/atelier-app/tests/jira-refresh.rs](../crates/atelier-app/tests/jira-refresh.rs) | Test / support |
| [crates/atelier-app/tests/jira-storage.rs](../crates/atelier-app/tests/jira-storage.rs) | Test / support |
| [crates/atelier-app/tests/jira-transitions.rs](../crates/atelier-app/tests/jira-transitions.rs) | Test / support |
| [crates/atelier-app/tests/launcher.rs](../crates/atelier-app/tests/launcher.rs) | Test / support |
| [crates/atelier-app/tests/local-commit-storage.rs](../crates/atelier-app/tests/local-commit-storage.rs) | Test / support |
| [crates/atelier-app/tests/local-commit.rs](../crates/atelier-app/tests/local-commit.rs) | Test / support |
| [crates/atelier-app/tests/local-http.rs](../crates/atelier-app/tests/local-http.rs) | Test / support |
| [crates/atelier-app/tests/native-import.rs](../crates/atelier-app/tests/native-import.rs) | Test / support |
| [crates/atelier-app/tests/native-tray.rs](../crates/atelier-app/tests/native-tray.rs) | Test / support |
| [crates/atelier-app/tests/package-maintenance.rs](../crates/atelier-app/tests/package-maintenance.rs) | Test / support |
| [crates/atelier-app/tests/preparation.rs](../crates/atelier-app/tests/preparation.rs) | Test / support |
| [crates/atelier-app/tests/process-tools-unix.rs](../crates/atelier-app/tests/process-tools-unix.rs) | Test / support |
| [crates/atelier-app/tests/prototype.rs](../crates/atelier-app/tests/prototype.rs) | Test / support |
| [crates/atelier-app/tests/queue.rs](../crates/atelier-app/tests/queue.rs) | Test / support |
| [crates/atelier-app/tests/repository-access.rs](../crates/atelier-app/tests/repository-access.rs) | Test / support |
| [crates/atelier-app/tests/reviews.rs](../crates/atelier-app/tests/reviews.rs) | Test / support |
| [crates/atelier-app/tests/run-storage.rs](../crates/atelier-app/tests/run-storage.rs) | Test / support |
| [crates/atelier-app/tests/settings.rs](../crates/atelier-app/tests/settings.rs) | Test / support |
| [crates/atelier-app/tests/support/tls.rs](../crates/atelier-app/tests/support/tls.rs) | Test / support |
| [crates/atelier-app/tests/synthetic-tls.rs](../crates/atelier-app/tests/synthetic-tls.rs) | Test / support |
| [crates/atelier-app/tests/task-templates.rs](../crates/atelier-app/tests/task-templates.rs) | Test / support |
| [crates/atelier-app/tests/tempo-export-storage.rs](../crates/atelier-app/tests/tempo-export-storage.rs) | Test / support |
| [crates/atelier-app/tests/tempo.rs](../crates/atelier-app/tests/tempo.rs) | Test / support |
| [crates/atelier-app/tests/time-tracking.rs](../crates/atelier-app/tests/time-tracking.rs) | Test / support |
| [crates/atelier-app/tests/verification.rs](../crates/atelier-app/tests/verification.rs) | Test / support |
| [crates/atelier-app/tests/worktrees.rs](../crates/atelier-app/tests/worktrees.rs) | Test / support |
| [crates/atelier-core/Cargo.toml](../crates/atelier-core/Cargo.toml) | Configuration |
| [crates/atelier-core/src/application.rs](../crates/atelier-core/src/application.rs) | Source Rust |
| [crates/atelier-core/src/application/agent_account.rs](../crates/atelier-core/src/application/agent_account.rs) | Source Rust |
| [crates/atelier-core/src/application/agent_cli.rs](../crates/atelier-core/src/application/agent_cli.rs) | Source Rust |
| [crates/atelier-core/src/application/agent_protocol.rs](../crates/atelier-core/src/application/agent_protocol.rs) | Source Rust |
| [crates/atelier-core/src/application/agent_providers.rs](../crates/atelier-core/src/application/agent_providers.rs) | Source Rust |
| [crates/atelier-core/src/application/agent_usage.rs](../crates/atelier-core/src/application/agent_usage.rs) | Source Rust |
| [crates/atelier-core/src/application/automation.rs](../crates/atelier-core/src/application/automation.rs) | Source Rust |
| [crates/atelier-core/src/application/backups.rs](../crates/atelier-core/src/application/backups.rs) | Source Rust |
| [crates/atelier-core/src/application/brainstorm.rs](../crates/atelier-core/src/application/brainstorm.rs) | Source Rust |
| [crates/atelier-core/src/application/card_lifecycle.rs](../crates/atelier-core/src/application/card_lifecycle.rs) | Source Rust |
| [crates/atelier-core/src/application/extension_repositories.rs](../crates/atelier-core/src/application/extension_repositories.rs) | Source Rust |
| [crates/atelier-core/src/application/extension_ticket_reset.rs](../crates/atelier-core/src/application/extension_ticket_reset.rs) | Source Rust |
| [crates/atelier-core/src/application/extensions.rs](../crates/atelier-core/src/application/extensions.rs) | Source Rust |
| [crates/atelier-core/src/application/forge.rs](../crates/atelier-core/src/application/forge.rs) | Source Rust |
| [crates/atelier-core/src/application/jira.rs](../crates/atelier-core/src/application/jira.rs) | Source Rust |
| [crates/atelier-core/src/application/preparation.rs](../crates/atelier-core/src/application/preparation.rs) | Source Rust |
| [crates/atelier-core/src/application/projects.rs](../crates/atelier-core/src/application/projects.rs) | Source Rust |
| [crates/atelier-core/src/application/reviews.rs](../crates/atelier-core/src/application/reviews.rs) | Source Rust |
| [crates/atelier-core/src/application/services.rs](../crates/atelier-core/src/application/services.rs) | Source Rust |
| [crates/atelier-core/src/application/settings.rs](../crates/atelier-core/src/application/settings.rs) | Source Rust |
| [crates/atelier-core/src/application/task_creation.rs](../crates/atelier-core/src/application/task_creation.rs) | Source Rust |
| [crates/atelier-core/src/application/task_editing.rs](../crates/atelier-core/src/application/task_editing.rs) | Source Rust |
| [crates/atelier-core/src/application/task_templates.rs](../crates/atelier-core/src/application/task_templates.rs) | Source Rust |
| [crates/atelier-core/src/application/time_reports.rs](../crates/atelier-core/src/application/time_reports.rs) | Source Rust |
| [crates/atelier-core/src/application/time_tracking.rs](../crates/atelier-core/src/application/time_tracking.rs) | Source Rust |
| [crates/atelier-core/src/application/updates.rs](../crates/atelier-core/src/application/updates.rs) | Source Rust |
| [crates/atelier-core/src/application/worktrees.rs](../crates/atelier-core/src/application/worktrees.rs) | Source Rust |
| [crates/atelier-core/src/contracts.rs](../crates/atelier-core/src/contracts.rs) | Source Rust |
| [crates/atelier-core/src/document.rs](../crates/atelier-core/src/document.rs) | Source Rust |
| [crates/atelier-core/src/domain.rs](../crates/atelier-core/src/domain.rs) | Source Rust |
| [crates/atelier-core/src/domain/agent_context.rs](../crates/atelier-core/src/domain/agent_context.rs) | Source Rust |
| [crates/atelier-core/src/domain/agent_options.rs](../crates/atelier-core/src/domain/agent_options.rs) | Source Rust |
| [crates/atelier-core/src/domain/checklist.rs](../crates/atelier-core/src/domain/checklist.rs) | Source Rust |
| [crates/atelier-core/src/domain/files.rs](../crates/atelier-core/src/domain/files.rs) | Source Rust |
| [crates/atelier-core/src/domain/projects.rs](../crates/atelier-core/src/domain/projects.rs) | Source Rust |
| [crates/atelier-core/src/domain/tasks.rs](../crates/atelier-core/src/domain/tasks.rs) | Source Rust |
| [crates/atelier-core/src/domain/time.rs](../crates/atelier-core/src/domain/time.rs) | Source Rust |
| [crates/atelier-core/src/lib.rs](../crates/atelier-core/src/lib.rs) | Source Rust |
| [crates/atelier-core/tests/agent-account.rs](../crates/atelier-core/tests/agent-account.rs) | Test / support |
| [crates/atelier-core/tests/agent-cli.rs](../crates/atelier-core/tests/agent-cli.rs) | Test / support |
| [crates/atelier-core/tests/agent-protocol.rs](../crates/atelier-core/tests/agent-protocol.rs) | Test / support |
| [crates/atelier-core/tests/agent-usage.rs](../crates/atelier-core/tests/agent-usage.rs) | Test / support |
| [crates/atelier-core/tests/brainstorm.rs](../crates/atelier-core/tests/brainstorm.rs) | Test / support |
| [crates/atelier-core/tests/connector-ticket-transitions.rs](../crates/atelier-core/tests/connector-ticket-transitions.rs) | Test / support |
| [crates/atelier-core/tests/connector-time.rs](../crates/atelier-core/tests/connector-time.rs) | Test / support |
| [crates/atelier-core/tests/domain-contracts.rs](../crates/atelier-core/tests/domain-contracts.rs) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-domain-reference.json](../crates/atelier-core/tests/fixtures/rust-domain-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-project-detection-reference.json](../crates/atelier-core/tests/fixtures/rust-project-detection-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-project-lifecycle-reference.json](../crates/atelier-core/tests/fixtures/rust-project-lifecycle-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-project-settings-reference.json](../crates/atelier-core/tests/fixtures/rust-project-settings-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-provider-settings-reference.json](../crates/atelier-core/tests/fixtures/rust-provider-settings-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-settings-reference.json](../crates/atelier-core/tests/fixtures/rust-settings-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-task-creation-reference.json](../crates/atelier-core/tests/fixtures/rust-task-creation-reference.json) | Test / support |
| [crates/atelier-core/tests/fixtures/rust-task-editing-reference.json](../crates/atelier-core/tests/fixtures/rust-task-editing-reference.json) | Test / support |
| [crates/atelier-core/tests/jira-configuration.rs](../crates/atelier-core/tests/jira-configuration.rs) | Test / support |
| [crates/atelier-core/tests/jira-creation.rs](../crates/atelier-core/tests/jira-creation.rs) | Test / support |
| [crates/atelier-core/tests/preparation.rs](../crates/atelier-core/tests/preparation.rs) | Test / support |
| [crates/atelier-core/tests/project-detection.rs](../crates/atelier-core/tests/project-detection.rs) | Test / support |
| [crates/atelier-core/tests/project-lifecycle.rs](../crates/atelier-core/tests/project-lifecycle.rs) | Test / support |
| [crates/atelier-core/tests/project-settings.rs](../crates/atelier-core/tests/project-settings.rs) | Test / support |
| [crates/atelier-core/tests/provider-settings.rs](../crates/atelier-core/tests/provider-settings.rs) | Test / support |
| [crates/atelier-core/tests/settings.rs](../crates/atelier-core/tests/settings.rs) | Test / support |
| [crates/atelier-core/tests/support/json.rs](../crates/atelier-core/tests/support/json.rs) | Test / support |
| [crates/atelier-core/tests/task-creation.rs](../crates/atelier-core/tests/task-creation.rs) | Test / support |
| [crates/atelier-core/tests/task-editing.rs](../crates/atelier-core/tests/task-editing.rs) | Test / support |
| [crates/atelier-ui/Cargo.toml](../crates/atelier-ui/Cargo.toml) | Configuration |
| [crates/atelier-ui/Trunk.toml](../crates/atelier-ui/Trunk.toml) | Configuration |
| [crates/atelier-ui/atelier-kanban.png](../crates/atelier-ui/atelier-kanban.png) | Ressource / documentation |
| [crates/atelier-ui/index.html](../crates/atelier-ui/index.html) | Ressource / documentation |
| [crates/atelier-ui/locales/en-US.json](../crates/atelier-ui/locales/en-US.json) | Ressource / documentation |
| [crates/atelier-ui/src/agent_cli.rs](../crates/atelier-ui/src/agent_cli.rs) | Source Rust |
| [crates/atelier-ui/src/agent_diagnostics.rs](../crates/atelier-ui/src/agent_diagnostics.rs) | Source Rust |
| [crates/atelier-ui/src/agent_profiles.rs](../crates/atelier-ui/src/agent_profiles.rs) | Source Rust |
| [crates/atelier-ui/src/agent_usage.rs](../crates/atelier-ui/src/agent_usage.rs) | Source Rust |
| [crates/atelier-ui/src/automation.rs](../crates/atelier-ui/src/automation.rs) | Source Rust |
| [crates/atelier-ui/src/backups.rs](../crates/atelier-ui/src/backups.rs) | Source Rust |
| [crates/atelier-ui/src/brainstorm.rs](../crates/atelier-ui/src/brainstorm.rs) | Source Rust |
| [crates/atelier-ui/src/brainstorm_page.rs](../crates/atelier-ui/src/brainstorm_page.rs) | Source Rust |
| [crates/atelier-ui/src/commits.rs](../crates/atelier-ui/src/commits.rs) | Source Rust |
| [crates/atelier-ui/src/connector_effects.rs](../crates/atelier-ui/src/connector_effects.rs) | Source Rust |
| [crates/atelier-ui/src/connector_repositories.rs](../crates/atelier-ui/src/connector_repositories.rs) | Source Rust |
| [crates/atelier-ui/src/connector_ticket.rs](../crates/atelier-ui/src/connector_ticket.rs) | Source Rust |
| [crates/atelier-ui/src/connector_ticket_reset.rs](../crates/atelier-ui/src/connector_ticket_reset.rs) | Source Rust |
| [crates/atelier-ui/src/connector_ticket_transitions.rs](../crates/atelier-ui/src/connector_ticket_transitions.rs) | Source Rust |
| [crates/atelier-ui/src/connector_time.rs](../crates/atelier-ui/src/connector_time.rs) | Source Rust |
| [crates/atelier-ui/src/connectors.rs](../crates/atelier-ui/src/connectors.rs) | Source Rust |
| [crates/atelier-ui/src/files.rs](../crates/atelier-ui/src/files.rs) | Source Rust |
| [crates/atelier-ui/src/forge.rs](../crates/atelier-ui/src/forge.rs) | Source Rust |
| [crates/atelier-ui/src/forge_checks.rs](../crates/atelier-ui/src/forge_checks.rs) | Source Rust |
| [crates/atelier-ui/src/forge_content.rs](../crates/atelier-ui/src/forge_content.rs) | Source Rust |
| [crates/atelier-ui/src/forge_publication.rs](../crates/atelier-ui/src/forge_publication.rs) | Source Rust |
| [crates/atelier-ui/src/forge_task.rs](../crates/atelier-ui/src/forge_task.rs) | Source Rust |
| [crates/atelier-ui/src/git_push.rs](../crates/atelier-ui/src/git_push.rs) | Source Rust |
| [crates/atelier-ui/src/i18n.rs](../crates/atelier-ui/src/i18n.rs) | Source Rust |
| [crates/atelier-ui/src/jira.rs](../crates/atelier-ui/src/jira.rs) | Source Rust |
| [crates/atelier-ui/src/jira_batch.rs](../crates/atelier-ui/src/jira_batch.rs) | Source Rust |
| [crates/atelier-ui/src/jira_creation.rs](../crates/atelier-ui/src/jira_creation.rs) | Source Rust |
| [crates/atelier-ui/src/jira_refresh.rs](../crates/atelier-ui/src/jira_refresh.rs) | Source Rust |
| [crates/atelier-ui/src/jira_task.rs](../crates/atelier-ui/src/jira_task.rs) | Source Rust |
| [crates/atelier-ui/src/live.rs](../crates/atelier-ui/src/live.rs) | Source Rust |
| [crates/atelier-ui/src/main.rs](../crates/atelier-ui/src/main.rs) | Source Rust |
| [crates/atelier-ui/src/preparation.rs](../crates/atelier-ui/src/preparation.rs) | Source Rust |
| [crates/atelier-ui/src/projects.rs](../crates/atelier-ui/src/projects.rs) | Source Rust |
| [crates/atelier-ui/src/queue.rs](../crates/atelier-ui/src/queue.rs) | Source Rust |
| [crates/atelier-ui/src/repository_access.rs](../crates/atelier-ui/src/repository_access.rs) | Source Rust |
| [crates/atelier-ui/src/review.rs](../crates/atelier-ui/src/review.rs) | Source Rust |
| [crates/atelier-ui/src/runs.rs](../crates/atelier-ui/src/runs.rs) | Source Rust |
| [crates/atelier-ui/src/services.rs](../crates/atelier-ui/src/services.rs) | Source Rust |
| [crates/atelier-ui/src/settings.rs](../crates/atelier-ui/src/settings.rs) | Source Rust |
| [crates/atelier-ui/src/task_editor.rs](../crates/atelier-ui/src/task_editor.rs) | Source Rust |
| [crates/atelier-ui/src/tempo.rs](../crates/atelier-ui/src/tempo.rs) | Source Rust |
| [crates/atelier-ui/src/tempo_export.rs](../crates/atelier-ui/src/tempo_export.rs) | Source Rust |
| [crates/atelier-ui/src/time.rs](../crates/atelier-ui/src/time.rs) | Source Rust |
| [crates/atelier-ui/src/time_report.rs](../crates/atelier-ui/src/time_report.rs) | Source Rust |
| [crates/atelier-ui/src/updates.rs](../crates/atelier-ui/src/updates.rs) | Source Rust |
| [crates/atelier-ui/src/workspace_views.rs](../crates/atelier-ui/src/workspace_views.rs) | Source Rust |
| [crates/atelier-ui/src/worktree.rs](../crates/atelier-ui/src/worktree.rs) | Source Rust |
| [crates/atelier-ui/style.css](../crates/atelier-ui/style.css) | Ressource / documentation |
| [rust-toolchain.toml](../rust-toolchain.toml) | Configuration |
| [scripts/preview-watch.tests.ps1](../scripts/preview-watch.tests.ps1) | Outil natif / fabrication |
| [scripts/preview.ps1](../scripts/preview.ps1) | Outil natif / fabrication |
| [scripts/rust-keyring-accounts.sh](../scripts/rust-keyring-accounts.sh) | Outil natif / fabrication |
| [scripts/rust-keyring-macos-accounts.sh](../scripts/rust-keyring-macos-accounts.sh) | Outil natif / fabrication |
| [xtask/Cargo.toml](../xtask/Cargo.toml) | Configuration |
| [xtask/licenses/README.md](../xtask/licenses/README.md) | Ressource / documentation |
| [xtask/licenses/bootstrap-5.3.8.txt](../xtask/licenses/bootstrap-5.3.8.txt) | Ressource / documentation |
| [xtask/licenses/connector-supplements-provenance.md](../xtask/licenses/connector-supplements-provenance.md) | Ressource / documentation |
| [xtask/licenses/connector-supplements.json](../xtask/licenses/connector-supplements.json) | Ressource / documentation |
| [xtask/licenses/core-supplements.json](../xtask/licenses/core-supplements.json) | Ressource / documentation |
| [xtask/licenses/leptos-supplements-provenance.md](../xtask/licenses/leptos-supplements-provenance.md) | Ressource / documentation |
| [xtask/licenses/leptos-supplements.json](../xtask/licenses/leptos-supplements.json) | Ressource / documentation |
| [xtask/licenses/other-supplements-provenance.md](../xtask/licenses/other-supplements-provenance.md) | Ressource / documentation |
| [xtask/licenses/other-supplements.json](../xtask/licenses/other-supplements.json) | Ressource / documentation |
| [xtask/licenses/upstream/SPDX-MIT.txt](../xtask/licenses/upstream/SPDX-MIT.txt) | Ressource / documentation |
| [xtask/licenses/upstream/any_spawner-0.3.0-LICENSE](../xtask/licenses/upstream/any_spawner-0.3.0-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/block2-0.6.2-LICENSE.md](../xtask/licenses/upstream/block2-0.6.2-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/const_str_slice_concat-0.1.0-LICENSE](../xtask/licenses/upstream/const_str_slice_concat-0.1.0-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/convert_case_extras-0.2.0-LICENSE-DECLARATION](../xtask/licenses/upstream/convert_case_extras-0.2.0-LICENSE-DECLARATION) | Ressource / documentation |
| [xtask/licenses/upstream/dispatch2-0.3.1-LICENSE.md](../xtask/licenses/upstream/dispatch2-0.3.1-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/drain_filter_polyfill-0.1.3-LICENSE-DECLARATION](../xtask/licenses/upstream/drain_filter_polyfill-0.1.3-LICENSE-DECLARATION) | Ressource / documentation |
| [xtask/licenses/upstream/either_of-0.1.9-LICENSE](../xtask/licenses/upstream/either_of-0.1.9-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/erased-0.1.2-LICENSE-DECLARATION](../xtask/licenses/upstream/erased-0.1.2-LICENSE-DECLARATION) | Ressource / documentation |
| [xtask/licenses/upstream/gloo-net-0.6.0-LICENSE-MIT](../xtask/licenses/upstream/gloo-net-0.6.0-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/gloo-net-0.7.0-LICENSE-MIT](../xtask/licenses/upstream/gloo-net-0.7.0-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/gloo-utils-0.2.0-LICENSE-MIT](../xtask/licenses/upstream/gloo-utils-0.2.0-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/gloo-utils-0.3.0-LICENSE-MIT](../xtask/licenses/upstream/gloo-utils-0.3.0-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/hydration_context-0.3.2-LICENSE](../xtask/licenses/upstream/hydration_context-0.3.2-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/jni-0.22.4-LICENSE-MIT](../xtask/licenses/upstream/jni-0.22.4-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/jni-macros-0.22.4-LICENSE-MIT](../xtask/licenses/upstream/jni-macros-0.22.4-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/jni-sys-macros-0.4.1-LICENSE-MIT](../xtask/licenses/upstream/jni-sys-macros-0.4.1-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/leptos-0.8.22-LICENSE](../xtask/licenses/upstream/leptos-0.8.22-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/leptos_config-0.8.10-LICENSE](../xtask/licenses/upstream/leptos_config-0.8.10-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/leptos_dom-0.8.8-LICENSE](../xtask/licenses/upstream/leptos_dom-0.8.8-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/leptos_hot_reload-0.8.7-LICENSE](../xtask/licenses/upstream/leptos_hot_reload-0.8.7-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/leptos_macro-0.8.19-LICENSE](../xtask/licenses/upstream/leptos_macro-0.8.19-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/leptos_server-0.8.8-LICENSE](../xtask/licenses/upstream/leptos_server-0.8.8-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/libsqlite3-sys-0.38.2-SQLite-copyright.txt](../xtask/licenses/upstream/libsqlite3-sys-0.38.2-SQLite-copyright.txt) | Ressource / documentation |
| [xtask/licenses/upstream/next_tuple-0.1.0-LICENSE](../xtask/licenses/upstream/next_tuple-0.1.0-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/objc2-0.6.5-LICENSE.md](../xtask/licenses/upstream/objc2-0.6.5-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/objc2-app-kit-0.3.2-LICENSE.md](../xtask/licenses/upstream/objc2-app-kit-0.3.2-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/objc2-core-foundation-0.3.2-LICENSE.md](../xtask/licenses/upstream/objc2-core-foundation-0.3.2-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/objc2-core-graphics-0.3.2-LICENSE.md](../xtask/licenses/upstream/objc2-core-graphics-0.3.2-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/objc2-encode-4.1.0-LICENSE.md](../xtask/licenses/upstream/objc2-encode-4.1.0-LICENSE.md) | Ressource / documentation |
| [xtask/licenses/upstream/oco_ref-0.2.1-LICENSE](../xtask/licenses/upstream/oco_ref-0.2.1-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/or_poisoned-0.1.0-LICENSE](../xtask/licenses/upstream/or_poisoned-0.1.0-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/r-efi-6.0.0-LICENSE-AUTHORS](../xtask/licenses/upstream/r-efi-6.0.0-LICENSE-AUTHORS) | Ressource / documentation |
| [xtask/licenses/upstream/reactive_graph-0.2.16-LICENSE](../xtask/licenses/upstream/reactive_graph-0.2.16-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/reactive_stores-0.4.4-LICENSE](../xtask/licenses/upstream/reactive_stores-0.4.4-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/reactive_stores_macro-0.4.4-LICENSE](../xtask/licenses/upstream/reactive_stores_macro-0.4.4-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/rsqlite-vfs-0.1.1-LICENSE](../xtask/licenses/upstream/rsqlite-vfs-0.1.1-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/rstml-0.13.1-LICENSE](../xtask/licenses/upstream/rstml-0.13.1-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/rustls-platform-verifier-android-0.2.0-LICENSE-MIT](../xtask/licenses/upstream/rustls-platform-verifier-android-0.2.0-LICENSE-MIT) | Ressource / documentation |
| [xtask/licenses/upstream/server_fn-0.8.13-LICENSE](../xtask/licenses/upstream/server_fn-0.8.13-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/server_fn_macro-0.8.12-LICENSE](../xtask/licenses/upstream/server_fn_macro-0.8.12-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/server_fn_macro_default-0.8.6-LICENSE](../xtask/licenses/upstream/server_fn_macro_default-0.8.6-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/tachys-0.2.19-LICENSE](../xtask/licenses/upstream/tachys-0.2.19-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/throw_error-0.3.1-LICENSE](../xtask/licenses/upstream/throw_error-0.3.1-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/wasm_split_helpers-0.2.3-LICENSE](../xtask/licenses/upstream/wasm_split_helpers-0.2.3-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/wasm_split_macros-0.2.2-LICENSE](../xtask/licenses/upstream/wasm_split_macros-0.2.2-LICENSE) | Ressource / documentation |
| [xtask/licenses/upstream/webdriver-0.53.0-MPL-2.0.txt](../xtask/licenses/upstream/webdriver-0.53.0-MPL-2.0.txt) | Ressource / documentation |
| [xtask/packaging/atelier-kanban-rust](../xtask/packaging/atelier-kanban-rust) | Outil natif / fabrication |
| [xtask/packaging/atelier-kanban-rust.desktop](../xtask/packaging/atelier-kanban-rust.desktop) | Outil natif / fabrication |
| [xtask/packaging/install-macos-git.sh](../xtask/packaging/install-macos-git.sh) | Outil natif / fabrication |
| [xtask/packaging/linux-postinstall.sh](../xtask/packaging/linux-postinstall.sh) | Outil natif / fabrication |
| [xtask/packaging/linux-postremove.sh](../xtask/packaging/linux-postremove.sh) | Outil natif / fabrication |
| [xtask/packaging/linux-preinstall.sh](../xtask/packaging/linux-preinstall.sh) | Outil natif / fabrication |
| [xtask/packaging/linux-preremove.sh](../xtask/packaging/linux-preremove.sh) | Outil natif / fabrication |
| [xtask/packaging/qualify-linux.sh](../xtask/packaging/qualify-linux.sh) | Outil natif / fabrication |
| [xtask/packaging/qualify-tray-windows.ps1](../xtask/packaging/qualify-tray-windows.ps1) | Outil natif / fabrication |
| [xtask/packaging/qualify-windows.ps1](../xtask/packaging/qualify-windows.ps1) | Outil natif / fabrication |
| [xtask/packaging/windows.iss](../xtask/packaging/windows.iss) | Outil natif / fabrication |
| [xtask/packaging/with-macos-keychain.sh](../xtask/packaging/with-macos-keychain.sh) | Outil natif / fabrication |
| [xtask/src/bin/native-fixture.rs](../xtask/src/bin/native-fixture.rs) | Source Rust |
| [xtask/src/fake_agent.rs](../xtask/src/fake_agent.rs) | Source Rust |
| [xtask/src/fake_extension.rs](../xtask/src/fake_extension.rs) | Source Rust |
| [xtask/src/fake_repository.rs](../xtask/src/fake_repository.rs) | Source Rust |
| [xtask/src/installers.rs](../xtask/src/installers.rs) | Source Rust |
| [xtask/src/inventory.rs](../xtask/src/inventory.rs) | Source Rust |
| [xtask/src/keyring.rs](../xtask/src/keyring.rs) | Source Rust |
| [xtask/src/keyring/accounts.rs](../xtask/src/keyring/accounts.rs) | Source Rust |
| [xtask/src/main.rs](../xtask/src/main.rs) | Source Rust |
| [xtask/src/notices.rs](../xtask/src/notices.rs) | Source Rust |
| [xtask/src/package.rs](../xtask/src/package.rs) | Source Rust |
| [xtask/src/package_delivery.rs](../xtask/src/package_delivery.rs) | Source Rust |
| [xtask/src/ui.rs](../xtask/src/ui.rs) | Source Rust |
| [xtask/src/ui_repositories.rs](../xtask/src/ui_repositories.rs) | Source Rust |
| [xtask/src/update_flow.rs](../xtask/src/update_flow.rs) | Source Rust |
| [xtask/tests/agent-account.rs](../xtask/tests/agent-account.rs) | Test / support |
| [xtask/tests/agent-cli.rs](../xtask/tests/agent-cli.rs) | Test / support |
| [xtask/tests/command-runs.rs](../xtask/tests/command-runs.rs) | Test / support |
| [xtask/tests/connector-repositories.rs](../xtask/tests/connector-repositories.rs) | Test / support |
| [xtask/tests/extensions.rs](../xtask/tests/extensions.rs) | Test / support |
| [xtask/tests/forge-review-tls.rs](../xtask/tests/forge-review-tls.rs) | Test / support |
| [xtask/tests/interactive-runs.rs](../xtask/tests/interactive-runs.rs) | Test / support |
| [xtask/tests/keyring-execution.rs](../xtask/tests/keyring-execution.rs) | Test / support |
| [xtask/tests/posix-runtime.rs](../xtask/tests/posix-runtime.rs) | Test / support |
| [xtask/tests/support/keyring_windows.rs](../xtask/tests/support/keyring_windows.rs) | Test / support |
| [xtask/tests/support/tray.rs](../xtask/tests/support/tray.rs) | Test / support |
| [xtask/tests/support/tray_posix.rs](../xtask/tests/support/tray_posix.rs) | Test / support |
| [xtask/tests/support/windows.rs](../xtask/tests/support/windows.rs) | Test / support |
| [xtask/tests/translations.rs](../xtask/tests/translations.rs) | Test / support |
| [xtask/tests/verification-runs.rs](../xtask/tests/verification-runs.rs) | Test / support |
| [xtask/tests/windows-native.rs](../xtask/tests/windows-native.rs) | Test / support |
