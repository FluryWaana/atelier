# Repository Guidelines

## Project Structure & Module Organization

This directory is the Windows x64 portable distribution of Atelier Kanban, a local task board with Jira, Azure DevOps, Tempo, Codex, and Claude integrations. `server.mjs` runs the HTTP server; `launcher.mjs` manages startup and shutdown. Backend modules live in `lib/`, including storage, agents, worktrees, and service integrations. `public/` contains the native HTML, CSS, and browser JavaScript. Tests live in `test/`. `runtime/` supplies Node.js and its license; `data/` stores local SQLite data, backups, and logs.

## Build, Test, and Development Commands

- `.\Start.cmd`: start the portable application and open `http://127.0.0.1:4317`.
- `.\Stop.cmd`: stop the application through its launcher.
- `npm start`: run the server in the foreground with an installed Node.js 24 or newer.
- `npm test`: run `node --test test/*.test.mjs`.
- `.\runtime\node.exe --test test/*.test.mjs`: run tests using the bundled runtime.

No npm dependencies are needed at runtime. For style development, run `npm ci`, then `npm run build:css` (or `npm run watch:css`). Edit `styles/` Sass sources and regenerate the committed `public/style.css`; Bootstrap and Sass are development dependencies only. Use the Node version pinned in `.node-version` and run `npm run check:release` before releasing. `npm run package -- owner/repository` builds the native release; `npm run test:release` checks the extracted package and rollback. See `docs/releases.md` and `.github/workflows/release.yml`.

After the portable package, `npm run package:installers` builds the Windows per-user Setup or Linux DEB/RPM packages. Inno Setup 6 is required on Windows; `dpkg-deb`, `rpmbuild` and `desktop-file-validate` are required on Linux. Set `ATELIER_REQUIRE_INSTALLERS=1` for `npm run test:installers` on a test account. Native installations use `installation.json` to select user data storage; preserve this marker during in-app updates. Linux system packages must be updated by their package manager. macOS remains portable only.

Pure rules live in `lib/domain/`, use cases in `lib/application/`, HTTP adapters in `lib/http/` and storage/system adapters in `lib/infrastructure/` or the existing integration modules in `lib/`. `server.mjs` composes these dependencies. Tasks, projects, settings, runs, card workflows, Jira sync, automation, time tracking and updates use these boundaries, enforced by `test/architecture.test.mjs`. Board, project-list, process and project-settings views live in `public/views/`, with state and actions passed explicitly. Keep these boundaries when extending them; see `docs/architecture.md`. The release matrix builds Windows x64, Linux x64/ARM64 and macOS x64/ARM64; native release tests must pass before publishing. This checkout still contains only the Windows runtime.

## Coding Style & Naming Conventions

### Rust migration coexistence

The Node application remains the production implementation. Follow `docs/rust-migration-plan.md` and its parity matrix; never open user `data/` with the Rust prototype. M01 provides `atelier serve-prototype --data-dir <empty-test-directory> --assets .artifacts/rust-ui --port 4318`; its deliberately limited API is not a production replacement.

The Cargo workspace contains exactly `atelier-core` (pure domain/application/public contracts), `atelier-app` (HTTP and native adapters), `atelier-ui` (Leptos CSR/WASM), and `xtask` (build and verification). Preserve those boundaries. Use the pinned `rust-toolchain.toml`, committed `Cargo.lock`, `cargo fmt`, and Clippy with warnings denied. Native checks exclude `atelier-ui`; check it separately with `--target wasm32-unknown-unknown`.

Run `cargo test --workspace --exclude atelier-ui --locked` and the existing full Node suite. `cargo xtask build-ui` requires Trunk 0.22.0 and wasm-bindgen CLI 0.2.129 on PATH; run `cargo fetch --locked` first because the UI build is offline. `cargo xtask test-ui` starts a separate server process and a temporary WebDriver profile, creates a synthetic task, restarts the server and checks persistence. Set `ATELIER_WEBDRIVER` to the driver executable if needed; `ATELIER_TEST_BROWSER` supports `edge`, `chrome`, `firefox`, or `safari`. A configured browser is not a verified browser until the test runs. See `docs/rust-migration-m01.md` for commands and remaining native proofs. Packaging commands remain Node until their migration lots are qualified.

`cargo test -p xtask --test windows-native --locked` exercises synthetic Windows process trees and copied update workers. `cargo xtask test-tray` checks adding/removing a notification icon on a native desktop: Windows shell rectangle, Linux StatusNotifier host registration/pixels, or macOS AppKit window visibility. It fails if the required desktop is unavailable. CI starts an isolated XFCE panel for Linux. These feasibility fixtures are not production launchers or updaters and must stay outside release packages.

`cargo xtask test-storage-parity` compares storage, restoration and token envelopes against the historical Node runtime (`ATELIER_NODE` overrides its path). Ordinary Rust tests do not need Node. `cargo xtask test-keyring` requires a native Linux/macOS keyring and creates, reads and deletes only a unique synthetic service; it never uses the production token key. Run it on disposable CI/test accounts.

### JavaScript conventions

Use ES modules: `.mjs` for backend modules and tests, with browser code under `public/`. Follow nearby indentation and existing semicolon and single-quote conventions. Use camelCase for functions and variables and descriptive lowercase, hyphenated module names such as `time-tracking.mjs`. Keep integration logic in its corresponding `lib/` module. No formatter or linter is configured. Preserve the French user-facing text.

## Testing Guidelines

Use `node:test` and `node:assert/strict`; name tests `test/<feature>.test.mjs`. Existing tests cover Jira authentication and token storage. Add behavioral regression tests for changes, mock external services, and avoid live credentials or paid agent calls. Run the full suite before review. No coverage threshold is configured.

## Commit & Pull Request Guidelines

Conventional Commits are mandatory for every commit. Use `<type>[optional scope][!]: <description>`, with a concise, imperative description in English. Use `feat` for new features, `fix` for bug fixes, and `docs`, `refactor`, `test`, `build`, `ci`, `perf`, `style`, or `chore` as appropriate. Mark breaking changes with `!` before the colon or a `BREAKING CHANGE:` footer, and explain the impact.

Examples: `docs: require conventional commits`, `fix(updater): preserve user data during rollback`, `feat!: rename portable launchers to Start and Stop`.

Describe the problem, changed behavior, and verification in each PR; link relevant issues and include screenshots for UI changes.

## Security & Configuration

Keep the server bound to loopback. Never commit tokens, local databases, backups, or logs. Back up `data/` before storage changes and stop the application before replacing distribution files.
