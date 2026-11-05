#!/usr/bin/env bash
# Finds the release-candidate build (release-build.yml) whose files a release uses:
#
#   REPO=owner/name scripts/find-release-run.sh <commit> <version> <rc-tag-prefix>
#
# A release is the tag vX.Y.Z on the commit of a candidate tag vX.Y.Z-rc.N, which release-build.yml
# built once. <rc-tag-prefix> is v<version>-rc. (v0.0.0-rc.dryrun for a dry run). Looks for a
# run of release-build.yml, started by pushing a tag with that prefix, that succeeded for exactly
# <commit> and still has every release artifact of <version>; the newest such run wins. No other
# commit is considered: files built from an earlier commit are not the release.
# Prints the run id on stdout; exits 1 (saying which runs it saw) when there is none.
# Needs the gh command, signed in. Used by release.yml.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  sed -n '2,12p' "$0" >&2
  exit 2
fi
sha=$1 version=$2 prefix=$3
repo=${REPO:?set REPO to owner/name}
workflow=release-build.yml
required=(
  "dutchman-$version-x86_64-unknown-linux-gnu.tar.gz"
  "dutchman-$version-aarch64-unknown-linux-gnu.tar.gz"
  "dutchman-$version-x86_64-pc-windows-msvc.zip"
  "dutchman-$version-macos"
)

# Every candidate run for the commit, newest first: "<id> <conclusion> <tag>".
runs=$(gh api --paginate "repos/$repo/actions/workflows/$workflow/runs?head_sha=$sha&event=push&per_page=100" \
  --jq ".workflow_runs[] | select(.head_branch | startswith(\"$prefix\")) | [.created_at, .id, (.conclusion // .status), .head_branch] | @tsv" \
  | sort -r | cut -f2-)

while read -r id state tag; do
  [[ -n ${id:-} && $state == success ]] || continue
  names=$(gh api "repos/$repo/actions/runs/$id/artifacts?per_page=100" \
    --jq '.artifacts[] | select(.expired | not) | .name')
  ok=1
  for name in "${required[@]}"; do
    grep -Fxq -- "$name" <<<"$names" || ok=0
  done
  if [[ $ok == 1 ]]; then
    echo "Candidate $tag, run $id, commit $sha." >&2
    echo "$id"
    exit 0
  fi
  echo "Run $id ($tag) succeeded but its release artifacts have expired or are incomplete." >&2
done <<<"$runs"

{
  echo "No successful $prefix* run of $workflow exists for commit $sha."
  if [[ -n $runs ]]; then
    echo "Candidate runs seen for it (run, state, tag):"
    sed 's/^/  /' <<<"$runs"
  else
    echo "No candidate tag has been built for this commit."
  fi
} >&2
exit 1
