#!/usr/bin/env bash
# Check discovery and conversion through the packaged Dutchman command.
set -euo pipefail
[[ $# -ge 1 ]] || { echo "Usage: $0 <dutchman-command> [arguments]" >&2; exit 2; }
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$@" convert "$root/crates/dutchman-core/tests/fixtures/electronics-smoke.kicad_pcb" "$work/board.sc" --no-verify
test -s "$work/board.sc"
