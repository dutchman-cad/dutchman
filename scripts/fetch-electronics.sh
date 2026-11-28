#!/usr/bin/env bash
# Fetch the checksum-pinned module and its notices into a release staging folder.
set -euo pipefail
[[ $# == 2 ]] || { echo "Usage: $0 <target> <dest-dir>" >&2; exit 2; }
exec python3 "$(dirname "$0")/electronics-lock.py" fetch "$@"
