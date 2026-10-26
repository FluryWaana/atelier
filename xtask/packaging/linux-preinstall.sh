#!/bin/sh
set -eu
if [ -e /opt/atelier-kanban-rust ] || [ -L /opt/atelier-kanban-rust ]; then
  echo 'Existing Atelier Rust installation preserved. Use signed updates or uninstall the stopped package first.' >&2
  exit 1
fi
