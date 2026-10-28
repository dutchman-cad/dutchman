#!/usr/bin/env bash
# Packages one target's release build as an archive, with its SHA-256 beside it:
#
#   scripts/package-archive.sh <target> <name> <ref> [binaries-dir]
#
# Takes dutchman, dutchman-mcp and dutchman-gui (plus dutchman-web on Linux) and writes
# dist/<name>.zip (Windows) or dist/<name>.tar.gz, holding a <name> folder with the programs,
# LICENSE, NOTICE, the font licences and the README, its links pinned to <ref> (a tag or a
# commit). An optional binaries-dir supplies prebuilt programs instead of target/<target>/release.
# Prints the archive's file name. The programs are run once to check them, unless this machine
# cannot run the target's (SKIP_RUN=1, for a cross-compiled target).
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  sed -n '2,10p' "$0" >&2
  exit 2
fi
target=$1 name=$2 ref=$3
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

exe=""
[[ $target == *windows* ]] && exe=".exe"
rm -rf "dist/$name"
mkdir -p "dist/$name/licences"
programs=(dutchman dutchman-mcp dutchman-gui)
[[ $target == *linux* ]] && programs+=(dutchman-web)
for program in "${programs[@]}"; do
  cp "${4:-target/$target/release}/$program$exe" "dist/$name/"
done
cp LICENSE NOTICE "dist/$name/"
# The archive holds none of the files the README links to: point them at the ref.
python3 -I scripts/archive-readme.py "$ref" README.md "dist/$name/README.md"
cp crates/dutchman-gui/assets/fonts/*.txt "dist/$name/licences/"

if [[ ${SKIP_RUN:-0} != 1 ]]; then
  "dist/$name/dutchman$exe" --version >&2
  if [[ $target == *linux* ]]; then
    "dist/$name/dutchman-web$exe" --version >&2
  fi
  echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | "dist/$name/dutchman-mcp$exe" | grep -q '"result"'
fi

cd dist
if [[ $target == *windows* ]]; then
  7z a -tzip "$name.zip" "$name" > /dev/null
  archive="$name.zip"
else
  tar czf "$name.tar.gz" "$name"
  archive="$name.tar.gz"
fi
if command -v sha256sum > /dev/null; then
  sha256sum "$archive" > "$archive.sha256"
else
  shasum -a 256 "$archive" > "$archive.sha256"
fi
echo "$archive"
