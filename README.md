# Atelier Kanban

[![Release](https://img.shields.io/github/v/release/FluryWaana/atelier?label=release)](https://github.com/FluryWaana/atelier/releases/latest)
[![Build](https://github.com/FluryWaana/atelier/actions/workflows/release.yml/badge.svg?branch=main&event=push)](https://github.com/FluryWaana/atelier/actions/workflows/release.yml?query=branch%3Amain)
[![Node.js](https://img.shields.io/badge/Node.js-%E2%89%A5%2024-339933?logo=nodedotjs&logoColor=white)](package.json)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS-blue)](#installation)

**A local Kanban board for taking work from an idea to a pull request with Codex or Claude Code.**

Organize projects, prepare tasks, run coding agents in dedicated Git worktrees, review changes, and track your time from one browser interface. Connect Jira, GitHub, GitLab, Azure DevOps, and Tempo when you need them.

Atelier runs on your computer and stores its data in local SQLite. Agents and connected services use your own accounts and communicate with their online providers. The interface supports **English (US)** and **French (France)**, with English as the default.

[Download](https://github.com/FluryWaana/atelier/releases/latest) · [Release notes](https://github.com/FluryWaana/atelier/releases) · [Report a bug](https://github.com/FluryWaana/atelier/issues) · [Documentation](#documentation)

## Contents

- [Features](#features)
- [Installation](#installation)
- [Your first task](#your-first-task)
- [From task to pull request](#from-task-to-pull-request)
- [Integrations](#integrations)
- [Data and privacy](#data-and-privacy)
- [Development](#development)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

## Features

- **Plan your work.** Manage multiple projects, Kanban cards, acceptance criteria, dependencies, and task templates. Explore ideas with an agent and turn proposals into tasks.
- **Work with coding agents.** Choose Codex or Claude Code, queue runs, follow live logs, and inspect duration, token usage, and available cost estimates.
- **Keep task changes separate.** Give each card a dedicated Git branch and worktree, with different cards able to run in parallel.
- **Prepare and review pull requests.** Preview changes, publish draft PRs or MRs, inspect remote checks, and run an agent review. Opt into an automatic development and review workflow per project.
- **Run your project locally.** Launch front-end and back-end services, inspect logs and listening ports, and stop or restart them from the Processes view.
- **Track human and agent time separately.** Use timers or manual entries, export CSV reports, view daily summaries, and send confirmed entries to Tempo Cloud.
- **Keep control of your data.** Use local backups and restoration, OS-protected credentials, and portable packages with a bundled Node.js runtime.
- **Extend your setup.** Add connectors without changing the core application, or contribute an interface translation.

## Installation

### Windows and Linux installers

Download the installer for your system from [GitHub Releases](https://github.com/FluryWaana/atelier/releases/latest). **Node.js is included.**

| System | Installer |
| --- | --- |
| Windows x64 | `atelier-kanban-X.Y.Z-windows-x64-setup.exe` |
| Ubuntu / Debian x64 or ARM64 | `atelier-kanban-X.Y.Z-linux-ARCH.deb` |
| Fedora x64 or ARM64 | `atelier-kanban-X.Y.Z-linux-ARCH.rpm` |

On Windows, run Setup, then open **Atelier Kanban** from the Start menu. Installation is per user and requires no administrator privileges. Use **Arrêter Atelier Kanban** to stop it. On Linux, open the package with your software installer, then launch **Atelier Kanban** from the applications menu. If your desktop does not handle local packages, use `sudo apt install ./atelier-kanban-X.Y.Z-linux-ARCH.deb` or `sudo dnf install ./atelier-kanban-X.Y.Z-linux-ARCH.rpm`. Replace `ARCH` with `x64` or `arm64`.

Installed versions keep data outside the application: `%LOCALAPPDATA%\AtelierKanban\data` on Windows and `${XDG_DATA_HOME:-~/.local/share}/atelier-kanban` on Linux. Reinstalling or uninstalling the application preserves these files. macOS keeps the portable archives below; no macOS installer is provided.

**Moving from a portable copy:** stop the old copy, install Atelier, and use **Importer les données portables** from the Windows Start menu before the first normal launch. Select the old `data` folder. On Linux, run `atelier-kanban --import-data /path/to/old/data`. The import creates a SQLite backup and leaves the source unchanged. It refuses to overwrite an existing destination; it does not merge boards. The same OS account and keyring are needed for saved credentials. See the [installation and release guide](docs/releases.md).

### Portable application

Download the archive for your system from [GitHub Releases](https://github.com/FluryWaana/atelier/releases/latest) and extract it into a writable folder. **Node.js is included; no npm installation is needed.**

| System | Architecture | Archive |
| --- | --- | --- |
| Windows | x64 | `atelier-kanban-X.Y.Z-windows-x64.zip` |
| Linux | x64 | `atelier-kanban-X.Y.Z-linux-x64.tar.gz` |
| Linux | ARM64 | `atelier-kanban-X.Y.Z-linux-arm64.tar.gz` |
| macOS | Intel | `atelier-kanban-X.Y.Z-macos-x64.tar.gz` |
| macOS | Apple Silicon | `atelier-kanban-X.Y.Z-macos-arm64.tar.gz` |

`X.Y.Z` is the release version. Each archive has a companion `.sha256` checksum file.

**Windows:** double-click `Start.cmd` to start and `Stop.cmd` to stop.

**Linux / macOS:** run these commands from the extracted folder:

```sh
sh Start.command
# When you are finished:
sh Stop.command
```

The launcher opens **http://127.0.0.1:4317** in your browser.

**Upgrading from a version with French launcher filenames:** its updater cannot accept the renamed files. Stop the old application, back up its data, then install this version manually. For a portable copy, extract it into a new folder and transfer the data before the first launch. Update any personal shortcuts.

To use coding agents, install **Git** and your chosen **Codex or Claude Code CLI**, authenticate them, and make sure they are available in `PATH`. They are installed separately from Atelier. Local task management does not require an agent account.

On Linux, saving tokens requires `secret-tool` and an unlocked Secret Service keyring. `xdg-open`, `lsof`, and Zenity support browser opening, port inspection, and folder selection respectively; you can also enter folder paths manually. Windows uses DPAPI for stored credentials, and macOS uses Keychain.

### Run from source

Use **Node.js 24 or newer**; the exact development and CI version is pinned in [`.node-version`](.node-version).

```sh
git clone https://github.com/FluryWaana/atelier.git
cd atelier
node launcher.mjs
```

There are no runtime npm dependencies. The launcher starts the server in the background and reuses an existing instance when run again.

```sh
node launcher.mjs --stop       # Stop the application
node launcher.mjs --no-browser # Start without opening a browser tab
npm start                      # Run the server in the foreground instead
```

The server listens on `127.0.0.1` only. Set the `PORT` environment variable before starting it to change the default port, `4317`. Launcher logs are written to `data/server.stdout.log` and `data/server.stderr.log`.

## Your first task

1. Open **My projects** and add an existing local Git repository.
2. Create a card with a clear description and acceptance criteria.
3. Choose Codex or Claude Code. Analyze the task first, or use **Edit code** to start implementation in the card's worktree.
4. Follow the run in the card or the **Agents** view, then inspect the resulting changes and run your project's checks.
5. Prepare a draft pull request when ready, review it, and mark the card done after your own verification.

A successful manual coding run moves the card to **To review**. Completing a run does not automatically mark the task done.

Configure optional services in **Settings → Modules**. Change the interface language in **Settings → General → Language**; the preference is saved in your browser.

## From task to pull request

Atelier keeps the task, its worktree, agent history, checks, and PR/MR link together. You can drive each step yourself or enable **Automatic development** in the project's settings.

The automatic workflow is **off by default**. Configure a target branch and a verification command, then enable it for the project:

```text
Task → Agent implementation → Verification → Draft PR/MR → Agent review
                                  ↑                            |
                                  └──── Fix blocking issues ───┘
                                                               |
                                                        Human review
```

Atelier runs the verification command itself, commits and pushes changes, then creates or updates a draft PR/MR. The review checks the published code and acceptance criteria. Optional automatic fixes handle critical or high-priority findings, with at most two correction cycles per workflow. Remaining blocking findings put the card on hold.

**Enabling this workflow authorizes committing and publishing all non-ignored files in the card's worktree.** Check your repository's `.gitignore` first. Atelier does not force-push, merge the PR, or automatically pick up the next task. Final approval and merging remain yours.

Worktrees separate source changes, but installed dependency folders may be shared with the original checkout. Changing a shared dependency folder affects every worktree using it; use a separate local installation when a task changes dependencies. Stopping an agent does not undo files it has already changed.

## Integrations

Select ticket, repository, and time providers independently in **Settings → Modules**. Repository providers can also be overridden per project.

| Integration | Purpose | Authentication |
| --- | --- | --- |
| Codex / Claude Code | Task preparation, implementation, brainstorming, and review | Locally installed and authenticated agent CLI |
| Jira Cloud / Data Center | Import and sync tickets, create Jira tickets, and map board columns to statuses | Jira credentials configured in the module |
| GitHub | Draft pull requests, remote checks, and review context | GitHub CLI session or personal token |
| GitLab | Draft merge requests, pipeline status, and review context | GitLab CLI session or personal token |
| Azure DevOps | Pull requests and repository status | Azure CLI session or personal access token |
| Tempo Cloud | Export confirmed human time entries linked to Jira | Tempo token and Jira Cloud connection |

GitHub and GitLab connectors manage PRs/MRs; they do not import issues. Git fetch/push authentication uses your existing SSH keys or Git credential helper, separately from the provider's API credentials. Agent runs use your provider's quotas; displayed costs are estimates, not billing records.

See the [repository connection guide](docs/repository-connectors.md) for authentication, private instances, and provider limitations. To add a provider, follow the [connector guide](docs/connectors.md) and [example module](examples/connectors/demo/index.mjs).

## Data and privacy

- **Local storage:** cards, projects, settings, and history are stored in `kanban.sqlite` in the data directory described above, or `data/` beside a portable copy. Use the application's backup tools, or stop Atelier before copying the complete data directory.
- **Credentials:** saved tokens use encrypted storage backed by Windows DPAPI or the Linux/macOS system keyring. Moving a backup to another machine may require entering credentials again.
- **External services:** coding agents and configured integrations communicate with their providers. A local board does not make those services offline.
- **Logs:** agent output can contain source code and repository information. Keep databases, backups, credentials, and logs out of version control and public issue reports.
- **Extensions:** local connectors run with Atelier's system permissions. Install only code you trust.

Portable copies and the Windows installation support in-app updates with a backup and rollback workflow. Linux `.deb`/`.rpm` installations announce new releases, then require installing the new package with the system package manager after stopping Atelier. Source checkouts do not support automatic installation of updates. See the [release and update guide](docs/releases.md) for packaging, platform requirements, and recovery procedures.

## Development

The application uses Node.js ES modules, SQLite, and native browser HTML, CSS, and JavaScript. Bootstrap and Sass are development dependencies used to build the committed stylesheet.

### Tests

```sh
npm test
```

The suite uses `node:test` and mocks external services; it does not require live credentials or paid agent calls. With the bundled Windows runtime:

```powershell
.\runtime\node.exe --test test/*.test.mjs
```

### Styles

```sh
npm ci
npm run build:css
# Rebuild as you edit:
npm run watch:css
```

Edit the Sass sources in `styles/` and include the regenerated `public/style.css` in your changes. No CSS build is needed to start the application.

### Project structure

```text
server.mjs          HTTP server and dependency composition
launcher.mjs        Application startup and shutdown
lib/domain/         Pure business rules
lib/application/    Application use cases
lib/http/           HTTP route adapters
lib/infrastructure/ Storage and system adapters
lib/connectors/     Built-in service connectors
public/             Browser interface, views, and translations
styles/             Sass sources
connectors/         Optional local connectors
examples/           Connector examples
scripts/            Packaging and update tooling
test/               Automated tests
docs/               Technical guides
data/               Local data and logs (not committed)
runtime/            Bundled Node.js runtime in portable packages
```

Before packaging, run `npm run check:release`. `npm run package -- owner/repository` builds a native package, and `npm run test:release` checks the extracted package and rollback behavior. See the [release guide](docs/releases.md) for prerequisites and the full process.

The [GitHub Actions workflow](.github/workflows/release.yml) builds and tests all five platform targets. Pull requests and pushes to `main` produce test artifacts; matching version tags publish releases only after the full matrix succeeds.

## Documentation

The detailed guides are currently written in French.

| Guide | Covers |
| --- | --- |
| [Architecture](docs/architecture.md) | Module boundaries, responsibilities, and dependency rules |
| [Connectors](docs/connectors.md) | Extension API and an executable example |
| [Repository providers](docs/repository-connectors.md) | GitHub, GitLab, Azure DevOps, CLI sessions, and tokens |
| [Localization](docs/localization.md) | Adding a language and translating connectors |
| [Releases and updates](docs/releases.md) | Builds, publishing, checksums, updates, and recovery |
| [Repository guidelines](AGENTS.md) | Coding conventions and expected checks |

## Contributing

Bug reports, focused pull requests, translations, and connector contributions are welcome.

- For a bug, [open an issue](https://github.com/FluryWaana/atelier/issues) with your OS, Atelier version, steps to reproduce, and expected versus actual behavior. Remove secrets and private repository content from logs.
- For a change, describe the problem, resulting behavior, and verification in your pull request. Add regression tests for behavior changes and run the full test suite.
- For UI changes, include screenshots and check both English and French. Rebuild the stylesheet when changing Sass.
- Follow the existing module boundaries and [repository guidelines](AGENTS.md). Never commit local databases, backups, tokens, or logs.

## License

Atelier Kanban is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version (`AGPL-3.0-or-later`).

Atelier Kanban is distributed WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See [LICENSE](LICENSE) for the full terms (included as `docs/LICENSE` in release packages).

Commercial use is permitted. If you distribute a modified version, you must make its corresponding source available to recipients under this license. If users interact with your modified version over a network, you must prominently offer them access to its corresponding source. Contributions back to this repository through pull requests are welcome, but are not required by the license.

Third-party components retain their respective licenses; release packages include the Node.js runtime license in `runtime/LICENSE`.
