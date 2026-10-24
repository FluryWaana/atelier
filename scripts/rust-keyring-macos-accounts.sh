#!/usr/bin/env bash
set -euo pipefail
trap 'echo "macOS account fixture failed at line $LINENO" >&2' ERR

# CI-only directory-service records; no password or interactive login is provisioned.
[[ ${GITHUB_ACTIONS:-} == true && ${ATELIER_TEST_DISPOSABLE_KEYRING:-} == 1 && $(uname -s) == Darwin && $# == 0 ]] || exit 2
probe_root=$(mktemp -d /Users/Shared/atelier-keyring-accounts.XXXXXXXX)
probe_user="atelier$(uuidgen | cut -c1-12 | tr -d '-' | tr '[:upper:]' '[:lower:]')"
probe_uid=600
created=0
other_started=0
as_other() {
  sudo -n -H -u "$probe_user" env GITHUB_ACTIONS=true ATELIER_TEST_DISPOSABLE_KEYRING=1 \
    "$probe_root/fixture" test-keyring-account "$1" "$probe_root/shared"
}
cleanup() {
  local result=$?
  if [[ $other_started == 1 ]]; then
    if [[ -f "$probe_root/shared/other.json" ]]; then
      as_other finish-other || result=1
    else
      as_other cleanup || result=1
    fi
  fi
  if [[ -f "$probe_root/shared/service" ]]; then
    "$probe_root/fixture" test-keyring-account cleanup "$probe_root/shared" || result=1
  fi
  if [[ $created == 1 ]]; then sudo dscl . -delete "/Users/$probe_user" || result=1; fi
  [[ $(dirname "$probe_root") == /Users/Shared && $(basename "$probe_root") == atelier-keyring-accounts.* ]] || return 2
  sudo rm -rf -- "$probe_root"
  exit "$result"
}
trap cleanup EXIT
while id -nu "$probe_uid" >/dev/null 2>&1; do
  probe_uid=$((probe_uid + 1))
  [[ $probe_uid -lt 1600 ]] || exit 2
done
chmod 755 "$probe_root"
mkdir "$probe_root/shared" "$probe_root/home"
chmod 1777 "$probe_root/shared"
chmod 700 "$probe_root/home"
printf '%s' 'atelier-m02-synthetic-keyring' > "$probe_root/shared/.keyring-fixture"
chmod 644 "$probe_root/shared/.keyring-fixture"
cp target/debug/xtask "$probe_root/fixture"
chmod 755 "$probe_root/fixture"
if id "$probe_user" >/dev/null 2>&1; then exit 2; fi
sudo dscl . -create "/Users/$probe_user"
created=1
sudo dscl . -create "/Users/$probe_user" UniqueID "$probe_uid"
sudo dscl . -create "/Users/$probe_user" PrimaryGroupID 20
sudo dscl . -read "/Users/$probe_user" GeneratedUID >/dev/null
sudo dscl . -create "/Users/$probe_user" NFSHomeDirectory "$probe_root/home"
sudo dscl . -create "/Users/$probe_user" UserShell /bin/bash
sudo chown "$probe_uid:20" "$probe_root/home"
[[ $(id -u "$probe_user") == "$probe_uid" ]]
"$probe_root/fixture" test-keyring-account prepare "$probe_root/shared"
if "$probe_root/fixture" test-keyring-account other "$probe_root/shared"; then
  echo 'Reusing the same native account must fail' >&2
  exit 1
fi
[[ ! -e "$probe_root/shared/other.json" ]]
other_started=1
as_other other
"$probe_root/fixture" test-keyring-account verify "$probe_root/shared"
