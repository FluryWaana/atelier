# Contributing

Focused bug reports, documentation improvements and pull requests are welcome. Explain the problem, intended behavior and reproduction or verification steps. Use synthetic examples and remove credentials, session URLs and personal data from reports.

## Setup

Follow [getting started](docs/getting-started.md) and use the committed toolchain and lockfile. The application has four workspace crates and no Node.js build dependency. Read the [architecture](docs/architecture.md) before changing domain, adapter or UI boundaries.

[Atelier.cmd](Atelier.cmd) starts the Windows preview. Other environments can build the UI and run the application's `serve` command directly.

## Changes

Keep changes focused and preserve local task creation and time tracking. Use native Rust types and explicit errors at boundaries. Reuse existing components and helpers. Add meaningful regression tests for behavior, security boundaries, concurrency or data loss.

Update documentation when a public contract changes. Write documentation in English with ordinary punctuation. Avoid decorative dashes, semicolons and arrow symbols in prose. Preserve exact identifiers, command options, source examples and upstream legal texts.

UI changes must preserve English and French translations, accessible names, keyboard behavior and unsaved drafts. See [UI components](docs/ui-components.md).

## Local checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings
cargo test --workspace --exclude atelier-ui --locked
cargo clippy -p atelier-ui --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
cargo check -p atelier-ui --target wasm32-unknown-unknown --locked
cargo fetch --locked
cargo xtask build-ui
```

Run relevant real WebDriver journeys against synthetic data for behavior changes. See [verification](docs/rust-final-qualification.md) for preparation and platform requirements. A skipped test is not a passed test. Report checks actually run and unresolved failures.

## Test data

Use synthetic databases, repositories, credentials and compiled fake agents. Never open, modify or remove personal `data/` during development. Do not use real accounts or paid agent calls.

Native credential tests require disposable environments. Linux account probes use isolated D-Bus/XDG sessions. macOS probes use dedicated temporary keychains. Windows temporary account provisioning is restricted to CI. Never test against a personal or default keychain.

## Pull requests

Use Conventional Commits in English, such as `docs: describe source build requirements`. Describe the final behavior, verification and remaining limits. Keep unrelated edits out of the change.

Workflows are manual. This contribution process does not authorize launching CI, creating tags or publishing releases. Repository automation rules are documented in [AGENTS.md](AGENTS.md).
