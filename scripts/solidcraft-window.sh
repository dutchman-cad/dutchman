#!/usr/bin/env bash
# Print the constants and CI entries for the oldest and newest window releases, reading
# each archive's SHA-256 from the release's SHA256SUMS.txt.
# Usage: scripts/solidcraft-window.sh <oldest-x.y.z> <newest-x.y.z>
set -euo pipefail

if [[ $# != 2 ]]; then
    echo "Usage: $0 <oldest-x.y.z> <newest-x.y.z>" >&2
    exit 2
fi
oldest=$1 newest=$2
for version in "$oldest" "$newest"; do
    if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "Use versions in x.y.z form." >&2
        exit 2
    fi
done
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for version in "$oldest" "$newest"; do
    archive="solidcraft-${version}-linux-x86_64.tar.gz"
    url="https://github.com/tangency-labs/solidcraft/releases/download/v${version}/$archive"
    sums="https://github.com/tangency-labs/solidcraft/releases/download/v${version}/SHA256SUMS.txt"
    curl -fsSL --connect-timeout 15 --max-time 120 "$sums" -o "$work/$version.sums"
    # Take the one line naming this archive; reject a missing or ambiguous entry.
    if ! checksum=$(awk -v a="$archive" '{ f = $2; sub(/^\*/, "", f) } f == a && length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ { print tolower($1); n++ } END { if (n != 1) exit 1 }' "$work/$version.sums"); then
        echo "No unique SHA-256 for $archive in $sums" >&2
        exit 1
    fi
    printf '          # %s\n          - version: %s\n            sha256: %s\n' "$url" "$version" "$checksum" >> "$work/matrix"
done
# Print only after both checksum files were read successfully.
printf 'pub const NEWEST_SOLIDCRAFT: [u64; 3] = [%s];\n' "${newest//./, }"
printf 'pub const WINDOW_OLDEST: [u64; 3] = [%s];\n\n' "${oldest//./, }"
cat "$work/matrix"
