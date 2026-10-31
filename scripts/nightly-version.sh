#!/usr/bin/env bash
# Prints the version a nightly build shows: the workspace version, "-nightly." and the short commit.
set -euo pipefail
commit=${1:?usage: nightly-version.sh <commit>}
version=$(sed -n 's/^version = "\([^"]*\)".*/\1/p' Cargo.toml | head -1 | tr -d '\r')
echo "${version}-nightly.${commit:0:7}"
