#!/usr/bin/env bash
# Packages the checked Mac archives' binaries for both processors:
#
#   scripts/package-universal.sh <name> <ref> <aarch64-dir> <x86_64-dir>
#
# Writes dist/<name>.tar.gz and its SHA-256 with the per-target archive layout.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  sed -n '2,6p' "$0" >&2
  exit 2
fi
name=$1 ref=$2 arm=$3 intel=$4
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for program in dutchman dutchman-mcp dutchman-gui; do
  lipo -create -output "$work/$program" "$arm/$program" "$intel/$program"
  chmod 755 "$work/$program"
  lipo -info "$work/$program" >&2
done
"$work/dutchman-gui" --version >&2
# The shared packager checks dutchman --version and the MCP server's ping response.
"$root/scripts/package-archive.sh" macos-universal "$name" "$ref" "$work"
