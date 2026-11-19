#!/usr/bin/env bash
# Points the Arch package at a release and rewrites .SRCINFO (no makepkg needed):
#
#   packaging/arch/update.sh <version> [dir-with-.sha256-files]
#
# Sets pkgver, resets pkgrel to 1 and takes the two Linux archives' SHA-256 from their
# .sha256 files: downloaded from the release v<version> by default, or read from the
# given directory (the release workflow does this while the release is still a draft).
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 || ! $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  sed -n '2,8p' "$0" >&2
  exit 2
fi
version=$1 dir=${2:-}
cd "$(dirname "$0")"
base=https://github.com/dutchman-cad/dutchman/releases/download/v$version

sum() { # <arch>: the archive's SHA-256
  local file="dutchman-$version-$1-unknown-linux-gnu.tar.gz.sha256" line
  if [[ -n $dir ]]; then line=$(cat "$dir/$file"); else line=$(curl -fsSL "$base/$file"); fi
  line=${line%% *}
  [[ $line =~ ^[0-9a-f]{64}$ ]] || { echo "No SHA-256 in $file" >&2; exit 1; }
  echo "$line"
}
x86=$(sum x86_64)
arm=$(sum aarch64)

sed -i -E \
  -e "s/^pkgver=.*/pkgver=$version/" \
  -e "s/^pkgrel=.*/pkgrel=1/" \
  -e "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$x86')/" \
  -e "s/^sha256sums_aarch64=.*/sha256sums_aarch64=('$arm')/" \
  PKGBUILD

# .SRCINFO, in the documented format, from the same facts the PKGBUILD states.
pkgdesc=$(sed -n "s/^pkgdesc='\(.*\)'$/\1/p" PKGBUILD)
url=https://github.com/dutchman-cad/dutchman
file_sums=$(sed -n "/^sha256sums=(/,/)/p" PKGBUILD | grep -oE '[0-9a-f]{64}')
f1=$(sed -n 1p <<<"$file_sums") f2=$(sed -n 2p <<<"$file_sums")
{
  echo "pkgbase = dutchman-bin"
  echo "	pkgdesc = $pkgdesc"
  echo "	pkgver = $version"
  echo "	pkgrel = 1"
  echo "	url = $url"
  echo "	arch = x86_64"
  echo "	arch = aarch64"
  echo "	license = Apache-2.0"
  for d in glibc gcc-libs fontconfig libxkbcommon libx11 libxcursor libxi libxrandr wayland \
           vulkan-icd-loader mesa hicolor-icon-theme; do
    echo "	depends = $d"
  done
  echo "	optdepends = vulkan-driver: GPU drawing in the desktop app"
  echo "	provides = dutchman"
  echo "	conflicts = dutchman"
  echo "	options = !strip"
  echo "	options = !debug"
  echo "	source = dutchman.desktop"
  echo "	source = dutchman.svg"
  echo "	sha256sums = $f1"
  echo "	sha256sums = $f2"
  echo "	source_x86_64 = dutchman-$version-x86_64-unknown-linux-gnu.tar.gz::$base/dutchman-$version-x86_64-unknown-linux-gnu.tar.gz"
  echo "	sha256sums_x86_64 = $x86"
  echo "	source_aarch64 = dutchman-$version-aarch64-unknown-linux-gnu.tar.gz::$base/dutchman-$version-aarch64-unknown-linux-gnu.tar.gz"
  echo "	sha256sums_aarch64 = $arm"
  echo
  echo "pkgname = dutchman-bin"
} > .SRCINFO
echo "packaging/arch is at $version"
