# Testing

The test suite covers domain rules, native adapters, browser journeys, packages and desktop interactions. Use synthetic fixtures and a separate workspace for every integration test.

## Local source checks

Run from the repository root with the committed toolchain and lockfile:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings
cargo test --workspace --exclude atelier-ui --locked
cargo test -p atelier-ui --locked
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
cargo check -p atelier-ui --target wasm32-unknown-unknown --locked
cargo fetch --locked
cargo xtask build-ui
```

Use a separate `CARGO_TARGET_DIR` when another development process owns build outputs. Native UI tests cover pure presentation rules. WebAssembly checks and browser journeys cover the browser interface. Set `CARGO_BUILD_JOBS=2` if parallel compilation exceeds the machine's available memory.

## Browser journeys

Use a compatible installed browser and local WebDriver. Browser sessions, repositories, credentials, agents and application data must be synthetic. Journeys run the actual application's `serve` entry point.

```text
cargo xtask test-ui --prepared-assets
```

| Variable | Purpose |
| --- | --- |
| `ATELIER_TEST_BROWSER` | Select `chrome`, `firefox`, `edge` or `safari` |
| `ATELIER_TEST_JOURNEY` | Select `application`, `diagnostics`, `jira`, `updates` or `all` |
| `ATELIER_WEBDRIVER` | Explicit driver executable |
| `ATELIER_TEST_BROWSER_BINARY` | Explicit supported browser executable |
| `ATELIER_TEST_ASSETS` | Prepared UI assets |
| `ATELIER_TEST_SERVER` | Prepared application binary |

`--prepared-assets` reuses built UI files. `--prepared-assets --prepared-binaries` also reuses prepared native executables and notices without a Cargo build. Missing or invalid prepared files fail the check. Use a matching browser and driver version.

Browser checks cover form validation, keyboard and focus behavior, drafts, task operations, profiles, integrations and updates. Inspect screenshots at desktop, tablet and mobile widths for presentation changes. Synthetic drag events do not establish physical drag behavior on every desktop.

## Packages and desktop checks

```text
cargo xtask qualify-packages .artifacts/rust-delivery
cargo xtask test-updates --prepared-delivery .artifacts/rust-delivery
```

Package qualification builds real release binaries and synthetic successors, checks notices and signatures, installs portable payloads and observes runtime versions, persistence, session renewal and shutdown. Update tests check actual version changes and rollback while preserving workspace records and human files.

Native installer checks use disposable environments. Setup checks exercise the installed shortcut, icon and version resources, launch, cancellation, data preservation, update, uninstallation and reinstallation. Linux checks use real package managers in disposable native containers. macOS tray checks exercise Open and Quit, server shutdown and icon removal.

## Isolated native checks

Ignored native tests require their disposable CI or native environment. Their guards prevent execution against personal accounts or keychains.

Linux credential probes require an isolated D-Bus/XDG session and disposable collection. macOS probes use dedicated temporary keychains and explicit account scope. Windows temporary account provisioning is CI only. Never read, lock or replace a personal credential collection.

See [synthetic TLS tests](synthetic-tls.md) and [process supervision](process-supervision.md) for transport and process fixtures.

## Manual workflows

The repository contains three workflows with manual triggers:

| Workflow | Purpose |
| --- | --- |
| [Rust verification](../.github/workflows/rust.yml) | Source checks, native tests, browsers and delivery qualification |
| [Windows prerelease](../.github/workflows/prerelease.yml) | Signed Windows distribution from an exact qualified source run |
| [Linux and macOS prerelease](../.github/workflows/unix-prerelease.yml) | Complete platform assets for an existing prerelease |

Rust verification accepts these selections:

| Input | Values |
| --- | --- |
| `scope` | `browser`, `delivery`, `native`, `desktop`, `full` |
| `browser` | `chrome`, `firefox`, `edge`, `safari`, `all` |
| `journey` | `jira`, `application`, `updates`, `all` |
| `platform` | `windows`, `linux`, `linux-arm`, `macos`, `macos-intel`, `all` |

Defaults select one browser journey. `desktop` selects Windows Setup and Safari on macOS 26. Native macOS targets use macOS 15 ARM and Intel. `full` with `platform=all` selects all five native targets and the browser matrix. `full_matrix=true` also overrides the platform selection to all five targets.

Each run records its source revision, selected targets, job logs and artifacts. Use those details when investigating a failure or reporting test coverage. Badges show the workflow's run status.
