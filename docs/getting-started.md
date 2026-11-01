# Getting started

## Choose a package

Open [GitHub Releases](https://github.com/FluryWaana/atelier/releases), choose a release and read its platform instructions. Package availability is specific to each release. Preproduction packages can have limits described in their release notes.

Use the installed shortcut or package launcher to open Atelier Kanban. The application runs locally and opens a browser session. Local tasks and time tracking do not require a service account. Configure an external integration when you need one.

See [packages](packaging.md) for verification, target formats and updates.

## Runtime requirements

Use a current desktop browser. Install Git for repository operations and install each coding agent you want to use separately.

On Linux, credential storage requires `secret-tool` and an unlocked Secret Service collection in the desktop session. Folder selection uses Zenity and browser opening uses `xdg-open`. On macOS, credential storage uses Keychain and service port inspection uses the system `lsof` utility. Windows uses its native protected storage and Shell dialogs.

## Build from source

Install Git and the Rust toolchain pinned by [rust-toolchain.toml](../rust-toolchain.toml), including the `wasm32-unknown-unknown` target. Install Trunk 0.22.1 and wasm-bindgen CLI 0.2.130 in your PATH. Native compiler requirements are listed in [packaging](packaging.md).

With Rustup and the native compiler tools installed, prepare the build tools from the repository root:

```sh
rustup show
cargo install --locked trunk --version 0.22.1
cargo install --locked wasm-bindgen-cli --version 0.2.130
```

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

For foreground development, replace `launch` with `serve` and keep the terminal open. Run the same `launch` command from another terminal to open that instance's authenticated browser session.

The example keeps its workspace under `.artifacts/quickstart/data`. Choose a distinct directory for development. Never run tests against a personal workspace.

On Windows, [Atelier.cmd](../Atelier.cmd) opens the [development preview](development.md) and rebuilds after source changes.

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

An unsupported database is refused. Keep its directory intact and use a separate empty directory when trying the application. See [launch and data](launcher.md) for instance identity and import rules.
