#!/bin/sh
set -eu
# During upgrade the new postinstall owns activation; the old package must not remove it.
case "${1:-}" in upgrade|failed-upgrade|1|2) exit 0 ;; esac
exec /usr/lib/atelier-kanban-rust/package-tool system-package remove --root /opt/atelier-kanban-rust --confirm
