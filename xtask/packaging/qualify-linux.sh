#!/bin/sh
# Only the final workflow invokes this inside a disposable native-architecture container.
set -eu
test "${CI:-}" = true
test "${ATELIER_RUST_FINAL_CAMPAIGN:-}" = 1
test "$(id -u)" = 0
test -f /.dockerenv || test -f /run/.containerenv
format=${1:?deb or rpm required}
case "$format" in
  deb)
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y passwd util-linux gnome-keyring dbus-x11 /delivery/installers/*.deb
    ;;
  rpm)
    dnf install -y shadow-utils util-linux gnome-keyring dbus-daemon /delivery/installers/*.rpm
    ;;
  *) echo 'Unknown package format.' >&2; exit 1 ;;
esac
root=/opt/atelier-kanban-rust
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
export XDG_DATA_HOME="$HOME/package-fixture/xdg-data"
export XDG_RUNTIME_DIR="$HOME/package-fixture/xdg-runtime"
mkdir -p "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
printf '%s' synthetic-package-keyring-password | gnome-keyring-daemon --unlock --components=secrets
atelier-kanban-rust launch --data-dir "$HOME/package-fixture/synthetic-data" --port 4389 --no-browser
touch "$HOME/package-fixture/started"
counter=0
while [ ! -f "$HOME/package-fixture/stop-requested" ]; do
  counter=$((counter + 1))
  test "$counter" -lt 1200
  sleep 0.1
done
atelier-kanban-rust stop --data-dir "$HOME/package-fixture/synthetic-data" --port 4389
SESSION
chown atelier-rust-synthetic:atelier-rust-synthetic "$fixture/session.sh"
runuser -u atelier-rust-synthetic -- dbus-run-session -- sh "$fixture/session.sh" &
session_pid=$!
trap 'touch "$fixture/stop-requested"; wait "$session_pid" 2>/dev/null || true' EXIT HUP INT TERM
counter=0
while [ ! -f "$fixture/started" ]; do
  counter=$((counter + 1))
  if [ "$counter" -gt 300 ] || ! kill -0 "$session_pid" 2>/dev/null; then
    echo 'Synthetic installed application failed to start.' >&2
    exit 1
  fi
  sleep 0.1
done
remove() {
  case "$format" in
    deb) dpkg --remove atelier-kanban-rust ;;
    rpm) rpm --erase atelier-kanban-rust ;;
  esac
}
if remove; then echo 'Native removal accepted an active server in another user session.' >&2; exit 1; fi
test -x "$root/atelier"
touch "$fixture/stop-requested"
wait "$session_pid"
trap - EXIT HUP INT TERM
database="$fixture/synthetic-data/kanban.sqlite"
test -f "$database"
before=$(sha256sum "$database" | cut -d ' ' -f 1)

# An abandoned manager marker blocks launch until the signed, stopped installation is recovered.
ui=$(find "$root/versions" -mindepth 2 -maxdepth 2 -name ui -type d)
test -n "$ui"
printf '%s\n' atelier-package-maintenance/1 > "$root/.package-maintenance"
chmod 600 "$root/.package-maintenance"
if runuser -u atelier-rust-synthetic -- "$root/atelier" serve --data-dir "$fixture/synthetic-data" --assets "$ui" --port 4389 > "$fixture/maintenance-refusal.log" 2>&1; then
  echo 'Maintenance marker did not block launch.' >&2
  exit 1
fi
grep -q 'installation ou désinstallation' "$fixture/maintenance-refusal.log"
"$root/atelier" recover-package-maintenance --root "$root" --confirm
test ! -e "$root/.package-maintenance"
set -- "$root"/.package-maintenance-recovered-*
test "$#" = 1
test "$(cat "$1")" = atelier-package-maintenance/1

# verify-installation refuses a human edit before the package manager deletes owned files.
readme=$(find "$root/versions" -mindepth 2 -maxdepth 2 -name README.txt -type f)
test -n "$readme"
cp -- "$readme" "$fixture/original-readme"
printf '%s' synthetic-human-edit >> "$readme"
if remove; then echo 'Native removal accepted modified signed code.' >&2; exit 1; fi
grep -q synthetic-human-edit "$readme"
cp -- "$fixture/original-readme" "$readme"
printf '%s' preserve-untracked > "$root/human-untracked-note.txt"
remove
test ! -e "$root/atelier"
test "$(cat "$root/human-untracked-note.txt")" = preserve-untracked
test "$(sha256sum "$database" | cut -d ' ' -f 1)" = "$before"

# Preserve the retained root and prove that a fresh native reinstall works.
mv -- "$root" "$fixture/retained-installation"
case "$format" in
  deb) dpkg --install /delivery/installers/*.deb ;;
  rpm) rpm --install /delivery/installers/*.rpm ;;
esac
"$root/atelier" verify-installation
remove
printf '{"format":"atelier-rust-native-installer-qualification/1","packageFormat":"%s","nativeArchitecture":"%s","installed":true,"launcherExecuted":true,"otherUserActiveRemovalRefused":true,"maintenanceRecovered":true,"modifiedCodePreserved":true,"uninstallExecuted":true,"databasePreserved":true,"untrackedFilePreserved":true,"reinstallExecuted":true,"published":false}\n' "$format" "$(uname -m)" > "/results/native-$format-qualification.json"
chmod 644 "/results/native-$format-qualification.json"
