#!/bin/sh
set -eu
for executable in /proc/[0-9]*/exe; do
  if [ "$(readlink "$executable" 2>/dev/null || true)" = /opt/atelier-kanban/runtime/node ]; then
    echo 'Arrêtez Atelier Kanban (atelier-kanban --stop) dans chaque session utilisateur avant de modifier le paquet.' >&2
    exit 1
  fi
done
