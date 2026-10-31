#!/usr/bin/env bash
# Points this checkout's build at cadmpeg's main branch, at commit <sha>, for the nightly
# build: every cadmpeg dependency the workspace Cargo.toml pins (from cadmpeg's repository
# or a fork of it) is pointed at <repo> (default: cadmpeg's own repository) and <sha>, and
# Cargo.lock follows. Never commit what it changes; stable releases keep the pinned commit.
#
#   scripts/use-cadmpeg-main.sh <cadmpeg commit sha> [<repo url>]
set -euo pipefail

sha=$1
repo=${2:-https://github.com/cadmpeg/cadmpeg}
[[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo "not a full commit sha: $sha" >&2; exit 1; }
pinned='git = "https://github\.com/[A-Za-z0-9_.-]+/cadmpeg", rev = "[0-9a-f]+"'
grep -Eq "$pinned" Cargo.toml || {
    echo "Cargo.toml pins no cadmpeg commit to replace" >&2
    exit 1
}
sed -i.bak -E "s|$pinned|git = \"$repo\", rev = \"$sha\"|" Cargo.toml
rm -f Cargo.toml.bak

crates=$(sed -n 's/^name = "\(cadmpeg[a-z0-9-]*\)"$/\1/p' Cargo.lock | sort -u)
[ -n "$crates" ] || { echo "no cadmpeg crates in Cargo.lock" >&2; exit 1; }
cargo update $(printf -- '-p %s ' $crates) > /dev/null
for crate in $(sed -n 's/^name = "\(cadmpeg[a-z0-9-]*\)"$/\1/p' Cargo.lock | sort -u); do
    if ! grep -A2 "^name = \"$crate\"$" Cargo.lock | grep -q "source = \"git+$repo?rev=$sha"; then
        echo "$crate is not taken from cadmpeg main @ $sha" >&2
        exit 1
    fi
done
echo "cadmpeg crates from main @ $sha: $(echo $crates | tr '\n' ' ')"
