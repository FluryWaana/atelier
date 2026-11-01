# Verification

Verification covers source checks, native tests, browser journeys, packages and desktop interactions. Each result applies to the exact source revision, target and scenario executed. A written scenario, successful compilation or skipped job is not evidence that a user journey passed.

## Local source checks

Run from the repository root with the committed toolchain and lockfile:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings
cargo test --workspace --exclude atelier-ui --locked
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
cargo check -p atelier-ui --target wasm32-unknown-unknown --locked
cargo fetch --locked
cargo xtask build-ui
```

Use a separate `CARGO_TARGET_DIR` when another development process owns build outputs. Native and WebAssembly checks are separate because the UI targets `wasm32-unknown-unknown`.

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

Native installer checks must use disposable environments. Setup checks exercise the installed shortcut, icon and version resources, launch, cancellation, data preservation, update, uninstallation and reinstallation. Linux checks use real package managers in disposable native containers. macOS tray checks must exercise Open and Quit rather than infer behavior from icon presence.

A browser passing on one system does not qualify another browser or a native installer.

## Isolated native checks

Ignored tests are opt in and retain their guards. They require the documented disposable CI or native environment rather than personal accounts or keychains. Setting a guard variable alone does not establish that the environment is safe.

Linux credential probes require an isolated D-Bus/XDG session and disposable collection. macOS probes use dedicated temporary keychains and explicit account scope. Windows temporary account provisioning is CI only. Never read, lock or replace a personal credential collection.

See [synthetic TLS tests](rust-synthetic-tls.md) and [process supervision](rust-posix-runtime.md) for transport and process fixtures.

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

Workflow badges link to their actual run status. They do not certify every platform at the current source revision. Inspect job logs and artifacts before reporting a result. Local verification does not authorize workflow dispatch, tags or release publication.
