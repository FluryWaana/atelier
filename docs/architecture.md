# Architecture

Atelier Kanban is a Cargo workspace with four members. The native application serves a browser interface over loopback HTTP and stores workspace data in SQLite.

| Crate | Responsibility |
| --- | --- |
| `atelier-core` | Domain models, validation, application use cases and public contracts |
| `atelier-app` | Composition, HTTP, SQLite, filesystem, credentials, Git, processes and integrations |
| `atelier-ui` | Leptos CSR interface compiled to WebAssembly |
| `xtask` | UI builds, source inventory, verification and packaging |

## Boundaries

Domain and application contracts in `atelier-core` remain independent of filesystem, network and platform APIs. Native adapters belong to `atelier-app`. The UI consumes public contracts and keeps translated presentation logic in `atelier-ui`.

The build uses [rust-toolchain.toml](../rust-toolchain.toml) and [Cargo.lock](../Cargo.lock). Trunk bundles the UI and CSS directly. No Node.js or Sass pipeline is required. Regression fixtures belong in each owning crate's `tests/fixtures/` directory.

## Local security

The HTTP server binds to loopback and validates the local session, Host and Origin. The initial browser session fragment is removed after use. Credentials remain in native protected storage and are not returned to the frontend. The content security policy uses an external module loader.

Repository access requires explicit authorization and bounded reads. Paths and service directories reject unsupported names and traversal. Agents and connectors receive selected inputs. Output, execution time and persisted logs have explicit limits.

See [process supervision](rust-posix-runtime.md) for platform guarantees. Agents and connectors execute with the user's rights. Supervision does not provide a security sandbox.

## Storage and concurrency

SQLite has an explicit application identity and schema version. Unsupported databases are refused without erasure. Transactions remain short. Operations reserve resources before external I/O and revalidate revisions before committing results.

Remote effects use persistent intentions and confirmation. An interrupted write becomes uncertain and requires reconciliation. Recovery does not blindly repeat a remote write. Backups, restoration and updates preserve human changes and block incompatible activity.

## Project cloning

The core validates requests and destination names. The app reserves the operation and uses installed Git. HTTPS supports public repositories or a selected forge connection restricted to its exact origin. SSH uses the local SSH agent and known hosts. Unknown hosts are refused. Personal Git credential helpers, SSH aliases, private key files and SSH configuration are not loaded.

The parent directory is opened without following links. A unique temporary directory receives the repository and default branch checkout. Downloaded objects and checked out content are each limited to 512 MiB, with at most 10,000 files. Hooks, personal filters and remote submodules are not executed.

Publication atomically refuses an existing destination. Cancellation stops owned descendants before removing the verified temporary directory. A crash can leave a temporary directory that is not automatically adopted or deleted. A completed repository remains on disk if authorization or project creation fails and can be added as an existing directory.

## References

* [Launch and data](rust-launcher.md)
* [Agents](rust-agent-cli.md)
* [Integrations](rust-integrations.md)
* [Connectors](rust-connectors.md)
* [Workflows](rust-workflows.md)
* [Packages](rust-packages.md)
* [Verification](rust-final-qualification.md)
