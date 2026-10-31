#!/usr/bin/env bash
set -euo pipefail
[[ ${CI:-} == true && ${ATELIER_RUST_FINAL_CAMPAIGN:-} == 1 && $(uname -s) == Darwin ]] || exit 2

# Apply automation settings before starting a fresh browser in the disposable runner.
if pgrep -x -u "$UID" Safari >/dev/null; then
  pkill -TERM -x -u "$UID" Safari
  deadline=$((SECONDS + 10))
  while pgrep -x -u "$UID" Safari >/dev/null; do
    if (( SECONDS >= deadline )); then
      printf '%s\n' 'Safari ne s’est pas arrêté avant sa préparation.' >&2
      exit 1
    fi
    sleep 0.1
  done
fi
sudo safaridriver --enable
safaridriver --version
open -a /Applications/Safari.app
