#!/bin/sh
set -eu
root=/opt/atelier-kanban-rust
tool=/usr/lib/atelier-kanban-rust/package-tool
if [ -e "$root" ] || [ -L "$root" ]; then
  if [ ! -x "$tool" ] || [ -L "$tool" ]; then
    echo 'Existing foreign installation preserved; administrator review required.' >&2
    exit 1
  fi
  "$tool" system-package reserve --root "$root" --confirm
fi
