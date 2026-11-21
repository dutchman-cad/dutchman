#!/usr/bin/env bash
# Wrap the wasm32-unknown-unknown build of dutchman-plugin as a SolidCraft plugin package:
#   scripts/package-plugin.sh <dutchman_plugin.wasm> <output folder> [<version>] [--dialog-conditions]
# writes <output folder>/dutchman.wasm, plugin.json, icons/ and dutchman-plugin.zip. With a version
# (a release's X.Y.Z) the manifest says that version and the zip is dutchman-plugin-<version>.zip,
# with a .sha256 beside it.
#
# SolidCraft links only craft:plugin interfaces. `zip` turns on getrandom's JavaScript
# backend for every wasm32 target, so the core module also imports a few wasm-bindgen
# functions (randomness for archive extraction and encryption, which Dutchman never calls).
# Each such import is bound to a stub that traps, and the script fails unless the finished
# component imports craft:plugin interfaces only.
set -euo pipefail
dialog_conditions=false
if [[ ${!#} == --dialog-conditions ]]; then
    dialog_conditions=true
    set -- "${@:1:$#-1}"
fi
if [ $# -lt 2 ] || [ $# -gt 3 ]; then
    echo "usage: $0 <dutchman_plugin.wasm> <output folder> [<version>] [--dialog-conditions]" >&2
    exit 2
fi
core=$1
out=$2
version=${3:-}
wasm_tools=${WASM_TOOLS:-wasm-tools}
root=$(cd "$(dirname "$0")/.." && pwd)
manifest="$root/crates/dutchman-plugin/plugin.json"
if $dialog_conditions; then
    manifest="$root/crates/dutchman-plugin/plugin-dialog-conditions.json"
fi
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
zip_name=dutchman-plugin.zip
if [ -n "$version" ]; then
    case "$version" in
        *[!0-9.]* | .* | *. | *..*) echo "The version must look like X.Y.Z, not $version" >&2; exit 2 ;;
    esac
    python3 - "$manifest" "$out/plugin.json" "$version" <<'PY'
import json, sys
manifest = json.load(open(sys.argv[1]))
manifest["version"] = sys.argv[3]
with open(sys.argv[2], "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PY
    zip_name=dutchman-plugin-$version.zip
else
    cp "$manifest" "$out/plugin.json"
fi
rm -f "$out/$zip_name"
mkdir -p "$out/icons"
cp "$root/crates/dutchman-plugin/icons/dutchman.svg" "$out/icons/dutchman.svg"
(cd "$out" && zip -q -X "$zip_name" plugin.json dutchman.wasm icons/dutchman.svg)
if [ -n "$version" ]; then
    (cd "$out" && sha256sum "$zip_name" > "$zip_name.sha256")
fi
wc -c "$out/dutchman.wasm"
