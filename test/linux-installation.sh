#!/bin/sh
# Run only in a disposable Debian/Ubuntu or Fedora container, as root.
set -eu
format=$1
case "$format" in
  deb)
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y passwd util-linux /workspace/dist/*.deb
    ;;
  rpm)
    dnf install -y shadow-utils util-linux /workspace/dist/*.rpm
    ;;
  *) exit 1 ;;
esac
useradd --create-home atelier-test
command -v secret-tool
runuser -u atelier-test -- /opt/atelier-kanban/runtime/node /workspace/test/linux-installed-check.mjs create
case "$format" in
  deb)
    if dpkg -i /workspace/dist/*.deb; then echo 'Une application active a été remplacée.' >&2; exit 1; fi
    ;;
  rpm)
    if rpm -U --replacepkgs /workspace/dist/*.rpm; then echo 'Une application active a été remplacée.' >&2; exit 1; fi
    ;;
esac
runuser -u atelier-test -- atelier-kanban --stop
case "$format" in
  deb) dpkg -i /workspace/dist/*.deb ;;
  rpm) rpm -U --replacepkgs /workspace/dist/*.rpm ;;
esac
runuser -u atelier-test -- /opt/atelier-kanban/runtime/node /workspace/test/linux-installed-check.mjs verify
runuser -u atelier-test -- atelier-kanban --stop
case "$format" in
  deb) apt-get purge -y atelier-kanban ;;
  rpm) dnf remove -y atelier-kanban ;;
esac
test ! -e /opt/atelier-kanban/runtime/node
test -f /home/atelier-test/.local/share/atelier-kanban/kanban.sqlite
