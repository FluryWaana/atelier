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

The former Node implementation is preserved only in Git history at `7f48996b9626ccff79ecf0e1be7f8fa43b7b90e6`. Its source trees, launchers, delivery tools and code archives have been removed from the active Rust project. Ten useful JSON regression fixtures remain under their owning Rust crates' `tests/fixtures/` directories. Do not restore Node comparators or runtime dependencies as delivery gates. The source inventory helps find current Rust capabilities and tests; `scripts/` contains Rust CI keyring probes and the optional local preview helper.

## Data and security

Work exclusively with synthetic databases, repositories, credentials and agent fixtures. Never open, modify or remove user `data/` during development. The authorization to reset the design does not require touching local user files.

The rebuilt application uses its own explicit database identity/version and refuses unsupported data; never silently erase a database. Preserve atomic writes, backups, rollback and bounded storage access. Bind HTTP to loopback, require the local session and validate Host/Origin. Never commit tokens, databases, backups or logs, or expose secrets to the frontend.

Native keyring tests use unique synthetic services. Windows temporary-account provisioning is CI-only. Linux account/collection-lock probes require the disposable isolated D-Bus/XDG session and their CI guard. macOS probes use dedicated temporary keychains/accounts and must never read or lock a personal/default keychain. Keep native feasibility executables outside packages.

No real account access, paid agent calls, tags or release publication. Test integrations with local HTTP doubles and compiled fake agents.

## Workstation resource budget

On 2026-10-27 the owner explicitly authorized local builds with normal workstation resources. Compilation is now allowed, including compiling test targets without executing them. **Do not run local tests, Clippy or browser scenarios unless separately authorized.** Local editing, Git, formatting and lightweight inventory generation remain allowed. Never touch user `data/`.

The earlier build limits (one Cargo job, BelowNormal and two CPUs) were explicitly lifted. The Rust workflow remains manual-only; commits and pushes must not trigger it. On 2026-10-27 the owner withdrew authorization for CI: do not launch or rerun any workflow. Earlier corrective-rerun authorization is superseded. Local compilation remains authorized; local tests, Clippy and browser scenarios remain prohibited. Do not treat deferred checks as passed.

## Build and tests

Use `rust-toolchain.toml` and the committed `Cargo.lock`. The final manual verification includes:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings`
- `cargo test --workspace --exclude atelier-ui --locked`
- separate `atelier-ui` Clippy/check for `wasm32-unknown-unknown`
- real WebDriver journeys against synthetic data.

`cargo xtask build-ui` uses pinned Trunk 0.22.0 and wasm-bindgen CLI 0.2.129; fetch locked dependencies before its offline build. Browser selection is not browser qualification. Read actual job logs before claiming success.

Use the real Rust `serve` entry point for synthetic development and browser qualification. The `serve-prototype` command, Node comparators and legacy UI bridge have been retired. Do not regenerate an old expectation to disguise a bug: document intentional contract changes and test the new behavior.

Add meaningful regression tests for nontrivial behavior, trust boundaries, concurrency and data loss. Prefer complete user journeys over tests mirroring implementation. Existing Node tests may inform the new tests, but exact Node output is not the specification.

## UI and coding style

Local tickets and local time tracking must always remain available, including when an external module is selected by default. Defaults must never disable local creation.

Use plain, natural user-facing text in French and English. Do not use em dashes, en dashes, semicolons or decorative text arrows. Use short sentences and ordinary words instead. These punctuation restrictions apply to prose, not Rust or other code syntax.

Preserve the existing capabilities and translations, including French user-facing text. Build each screen with its use case, including loading, empty/error states, keyboard/focus and unsaved drafts. Avoid exposing implementation details in user flows.

Use idiomatic Rust, explicit errors at boundaries and justified unsafe blocks. Edit the Rust UI styling sources directly; final build dependencies must be documented. Do not reintroduce the retired Node/Sass pipeline.

## Git and progress

Conventional Commits in English are required: `<type>[scope][!]: <imperative description>`. Commit and push coherent increments on the working branch; both are already authorized. No CI launch or rerun is currently authorized. Existing logs may be read. Mark pending/failed results honestly, fix known failures and record real evidence.

Keep concise French progress updates. Do not declare completion until the active rebuild checklist is satisfied. Do not infer permission to publish from permission to commit/push.
