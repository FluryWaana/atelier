#!/bin/sh
set -eu
marker=/opt/atelier-kanban-rust/.package-maintenance
if [ ! -L "$marker" ] && [ -f "$marker" ] && [ "$(cat "$marker")" = 'atelier-package-maintenance/1' ]; then
  rm -- "$marker"
fi
# User data and any untracked installation files are deliberately retained.
rmdir -- /opt/atelier-kanban-rust 2>/dev/null || true
