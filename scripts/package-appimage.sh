#!/usr/bin/env bash
# Packages one Linux target's release build as an AppImage with update information:
#
#   scripts/package-appimage.sh <target> <version> [binaries-dir]
#
# Takes dutchman, dutchman-mcp, dutchman-gui and dutchman-web (from target/<target>/release, or
# binaries-dir) and writes, in dist/:
#   dutchman-<version>-<arch>.AppImage         (<arch> is x86_64 or aarch64)
#   dutchman-<version>-<arch>.AppImage.zsync   for delta updates (AppImageUpdate and the like)
#   each with its .sha256
# The AppImage starts the desktop app; packaging/linux/AppRun says how to reach the command
# line programs. The desktop entry and icon are the ones the Arch package uses
# (packaging/arch/). Prints the AppImage's file name.
#
# appimagetool and its runtime are downloaded from their official releases, pinned by version and
# SHA-256, into $APPIMAGE_TOOLS (default target/appimage-tools). Needs squashfs-tools and
# zsyncmake (the zsync package) and file. The programs are run once to check them, unless this machine
# cannot run the target's (SKIP_RUN=1).
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  sed -n '2,15p' "$0" >&2
  exit 2
fi
target=$1 version=$2
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

APPIMAGETOOL_VERSION=1.9.1
RUNTIME_VERSION=20251108
case "$target" in
  x86_64-unknown-linux-gnu)
    arch=x86_64
    tool_sha=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
    runtime_sha=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
  aarch64-unknown-linux-gnu)
    arch=aarch64
    tool_sha=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
    runtime_sha=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
  *) echo "No AppImage for $target" >&2; exit 2 ;;
esac
command -v zsyncmake > /dev/null || { echo "zsyncmake (the zsync package) is needed" >&2; exit 1; }
command -v file > /dev/null || { echo "file (the file package) is needed by appimagetool" >&2; exit 1; }

tools=$(mkdir -p "${APPIMAGE_TOOLS:-target/appimage-tools}" && cd "${APPIMAGE_TOOLS:-target/appimage-tools}" && pwd)
fetch() { # <url> <file> <sha256>
  if [[ ! -f $tools/$2 ]]; then
    curl -fsSL --retry 3 -o "$tools/$2.part" "$1"
    mv "$tools/$2.part" "$tools/$2"
  fi
  echo "$3  $tools/$2" | sha256sum --check --strict - >&2
}
fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$arch.AppImage" \
  "appimagetool-$APPIMAGETOOL_VERSION-$arch.AppImage" "$tool_sha"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-$arch" \
  "runtime-$RUNTIME_VERSION-$arch" "$runtime_sha"
chmod +x "$tools/appimagetool-$APPIMAGETOOL_VERSION-$arch.AppImage"

bins=${3:-target/$target/release}
appdir="dist/appimage-$arch/Dutchman.AppDir"
rm -rf "dist/appimage-$arch"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" \
  "$appdir/usr/share/icons/hicolor/scalable/apps" "$appdir/usr/share/licenses/dutchman/licences"
for program in dutchman dutchman-mcp dutchman-gui dutchman-web; do
  install -m755 "$bins/$program" "$appdir/usr/bin/$program"
done
install -m755 packaging/linux/AppRun "$appdir/AppRun"
install -m644 packaging/arch/dutchman.desktop "$appdir/dutchman.desktop"
install -m644 packaging/arch/dutchman.desktop "$appdir/usr/share/applications/dutchman.desktop"
install -m644 packaging/arch/dutchman.svg "$appdir/dutchman.svg"
install -m644 packaging/arch/dutchman.svg "$appdir/usr/share/icons/hicolor/scalable/apps/dutchman.svg"
ln -s dutchman.svg "$appdir/.DirIcon"
cp LICENSE NOTICE "$appdir/usr/share/licenses/dutchman/"
cp crates/dutchman-gui/assets/fonts/*.txt licences/*.txt "$appdir/usr/share/licenses/dutchman/licences/"

name="dutchman-$version-$arch.AppImage"
mkdir -p dist
rm -f "dist/$name" "dist/$name.zsync"
# Update information: AppImageUpdate fetches only the changed blocks of the newest published
# release, through the .zsync attached beside each AppImage. Extract-and-run needs no FUSE (CI
# containers); appimagetool writes the .zsync into its working directory.
update="gh-releases-zsync|dutchman-cad|dutchman|latest|dutchman-*-$arch.AppImage.zsync"
(cd dist && ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 \
  "$tools/appimagetool-$APPIMAGETOOL_VERSION-$arch.AppImage" --no-appstream \
  --runtime-file "$tools/runtime-$RUNTIME_VERSION-$arch" -u "$update" "../$appdir" "$name" >&2)
[[ -s dist/$name.zsync ]] || { echo "appimagetool wrote no $name.zsync" >&2; exit 1; }

if [[ ${SKIP_RUN:-0} != 1 ]]; then
  export APPIMAGE_EXTRACT_AND_RUN=1
  reported=$("dist/$name" --run dutchman --version)
  echo "$reported" >&2
  grep -qF -- "$version" <<<"$reported" || { echo "The AppImage's dutchman reports another version than $version." >&2; exit 1; }
  # Started under the name dutchman (a link), the AppImage is the command line program.
  mkdir -p "dist/appimage-$arch/links"
  ln -sf "$root/dist/$name" "dist/appimage-$arch/links/dutchman"
  timeout 60 "dist/appimage-$arch/links/dutchman" --version > /dev/null
  echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | "dist/$name" --run dutchman-mcp | grep -q '"result"'
fi
rm -rf "dist/appimage-$arch"

cd dist
for file in "$name" "$name.zsync"; do
  sha256sum "$file" > "$file.sha256"
done
echo "$name"
