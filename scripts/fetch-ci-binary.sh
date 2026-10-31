#!/usr/bin/env bash
# Downloads the programs CI built for a pull request or for main, instead of building them:
#
#   scripts/fetch-ci-binary.sh <pr-number|main> <target> <dir>
#
# <target> is x86_64-unknown-linux-gnu, aarch64-apple-darwin or x86_64-pc-windows-msvc.
# Takes the artifact dutchman-<target> (dutchman, dutchman-mcp, dutchman-gui, LICENSE, NOTICE)
# from the newest CI run for the pull request's head commit, or for the latest push to main,
# preferring a run that succeeded, and unpacks it into <dir>. Prints the path of `dutchman`.
# These are quick builds (no LTO), not release binaries. Needs the gh command, signed in.
# REPO overrides the repository (default dutchman-cad/dutchman).
set -euo pipefail

if [[ $# -ne 3 ]]; then
  sed -n '2,10p' "$0" >&2
  exit 2
fi
ref=$1 target=$2 dir=$3
repo=${REPO:-dutchman-cad/dutchman}
artifact="dutchman-$target"

if [[ $ref == main ]]; then
  filter=(--branch main --event push)
elif [[ $ref =~ ^[0-9]+$ ]]; then
  sha=$(gh pr view "$ref" -R "$repo" --json headRefOid -q .headRefOid)
  filter=(--commit "$sha")
else
  echo "Expected a pull request number or main, got: $ref" >&2
  exit 2
fi

# Newest first; runs that succeeded before the others. A run that failed elsewhere still has
# the artifact once its binaries job finished.
runs=$(gh run list -R "$repo" --workflow ci.yml "${filter[@]}" --limit 30 \
  --json databaseId,conclusion,createdAt \
  -q 'sort_by(.createdAt) | reverse | (map(select(.conclusion == "success")) + map(select(.conclusion != "success"))) | .[].databaseId')
if [[ -z $runs ]]; then
  echo "No CI run found for $ref." >&2
  exit 1
fi

mkdir -p "$dir"
for run in $runs; do
  if gh run download "$run" -R "$repo" -n "$artifact" -D "$dir" > /dev/null 2>&1; then
    exe=dutchman
    [[ $target == *windows* ]] && exe=dutchman.exe
    if [[ $target != *windows* ]]; then
      chmod +x "$dir/dutchman" "$dir/dutchman-mcp" "$dir/dutchman-gui"
    fi
    echo "CI run $run: https://github.com/$repo/actions/runs/$run" >&2
    echo "$(cd "$dir" && pwd)/$exe"
    exit 0
  fi
done
echo "No CI run for $ref has an artifact named $artifact yet (it is kept for 14 days)." >&2
exit 1
