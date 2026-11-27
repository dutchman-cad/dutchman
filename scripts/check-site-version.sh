#!/usr/bin/env bash
# Checks that the public site (dutchman-cad/dutchman-cad.github.io, main) points at a release.
# usage: scripts/check-site-version.sh vX.Y.Z
# The site reads its version label and Arch package link from the latest-release API, so the
# only fixed references are the fallbacks and the formats report. Any pinned version in
# index.html that is not this release is drift: it is listed and the script exits 1.
# Needs gh with a token that can read public repositories.
set -euo pipefail
tag=${1:?usage: check-site-version.sh vX.Y.Z}
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "not a final release tag: $tag" >&2; exit 2; }
version=${tag#v}
site=dutchman-cad/dutchman-cad.github.io
page=$(gh api "repos/$site/contents/index.html" -H 'Accept: application/vnd.github.raw')
stale=$(grep -oE "(releases/download/v|blob/v|formats-|dutchman-bin-)[0-9]+\.[0-9]+\.[0-9]+|>v[0-9]+\.[0-9]+\.[0-9]+<" <<<"$page" \
  | grep -vF "$version" | sort -u || true)
if [[ -n "$stale" ]]; then
  echo "The site (index.html on main) still pins another version than $version:" >&2
  echo "$stale" >&2
  echo "Update $site: index.html fallbacks and assets/formats-$version.json." >&2
  exit 1
fi
gh api "repos/$site/contents/assets/formats-$version.json" --jq .name >/dev/null \
  || { echo "assets/formats-$version.json is missing on the site" >&2; exit 1; }
echo "The site points at $version."
