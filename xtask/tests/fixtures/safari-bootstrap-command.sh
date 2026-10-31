#!/usr/bin/env bash
set -euo pipefail
name=${0##*/}
printf '%s %s\n' "$name" "$*" >> "$SAFARI_FIXTURE/commands"
case "$name" in
  uname) printf '%s\n' Darwin ;;
  pgrep) test -f "$SAFARI_FIXTURE/running" ;;
  pkill) rm "$SAFARI_FIXTURE/running" ;;
  sudo) "$@" ;;
  safaridriver)
    if [[ $1 == --enable ]]; then
      [[ ${FAIL_ENABLE:-0} == 0 ]] || exit 23
      touch "$SAFARI_FIXTURE/enabled"
    else
      test "$1" == --version
    fi
    ;;
  open)
    test -f "$SAFARI_FIXTURE/enabled"
    test ! -f "$SAFARI_FIXTURE/running"
    touch "$SAFARI_FIXTURE/running"
    ;;
  *) exit 99 ;;
esac
