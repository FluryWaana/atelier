# Getting started

## Choose a package

Open [GitHub Releases](https://github.com/FluryWaana/atelier/releases), choose a release and read its platform instructions. Package availability is specific to each release. Preproduction packages can have limits described in their release notes.

Use the installed shortcut or package launcher to open Atelier Kanban. The application runs locally and opens a browser session. Local tasks and time tracking do not require a service account. Configure an external integration when you need one.

See [packages](rust-packages.md) for verification, target formats and updates.

## Build from source

Install Git and the Rust toolchain pinned by [rust-toolchain.toml](../rust-toolchain.toml), including the `wasm32-unknown-unknown` target. Install Trunk 0.22.1 and wasm-bindgen CLI 0.2.130 in your PATH. Native compiler requirements are listed in [packaging](rust-packages.md).

Run these commands from the repository root:

```sh
cargo fetch --locked
cargo xtask build-ui
cargo build -p atelier-app --locked
cargo run -p atelier-app --locked -- launch --data-dir .artifacts/quickstart/data --assets .artifacts/rust-ui --port 4317
```

The UI build uses locked dependencies offline after fetching them. The launcher starts the native server and opens an authenticated browser session. Keep its session URL private.

Stop this instance with:

```sh
cargo run -p atelier-app --locked -- stop --data-dir .artifacts/quickstart/data --port 4317
```

For foreground development, replace `launch` with `serve`, keep the terminal open and use `launch` from another terminal to open the verified browser session.

The example keeps its workspace under `.artifacts/quickstart/data`. Choose a distinct directory for development. Never run tests against a personal workspace.

On Windows, [Atelier.cmd](../Atelier.cmd) opens the [development preview](development-launcher.md) and rebuilds after source changes.

## First task

1. Create a local task from the welcome screen or board.
2. Enter its description and acceptance criteria.
3. Add a project if the task needs a Git repository. Authorize access to that directory.
4. Configure an agent profile if you want to run a coding agent.
5. Prepare the task worktree, run the profile and inspect its output and changes.
6. Review and confirm commits or remote publication when ready.

Agent programs have their own installation and authentication requirements. Atelier's build does not install them. Use the profile's checks to inspect the selected program.

## Troubleshooting

If a build rejects an asset, check the pinned UI tools and use `cargo xtask build-ui` so loader and size checks run together. If a port is occupied, choose a free port without stopping an unknown process.

An unsupported database is refused. Keep its directory intact and use a separate empty directory when trying the application. See [launch and data](rust-launcher.md) for instance identity and import rules.
