#!/usr/bin/env bash
# Run on a networked machine after the module release is published.
set -euo pipefail
[[ $# == 1 ]] || { echo "Usage: $0 <version>" >&2; exit 2; }
exec python3 "$(dirname "$0")/electronics-lock.py" update "$1"
