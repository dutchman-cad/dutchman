#!/usr/bin/env bash
# Points this checkout's build at cadmpeg's main branch, at commit <sha>, for the nightly
# build: a [patch.crates-io] entry for every cadmpeg crate in Cargo.lock, in the checkout's
# .cargo/config.toml. Never commit what it writes; stable releases use crates.io cadmpeg.
#
#   scripts/use-cadmpeg-main.sh <cadmpeg commit sha>
#
# Fails when cadmpeg's main no longer matches the cadmpeg version Dutchman asks for (the
# patch would then be left unused, and the build would quietly use crates.io).
set -euo pipefail

sha=$1
repo=https://github.com/cadmpeg/cadmpeg
crates=$(sed -n 's/^name = "\(cadmpeg[a-z0-9-]*\)"$/\1/p' Cargo.lock | sort -u)
[ -n "$crates" ] || { echo "no cadmpeg crates in Cargo.lock" >&2; exit 1; }

mkdir -p .cargo
{
    echo
    echo "[patch.crates-io]"
    for crate in $crates; do
        echo "$crate = { git = \"$repo\", rev = \"$sha\" }"
    done
} >> .cargo/config.toml

cargo update $(printf -- '-p %s ' $crates) > /dev/null
for crate in $crates; do
    if ! grep -A2 "^name = \"$crate\"$" Cargo.lock | grep -q "source = \"git+$repo?rev=$sha"; then
        echo "$crate is not taken from cadmpeg main @ $sha: its version there no longer matches" \
             "the one Dutchman asks for; update Dutchman's cadmpeg requirement" >&2
        exit 1
    fi
done
echo "cadmpeg crates from main @ $sha: $(echo $crates | tr '\n' ' ')"
