#!/usr/bin/env bash
# Keep the old cleanup as a fallback; current hosted images have ample free disk.
set -euo pipefail
df -h /
available_kib=$(df -Pk / | awk 'NR == 2 { print $4 }')
# Observed pre-cleanup free space: 88 GiB x86_64, 107 GiB ARM64. Leave a conservative
# 64 GiB minimum before starting a debug=0 job; smaller images keep the old fallback.
if (( available_kib >= 64 * 1024 * 1024 )); then
  echo 'At least 64 GiB free; skipping unused toolchain deletion.'
else
  sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /opt/hostedtoolcache/CodeQL
  df -h /
fi
