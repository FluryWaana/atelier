# Features

Atelier Kanban is a local workspace for planning tasks and working on Git repositories with coding agents. It is in preproduction.

## Projects

Add an existing repository or clone one over HTTPS or SSH. A project groups repository access, agent defaults, launch commands and automation settings. Related projects and service repositories let a task work with more than one codebase.

The project list offers separate actions to open a project's tasks or configure its settings. With no projects, it explains how to add a working folder or clone a repository.

The editor groups settings into General, Agent, Launch and Automation tabs. Values remain available when switching tabs. Saving validates all tabs and focuses the first invalid field. Leaving a modified form requires a choice to keep editing or discard the draft.

Removing a project requires its exact name and can archive its tasks. The Git directory remains on disk. Repository access is a separate explicit authorization.

Revoking repository authorization preserves saved projects, tasks and files. New access to an external repository requires authorization again. Running operations and concurrent writes must finish before authorization can be revoked.

## Tasks and the board

Create, edit, duplicate, archive or delete tasks. Tasks hold descriptions, instructions, criteria, dependencies and project or ticket links. Search and project filters find work across the board and archives.

The five stages are `todo`, `doing`, `review`, `done` and `standby`. Projects can rename and reorder their displayed columns. Dragging a card changes its stage and does not start an agent. See [board configuration](board-columns.md).

Local task creation remains available with an external ticket provider selected. Brainstorm discussions can produce proposals that become tasks. The welcome screen offers task creation and optional project setup without requiring a connector.

## Agents and execution

Profiles configure Codex, Claude Code or a custom command. Projects and tasks can select a profile. Profiles provide installation and connection checks, version information and supported usage or quota information.

Conversations retain output and supported interactions. Runs can be cancelled. A queue coordinates individual runs or project workflows. Services have separate commands, logs, ports and stop controls. See [agents](agents.md), [workflows](workflows.md) and [process supervision](process-supervision.md).

## Git and reviews

Dedicated branches and worktrees keep task work separate. Browse tracked and untracked files, inspect diffs and prepare explicit local commits. Push and pull request or merge request publication use previews tied to repositories, branches and commits.

Reviews bind findings to inspected commits and task criteria. Source changes make a review stale until revalidated. Agent findings never automatically complete human acceptance criteria.

## Integrations

Jira provides search, creation, links, status mapping and confirmed synchronization. GitHub, GitLab and Azure DevOps provide repository integrations. Tempo supports confirmed human time exports. Process connectors expose their declared capabilities.

Ticket and time providers are independent preferences. Changing a default does not convert records or send remote writes. Uncertain operations remain available for recovery. See [integrations](integrations.md) and [connectors](connectors.md).

## Time tracking

Record manual entries or use local timers. Human time remains separate from agent duration, usage and cost information. Summaries and CSV reports use recorded entries. Remote export requires a preview and confirmation, with reconciliation after an uncertain response. Local time entry remains available with any provider.

## Data and updates

Backups and restoration protect the local workspace. Portable data import copies a supported workspace into an empty destination. Unsupported formats are refused without silent conversion or erasure.

Installed packages can inspect compatible stable releases and prepare authenticated updates. Application updates and CLI updates have separate controls. See [launch and import](launcher.md) and [packages and updates](packaging.md).

## Interface

The interface supports English and French, keyboard navigation, accessible labels, loading and error states, and draft protection. Project, settings and task tabs retain unsaved values. Common controls are documented in [UI components](ui-components.md).
