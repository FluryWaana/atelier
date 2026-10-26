#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Darwin && ${CI:-} == true && ${ATELIER_RUST_FINAL_CAMPAIGN:-} == 1 && -n ${RUNNER_TEMP:-} && -n ${GITHUB_PATH:-} ]] || exit 2
# Homebrew's Git uses the system curl; this isolated test build must use an explicit OpenSSL curl.
brew install curl
curl_prefix=$(brew --prefix curl)
curl_version=$("$curl_prefix/bin/curl" --version)
[[ $curl_version == *OpenSSL* ]]
printf '%s\n' "$curl_version"
fixture=$(mktemp -d "$RUNNER_TEMP/atelier-git-openssl-XXXXXXXX")
version=2.56.0
archive="$fixture/git.tar.xz"
/usr/bin/curl --disable --fail --location --proto '=https' --tlsv1.2 --output "$archive" "https://mirrors.edge.kernel.org/pub/software/scm/git/git-$version.tar.xz"
printf '%s  %s\n' 26c56c296b38c0695b26fa95f475f1d01704d2d38e73465ca30b0b2f5dc789d3 "$archive" | /usr/bin/shasum -a 256 -c -
tar -xf "$archive" -C "$fixture"
prefix="$fixture/installed"
make -C "$fixture/git-$version" -j2 prefix="$prefix" CURL_CONFIG="$curl_prefix/bin/curl-config" NO_OPENSSL=1 APPLE_COMMON_CRYPTO=1 NO_TCLTK=1 NO_GETTEXT=1 NO_PERL=1 NO_PYTHON=1 NO_RUST=1 all
make -C "$fixture/git-$version" prefix="$prefix" CURL_CONFIG="$curl_prefix/bin/curl-config" NO_OPENSSL=1 APPLE_COMMON_CRYPTO=1 NO_TCLTK=1 NO_GETTEXT=1 NO_PERL=1 NO_PYTHON=1 NO_RUST=1 install
"$prefix/bin/git" version --build-options
links=$(otool -L "$prefix/libexec/git-core/git-remote-http")
printf '%s\n' "$links"
grep -F "$curl_prefix/lib/libcurl" <<< "$links"
printf '%s\n' "$prefix/bin" >> "$GITHUB_PATH"
