# Process supervision

Agents, commands, services, connectors, reviews and workflows share application reservations, mutation guards, stream limits, redaction, timeouts and recovery rules. Process adapters implement the operating system details.

## Windows

Owned processes use Windows Job Objects for descendant management. Operations that require an approved executable pin and recheck its identity before execution. Cancellation waits for owned process cleanup before releasing activity guards.

## Linux and macOS

Each command has a distinct POSIX process group. The leader remains owned until cleanup to avoid signalling a reused group identifier. Linux observes it through `waitid(WNOWAIT)`. macOS observes leader identity and zombie state through `proc_pidinfo`.

Stopping sends SIGKILL to the owned group and checks for remaining members before reaping the leader. Numeric `/bin/ps` inspection has no shell or inherited environment and is limited to two seconds, 4 MiB and 100,000 lines. Failed inspection or cleanup keeps the activity guard rather than reporting success.

A descendant can escape by calling `setsid` or changing its process group. Use foreground programs and inspect detached daemons before restoring or updating. This supervision is not a sandbox. An application crash or forced shutdown can leave processes. Recovery does not adopt or kill a persisted PID blindly.

## Working directories and executables

POSIX run and service directories are fixed with `fchdir` on retained descriptors. Renaming a path cannot silently redirect writes. Service and verification commands explicitly use `/bin/sh -c`. Agents and connectors launch directly.

POSIX connector invocations copy approved executable bytes into a private directory, verify SHA256, set mode `0500` and execute the copy. Source changes before copying refuse the call. Adjacent libraries are not copied, so modules must be standalone or use system libraries. Secrets use stdin. A process with the same local user rights is not isolated from that account.

## Service ports

Linux reads selected process sockets through `/proc`, bounded to 65,536 descriptors, 1 MiB per table and 16 MiB in total. macOS uses `/usr/sbin/lsof` with exact PIDs and numeric IPv4 and IPv6 fields, no DNS lookup, a two second timeout and 1 MiB output limit.

Group membership is checked before and after inspection. A displayed listener is a point in time observation and does not prove continuous availability or an HTTP response.

Restarting a service with an automatic port reserves a new port. A configured port is requested exactly and fails if unavailable. Other services remain independent.

## Tests

Synthetic process fixtures cover ordinary descendants, cancellation, retained activity on cleanup failure, interactive protocols, connector copies and listener inspection. The `posix-runtime` fixture deliberately creates a detached descendant and closes it through a pipe owned by the test. It verifies the documented limit rather than claiming containment of detached processes.

See [verification](testing.md) for platform execution requirements.
