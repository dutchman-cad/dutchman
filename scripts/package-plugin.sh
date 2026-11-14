#!/usr/bin/env bash
# Wrap the wasm32-unknown-unknown build of dutchman-plugin as a SolidCraft plugin package:
#   scripts/package-plugin.sh <dutchman_plugin.wasm> <output folder>
# writes <output folder>/dutchman.wasm, plugin.json and dutchman-plugin.zip.
#
# SolidCraft links only craft:plugin interfaces. `zip` turns on getrandom's JavaScript
# backend for every wasm32 target, so the core module also imports a few wasm-bindgen
# functions (randomness for archive extraction and encryption, which Dutchman never calls).
# Each such import is bound to a stub that traps, and the script fails unless the finished
# component imports craft:plugin interfaces only.
set -euo pipefail
if [ $# -ne 2 ]; then
    echo "usage: $0 <dutchman_plugin.wasm> <output folder>" >&2
    exit 2
fi
core=$1
out=$2
wasm_tools=${WASM_TOOLS:-wasm-tools}
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$out"
stubs=$(mktemp -d "$out/.stubs.XXXXXX")
trap 'rm -rf "$stubs"' EXIT

# Imports come before the first function body; stop printing there.
"$wasm_tools" print "$core" | awk '/^  \(func /{exit} /^  \(import "__wbindgen/{print}' >"$stubs/imports.txt" || true
adapt=()
for module in $(sed -nE 's/^  \(import "(__wbindgen[A-Za-z0-9_]*)".*/\1/p' "$stubs/imports.txt" | sort -u); do
    {
        echo "(module"
        grep -F "(import \"$module\" " "$stubs/imports.txt" | sed -nE \
            's/^  \(import "[^"]+" "([^"]+)" \(func [^(]*(\(;[0-9]+;\) )?\(type [0-9]+\) ?(.*)\)\)$/  (func (export "\1") \3 unreachable)/p'
        echo ")"
    } >"$stubs/$module.wat"
    "$wasm_tools" parse "$stubs/$module.wat" -o "$stubs/$module.wasm"
    adapt+=(--adapt "$module=$stubs/$module.wasm")
done

"$wasm_tools" component new "$core" "${adapt[@]}" -o "$out/dutchman.wasm"
imports=$("$wasm_tools" component wit "$out/dutchman.wasm" | sed -nE 's/^ *import ([^;]+);/\1/p')
echo "$imports"
if echo "$imports" | grep -qv '^craft:plugin/'; then
    echo "The component imports something besides craft:plugin interfaces" >&2
    exit 1
fi
cp "$root/crates/dutchman-plugin/plugin.json" "$out/plugin.json"
rm -f "$out/dutchman-plugin.zip"
(cd "$out" && zip -q -X dutchman-plugin.zip plugin.json dutchman.wasm)
wc -c "$out/dutchman.wasm"
