#!/usr/bin/env python3
"""Print a release body: scripts/release-notes.py X.Y.Z."""

from pathlib import Path
import re
import sys

from downloads import REPO, table

ROOT = Path(__file__).resolve().parent.parent


def body(version: str, root: Path = ROOT) -> str:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Use a release version in X.Y.Z form.")
    path = root / "release-notes" / f"{version}.md"
    if not path.is_file():
        raise ValueError(f"{path}: notes file is missing; write it before releasing.")
    notes = path.read_text(encoding="utf-8").strip()
    sections = re.split(r"^## What's new[ \t]*$", notes, maxsplit=1, flags=re.MULTILINE)
    if len(sections) != 2:
        raise ValueError(f"{path}: add a ## What's new section.")
    intro = sections[0].strip()
    if not intro or re.match(r"[#>*+-]|[0-9]+[.)]\s", intro):
        raise ValueError(f"{path}: add an intro paragraph before ## What's new.")
    highlights = re.split(r"^##\s", sections[1], maxsplit=1, flags=re.MULTILINE)[0]
    if len(re.findall(r"^\s*[-+*]\s+", highlights, flags=re.MULTILINE)) > 7:
        raise ValueError(f"{path}: What's new has more than 7 bullets; shorten it.")
    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    entry = re.search(
        rf"^## ({re.escape(version)} \([0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}\))[ \t]*$",
        changelog,
        flags=re.MULTILINE,
    )
    if entry is None:
        raise ValueError(f"CHANGELOG.md: add a dated entry for {version} before releasing.")
    anchor = re.sub(r"[^\w -]", "", entry.group(1).lower()).replace(" ", "-")
    return (
        f"{notes}\n\n## Download\n\n{table(version)}\n"
        "Or install with Homebrew (`brew install dutchman-cad/tap/dutchman`) or Scoop "
        "(`scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket`, "
        "then `scoop install dutchman-cad/dutchman`).\n\n"
        f"Every change in detail: [CHANGELOG]({REPO}/blob/v{version}/CHANGELOG.md#{anchor}).\n\n"
    )


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        result = body(sys.argv[1])
    except (OSError, ValueError) as error:
        print(f"Release notes: {error}", file=sys.stderr)
        return 1
    sys.stdout.write(result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
