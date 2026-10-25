#!/usr/bin/env bash
# Verify the two Linux release archives and unpack a multi-platform image context.
set -euo pipefail
artifacts=$(cd "$1" && pwd)
context=$2
mkdir -p "$context/image"
for pair in amd64:x86_64 arm64:aarch64; do
  arch=${pair%:*}
  target=${pair#*:}-unknown-linux-gnu
  shopt -s nullglob
  archives=("$artifacts"/dutchman-*-${target}.tar.gz)
  if [[ ${#archives[@]} -ne 1 ]]; then
    echo "Expected one archive for $target in $artifacts" >&2
    exit 1
  fi
  archive=${archives[0]##*/}
  (cd "$artifacts" && sha256sum -c "$archive.sha256")
  mkdir -p "$context/image/$arch"
  tar xzf "$artifacts/$archive" --strip-components=1 -C "$context/image/$arch"
done
