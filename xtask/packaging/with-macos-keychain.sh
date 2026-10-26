#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Darwin && ${CI:-} == true && ${ATELIER_RUST_FINAL_CAMPAIGN:-} == 1 && -n ${RUNNER_TEMP:-} && $# -gt 0 ]] || exit 2
fixture_base=$(cd -P "$RUNNER_TEMP" && pwd)
fixture=$(mktemp -d "$fixture_base/.atelier-final-keychain-XXXXXXXX")
keychain="$fixture/fixture.keychain-db"
umask 077
printf '%s\n' atelier-final-keychain/1 > "$fixture/.keychain-fixture"
quoted=${keychain//\\/\\\\}
quoted=${quoted//\"/\\\"}
password=$(/usr/bin/openssl rand -hex 32)
created=0
cleanup() {
  result=$?
  trap - EXIT
  if [[ $created == 1 ]]; then
    /usr/bin/security delete-keychain "$keychain" >/dev/null 2>&1 || result=1
  fi
  # Keep the dedicated marker for failed-run examination; never inspect a default/search-list keychain.
  exit "$result"
}
trap cleanup EXIT
created=1
printf 'create-keychain -p %s "%s"\nunlock-keychain -p %s "%s"\n' "$password" "$quoted" "$password" "$quoted" | /usr/bin/security -i >/dev/null
[[ -f "$keychain" && ! -L "$keychain" ]]
/usr/bin/security set-keychain-settings -lut 21600 "$keychain"
unset password
export ATELIER_TEST_KEYCHAIN="$keychain"
export ATELIER_TEST_DISPOSABLE_KEYRING=1
"$@"
