#!/bin/sh
set -eu
root=/opt/atelier-kanban-rust
if [ ! -d "$root" ] || [ -L "$root" ] || [ -L "$root/.package-lock" ] || [ ! -f "$root/.package-lock" ]; then
  echo 'Unexpected installation preserved; manual review required.' >&2
  exit 1
fi
exec 9<"$root/.package-lock"
flock -xn 9 || { echo 'Stop all Atelier Rust instances in every user session first.' >&2; exit 1; }
if [ -e "$root/.package-maintenance" ] || [ -L "$root/.package-maintenance" ]; then
  echo 'Existing maintenance state preserved; finish its recovery first.' >&2
  exit 1
fi
(umask 077; set -C; printf '%s\n' 'atelier-package-maintenance/1' > "$root/.package-maintenance")
if ! "$root/atelier" verify-installation; then
  if [ "$(cat "$root/.package-maintenance")" = 'atelier-package-maintenance/1' ]; then
    rm -- "$root/.package-maintenance"
  fi
  echo 'Modified installation preserved; no forced uninstall.' >&2
  exit 1
fi
