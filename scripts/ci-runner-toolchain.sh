#!/usr/bin/env bash
# Check, never install: a self-hosted runner keeps its own stable toolchain, and a job that
# installs or updates it can pull files from under another runner's build.
# Usage: ci-runner-toolchain.sh [clippy] [rustfmt] [target:<triple>]...
# Always checks rustc, cargo, rustdoc and rust-lld (the default link); each argument adds one more.
set -u
missing=""
for tool in rustc cargo rustdoc; do
  rustup which "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
lld="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/rust-lld"
[ -x "$lld" ] || missing="$missing rust-lld"
for need in "$@"; do
  case "$need" in
    clippy) cargo clippy --version >/dev/null 2>&1 || missing="$missing clippy" ;;
    rustfmt) cargo fmt --version >/dev/null 2>&1 || missing="$missing rustfmt" ;;
    target:*)
      t="${need#target:}"
      rustup target list --installed 2>/dev/null | grep -qx "$t" || missing="$missing target-$t" ;;
    *) echo "unknown requirement: $need" >&2; exit 2 ;;
  esac
done
if [ -n "$missing" ]; then
  echo "::error title=Runner toolchain::${RUNNER_NAME:-this runner} lacks$missing; install the default profile of stable on the runner (rustup toolchain install stable --profile default)."
  exit 1
fi
rustc -V
