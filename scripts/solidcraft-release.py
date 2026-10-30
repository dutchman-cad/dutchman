#!/usr/bin/env python3
"""Read release metadata and checksums; network fetching stays in solidcraft-window.sh."""
import json
import re
import sys
from pathlib import Path


def newest(minor, releases):
    if not re.fullmatch(r"[0-9]+\.[0-9]+", minor):
        raise ValueError("Use a newest minor in x.y form")
    candidates = []
    for release in releases:
        if release.get("draft") or release.get("prerelease"):
            continue
        match = re.fullmatch(r"v([0-9]+\.[0-9]+)\.([0-9]+)", release.get("tag_name", ""))
        if match and match[1] == minor:
            candidates.append(int(match[2]))
    if not candidates:
        raise ValueError(f"No published stable SolidCraft {minor} patch found")
    return f"{minor}.{max(candidates)}"


def checksum(archive, contents):
    matches = []
    for line in contents.splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[1].removeprefix("*") == archive:
            if not re.fullmatch(r"[0-9a-fA-F]{64}", fields[0]):
                raise ValueError(f"Malformed SHA-256 for {archive}")
            matches.append(fields[0].lower())
    if len(matches) != 1:
        raise ValueError(f"No unique SHA-256 for {archive}")
    return matches[0]


def main():
    mode, value, *files = sys.argv[1:]
    if mode == "resolve":
        releases = []
        for filename in files:
            page = json.loads(Path(filename).read_text())
            if not isinstance(page, list):
                raise ValueError("GitHub releases response must be a list")
            releases.extend(page)
        print(newest(value, releases))
    elif mode == "checksum":
        print(checksum(value, Path(files[0]).read_text()))
    else:
        raise ValueError("Expected resolve or checksum")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, IndexError) as error:
        sys.exit(str(error))
