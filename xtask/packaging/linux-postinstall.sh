#!/bin/sh
set -eu
bundle=/usr/lib/atelier-kanban-rust
"$bundle/package-tool" system-package apply --root /opt/atelier-kanban-rust --archive "$bundle/payload.zip" --signature "$bundle/signed-package.json" --trust "$bundle/trusted-update-key.json" --confirm
