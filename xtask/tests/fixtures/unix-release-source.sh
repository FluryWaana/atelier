#!/usr/bin/env bash
set -euo pipefail
helper=$1
fixture=$2
source "$helper"
commit=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
tag=v0.1.10
for scenario in valid commit fork workflow event failed running missing skipped duplicate windows-only immutable stable moved-tag invalid-id invalid-tag; do
  jq -n --arg commit "$commit" '{id:123,head_sha:$commit,head_repository:{full_name:"FluryWaana/atelier"},path:".github/workflows/rust.yml",event:"workflow_dispatch",status:"completed",conclusion:"success"}' > "$fixture/run.json"
  jq -n '
    (["selection","checks","ui-payload"] +
      (["windows-latest","ubuntu-24.04","macos-26"] | map("browser-build (" + . + ")", "browser (" + . + ")")) +
      (["windows-latest","ubuntu-24.04","ubuntu-24.04-arm","macos-15","macos-15-intel"] | map("native (" + . + ")", "delivery (" + . + ")"))) |
    {total_count:length,jobs:map({name:.,status:"completed",conclusion:"success"})}
  ' > "$fixture/jobs.json"
  printf '%s\n' '{"tag_name":"v0.1.10","prerelease":true,"draft":false,"immutable":false}' > "$fixture/release.json"
  jq -n --arg commit "$commit" '{object:{type:"commit",sha:$commit}}' > "$fixture/ref.json"
  id=123
  requested_tag=$tag
  file=run
  case "$scenario" in
    commit) filter='.head_sha = ("b" * 40)';;
    fork) filter='.head_repository.full_name = "other/atelier"';;
    workflow) filter='.path = ".github/workflows/other.yml"';;
    event) filter='.event = "pull_request"';;
    failed) filter='.conclusion = "failure"';;
    running) filter='.status = "in_progress"';;
    missing) file=jobs; filter='.jobs |= map(select(.name != "native (ubuntu-24.04-arm)")) | .total_count = (.jobs | length)';;
    skipped) file=jobs; filter='(.jobs[] | select(.name == "delivery (macos-15-intel)").conclusion) = "skipped"';;
    duplicate) file=jobs; filter='.jobs += [.jobs[] | select(.name == "browser (macos-26)")] | .total_count = (.jobs | length)';;
    windows-only) file=jobs; filter='.jobs |= map(select(.name | test("selection|checks|ui-payload|windows-latest"))) | .total_count = (.jobs | length)';;
    immutable) file=release; filter='.immutable = true';;
    stable) file=release; filter='.prerelease = false';;
    moved-tag) file=ref; filter='.object.sha = ("b" * 40)';;
    invalid-id) id=../123; filter='.';;
    invalid-tag) requested_tag=main; filter='.';;
    valid) filter='.';;
  esac
  jq "$filter" "$fixture/$file.json" > "$fixture/changed.json"
  mv "$fixture/changed.json" "$fixture/$file.json"
  accepted=0
  if assert_qualified_unix_release "$fixture/run.json" "$fixture/jobs.json" "$fixture/release.json" "$fixture/ref.json" "$commit" "$requested_tag" "$id"; then accepted=1; fi
  if [[ $scenario == valid ]]; then
    [[ $accepted == 1 ]] || exit 1
  else
    [[ $accepted == 0 ]] || { printf 'Source gate accepted %s\n' "$scenario" >&2; exit 1; }
  fi
done
printf '%s\n' 'Unix release provenance contract passed.'
