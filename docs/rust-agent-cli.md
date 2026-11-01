# Agent profiles and CLI updates

Profiles select Codex, Claude Code or a custom command. Configure the executable and supported model or effort settings in the agent screen. Projects and tasks can select a profile.

Installation and connection checks belong to the profile. A failed task run can open the relevant profile for investigation. A configured connection and verified service access are distinct states. Version, usage and quota information depend on provider support.

## Discovery

Discovery uses explicit paths and known installation locations. On POSIX, candidates must be ordinary executable files. PATH entries must be absolute, with at most 256 entries inspected. The current directory is not searched implicitly.

Checks have bounded execution and filtered output. They must not expose credentials. Custom programs and unsupported installation layouts use their normal installation manager.

## Native CLI updates

Native CLI updating is available for supported provider installations on Windows. Linux and macOS use the program's normal package manager. Atelier does not provide initial installation of agent CLIs.

An explicit check reads the installed version and public package metadata over HTTPS. Only stable versions with three numeric components are accepted. This metadata lookup does not require a Node.js runtime or npm account.

The update preview binds the saved profile, native directory, installed and published versions, check time and executable SHA256. It expires after ten minutes. Confirmation revalidates these values. Close other sessions of the CLI before updating.

The native command is `update`, with a filtered environment and no prompt or token. Windows pins the executable until suspended creation inside its Job Object and verifies its identity before resuming. A file lock prevents concurrent Atelier updates of that installation. Execution is bounded to 180 seconds and 1 MiB per output stream. Raw output is not displayed.

## Recovery and limits

The persistent journal distinguishes preparation, running, observed version, refusal before launch and uncertainty. Interruption never causes automatic replay. A fresh check can observe the current version without proving how a prior command completed.

A version below the target remains an explicit failure. Providers can install a newer stable version than the preview. CLI rollback is not promised.

Runs, services, active queues, timers, remote effects, exports, backups and application updates block incompatible CLI operations. Changing a profile or check invalidates its confirmation. See [process supervision](rust-posix-runtime.md) for execution guarantees and [verification](rust-final-qualification.md) for synthetic checks.
