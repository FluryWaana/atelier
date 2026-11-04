# Getting started

## Choose a package

Open [GitHub Releases](https://github.com/FluryWaana/atelier/releases), choose a release and read its platform instructions. Package availability is specific to each release. Preproduction packages can have limits described in their release notes.

Use the installed shortcut or package launcher to open Atelier Kanban. The application runs locally and opens a browser session. Local tasks and time tracking do not require a service account. Configure an external integration when you need one.

See [packages](packaging.md) for verification, target formats and updates.

## Runtime requirements

Use a current desktop browser. Install Git for repository operations and install each coding agent you want to use separately.

On Linux, credential storage requires `secret-tool` and an unlocked Secret Service collection in the desktop session. Folder selection uses Zenity and browser opening uses `xdg-open`. On macOS, credential storage uses Keychain and service port inspection uses the system `lsof` utility. Windows uses its native protected storage and Shell dialogs.

## Build from source

Install Git and Rustup. The repository pins its Rust toolchain and the `wasm32-unknown-unknown` target in [rust-toolchain.toml](../rust-toolchain.toml). Install the native compiler tools for your platform:

| Platform | Source build tools |
| --- | --- |
| Windows x64 | Visual Studio Build Tools with the MSVC C++ toolchain and Windows SDK, including `rc.exe` |
| Linux | Native C compiler and archiver |
| macOS | Xcode command line tools |

Building installers requires additional tools listed in [packaging](packaging.md). They are not required to run from source.

Clone the repository and enter its directory:

```sh
git clone https://github.com/FluryWaana/atelier.git
cd atelier
```

Prepare the pinned Rust toolchain and UI tools from the repository root. Keep Cargo's binary directory in your PATH so `trunk` and `wasm-bindgen` are available:

```sh
rustup show
cargo install --locked trunk --version 0.22.1
cargo install --locked wasm-bindgen-cli --version 0.2.130
```

For release UI builds, download [Binaryen 133](https://github.com/WebAssembly/binaryen/releases/tag/version_133) for your platform and add its `bin` directory to PATH. `wasm-opt --version` must print `wasm-opt version 133 (version_133)`. Install this tool before the offline build. Development UI builds with `cargo xtask build-ui --dev` do not require it.

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

For help, open a [GitHub issue](https://github.com/FluryWaana/atelier/issues) with the version, operating system and reproduction steps. Remove credentials and session URLs from shared output.
