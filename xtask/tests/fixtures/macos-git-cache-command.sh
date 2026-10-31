#!/usr/bin/env bash
set -euo pipefail
case "${0##*/}" in
  uname) printf 'Darwin\n' ;;
  brew) printf '%s/curl\n' "$GIT_CACHE_FIXTURE" ;;
  curl) printf '%s\n' "$CURL_VERSION" ;;
  git)
    if [[ $1 == --version ]]; then
      printf '%s\n' "$GIT_VERSION"
    else
      printf 'synthetic cached Git\n'
    fi
    ;;
  otool) printf '%s\n' "$GIT_LINKS" ;;
  *) exit 99 ;;
esac
