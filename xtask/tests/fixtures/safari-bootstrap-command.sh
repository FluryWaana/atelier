#!/usr/bin/env bash
set -euo pipefail
name=${0##*/}
printf '%s %s\n' "$name" "$*" >> "$SAFARI_FIXTURE/commands"
case "$name" in
  uname) printf '%s\n' Darwin ;;
  pgrep) test -f "$SAFARI_FIXTURE/running" ;;
  pkill)
    [[ ${FAIL_STEP:-none} != stop ]] || exit 24
    rm "$SAFARI_FIXTURE/running"
    ;;
  sudo) "$@" ;;
  safaridriver)
    if [[ $1 == --enable ]]; then
      [[ ${FAIL_STEP:-none} != enable ]] || exit 23
      touch "$SAFARI_FIXTURE/enabled"
    else
      test "$1" == --version
    fi
    ;;
  open)
    test -f "$SAFARI_FIXTURE/enabled"
    test ! -f "$SAFARI_FIXTURE/running"
    [[ ${FAIL_STEP:-none} != launch ]] || exit 25
    touch "$SAFARI_FIXTURE/running"
    ;;
  *) exit 99 ;;
esac
