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
    build --jobs 1 --release --target wasm32-wasip2 -p dutchman-plugin || plugin_build_code=$?
# Read the kernel's retained scope peak while this script still keeps the scope alive.
plugin_cgroup_path=$(sed -n 's/^0:://p' /proc/self/cgroup)
if [ -r "/sys/fs/cgroup${plugin_cgroup_path}/memory.peak" ]; then
    cat "/sys/fs/cgroup${plugin_cgroup_path}/memory.peak" > "$plugin_measure_dir/build-memory-peak.txt"
    printf 'peak_build_cgroup_bytes='
    cat "$plugin_measure_dir/build-memory-peak.txt"
fi
rustc --version
rustc --print cfg --target wasm32-wasip2 | sed -n '/^panic=/p'
cat "$plugin_measure_dir/build-time.txt"
printf 'Measurement artifacts: %s\n' "$plugin_measure_dir"
if [ "$plugin_build_code" -ne 0 ]; then
    exit "$plugin_build_code"
fi
plugin_package_dir="$(pwd)/target/dutchman-plugin"
mkdir -p "$plugin_package_dir"
cp "$CARGO_TARGET_DIR/wasm32-wasip2/release/dutchman_plugin.wasm" "$plugin_package_dir/dutchman_plugin.wasm"
cp crates/dutchman-plugin/plugin.json "$plugin_package_dir/plugin.json"
printf 'Plugin package: %s\n' "$plugin_package_dir"
wc -c "$CARGO_TARGET_DIR/wasm32-wasip2/release/dutchman_plugin.wasm"
