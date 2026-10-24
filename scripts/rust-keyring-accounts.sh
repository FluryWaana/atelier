#!/usr/bin/env bash
set -euo pipefail

# This provisions accounts only on disposable CI hosts, never a developer desktop.
[[ ${GITHUB_ACTIONS:-} == true && ${ATELIER_TEST_DISPOSABLE_KEYRING:-} == 1 && $(uname -s) == Linux ]] || exit 2

if [[ $# == 3 && ($1 == first || $1 == other) ]]; then
  phase=$1
  probe_root=$2
  probe_user=$3
  unset GNOME_KEYRING_CONTROL DISPLAY WAYLAND_DISPLAY
  umask 077
  account_directory="$probe_root/first"
  [[ $phase == first ]] || account_directory="$probe_root/home"
  export XDG_DATA_HOME="$account_directory/data"
  export XDG_RUNTIME_DIR="$account_directory/runtime"
  mkdir -p "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
  keyring_pid=''
  cleanup_session() {
    local result=$?
    if [[ $phase == first && -f "$probe_root/shared/service" ]]; then
      "$probe_root/fixture" test-keyring-account cleanup "$probe_root/shared" || result=1
    fi
    if [[ -n $keyring_pid ]]; then
      kill "$keyring_pid" 2>/dev/null || true
      wait "$keyring_pid" 2>/dev/null || true
    fi
    exit "$result"
  }
  trap cleanup_session EXIT
  gnome-keyring-daemon --foreground --unlock --components=secrets < <(printf '%s' 'synthetic-account-password') &
  keyring_pid=$!
  gdbus wait --session --timeout 15 org.freedesktop.secrets
  if [[ $phase == first ]]; then
    "$probe_root/fixture" test-keyring-account prepare "$probe_root/shared"
    if "$probe_root/fixture" test-keyring-account other "$probe_root/shared"; then
      echo 'Reusing the same native account must fail' >&2
      exit 1
    fi
    [[ ! -e "$probe_root/shared/other.json" ]]
    # Only the encrypted fixtures are shared; each process has its own native UID and bus.
    sudo -n -H -u "$probe_user" env GITHUB_ACTIONS=true ATELIER_TEST_DISPOSABLE_KEYRING=1 \
      dbus-run-session -- bash "$probe_root/probe.sh" other "$probe_root" "$probe_user"
    "$probe_root/fixture" test-keyring-account verify "$probe_root/shared"
  else
    "$probe_root/fixture" test-keyring-account other "$probe_root/shared"
  fi
  exit 0
fi

[[ $# == 0 ]] || exit 2
probe_root=$(mktemp -d /tmp/atelier-keyring-accounts.XXXXXXXX)
probe_user="atelier$(basename "$probe_root" | cut -d. -f2 | tr '[:upper:]' '[:lower:]')"
created=0
cleanup() {
  local result=$?
  if [[ $created == 1 ]]; then sudo userdel "$probe_user" || result=1; fi
  [[ $(dirname "$probe_root") == /tmp && $(basename "$probe_root") == atelier-keyring-accounts.* ]] || return 2
  sudo rm -rf -- "$probe_root"
  exit "$result"
}
trap cleanup EXIT
chmod 755 "$probe_root"
mkdir "$probe_root/shared" "$probe_root/home"
chmod 1777 "$probe_root/shared"
printf '%s' 'atelier-m02-synthetic-keyring' > "$probe_root/shared/.keyring-fixture"
chmod 644 "$probe_root/shared/.keyring-fixture"
cp target/debug/xtask "$probe_root/fixture"
cp scripts/rust-keyring-accounts.sh "$probe_root/probe.sh"
chmod 755 "$probe_root/fixture" "$probe_root/probe.sh"
sudo useradd --no-create-home --home-dir "$probe_root/home" --shell /bin/bash "$probe_user"
created=1
sudo chown "$probe_user" "$probe_root/home"
timeout --kill-after=5s 180s dbus-run-session -- bash "$probe_root/probe.sh" first "$probe_root" "$probe_user"
