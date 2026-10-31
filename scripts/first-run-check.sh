#!/usr/bin/env bash
# What a new user does with an unpacked release: run the command, then inspect and convert the
# repository's public fixtures. Used by .github/workflows/first-run.yml.
#
#   scripts/first-run-check.sh <dutchman> <fixtures-dir> <work-dir>
#
# Writes a Markdown summary to $GITHUB_STEP_SUMMARY (or stdout) and exits 1 if any command or
# fixture fails. TIME_CMD, if set (/usr/bin/time -l), wraps each conversion.
set -uo pipefail

if [[ $# -ne 3 ]]; then
  sed -n '2,9p' "$0" >&2
  exit 2
fi
bin=$1 fixtures=$2 work=$3
py=$(command -v python3 || command -v python)
summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
mkdir -p "$work"
failed=0

{
  echo "### Command"
  echo '```'
  "$bin" --version
  echo '```'
} >> "$summary"
"$bin" --version || failed=1
if ! "$bin" formats > "$work/formats.txt"; then
  echo "dutchman formats failed" >&2
  failed=1
fi
echo "- \`dutchman formats\`: $(wc -l < "$work/formats.txt" | tr -d ' ') lines" >> "$summary"

{
  echo
  echo "### Fixtures"
  echo
  echo "| Fixture | inspect | convert | mode | time (ms) | peak memory |"
  echo "|---|---|---|---|---|---|"
} >> "$summary"

fixture_files=(
  generated_unit_cube.f3d
  synthetic_cube.ipt
  solidworks_cube.sldprt
  core_design_product.FCStd
  design_history.FCStd
  product_assembly.FCStd
  freecad-1.1.4-A01/P01a.FCStd
)
for rel in "${fixture_files[@]}"; do
  file="$fixtures/$rel"
  name=$(basename "$rel")
  if [[ ! -f $file ]]; then
    echo "| $name | missing | | | | |" >> "$summary"
    failed=1
    continue
  fi
  inspect=ok
  "$bin" inspect "$file" > "$work/$name.inspect.txt" 2>&1 || { inspect=FAILED; failed=1; }
  out="$work/$name.sc"
  # shellcheck disable=SC2086
  ${TIME_CMD:-} "$bin" convert --json "$file" "$out" > "$work/$name.json" 2> "$work/$name.err"
  code=$?
  # The report is on stdout; /usr/bin/time's figures are on stderr.
  read -r ok mode ms < <("$py" -I - "$work/$name.json" <<'PY'
import json, sys
try:
    r = json.load(open(sys.argv[1]))
    print("ok" if r.get("ok") else "FAILED", r.get("mode") or "-", r.get("elapsed_ms", "-"))
except Exception:
    print("FAILED - -")
PY
)
  if [[ $code -ne 0 || $ok != ok || ! -s $out ]]; then
    ok=FAILED
    failed=1
    echo "::error::converting $name failed"
    cat "$work/$name.json" "$work/$name.err" >&2
  fi
  mem=$(grep -iE 'maximum resident set size|peak memory footprint' "$work/$name.err" | head -1 | awk '/kbytes/ {printf "%.0f MiB", $NF / 1024; next} {printf "%.0f MiB", $1 / 1048576}')
  echo "| $name | $inspect | $ok | $mode | $ms | ${mem:--} |" >> "$summary"
done

exit $failed
