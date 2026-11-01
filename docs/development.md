# Development preview

[Atelier.cmd](../Atelier.cmd) starts the Windows preview with automatic rebuilding. It uses installed Rust, the user's PATH, the committed toolchain and lockfile, Trunk 0.22.1 and wasm-bindgen CLI 0.2.130.

The preview serves `http://127.0.0.1:4328` through the application's `serve --dev` entry point. Its synthetic workspace lives in `.artifacts/local-dev/data`. The launcher does not use or watch personal `data/` directories. Keep its console open while working.

## Rebuilding

Unchanged sources reuse the last successful build. Missing outputs trigger a build. A failed attempt never validates the cache.

The watcher detects changes, additions and deletions in Rust, translations, HTML, CSS and Cargo configuration. It excludes `.git`, `target`, `.artifacts` and `data` at every level, rejects linked source paths and groups closely spaced saves.

UI changes rebuild only the UI. Server changes rebuild only the native server. Shared code rebuilds both. Manifest, lockfile and Cargo configuration changes trigger dependency fetching.

The UI uses `cargo xtask build-ui --dev` and `.artifacts/rust-ui-dev`. Release assets remain in `.artifacts/rust-ui`. The server stays available during UI compilation and restarts to load successful assets. Server compilation requires stopping the running binary. Errors appear in the console and a later save retries the build.

## Reloading and shutdown

The browser reloads the whole page after a successful restart. After form input it asks before reloading. Cancelling preserves the draft, which can be saved before a manual refresh. Reloading resets UI state in memory.

Development restarts retain the local session. Normal startup rotates it. Reload injection is limited to development mode and preserves session authentication, Host and Origin checks, content security policy and asset limits.

An exclusive lock prevents duplicate supervisors. Shutdown uses authenticated `stop`, verifies workspace identity and waits for the owned server. An unknown process occupying the port is never killed. Synthetic data remains after shutdown.

## Verification

`scripts/atelier-watch.tests.ps1` exercises watcher and cache behavior on synthetic files without building or opening a browser.

`scripts/atelier-browser.tests.ps1 -Driver PATH_TO_MSEDGEDRIVER` uses PowerShell 7, real builds, the server and Edge WebDriver. It checks startup, reuse, rebuilds, draft confirmation, session retention and recovery. It temporarily edits selected source files, keeps their original bytes, rejects concurrent changes and restores the originals. Do not edit those files during this test.

`scripts/atelier-windows51.tests.ps1` checks the Windows PowerShell engine used by `Atelier.cmd`. Reports belong in `.artifacts/launcher-verification/` and must not contain session tokens.

The automatic launcher is specific to Windows. On Linux and macOS, use the source build and foreground `serve` commands in [getting started](getting-started.md).
