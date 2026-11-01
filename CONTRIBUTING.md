# Contributing

Focused bug reports, documentation improvements and pull requests are welcome. Explain the problem, intended behavior and reproduction or verification steps. Use synthetic examples and remove credentials, session URLs and personal data from reports.

## Help and issues

Use [GitHub Issues](https://github.com/FluryWaana/atelier/issues) for usage questions, reproducible bugs and feature requests. Search existing issues before opening one.

For a bug, include the application version or commit, operating system and browser version, reproduction steps, expected behavior and actual behavior. Attach sanitized output or screenshots when useful. For a feature request, describe the problem and an example of the intended use.

Report suspected vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

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

Run the relevant WebDriver journeys against synthetic data for behavior changes. See [testing](docs/testing.md) for preparation and platform requirements. Include the commands and results in your pull request, including skipped checks and unresolved failures.

## Test data

Use synthetic databases, repositories, credentials and compiled fake agents. Never open, modify or remove personal `data/` during development. Do not use real accounts or paid agent calls.

Native credential tests require disposable environments. Linux account probes use isolated D-Bus/XDG sessions. macOS probes use dedicated temporary keychains. Windows temporary account provisioning is restricted to CI. Never test against a personal or default keychain.

## Pull requests

Use Conventional Commits in English, such as `docs: describe source build requirements`. Describe the final behavior, verification and remaining limits. Keep unrelated edits out of the change.

GitHub Actions workflows use manual triggers. Maintainers manage CI runs and release publication.
