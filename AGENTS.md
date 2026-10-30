# Repository Guidelines

## Application

Atelier Kanban is a local application written in Rust for task management, agent execution, Git reviews and time tracking. It is in preproduction. Follow the feature checklist in `docs/features.md` and the architecture in `docs/architecture.md`.

Preserve user-visible capabilities and improve quality with the smallest complete change. Use native Rust types and explicit contracts. There is no backward compatibility requirement for unsupported formats or invalid inputs.

## Structure and architecture

The Cargo workspace contains exactly:
- `atelier-core`: pure domain models, validation, application use cases and public contracts.
- `atelier-app`: composition, HTTP, SQLite, filesystem, processes, integrations and native adapters.
- `atelier-ui`: Leptos CSR/WASM interface.
- `xtask`: build, verification and packaging.

Keep domain/application independent of I/O and platform APIs. Prefer explicit typed models and focused functions. Keep transactions short, reserve external operations before I/O, and revalidate before committing their results. Do not add abstractions without a concrete caller.

Regression fixtures belong to their owning crates' `tests/fixtures/` directories. The application and its build use Rust without a Node runtime dependency. `scripts/` contains native keyring probes and the optional local preview helper. Do not recreate deleted reports unless requested.

## Data and security

Work exclusively with synthetic databases, repositories, credentials and agent fixtures. Never open, modify or remove user `data/` during development. The authorization to reset the design does not require touching local user files.

The application uses an explicit database identity/version and refuses unsupported data; never silently erase a database. Preserve atomic writes, backups, rollback and bounded storage access. Bind HTTP to loopback, require the local session and validate Host/Origin. Never commit tokens, databases, backups or logs, or expose secrets to the frontend.

Native keyring tests use unique synthetic services. Windows temporary-account provisioning is CI-only. Linux account/collection-lock probes require the disposable isolated D-Bus/XDG session and their CI guard. macOS probes use dedicated temporary keychains/accounts and must never read or lock a personal/default keychain. Keep native feasibility executables outside packages.

No real account access, paid agent calls, tags or release publication. Test integrations with local HTTP doubles and compiled fake agents.

## Local verification

Local formatting, compilation, unit and integration tests, Clippy and synthetic browser journeys are authorized with normal workstation resources. No additional confirmation is needed for these local checks or for fixing failures within the requested work. Never touch user `data/` or use real accounts, credentials or paid agents.

The Rust workflow remains manual-only. Do not launch or rerun CI, create tags or publish releases. Local verification does not authorize those actions or override the isolated native-account and keychain requirements above. Do not treat skipped or deferred checks as passed.

## Build and tests

Use `rust-toolchain.toml` and the committed `Cargo.lock`. The final manual verification includes:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --exclude atelier-ui --all-targets --locked -- -D warnings`
- `cargo test --workspace --exclude atelier-ui --locked`
- separate `atelier-ui` Clippy/check for `wasm32-unknown-unknown`
- real WebDriver journeys against synthetic data.

`cargo xtask build-ui` uses pinned Trunk 0.22.1 and wasm-bindgen CLI 0.2.130; fetch locked dependencies before its offline build. Browser selection is not browser qualification. Read actual job logs before claiming success.

Use the application's `serve` entry point for synthetic development and browser qualification. Do not regenerate an expectation to disguise a bug. Document intentional contract changes and test the new behavior.

Add meaningful regression tests for nontrivial behavior, trust boundaries, concurrency and data loss. Unit tests cover domain rules and boundary cases. Integration tests and user journeys cover adapters and complete behaviors. Keep tests deterministic and independent. Do not mirror the implementation merely to increase coverage.

## UI and coding style

Local tickets and local time tracking must always remain available, including when an external module is selected by default. Defaults must never disable local creation.

Use plain, natural user-facing text in French and English. Do not use em dashes, en dashes, semicolons or decorative text arrows. Use short sentences and ordinary words instead. These punctuation restrictions apply to prose, not Rust or other code syntax.

Preserve the existing capabilities and translations, including French user-facing text. Build each screen with its use case, including loading, empty/error states, keyboard/focus and unsaved drafts. Avoid exposing implementation details in user flows.

Use idiomatic Rust, explicit errors at boundaries and justified unsafe blocks. Edit the Rust UI styling sources directly; final build dependencies must be documented. Do not reintroduce the retired Node/Sass pipeline.

## Git and progress

Conventional Commits in English are required: `<type>[scope][!]: <imperative description>`. Commit and push coherent increments on the working branch; both are already authorized. No CI launch or rerun is currently authorized. Existing logs may be read. Mark pending/failed results honestly, fix known failures and record real evidence.

Keep concise French progress updates. Distinguish completion of the requested change from qualification of the whole application. Do not infer permission to publish from permission to commit/push.
