#!/usr/bin/env bash
# Converts fixtures with Dutchman and opens each result in SolidCraft's own CLI, failing when
# SolidCraft cannot evaluate the design, a timeline feature errors, or the geometry SolidCraft
# reads differs from what the fixture holds: total volume, face count and bounding box.
# CI runs the newest and oldest releases in the rolling 14-day window; update pins
# with scripts/solidcraft-window.sh (see docs/releasing.md).
#
#   scripts/solidcraft-roundtrip.sh <solidcraft-cli> <dutchman> <fixture>=<expected>...
#
# <expected> is <volume mm³>:<faces>:<min x,y,z>:<max x,y,z>, for example
# 1000:6:0,0,0:10,10,10 for a 10 mm cube at the origin.
set -euo pipefail

sc=$1 dutchman=$2
shift 2
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

status=0
for case in "$@"; do
    input=${case%%=*} expected=${case#*=}
    name=$(basename "${input%.*}")
    output="$work/$name.solidcraft"
    "$dutchman" convert --import-only --solidcraft "$sc" "$input" "$output"
    if ! "$sc" eval "$output" > "$work/$name.json"; then
        echo "FAIL $input: SolidCraft could not open the design" >&2
        status=1
        continue
    fi
    if ! python3 -I - "$work/$name.json" "$expected" <<'PY'
import json, sys

result = json.load(open(sys.argv[1]))
volume, faces, low, high = sys.argv[2].split(":")
want_low = [float(v) for v in low.split(",")]
want_high = [float(v) for v in high.split(",")]

problems = [f"{t['name']}: {t['error']}" for t in result["timeline"] if t.get("error")]
bodies = result.get("bodies", [])
if not bodies:
    problems.append("no bodies")
total = result.get("total", {})
got_volume = total.get("volume_mm3", 0.0)
if abs(got_volume - float(volume)) > 1e-6 * max(1.0, float(volume)):
    problems.append(f"volume {got_volume} mm³, expected {volume}")
if total.get("faces") != int(faces):
    problems.append(f"{total.get('faces')} faces, expected {faces}")
if bodies:
    got_low = [min(b["bbox"]["min"][i] for b in bodies) for i in range(3)]
    got_high = [max(b["bbox"]["max"][i] for b in bodies) for i in range(3)]
    if any(abs(g - w) > 1e-6 for g, w in zip(got_low + got_high, want_low + want_high)):
        problems.append(f"bounding box {got_low}–{got_high}, expected {want_low}–{want_high}")
for entry in result["timeline"]:
    if entry.get("warning"):
        problems.append(f"{entry['name']}: warning: {entry['warning']}")
if problems:
    print("\n".join(problems), file=sys.stderr)
    sys.exit(1)
print(f"{got_volume:g} mm³, {total.get('faces')} faces")
PY
    then
        echo "FAIL $input" >&2
        status=1
        continue
    fi
    echo "ok   $input"
done
exit $status
