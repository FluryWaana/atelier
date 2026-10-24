# Repository Guidelines

## Active objective

This is a **preproduction Rust rebuild**, not a compatibility migration. On 2026-10-24 the owner confirmed that no version is in production and requested the same application features with better quality, with **no backward compatibility requirement**. Follow `docs/rust-migration-plan.md` (the active rebuild plan) and `docs/rust-rebuild-features.md`.

The previous migration is preserved at commit `7326122` and local branch `archive/rust-migration-compat-2026-10-24`. Its M00–M11 notes and parity matrix are historical evidence, not current delivery gates. Reuse good Rust code and useful behavioral tests; do not reproduce accidental JavaScript coercions, invalid UTF-16, old SQLite formats, old token envelopes or Node package transitions merely for compatibility. Preserve user-visible capabilities.

## Structure and architecture

The Cargo workspace contains exactly:
- `atelier-core`: pure domain models, validation, application use cases and public contracts.
- `atelier-app`: composition, HTTP, SQLite, filesystem, processes, integrations and native adapters.
- `atelier-ui`: Leptos CSR/WASM interface.
- `xtask`: build, verification and packaging.

Keep domain/application independent of I/O and platform APIs. Prefer explicit typed models and focused functions. Keep transactions short, reserve external operations before I/O, and revalidate before committing their results. Do not add abstractions without a concrete caller.

The existing `server.mjs`, `launcher.mjs`, `lib/`, `public/`, `test/`, `scripts/` and Node runtime are reference implementations until their functions are replaced or archived. They are not a production compatibility obligation and must not enter final Rust packages. The source inventory helps find all capabilities and tests.

## Data and security

Work exclusively with synthetic databases, repositories, credentials and agent fixtures. Never open, modify or remove user `data/` during development. The authorization to reset the design does not require touching local user files.

The rebuilt application uses its own explicit database identity/version and refuses unsupported data; never silently erase a database. Preserve atomic writes, backups, rollback and bounded storage access. Bind HTTP to loopback, require the local session and validate Host/Origin. Never commit tokens, databases, backups or logs, or expose secrets to the frontend.

Native keyring tests use unique synthetic services. Windows temporary-account provisioning is CI-only. Linux account/collection-lock probes require the disposable isolated D-Bus/XDG session and their CI guard. macOS probes use dedicated temporary keychains/accounts and must never read or lock a personal/default keychain. Keep native feasibility executables outside packages.

No real account access, paid agent calls, tags or release publication. Test integrations with local HTTP doubles and compiled fake agents.

## Workstation resource budget

**Builds, tests, Clippy and browser scenarios run on hosted GitHub Windows runners, never on this PC.** Local editing, Git, formatting and lightweight inventory generation are allowed. Keep Linux/macOS native and browser checks deferred to final qualification. Do not treat those deferred checks as passed.

Only if the owner explicitly requests a local heavy diagnostic later: one command at a time, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, Node `--test-concurrency=1`, BelowNormal priority and at most two logical processors. The ignored `.artifacts/rust-tools/low-impact.ps1` helper applies those limits.

## Build and tests

Use `rust-toolchain.toml` and the committed `Cargo.lock`. Windows CI runs:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings`
- `cargo test --workspace --exclude atelier-ui --locked`
- separate `atelier-ui` Clippy/check for `wasm32-unknown-unknown`
- real WebDriver journeys against synthetic data.

`cargo xtask build-ui` uses pinned Trunk 0.22.0 and wasm-bindgen CLI 0.2.129; fetch locked dependencies before its offline build. Browser selection is not browser qualification. Read actual job logs before claiming success.

The existing `serve-prototype` command is the current development entry point, not the finished product. Its API remains partial until the rebuild delivers its screens and use cases. Historical Node comparators and the legacy UI bridge are reference tools; they are no longer mandatory gates for every increment. Do not regenerate an old expectation to disguise a bug: document intentional contract changes and test the new behavior.

Add meaningful regression tests for nontrivial behavior, trust boundaries, concurrency and data loss. Prefer complete user journeys over tests mirroring implementation. Existing Node tests may inform the new tests, but exact Node output is not the specification.

## UI and coding style

Preserve the existing capabilities and translations, including French user-facing text. Build each screen with its use case, including loading, empty/error states, keyboard/focus and unsaved drafts. Avoid exposing implementation details in user flows.

Use idiomatic Rust, explicit errors at boundaries and justified unsafe blocks. Keep existing JavaScript ES module conventions while reference code remains. Edit Sass sources and regenerate CSS when using the legacy styling pipeline; final build dependencies must be documented.

## Git and progress

Conventional Commits in English are required: `<type>[scope][!]: <imperative description>`. Commit and push coherent increments on the working branch; both are already authorized. Remote verification may follow a candidate push because local heavy checks are prohibited. Mark pending/failed results honestly, fix failures and record real evidence.

Keep concise French progress updates. Do not declare completion until the active rebuild checklist is satisfied. Do not infer permission to publish from permission to commit/push.
