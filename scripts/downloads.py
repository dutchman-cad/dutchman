#!/usr/bin/env python3
"""Writes the table of direct downloads, one row per platform, for a release's archives.

    scripts/downloads.py <version>            print it, for the release notes
    scripts/downloads.py <version> README.md  replace the table between the README's markers
    scripts/downloads.py nightly              print it for the nightly pre-release
    scripts/downloads.py latest README.md     the README's table: version-less files of the latest
                                              published release (releases/latest/download/...)

The README's table uses `latest`, so it never names a release before that release is published;
each release also uploads version-less copies of its files for it. The tests check the README.
"""

import sys

REPO = "https://github.com/dutchman-cad/dutchman"
START = "<!-- downloads -->"
END = "<!-- /downloads -->"

# The first release with the macOS disk image, Dutchman.app for Apple silicon and Intel.
DMG_SINCE = (0, 1, 3)

# (who it is for, target, archive extension)
PLATFORMS = [
    ("Mac command line (any Mac)", "macos-universal", "tar.gz"),
    ("Windows, 64-bit", "x86_64-pc-windows-msvc", "zip"),
    ("Linux, x86_64", "x86_64-unknown-linux-gnu", "tar.gz"),
    ("Linux, ARM 64-bit (aarch64)", "aarch64-unknown-linux-gnu", "tar.gz"),
]


def table(version: str) -> str:
    if version == "nightly":
        base, stem = f"{REPO}/releases/download/nightly", "dutchman-nightly"
    elif version == "latest":
        base, stem = f"{REPO}/releases/latest/download", "dutchman"
    else:
        base, stem = f"{REPO}/releases/download/v{version}", f"dutchman-{version}"
    rows = [
        "| Your computer | Download | Checksum |",
        "|---|---|---|",
    ]
    if version in ("nightly", "latest") or tuple(int(n) for n in version.split(".")[:3]) >= DMG_SINCE:
        name = f"{stem}-macos-universal.dmg"
        url = f"{base}/{name}"
        rows.append(f"| Mac (any Mac): the desktop app | [{name}]({url}) | [SHA-256]({url}.sha256) |")
    for who, target, ext in PLATFORMS:
        name = f"{stem}-{target}.{ext}"
        url = f"{base}/{name}"
        rows.append(f"| {who} | [{name}]({url}) | [SHA-256]({url}.sha256) |")
    return "\n".join(rows) + "\n"


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    block = table(sys.argv[1])
    if len(sys.argv) == 2:
        sys.stdout.write(block)
        return 0
    path = sys.argv[2]
    with open(path, encoding="utf-8") as f:
        text = f.read()
    start, end = text.find(START), text.find(END)
    if start < 0 or end < start:
        print(f"{path}: no {START} … {END} markers", file=sys.stderr)
        return 1
    text = text[: start + len(START)] + "\n" + block + text[end:]
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
