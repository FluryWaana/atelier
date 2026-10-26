#!/bin/sh
# Run only in the final campaign, inside a disposable native-architecture container.
set -eu
test "${CI:-}" = true
test "${ATELIER_RUST_FINAL_CAMPAIGN:-}" = 1
test "$(id -u)" = 0
test -f /.dockerenv || test -f /run/.containerenv
format=${1:?deb or rpm required}
case "$format" in
  deb)
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y passwd util-linux gnome-keyring dbus-x11 curl jq /delivery/installers/*.deb
    ;;
  rpm) dnf install -y shadow-utils util-linux gnome-keyring dbus-daemon curl jq /delivery/installers/*.rpm ;;
  *) exit 1 ;;
esac
root=/opt/atelier-kanban-rust
bundle=/usr/lib/atelier-kanban-rust
tool="$bundle/package-tool"
test -x "$root/atelier"
test ! -e "$root/runtime/node"
"$root/atelier" verify-installation
useradd --create-home atelier-rust-synthetic
fixture=/home/atelier-rust-synthetic/package-fixture
install -d -m 700 -o atelier-rust-synthetic -g atelier-rust-synthetic "$fixture"
cat > "$fixture/session.sh" <<'SESSION'
#!/bin/sh
set -eu
umask 077
phase=${ATELIER_NATIVE_PHASE:?}
export XDG_DATA_HOME="$HOME/package-fixture/xdg-data"
export XDG_RUNTIME_DIR="$HOME/package-fixture/xdg-runtime"
mkdir -p "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
printf '%s' synthetic-package-keyring-password | gnome-keyring-daemon --unlock --components=secrets
atelier-kanban-rust launch --data-dir "$HOME/package-fixture/synthetic-data" --port 4389 --no-browser
token=$(cat "$HOME/package-fixture/synthetic-data/local-session")
request() {
  printf 'header = "Authorization: Bearer %s"\nheader = "x-local-request: 1"\n' "$token" |
    curl --config - --noproxy '*' --max-time 10 --max-filesize 1048576 --silent --show-error --fail "$@"
}
if [ "$phase" = initial ]; then
  request --json '{"title":"Synthetic installed package persistence"}' http://127.0.0.1:4389/api/brainstorms > "$HOME/package-fixture/card.json"
else
  request http://127.0.0.1:4389/api/state > "$HOME/package-fixture/state.json"
  jq -e --slurpfile card "$HOME/package-fixture/card.json" '.cards | any(.id == $card[0].id and .title == $card[0].title)' "$HOME/package-fixture/state.json" >/dev/null
fi
request http://127.0.0.1:4389/api/health > "$HOME/package-fixture/health-$phase.json"
unset token
touch "$HOME/package-fixture/started-$phase"
counter=0
while [ ! -f "$HOME/package-fixture/stop-$phase" ]; do
  counter=$((counter + 1)); test "$counter" -lt 1800; sleep 0.1
done
atelier-kanban-rust stop --data-dir "$HOME/package-fixture/synthetic-data" --port 4389
SESSION
chown atelier-rust-synthetic:atelier-rust-synthetic "$fixture/session.sh"
start() {
  phase=$1
  ATELIER_NATIVE_PHASE="$phase" runuser -u atelier-rust-synthetic -- dbus-run-session -- sh "$fixture/session.sh" &
  session_pid=$!
  trap 'touch "$fixture/stop-$phase"; wait "$session_pid" 2>/dev/null || true' EXIT HUP INT TERM
  counter=0
  while [ ! -f "$fixture/started-$phase" ]; do
    counter=$((counter + 1))
    if [ "$counter" -gt 300 ] || ! kill -0 "$session_pid" 2>/dev/null; then echo 'Synthetic installed server failed to start.' >&2; exit 1; fi
    sleep 0.1
  done
}
stop() { touch "$fixture/stop-$phase"; wait "$session_pid"; trap - EXIT HUP INT TERM; }
remove() { case "$format" in deb) dpkg --remove atelier-kanban-rust ;; rpm) rpm --erase atelier-kanban-rust ;; esac; }
install_package() { case "$format" in deb) dpkg --install "$1"/*.deb ;; rpm) rpm --upgrade --oldpackage "$1"/*.rpm ;; esac; }

cp "$root/current.json" "$fixture/original-current.json"
start initial
if remove; then echo 'Removal accepted an active server in another user session.' >&2; exit 1; fi
if install_package /delivery/successor/installers; then echo 'Upgrade accepted an active server in another user session.' >&2; exit 1; fi
cmp "$root/current.json" "$fixture/original-current.json"
stop
database="$fixture/synthetic-data/kanban.sqlite"
test -f "$database"
before=$(sha256sum "$database" | cut -d ' ' -f 1)

# A manager interrupted between reserve and unpack recovers without opening a database.
"$tool" system-package reserve --root "$root" --confirm
"$tool" system-package recover --root "$root" --confirm
test ! -e "$root/.package-maintenance"

# The older v1 marker has its own explicit, preserving recovery path.
ui="$root/versions/$(jq -r '.active.id' "$root/current.json")/ui"
printf '%s\n' atelier-package-maintenance/1 > "$root/.package-maintenance"
chmod 600 "$root/.package-maintenance"
if runuser -u atelier-rust-synthetic -- "$root/atelier" serve --data-dir "$fixture/synthetic-data" --assets "$ui" --port 4389 > "$fixture/maintenance-refusal.log" 2>&1; then echo 'Maintenance marker failed to block launch.' >&2; exit 1; fi
grep -q 'installation ou désinstallation' "$fixture/maintenance-refusal.log"
"$root/atelier" recover-package-maintenance --root "$root" --confirm
test ! -e "$root/.package-maintenance"

# Foreign trust is rejected before a reservation or pointer change.
if "$tool" system-package apply --root "$root" --archive /delivery/successor/next.zip --signature /delivery/foreign-signed-package.json --trust /delivery/foreign-update-public.json --confirm > "$fixture/foreign-trust-refusal.log" 2>&1; then echo 'Foreign trust accepted.' >&2; exit 1; fi
grep -q 'clé du paquet diffère' "$fixture/foreign-trust-refusal.log"
cmp "$root/current.json" "$fixture/original-current.json"

# A signed manifest claiming the next version must fail when its real executable is older.
if install_package /delivery/negative-installers > "$fixture/wrong-runtime-refusal.log" 2>&1; then echo 'Wrong runtime version accepted.' >&2; exit 1; fi
grep -q 'version réellement exécutée' "$fixture/wrong-runtime-refusal.log"
cmp "$root/current.json" "$fixture/original-current.json"
# Prove recovery uses its journal and installed trust, with incoming files unavailable.
mv "$bundle/payload.zip" "$fixture/offline-payload.zip"
mv "$bundle/signed-package.json" "$fixture/offline-signature.json"
"$tool" system-package recover --root "$root" --confirm
mv "$fixture/offline-payload.zip" "$bundle/payload.zip"
mv "$fixture/offline-signature.json" "$bundle/signed-package.json"
test ! -e "$root/.package-maintenance"
test "$(sha256sum "$database" | cut -d ' ' -f 1)" = "$before"
install_package /delivery/installers

# Native manager upgrade selects actual newer code, preserves the old slot, and never touches data.
old=$(jq -r '.active.id' "$root/current.json")
install_package /delivery/successor/installers
"$root/atelier" verify-installation
test "$(jq -r '.previous.id' "$root/current.json")" = "$old"
test "$(sha256sum "$database" | cut -d ' ' -f 1)" = "$before"
start verify
test "$(jq -r '.version' "$fixture/health-verify.json")" = "$(jq -r '.syntheticSuccessorVersion' /delivery/portable-qualification.json)"
stop
before=$(sha256sum "$database" | cut -d ' ' -f 1)

readme="$root/versions/$(jq -r '.active.id' "$root/current.json")/README.txt"
cp "$readme" "$fixture/original-readme"
printf '%s' synthetic-human-edit >> "$readme"
if remove; then echo 'Removal accepted modified signed code.' >&2; exit 1; fi
grep -q synthetic-human-edit "$readme"
cp "$fixture/original-readme" "$readme"
printf '%s' preserve-untracked > "$root/human-untracked-note.txt"
remove
test ! -e "$root/atelier"
test "$(cat "$root/human-untracked-note.txt")" = preserve-untracked
test "$(sha256sum "$database" | cut -d ' ' -f 1)" = "$before"

# Retained human files are moved only by this explicit fixture action, never by the product.
mv "$root" "$fixture/retained-installation"
install_package /delivery/installers
"$root/atelier" verify-installation
remove
printf '{"format":"atelier-rust-native-installer-qualification/1","packageFormat":"%s","nativeArchitecture":"%s","installed":true,"launcherExecuted":true,"otherUserActiveRemovalRefused":true,"otherUserActiveUpgradeRefused":true,"maintenanceRecovered":true,"foreignTrustRefused":true,"wrongRuntimeRefused":true,"offlineRecoveryExecuted":true,"nativeUpgradeExecuted":true,"newRuntimeVersionObserved":true,"oldVersionRetained":true,"conversationPreserved":true,"modifiedCodePreserved":true,"uninstallExecuted":true,"databasePreserved":true,"untrackedFilePreserved":true,"reinstallExecuted":true,"published":false}\n' "$format" "$(uname -m)" > "/results/native-$format-qualification.json"
chmod 644 "/results/native-$format-qualification.json"
