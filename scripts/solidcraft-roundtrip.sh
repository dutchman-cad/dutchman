#!/usr/bin/env bash
# Converts every fixture with Dutchman and opens the result in SolidCraft's own CLI, failing
# when SolidCraft cannot evaluate the design or a timeline feature errors.
#
#   scripts/solidcraft-roundtrip.sh <solidcraft-cli> <dutchman> <fixture>...
set -euo pipefail

sc=$1 dutchman=$2
shift 2
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

status=0
for input in "$@"; do
    name=$(basename "${input%.*}")
    output="$work/$name.solidcraft"
    "$dutchman" convert "$input" "$output"
    if ! "$sc" eval "$output" > "$work/$name.json"; then
        echo "FAIL $input: SolidCraft could not open the design" >&2
        status=1
        continue
    fi
    if ! python3 -I - "$work/$name.json" <<'PY'
import json, sys
result = json.load(open(sys.argv[1]))
errors = [f"{t['name']}: {t['error']}" for t in result["timeline"] if t.get("error")]
bodies = result.get("bodies", [])
if errors or not bodies:
    print("\n".join(errors) or "no bodies", file=sys.stderr)
    sys.exit(1)
print(", ".join(f"{b['name']} ({b['kernel']['faces']} faces)" for b in bodies))
PY
    then
        echo "FAIL $input" >&2
        status=1
        continue
    fi
    echo "ok   $input"
done
exit $status
