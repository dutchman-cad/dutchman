#!/usr/bin/env bash
# Packages Dutchman Convert for macOS as one disk image: Dutchman Convert.app (the desktop app, with the
# `dutchman` and `dutchman-mcp` commands inside it) for Apple silicon and Intel alike, a link to
# Applications, and a note on opening it the first time.
#
#   scripts/macos-dmg.sh <version> <label> <aarch64-dir> <x86_64-dir> <out.dmg>
#
# <version> is the bundle's version (numbers only, 0.1.3); <label> names the image's volume
# (0.1.3, or nightly). Each dir holds one target's dutchman, dutchman-mcp and dutchman-gui.
#
# The app is signed ad hoc (`codesign -s -`), not with a Developer ID, and not notarized:
# macOS asks once before opening it (docs/install.md, "Opening the app the first time").
set -euo pipefail

if [[ $# -ne 5 ]]; then
  sed -n '2,9p' "$0" >&2
  exit 2
fi
version=$1 label=$2 arm=$3 intel=$4 out=$5
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
mount="" device=""
cleanup() {
  if [[ -n $device ]]; then
    hdiutil detach "$device" || hdiutil detach -force "$device" || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

stage="$work/Dutchman Convert"
app="$stage/Dutchman Convert.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Resources/licences"

# One binary for both processors. The app alone is in MacOS; the commands are helpers, as
# macOS's file system ignores case and `dutchman` there would be the app's executable,
# `Dutchman`.
lipo -create -output "$app/Contents/MacOS/Dutchman" "$arm/dutchman-gui" "$intel/dutchman-gui"
for program in dutchman dutchman-mcp; do
  lipo -create -output "$app/Contents/Helpers/$program" "$arm/$program" "$intel/$program"
done
lipo -info "$app/Contents/MacOS/Dutchman"

cp "$root/crates/dutchman-gui/assets/Dutchman.icns" "$app/Contents/Resources/Dutchman.icns"
cp "$root/LICENSE" "$root/NOTICE" "$app/Contents/Resources/"
cp "$root"/crates/dutchman-gui/assets/fonts/*.txt "$root"/licences/*.txt "$app/Contents/Resources/licences/"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Dutchman Convert</string>
  <key>CFBundleDisplayName</key><string>Dutchman Convert</string>
  <key>LSHasLocalizedDisplayName</key><true/>
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
# Launchpad and the Dock cut names past about 15 characters, so the app is shown as Dutchman.
# macOS shows a localized name only while the bundle's file name matches CFBundleName, so the
# file stays Dutchman Convert.app and the short name is its English localization (the one
# every language falls back to). Finder shows it too.
mkdir -p "$app/Contents/Resources/en.lproj"
cat > "$app/Contents/Resources/en.lproj/InfoPlist.strings" <<'STRINGS'
CFBundleName = "Dutchman";
CFBundleDisplayName = "Dutchman";
STRINGS
plutil -lint "$app/Contents/Resources/en.lproj/InfoPlist.strings"

# Ad hoc: the helpers first, then the bundle, which seals them with the app.
for program in dutchman dutchman-mcp; do
  codesign --force --sign - --timestamp=none "$app/Contents/Helpers/$program"
done
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict --verbose=2 "$app"
"$app/Contents/Helpers/dutchman" --version

ln -s /Applications "$stage/Applications"
cat > "$stage/Read me first.txt" <<'NOTE'
Dutchman Convert, part of Dutchman CAD, converts native CAD designs into SolidCraft designs.

Install: drag Dutchman Convert.app onto Applications. Launchpad, the Dock and Finder show it as
Dutchman.

Opening it the first time
-------------------------
Dutchman Convert is not signed with an Apple Developer ID, so the first time you open it macOS
shows "Dutchman" Not Opened ("Apple could not verify ... is free of malware"). Click Done, then:

1. Open System Settings > Privacy & Security.
2. Scroll down to Security, where it says "Dutchman" was blocked to protect your Mac.
3. Click Open Anyway, enter your password, and click Open Anyway once more.

macOS remembers this; Dutchman Convert opens normally from then on.

Or, in Terminal, once:

    xattr -dr com.apple.quarantine "/Applications/Dutchman Convert.app"

Command line
------------
The dutchman and dutchman-mcp commands are inside the app, in Contents/Helpers. To use them
from Terminal:

    sudo ln -sf "/Applications/Dutchman Convert.app/Contents/Helpers/dutchman" /usr/local/bin/dutchman
    sudo ln -sf "/Applications/Dutchman Convert.app/Contents/Helpers/dutchman-mcp" /usr/local/bin/dutchman-mcp
NOTE
ref=main
[[ $label != nightly ]] && ref="v$label"
printf '\nGetting started: https://github.com/dutchman-cad/dutchman/blob/%s/docs/guide.md\n' "$ref" \
  >> "$stage/Read me first.txt"

# Finder owns its view metadata. Configure a disposable writable image so a missing
# desktop or denied Finder access can fall back to the untouched staging folder.
read -r -d '' finder_script <<'APPLESCRIPT' || true
on run arguments
  set mountPath to item 1 of arguments
  with timeout of 30 seconds
    tell application "Finder"
      set imageFolder to (POSIX file mountPath) as alias
      open imageFolder
      set imageWindow to container window of imageFolder
      set current view of imageWindow to icon view
      -- Hiding the toolbar also hides Finder's sidebar.
      set toolbar visible of imageWindow to false
      set statusbar visible of imageWindow to false
      -- About 440 points of content below the title bar. The background is 480 points tall, so
      -- a shorter title bar on older macOS shows more of it rather than a blank strip.
      set bounds of imageWindow to {100, 100, 740, 576}
      set viewOptions to icon view options of imageWindow
      set arrangement of viewOptions to not arranged
      set icon size of viewOptions to 96
      set text size of viewOptions to 13
      set background picture of viewOptions to (POSIX file (mountPath & "/.background/background.tiff")) as alias
      set position of item "Dutchman Convert.app" of imageFolder to {180, 190}
      set position of item "Applications" of imageFolder to {460, 190}
      set position of item "Read me first.txt" of imageFolder to {320, 320}
      close imageWindow
      open imageFolder
      update imageFolder without registering applications
      delay 2
      close container window of imageFolder
    end tell
  end timeout
end run
APPLESCRIPT

# One Finder pass, bounded outside AppleScript's own timeout too, as Finder's launch and
# permission prompts are not covered by it.
style_window() {
  local finder_pid watchdog_pid status=0
  osascript - "$mount" <<< "$finder_script" &
  finder_pid=$!
  (sleep 45; kill "$finder_pid" 2>/dev/null || true) >/dev/null 2>&1 &
  watchdog_pid=$!
  wait "$finder_pid" || status=$?
  kill "$watchdog_pid" 2>/dev/null || true
  wait "$watchdog_pid" 2>/dev/null || true
  [[ $status == 0 && -s "$mount/.DS_Store" ]]
}

prepare_window() {
  hdiutil create -volname "Dutchman Convert $label" -srcfolder "$stage" -fs HFS+ \
    -format UDRW -ov "$work/window.dmg" || return 1
  # Finder only shows (and so only lays out) a volume mounted where it browses volumes.
  local attached attempt
  attached=$(hdiutil attach -readwrite -noverify -noautoopen "$work/window.dmg") || return 1
  device=$(awk -F '\t' '/Apple_HFS/ { sub(/[ \t]+$/, "", $1); print $1; exit }' <<< "$attached")
  mount=$(awk -F '\t' '/Apple_HFS/ { print $NF; exit }' <<< "$attached")
  [[ -n $device && -d $mount ]] || return 1
  mkdir -p "$mount/.background" || return 1
  sips -s format tiff -s dpiWidth 72 -s dpiHeight 72 \
    "$root/assets/dmg/background.png" --out "$work/background.tiff" || return 1
  sips -s format tiff -s dpiWidth 144 -s dpiHeight 144 \
    "$root/assets/dmg/background@2x.png" --out "$work/background@2x.tiff" || return 1
  tiffutil -cathidpicheck "$work/background.tiff" "$work/background@2x.tiff" \
    -out "$mount/.background/background.tiff" || return 1

  # Finder sometimes stalls on a volume it has only just noticed: give it a moment, and
  # a second and third try.
  for attempt in 1 2 3; do
    sleep 2
    style_window && return 0
    echo "Finder did not lay out the disk image window (try $attempt of 3)." >&2
  done
  return 1
}

if prepare_window; then
  hdiutil detach "$device"
  device=""
  hdiutil convert "$work/window.dmg" -format UDZO -ov -o "$out"
else
  echo "Disk image window could not be styled; packaging the plain installation window." >&2
  # Discard all partial Finder metadata rather than copying it into the fallback.
  if [[ -n $device ]]; then
    hdiutil detach "$device" || hdiutil detach -force "$device"
    device=""
  fi
  hdiutil create -volname "Dutchman Convert $label" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$out"
fi
hdiutil verify "$out"
