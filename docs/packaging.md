# Packages and updates

Atelier Kanban is in preproduction. Use [GitHub Releases](https://github.com/FluryWaana/atelier/releases) for published files, release notes and platform availability. A supported build target does not imply that every release contains a qualified package for it.

## Targets and tools

| Platform | Rust target | Distribution formats | Additional installer tools |
| --- | --- | --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc` | Signed ZIP payload and Setup | Inno Setup 6 |
| Linux x64 | `x86_64-unknown-linux-gnu` | Signed ZIP payload, DEB and RPM | `dpkg-deb`, `rpmbuild`, `desktop-file-validate` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | Signed ZIP payload, DEB and RPM | `dpkg-deb`, `rpmbuild`, `desktop-file-validate` |
| macOS Intel | `x86_64-apple-darwin` | Signed ZIP payload | None |
| macOS Apple Silicon | `aarch64-apple-darwin` | Signed ZIP payload | None |

All builds require the [source build tools](getting-started.md#build-from-source), the committed Rust toolchain and lockfile, Trunk 0.22.1 and wasm-bindgen CLI 0.2.130. Windows uses the static C runtime. Import checks use MSVC `dumpbin` discovered through `vswhere` to reject external Visual C++ runtime DLLs. Build tools are not runtime requirements for users.

## UI build

```sh
cargo fetch --locked
cargo xtask build-ui
```

Release assets are written to `.artifacts/rust-ui`. The build runs Trunk offline with locked dependencies, externalizes the module loader for the content security policy and validates assets. Limits are 64 MiB per asset and 256 MiB in total.

`cargo xtask build-ui --dev` writes `.artifacts/rust-ui-dev` using incremental compilation, no debug information and UI optimization level 1. Development output is separate from the release payload.

## Payload and archives

```text
cargo xtask third-party-notices NOTICES.txt
cargo xtask stage-package TARGET BINARY ASSETS NOTICES.txt OUTPUT
cargo xtask verify-package OUTPUT
cargo xtask archive-package OUTPUT ARCHIVE.zip
cargo xtask verify-archive ARCHIVE.zip TARGET
```

The payload contains the native `atelier` binary, approved UI assets, LICENSE, [README.txt](../xtask/packaging/README.txt), dependency notices and a manifest. It excludes databases, credentials, backups, logs, source tools and feasibility executables. The manifest records sizes, SHA256 values and architecture. A manifest alone does not authenticate an archive.

Staging uses a neighboring temporary directory and refuses existing destinations. ZIP handling bounds entries and sizes and rejects traversal, absolute paths, links, unsupported types and collisions. Extraction verifies CRC, hashes and target. Native executable permissions are preserved.

Dependency notices are generated offline from locked registry packages and their local texts, including build tools and all targets. Missing texts require explicit version specific [supplements](../xtask/licenses/README.md). Unknown or changed evidence is refused.

## Distribution signatures

Signed metadata binds application identity, schema, target, version, archive size and SHA256, plus the manifest fingerprint. Trust must be selected independently of the archive. A package cannot authorize its own key.

Published releases use the distribution key pinned in the application. Synthetic keys are restricted to explicit fixtures. Private signing material must never appear in payloads, reports or logs.

Release files can include an update archive named `atelier-VERSION-TARGET.zip`, its `.zip.signed.json` metadata, platform installers and `atelier-update-public.json`. `SHA256SUMS` records release file hashes. Its Ed25519 signature covers the bytes of `atelier-kanban-release-checksums/1\n` followed by the checksum file, where `\n` represents a newline.

Windows uses `SHA256SUMS` and `SHA256SUMS.sig`. Linux and macOS use `SHA256SUMS-TARGET` and the matching `.sig` file for each target.

```text
cargo xtask verify-release-checksums SHA256SUMS SHA256SUMS.sig
```

This command authenticates the checksum file. It does not compare the downloaded files to its entries. After signature verification, check those file hashes from the download directory:

| Platform | File hash check |
| --- | --- |
| Linux | `sha256sum --check SHA256SUMS-TARGET` |
| macOS | `shasum -a 256 --check SHA256SUMS-TARGET` |
| Windows with PowerShell 7 | `Get-FileHash -Algorithm SHA256 DOWNLOADED_FILE` and compare the result with its entry in `SHA256SUMS` |

Replace `TARGET` with the selected target and keep the checksum filename paired with its signature. Run the signature command from a trusted source checkout, using paths to the downloaded checksum files.

Distribution signatures are separate from Windows Authenticode and do not remove SmartScreen warnings. An update archive is not an initialized portable installation.

## Installation

Windows Setup integrates the official icon, version resources and shortcut. Uninstallation preserves workspace data. Linux system installations are maintained through the package manager.

For a portable installation, download the archive for your operating system and architecture together with its `.zip.signed.json` file. Use a trusted source checkout prepared as described in [getting started](getting-started.md). This provides the installer command without executing an unverified downloaded binary.

Export the distribution public key pinned in that checkout to a new file:

```text
cargo xtask release-update-trust TRUSTED_PUBLIC_KEY.json
```

Do not substitute a key supplied only by the archive being verified. Choose an installation directory that does not exist and whose parent directory already exists. Replace the example paths with the downloaded files and that destination. Quote paths containing spaces:

```text
cargo run -p atelier-app --locked -- install --archive ARCHIVE.zip --signature ARCHIVE.zip.signed.json --trust TRUSTED_PUBLIC_KEY.json --destination NEW_INSTALLATION --mode portable
```

Installation authenticates metadata and archive content before publishing the destination. It keeps a stable launcher, trusted metadata and separate payload versions. Existing destinations are refused.

Open `NEW_INSTALLATION/atelier.exe` on Windows or run `NEW_INSTALLATION/atelier` on Linux or macOS. The installed launcher selects the verified active payload and opens the browser. Portable data stays under `NEW_INSTALLATION/data`.

Data locations and first launch behavior are documented in [launch and data](launcher.md).

## Application updates

The Updates screen checks the latest stable release of `FluryWaana/atelier`. It accepts numeric `VERSION` or `vVERSION` tags with three components and offers only newer versions. Prereleases are excluded from this stable channel.

A compatible release must provide the target archive and signed metadata in the same release and repository. Download redirects are restricted to permitted GitHub HTTPS hosts. URLs, response sizes and duration are bounded. No GitHub token or replacement trust key is requested from the user.

The server rereads the selected release before download. Signed metadata is authenticated before downloading the archive. Archive size and hashes are then checked by the package preparer. Progress reports actual received bytes and the signed total. Only one download can run per installation.

A source build can inspect releases. Activation requires an installed package with trusted metadata. Unsaved drafts block restart.

## Backup and recovery

Activation requires all uncertain external effects to be resolved. The worker stops the server, locks the workspace, checks SQLite integrity and creates an atomic catalog backup. An absent database is not created by this step. A failure prevents activation.

The update journal records the backup and activation state. Rollback restores the previous binary while retaining the current database and human files. It does not automatically restore data that could erase recent work. Unsupported database schemas remain errors.

Manual archive controls use these commands:

```text
atelier update-preview --archive NEXT.zip --signature NEXT.json --data-dir WORKSPACE_DIRECTORY --port 4317
atelier update-status
atelier update-apply --operation ID --confirm
atelier update-cancel --operation ID --confirm
atelier update-recover --operation ID --confirm
```

## Synthetic package testing

```text
cargo xtask synthetic-update-key NEW_KEYS
cargo xtask sign-package ARCHIVE.zip TARGET NEW_KEYS SIGNED_METADATA.json
cargo xtask verify-signed-package ARCHIVE.zip SIGNED_METADATA.json NEW_KEYS/synthetic-update-public.json TARGET CURRENT_VERSION
```

These commands use synthetic keys in new directories and refuse replacement. Keep fixture private keys out of distributed files. Package, installer and update checks are described in [verification](testing.md).
