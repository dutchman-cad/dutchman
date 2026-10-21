#!/usr/bin/env bash
# Packages Dutchman for macOS as one disk image: Dutchman.app (the desktop app, with the
# `dutchman` and `dutchman-mcp` commands inside it) for Apple silicon and Intel alike, a link to
# Applications, and a note on opening it the first time.
#
#   scripts/macos-dmg.sh <version> <label> <aarch64-dir> <x86_64-dir> <out.dmg>
#
# <version> is the bundle's version (numbers only, 0.1.3); <label> names the image's volume
# (0.1.3, or nightly). Each dir holds one target's dutchman, dutchman-mcp and dutchman-gui.
#
# The app is signed ad hoc (`codesign -s -`), not with a Developer ID, and not notarized:
# macOS asks once before opening it (docs/guide.md, "Opening the app the first time").
set -euo pipefail

if [[ $# -ne 5 ]]; then
  sed -n '2,9p' "$0" >&2
  exit 2
fi
version=$1 label=$2 arm=$3 intel=$4 out=$5
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

stage="$work/Dutchman"
app="$stage/Dutchman.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/licences"

# One binary for both processors.
lipo -create -output "$app/Contents/MacOS/Dutchman" "$arm/dutchman-gui" "$intel/dutchman-gui"
for program in dutchman dutchman-mcp; do
  lipo -create -output "$app/Contents/MacOS/$program" "$arm/$program" "$intel/$program"
done
lipo -info "$app/Contents/MacOS/Dutchman"

cp "$root/crates/dutchman-gui/assets/Dutchman.icns" "$app/Contents/Resources/Dutchman.icns"
cp "$root/LICENSE" "$root/NOTICE" "$app/Contents/Resources/"
cp "$root"/crates/dutchman-gui/assets/fonts/*.txt "$app/Contents/Resources/licences/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Dutchman</string>
  <key>CFBundleDisplayName</key><string>Dutchman</string>
  <key>CFBundleIdentifier</key><string>io.github.dutchman-cad.dutchman</string>
  <key>CFBundleExecutable</key><string>Dutchman</string>
  <key>CFBundleIconFile</key><string>Dutchman</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Apache License 2.0</string>
</dict>
</plist>
PLIST
plutil -lint "$app/Contents/Info.plist"

# Ad hoc: the programs inside first, then the bundle, which seals them.
for program in dutchman dutchman-mcp Dutchman; do
  codesign --force --sign - --timestamp=none "$app/Contents/MacOS/$program"
done
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict --verbose=2 "$app"
"$app/Contents/MacOS/dutchman" --version

ln -s /Applications "$stage/Applications"
cat > "$stage/Read me first.txt" <<'NOTE'
Dutchman converts native CAD designs into SolidCraft designs.

Install: drag Dutchman.app onto Applications.

Opening it the first time
-------------------------
Dutchman is not signed with an Apple Developer ID, so the first time you open it macOS shows
"Dutchman" Not Opened ("Apple could not verify ... is free of malware"). Click Done, then:

1. Open System Settings > Privacy & Security.
2. Scroll down to Security, where it says "Dutchman" was blocked to protect your Mac.
3. Click Open Anyway, enter your password, and click Open Anyway once more.

macOS remembers this; Dutchman opens normally from then on.

Or, in Terminal, once:

    xattr -dr com.apple.quarantine /Applications/Dutchman.app

Command line
------------
The dutchman and dutchman-mcp commands are inside the app. To use them from Terminal:

    sudo ln -sf /Applications/Dutchman.app/Contents/MacOS/dutchman /usr/local/bin/dutchman
    sudo ln -sf /Applications/Dutchman.app/Contents/MacOS/dutchman-mcp /usr/local/bin/dutchman-mcp
NOTE
ref=main
[[ $label != nightly ]] && ref="v$label"
printf '\nGetting started: https://github.com/dutchman-cad/dutchman/blob/%s/docs/guide.md\n' "$ref" \
  >> "$stage/Read me first.txt"

hdiutil create -volname "Dutchman $label" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$out"
hdiutil verify "$out"
