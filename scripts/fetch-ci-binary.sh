#!/usr/bin/env bash
# Downloads the optimised programs CI built, instead of building them:
#
#   scripts/fetch-ci-binary.sh <main|PR-number|rc|TAG> <target> <dir>
#
# <target> is x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, aarch64-apple-darwin,
# x86_64-apple-darwin or x86_64-pc-windows-msvc.
# Takes a release-profile archive (dutchman, dutchman-mcp, dutchman-gui, LICENSE, NOTICE) and
# unpacks its programs into <dir>. Prints the path of `dutchman`, and on stderr which run, commit
# and date it came from.
#   main: the newest successful nightly build (nightly.yml builds main every day, against
#         cadmpeg's main; its files are kept 7 days).
#   <pr-number>: the full build of the pull request's head commit, which exists only for
#         packaging changes, the `release-build` label, or a run started by hand. Without one,
#         this says how to get it.
#   rc: the newest successful release-candidate build (tags vX.Y.Z-rc.N; files kept 30 days).
#   <tag> (v0.2.8-rc.1): that candidate's build.
# Needs the gh command, signed in, and python3. REPO overrides the repository
# (default dutchman-cad/dutchman).
set -euo pipefail

if [[ $# -ne 3 ]]; then
  sed -n '2,19p' "$0" >&2
  exit 2
fi
ref=$1 target=$2 dir=$3
repo=${REPO:-dutchman-cad/dutchman}
exe=dutchman
[[ $target == *windows* ]] && exe=dutchman.exe

# "<id> <sha> <date> <branch>" of a workflow's runs, newest first, runs that succeeded before the
# others. Takes the workflow file, then more gh run list flags.
runs_of() {
  gh run list -R "$repo" --workflow "$1" "${@:2}" --limit 50 \
    --json databaseId,conclusion,createdAt,headSha,headBranch \
    -q 'sort_by(.createdAt) | reverse | (map(select(.conclusion == "success")) + map(select(.conclusion != "success"))) | .[] | "\(.databaseId) \(.headSha) \(.createdAt) \(.headBranch)"'
}

# Unpacks a release archive (.tar.gz or .zip) into $dir without its top folder.
unpack() {
  local archive=$1
  case $archive in
    *.tar.gz) tar xzf "$archive" -C "$dir" --strip-components=1 ;;
    *.zip) python3 - "$archive" "$dir" <<'PY'
import sys, zipfile, pathlib
archive, dest = sys.argv[1], pathlib.Path(sys.argv[2])
with zipfile.ZipFile(archive) as z:
    for info in z.infolist():
        parts = pathlib.PurePosixPath(info.filename).parts[1:]
        if not parts or info.is_dir():
            continue
        out = dest.joinpath(*parts)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(z.read(info))
PY
    ;;
  esac
}

# Downloads the archive of a build run and unpacks it; false if the run has none left.
# $1 run id, $2 the artifact names that hold the archive (a regex), $3 what the run is, $4 its
# commit, $5 its date.
try_run() {
  local run=$1 pattern=$2 what=$3 sha=$4 date=$5 name tmp archive
  name=$(gh api "repos/$repo/actions/runs/$run/artifacts?per_page=100" \
    --jq ".artifacts[] | select(.expired | not) | .name" \
    | grep -E "$pattern" | head -1 || true)
  [[ -n $name ]] || return 1
  tmp=$(mktemp -d)
  if ! gh run download "$run" -R "$repo" -n "$name" -D "$tmp" > /dev/null 2>&1; then
    rm -rf "$tmp"
    return 1
  fi
  archive=$(find "$tmp" -type f \( -name "dutchman-*-$target.tar.gz" -o -name "dutchman-*-$target.zip" \) | head -1)
  if [[ -z $archive ]]; then
    rm -rf "$tmp"
    return 1
  fi
  mkdir -p "$dir"
  unpack "$archive"
  rm -rf "$tmp"
  if [[ $target != *windows* ]]; then
    chmod +x "$dir/dutchman" "$dir/dutchman-mcp" "$dir/dutchman-gui"
  fi
  echo "$what, run $run, commit ${sha:0:12}, built $date: https://github.com/$repo/actions/runs/$run" >&2
  echo "$(cd "$dir" && pwd)/$exe"
}

# Full release-build archives are artifacts named after the archive (Windows, Linux) or
# macos-archive-<target>; the nightly's are dutchman-nightly-<target>.
release_pattern="^(dutchman-[0-9][^ ]*-$target\\.(tar\\.gz|zip)|macos-archive-$target)\$"
nightly_pattern="^(dutchman-nightly-$target\\.(tar\\.gz|zip)|macos-archive-$target)\$"

case $ref in
  main)
    while read -r run sha date _; do
      try_run "$run" "$nightly_pattern" "Nightly build of main (optimised)" "$sha" "$date" && exit 0
    done < <(runs_of nightly.yml --status success)
    echo "No successful nightly build has an artifact for $target (kept for 7 days)." >&2
    echo "Nightly builds main every day; start one: gh workflow run nightly.yml -R $repo -f force=true" >&2
    exit 1
    ;;
  rc|v*-rc.*)
    while read -r run sha date branch; do
      [[ $branch == v*-rc.* ]] || continue
      [[ $ref == rc || $branch == "$ref" ]] || continue
      try_run "$run" "$release_pattern" "Release candidate $branch" "$sha" "$date" && exit 0
    done < <(runs_of release-build.yml --event push)
    echo "No successful release-candidate build ($ref) has an artifact for $target (kept for 30 days)." >&2
    echo "A candidate is built by pushing a tag vX.Y.Z-rc.N (docs/releasing.md)." >&2
    exit 1
    ;;
  *[!0-9]*|'')
    echo "Expected main, a pull request number, rc or a candidate tag (v0.2.8-rc.1), got: $ref" >&2
    exit 2
    ;;
  *)
    sha=$(gh pr view "$ref" -R "$repo" --json headRefOid -q .headRefOid)
    branch=$(gh pr view "$ref" -R "$repo" --json headRefName -q .headRefName)
    while read -r run runsha date _; do
      try_run "$run" "$release_pattern" "Full build of pull request $ref" "$runsha" "$date" && exit 0
    done < <(runs_of release-build.yml --commit "$sha")
    {
      echo "Pull request $ref has no full build at its head commit ($sha) for $target."
      echo "A pull request gets one only for packaging changes or the release-build label:"
      echo "  gh pr edit $ref -R $repo --add-label release-build"
      echo "or start it for the branch: gh workflow run release-build.yml -R $repo --ref $branch"
      echo "then run this script again when it finishes (about an hour cold, less with warm caches)."
      echo "Or take the nightly build of main: scripts/fetch-ci-binary.sh main $target $dir"
    } >&2
    exit 1
    ;;
esac
