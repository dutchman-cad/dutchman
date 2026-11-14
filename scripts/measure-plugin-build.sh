#!/usr/bin/env bash
# Linux measurement: keep rustc and all children inside buildq's capped job scope.
set -euo pipefail
unset RUSTC_WRAPPER
unset RUSTC_WORKSPACE_WRAPPER
plugin_measure_dir=$(mktemp -d "${TMPDIR:-/tmp}/dutchman-plugin-measure.XXXXXX")
export CARGO_TARGET_DIR="$plugin_measure_dir/target"
# Configured [env] variables must not reinstall a wrapper for this measurement.
plugin_build_code=0
/usr/bin/time -v -o "$plugin_measure_dir/build-time.txt" \
    cargo --config 'env.RUSTC_WRAPPER.value=""' --config 'env.RUSTC_WRAPPER.force=true' \
    build --jobs 1 --release --target wasm32-unknown-unknown -p dutchman-plugin || plugin_build_code=$?
# Read the kernel's retained scope peak while this script still keeps the scope alive.
plugin_cgroup_path=$(sed -n 's/^0:://p' /proc/self/cgroup)
if [ -r "/sys/fs/cgroup${plugin_cgroup_path}/memory.peak" ]; then
    cat "/sys/fs/cgroup${plugin_cgroup_path}/memory.peak" > "$plugin_measure_dir/build-memory-peak.txt"
    printf 'peak_build_cgroup_bytes='
    cat "$plugin_measure_dir/build-memory-peak.txt"
fi
rustc --version
rustc --print cfg --target wasm32-unknown-unknown | sed -n '/^panic=/p'
cat "$plugin_measure_dir/build-time.txt"
printf 'Measurement artifacts: %s\n' "$plugin_measure_dir"
if [ "$plugin_build_code" -ne 0 ]; then
    exit "$plugin_build_code"
fi
plugin_core="$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/dutchman_plugin.wasm"
wc -c "$plugin_core"
# The build makes a core module; SolidCraft loads a component with no WASI adapter.
if command -v "${WASM_TOOLS:-wasm-tools}" >/dev/null; then
    scripts/package-plugin.sh "$plugin_core" "$(pwd)/target/dutchman-plugin"
else
    printf 'Install wasm-tools, then: scripts/package-plugin.sh %s <folder>\n' "$plugin_core"
fi
