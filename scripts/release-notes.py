#!/usr/bin/env python3
"""Print a release body: scripts/release-notes.py X.Y.Z."""

from pathlib import Path
import re
import sys

from downloads import REPO, table

ROOT = Path(__file__).resolve().parent.parent

def image_url(version: str, name: str) -> str:
    """Where a release's image is served: the file committed for that release, at its tag."""
    return (f"https://raw.githubusercontent.com/dutchman-cad/dutchman/v{version}"
            f"/docs/images/releases/{version}/{name}")


def check_images(version: str, notes: str, path: Path, root: Path) -> None:
    """Images are optional (kept for milestones); each one shown needs alt text and its file."""
    prefix = re.escape(image_url(version, ""))
    for alt, name in re.findall(rf"!\[([^\]]*)\]\({prefix}([^)\s]+)\)", notes):
        if not alt.strip():
            raise ValueError(f"{path}: describe the image {name} in its alt text.")
        if not (root / "docs" / "images" / "releases" / version / name).is_file():
            raise ValueError(f"docs/images/releases/{version}/{name}: commit the image the notes show.")


def check_placeholders(notes: str, path: Path) -> None:
    """Template text left in the notes would be published as written."""
    if "(program)" in notes:
        raise ValueError(f"{path}: replace the placeholder (program) with real wording.")
    if "<!--" in notes:
        raise ValueError(f"{path}: remove the HTML comment left in the notes.")
    prose = re.sub(r"```.*?```|`[^`\n]*`", "", notes, flags=re.DOTALL)
    for marker in (r"X\.Y\.Z", r"\bTBD\b", r"\bTODO\b"):
        found = re.search(marker, prose)
        if found:
            raise ValueError(f"{path}: finish the template text {found.group(0)!r} before releasing.")


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
    check_placeholders(notes, path)
    check_images(version, notes, path, root)
    # The CHANGELOG entry is folded in after the tag, so it is not there yet when a release is cut:
    # link the entry when it exists, else the CHANGELOG on main.
    changelog_path = root / "CHANGELOG.md"
    changelog = changelog_path.read_text(encoding="utf-8") if changelog_path.is_file() else ""
    entry = re.search(
        rf"^## ({re.escape(version)} \([0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}\))[ \t]*$",
        changelog,
        flags=re.MULTILINE,
    )
    if entry is None:
        link = f"the {version} entry in the [CHANGELOG]({REPO}/blob/main/CHANGELOG.md)"
    else:
        anchor = re.sub(r"[^\w -]", "", entry.group(1).lower()).replace(" ", "-")
        link = f"[CHANGELOG]({REPO}/blob/v{version}/CHANGELOG.md#{anchor})"
    return (
        f"{notes}\n\n"
        "Made for the current SolidCraft (any release from the last two weeks); older "
        "versions get simplified features, or pass `--solidcraft-version`.\n\n"
        f"## Download\n\n{table(version)}\n"
        "Or install with Homebrew (`brew install dutchman-cad/tap/dutchman`) or Scoop "
        "(`scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket`, "
        "then `scoop install dutchman-cad/dutchman`).\n\n"
        f"Every change in detail: {link}.\n\n"
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
