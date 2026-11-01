#!/usr/bin/env bash
set -euo pipefail
phase=${1:?release phase required}
target=${2:?native target required}
case "$target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) ;; *) exit 2;; esac
tool() { "$GITHUB_WORKSPACE/target/debug/xtask" "$@"; }
assets="$GITHUB_WORKSPACE/.artifacts/unix-release-assets"
work="$GITHUB_WORKSPACE/.artifacts/unix-release-work"
source="$GITHUB_WORKSPACE/.artifacts/qualified-source"
version=${RELEASE_VERSION:-$(awk -F '"' '/^version = "/ {print $2; exit}' "$GITHUB_WORKSPACE/Cargo.toml")}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ && ${RELEASE_TAG:?} == "v$version" ]] || exit 2
archive="$assets/atelier-$version-$target.zip"
trust="$work/atelier-update-public.json"
case "$phase" in
  prepare)
    for path in Cargo.toml Cargo.lock crates xtask/src/installers.rs xtask/packaging; do
      [[ $(git -C "$GITHUB_WORKSPACE" rev-parse "HEAD:$path") == $(git -C "$GITHUB_WORKSPACE/release-control" rev-parse "HEAD:$path") ]] || { echo 'Release control and published product sources differ.' >&2; exit 1; }
    done
    printf 'RELEASE_VERSION=%s\n' "$version" >> "${GITHUB_ENV:?}"
    commit=$(git -C "$GITHUB_WORKSPACE" rev-parse HEAD)
    jq -e --arg commit "$commit" --arg version "$version" --arg target "$target" '
      .commit == $commit and .version == $version and .target == $target and
      .portable.runtimeVersionObserved == true and .portable.signatureVerified == true and .portable.stopped == true
    ' "$source/portable-qualification.json" >/dev/null
    mkdir "$assets" "$work"
    mkdir "$work/successor"
    original="$source/atelier-kanban-$version-$target.zip"
    tool verify-signed-package "$original" "$source/signed-package.json" "$source/synthetic-update-public.json" "$target" 0.0.0
    cp "$original" "$archive"
    cp "$source/successor/next.zip" "$work/successor/next.zip"
    cp "$source/portable-qualification.json" "$work/portable-qualification.json"
    tool release-update-trust "$trust"
    case "$target" in
      *linux*)
        tool prepare-release-negative "$original" "$source/signed-package.json" "$source/synthetic-update-public.json" "$work/wrong-runtime.zip"
        cp "$source/foreign-signed-package.json" "$work/foreign-signed-package.json"
        cp "$source/foreign-update-public.json" "$work/foreign-update-public.json"
        ;;
    esac
    ;;
  sign)
    tool sign-release-package "$archive" "$target" "$archive.signed.json"
    tool sign-release-package "$work/successor/next.zip" "$target" "$work/successor/next.json"
    case "$target" in *linux*) tool sign-release-package "$work/wrong-runtime.zip" "$target" "$work/wrong-runtime.json";; esac
    ;;
  build)
    tool verify-signed-package "$archive" "$archive.signed.json" "$trust" "$target" 0.0.0
    case "$target" in
      *linux*)
        tool build-installers "$archive" "$archive.signed.json" "$trust" "$work/installers"
        tool build-installers "$work/successor/next.zip" "$work/successor/next.json" "$trust" "$work/successor/installers"
        tool build-installers "$work/wrong-runtime.zip" "$work/wrong-runtime.json" "$trust" "$work/negative-installers"
        ;;
    esac
    ;;
  portable)
    tool qualify-signed-portable "$archive" "$archive.signed.json" "$trust" "$work/distribution-portable-qualification.json"
    ;;
  checksums)
    case "$target" in
      *linux*)
        cp "$work/installers/"*.deb "$work/installers/"*.rpm "$assets/"
        [[ $(find "$assets" -maxdepth 1 -type f | wc -l | tr -d ' ') == 4 ]] || exit 1
        ;;
      *apple*) [[ $(find "$assets" -maxdepth 1 -type f | wc -l | tr -d ' ') == 2 ]] || exit 1;;
    esac
    cd "$assets"
    : > "$work/SHA256SUMS-$target"
    for file in *; do
      [[ -f $file && ! -L $file && $file =~ ^[a-zA-Z0-9._-]+$ ]] || exit 1
      case "$target" in *apple*) hash=$(shasum -a 256 "$file" | cut -d ' ' -f 1);; *) hash=$(sha256sum "$file" | cut -d ' ' -f 1);; esac
      printf '%s  %s\n' "$hash" "$file" >> "$work/SHA256SUMS-$target"
    done
    cp "$work/SHA256SUMS-$target" .
    ;;
  sign-checksums) tool sign-release-checksums "$assets/SHA256SUMS-$target" "$assets/SHA256SUMS-$target.sig";;
  verify)
    tool verify-release-checksums "$assets/SHA256SUMS-$target" "$assets/SHA256SUMS-$target.sig"
    cd "$assets"
    case "$target" in *apple*) shasum -a 256 -c "SHA256SUMS-$target";; *) sha256sum --strict --check "SHA256SUMS-$target";; esac
    ;;
  *) exit 2;;
esac
