# Launch and data directories

The `atelier` binary provides `launch`, `stop`, `serve`, `--help` and `--version`. With no command, it uses `launch`.

```text
atelier --help
atelier --version
atelier launch --data-dir SYNTHETIC_DATA --assets BUILT_UI --port 4317 --no-browser
atelier stop --data-dir SYNTHETIC_DATA --port 4317
atelier serve --data-dir SYNTHETIC_DATA --assets BUILT_UI --port 4317
```

Replace the example paths with explicit directories. `launch` starts a background server or reuses a verified instance. `serve` keeps the server in the foreground. Use the same data directory and port to stop an instance. Unknown or repeated options and missing UI assets are refused.

## Instance identity

The launcher probes numeric loopback without proxies or redirects. It verifies an HMAC proof, the application identity, protocol `atelier-local/1` and canonical data directory before sending authenticated commands. A different service or workspace is never adopted or stopped.

Startup waits up to 20 seconds. A failed start stops only the child created by that attempt. Shutdown uses the authenticated `/api/shutdown` route and waits for the instance to exit, allowing the server to close its runs, services and accepted writes.

The browser opens through native platform facilities. Session tokens are not printed or passed in browser command arguments. A browser launch error leaves the server available.

## Data locations

The UI defaults to the binary's `ui` directory. Portable installations keep data in their `data` subdirectory. Installed workspaces use these locations:

| Platform | Workspace |
| --- | --- |
| Windows | `%LOCALAPPDATA%/AtelierKanban/workspace` |
| Linux | `$XDG_DATA_HOME/atelier-kanban-workspace`, with `~/.local/share` as the base fallback |
| macOS | `~/Library/Application Support/AtelierKanban/workspace` |

A compatible workspace at the alternate application data location remains usable. An unrecognized format requires confirmation before opening a separate empty workspace. Refusal creates no workspace. Invalid markers, links and inaccessible paths remain errors. No workspace is silently moved, converted or erased.

The database has an explicit identity and version. Unsupported formats are refused. Keep the original directory intact when investigating a startup error.

## Folder selection

Native folder selection reports a path without granting repository access. Authorization still requires confirmation. Cancelling or failing to select a directory preserves the form draft. Windows uses the Shell dialog, macOS uses the system dialog through `osascript`, and Linux uses Zenity.

## Portable data import

```text
atelier import-data --source ABSOLUTE_SOURCE --destination ABSOLUTE_EMPTY_DESTINATION --confirm
```

Close the source workspace first. Import holds its data lock while making a private copy, verifies database identity and integrity, and publishes into an empty distinct destination. It preserves the database WAL and included backups. A lock database can be created in the source without changing business records.

Sessions, locks and local repository authorizations are excluded. Reauthorize repositories after import. Credential decryption across accounts or machines is not guaranteed.

Limits are 100,000 entries, depth 32, 128 MiB per ordinary file and 1 GiB total outside the database. The database and WAL each have a 1 GiB limit. Links and special files are refused. Import does not delete content to satisfy a limit.

See [packages and updates](rust-packages.md) and [verification](rust-final-qualification.md) for installation and platform checks.
