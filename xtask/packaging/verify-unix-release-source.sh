#!/usr/bin/env bash
set -euo pipefail

assert_qualified_unix_release() {
  local run=$1 jobs=$2 release=$3 ref=$4 commit=$5 tag=$6 source_run=$7
  [[ $source_run =~ ^[1-9][0-9]{0,19}$ && $commit =~ ^[0-9a-f]{40}$ && $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || return 2
  jq -e --arg commit "$commit" --arg id "$source_run" '
    (.id | tostring) == $id and .head_sha == $commit and
    .head_repository.full_name == "FluryWaana/atelier" and
    .path == ".github/workflows/rust.yml" and .event == "workflow_dispatch" and
    .status == "completed" and .conclusion == "success"
  ' "$run" >/dev/null || return 1
  jq -e '
    ["selection", "checks", "ui-payload",
     "browser-build (windows-latest)", "browser-build (ubuntu-24.04)", "browser-build (macos-26)",
     "browser (windows-latest)", "browser (ubuntu-24.04)", "browser (macos-26)",
     "native (windows-latest)", "native (ubuntu-24.04)", "native (ubuntu-24.04-arm)", "native (macos-15)", "native (macos-15-intel)",
     "delivery (windows-latest)", "delivery (ubuntu-24.04)", "delivery (ubuntu-24.04-arm)", "delivery (macos-15)", "delivery (macos-15-intel)"] as $required |
    .jobs as $jobs | .total_count == ($jobs | length) and .total_count <= 100 and
    all($required[]; . as $name | [$jobs[] | select(.name == $name)] as $found |
      ($found | length) == 1 and $found[0].status == "completed" and $found[0].conclusion == "success")
  ' "$jobs" >/dev/null || return 1
  jq -e --arg tag "$tag" '.tag_name == $tag and .prerelease == true and .draft == false and .immutable != true' "$release" >/dev/null || return 1
  jq -e --arg commit "$commit" '.object.type == "commit" and .object.sha == $commit' "$ref" >/dev/null || return 1
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  [[ $# == 3 && $1 =~ ^[1-9][0-9]{0,19}$ && $2 =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && $3 =~ ^[0-9a-f]{40}$ ]] || exit 2
  scratch=$(mktemp -d "${RUNNER_TEMP:?}/atelier-unix-release-source-XXXXXXXX")
  gh api "repos/FluryWaana/atelier/actions/runs/$1" > "$scratch/run.json"
  gh api "repos/FluryWaana/atelier/actions/runs/$1/jobs?per_page=100" > "$scratch/jobs.json"
  gh api "repos/FluryWaana/atelier/releases/tags/$2" > "$scratch/release.json"
  gh api "repos/FluryWaana/atelier/git/ref/tags/$2" > "$scratch/ref.json"
  assert_qualified_unix_release "$scratch/run.json" "$scratch/jobs.json" "$scratch/release.json" "$scratch/ref.json" "$3" "$2" "$1"
fi
