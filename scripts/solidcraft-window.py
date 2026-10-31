#!/usr/bin/env python3
"""Move the SolidCraft window's newest end to a new minor.

Usage: solidcraft-window.py <new-minor 0.Y> <its newest patch 0.Y.Z> [--root DIR]

Makes the change of "Refresh the SolidCraft window to 0.18-0.Y": the newest-known-minor
constant, the version fixtures in the tests, every docs and
skill sentence naming the newest minor or the window, and changelog.d/solidcraft-window.md.
The oldest end and the design format the writer emits stay as they are; a human decides
those from the release notes.

Fails (exit 1) when an expected place is missing or a stale mention of the previous newest
minor is left behind, so a refactor cannot skip one silently. Running it again for the
same minor changes nothing.
"""
import argparse
import re
import sys
from pathlib import Path

VERIFY = "crates/dutchman-solidcraft/src/verify.rs"
CI = ".github/workflows/ci.yml"  # read only: pushes with the default token cannot edit workflows
CHANGELOG = "changelog.d/solidcraft-window.md"
# Rust files whose tests use SolidCraft version fixtures; each must hold at least one.
RUST_FIXTURES = [
    "crates/dutchman-solidcraft/src/verify.rs",
    "crates/dutchman/src/json.rs",
    "crates/dutchman/src/main.rs",
    "crates/dutchman-gui/tests/ui.rs",
]
# Prose files that state the newest minor or the window; each must hold at least one.
PROSE = [
    "docs/ARCHITECTURE.md", "docs/command-line.md", "docs/desktop.md", "docs/freecad.md",
    "docs/guide.md", "docs/install.md", "docs/json.md", "docs/mcp.md", "docs/releasing.md",
    "docs/testing-your-library.md", "docs/web.md", "skills/dutchman-converting/SKILL.md",
]
# Trees scanned for a stale mention of the previous newest minor.
SCAN_PROSE = ["docs", "skills", "README.md"]
SCAN_RUST = ["crates"]
# A line stating the design format decision is left for a human.
FIXTURE_SPAN = 2
PROTECTED = re.compile(r"solidcraft/\d+|format \d+")

CONSTANT = re.compile(r"(pub const NEWEST_SOLIDCRAFT: \[u64; 2\] = \[0, )(\d+)(\];)")
OLDEST = re.compile(r"pub const WINDOW_OLDEST: \[u64; 3\] = \[0, (\d+), (\d+)\];")
MATRIX = re.compile(r"(?m)^\s+- version: newest[ \t]*$")
# Version-like text in Rust tests: a whole quoted version, or one named by its context.
RUST_VERSION = re.compile(
    r'"0\.(\d+)(\.\d+)?"'
    r"|(?<=SolidCraft )0\.(\d+)(\.\d+)?|(?<=solidcraft-cli )0\.(\d+)(\.\d+)?"
    r"|(?<=written for )0\.(\d+)(\.\d+)?|(?<=fine in )0\.(\d+)(\.\d+)?"
    r"|(?<=found )0\.(\d+)(\.\d+)?"
)
PROSE_VERSION = re.compile(r"(?<![\w.])0\.(\d+)(\.\d+)?(?!\w|\.\d)|\[0, (\d+)\]")


class Fail(Exception):
    pass


def shift_tokens(text, pattern, old, delta):
    """Add delta to the minor of every matched version from the old newest minor up to
    FIXTURE_SPAN above it (tests and prose use the newest minor and the one or two above)."""
    count = 0

    def one(match):
        nonlocal count
        whole = match.group(0)
        groups = [i for i in range(1, pattern.groups + 1, 2) if match.group(i) is not None]
        if not groups:
            return whole
        minor = int(match.group(groups[0]))
        if not old <= minor <= old + FIXTURE_SPAN:
            return whole
        count += 1
        start = match.start(groups[0]) - match.start()
        end = match.end(groups[0]) - match.start()
        return whole[:start] + str(minor + delta) + whole[end:]

    return pattern.sub(one, text), count


def shift_lines(text, pattern, old, delta):
    out, total = [], 0
    for line in text.splitlines(keepends=True):
        if PROTECTED.search(line):
            out.append(line)
            continue
        line, n = shift_tokens(line, pattern, old, delta)
        total += n
        out.append(line)
    return "".join(out), total


def stale_mentions(root, trees, pattern, minor):
    found = []
    for tree in trees:
        base = root / tree
        files = [base] if base.is_file() else sorted(
            p for p in base.rglob("*") if p.is_file() and p.suffix in (".md", ".rs")
        )
        for path in files:
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if PROTECTED.search(line):
                    continue
                for match in pattern.finditer(line):
                    for i in range(1, pattern.groups + 1, 2):
                        if match.group(i) is not None and int(match.group(i)) == minor:
                            found.append(f"{path.relative_to(root)}:{number}: {line.strip()}")
    return found


def changelog(newest, oldest):
    return (
        "### Command line and install\n\n"
        f"- Recognize SolidCraft 0.{newest} as the newest known minor and default target without an\n"
        f"  installed CLI. The supported window is 0.{oldest}–0.{newest}; CI checks the newest 0.{newest} patch\n"
        f"  and the pinned 0.{oldest}.2 release. Designs keep format 23, which 0.{newest} and older targets open.\n"
    )


def read(root, rel):
    path = root / rel
    if not path.is_file():
        raise Fail(f"{rel}: expected file is missing")
    return path.read_text()


def run(root, minor, patch):
    m = re.fullmatch(r"0\.(\d+)", minor)
    p = re.fullmatch(r"0\.(\d+)\.(\d+)", patch)
    if not m or not p or m.group(1) != p.group(1):
        raise Fail(f"expected <0.Y> <0.Y.Z> of the same minor, got {minor} {patch}")
    new = int(m.group(1))

    verify = read(root, VERIFY)
    consts = CONSTANT.findall(verify)
    if len(consts) != 1:
        raise Fail(f"{VERIFY}: expected exactly one NEWEST_SOLIDCRAFT constant, found {len(consts)}")
    old = int(consts[0][1])
    window = OLDEST.findall(verify)
    if len(window) != 1:
        raise Fail(f"{VERIFY}: expected exactly one WINDOW_OLDEST constant, found {len(window)}")
    oldest = int(window[0][0])
    if new < old:
        raise Fail(f"{minor} is older than the newest known minor 0.{old}")
    delta = new - old
    changes = {}

    def current(rel):
        return changes[rel] if rel in changes else read(root, rel)

    def put(rel, text):
        if text != read(root, rel):
            changes[rel] = text

    # Newest-known-minor constant.
    put(VERIFY, CONSTANT.sub(lambda c: f"{c.group(1)}{new}{c.group(3)}", verify))

    # CI roundtrip matrix: the newest entry follows the constant ("newest"), so a new minor
    # needs no edit there; it must stay that way.
    ci = read(root, CI)
    entries = MATRIX.findall(ci)
    if len(entries) != 1:
        raise Fail(f'{CI}: expected exactly one "- version: newest" matrix entry, found {len(entries)}')

    # Version fixtures in tests and the sentences in docs and skills.
    for group, pattern in ((RUST_FIXTURES, RUST_VERSION), (PROSE, PROSE_VERSION)):
        for rel in group:
            text = current(rel)
            if delta:
                text, count = shift_lines(text, pattern, old, delta)
                if not count:
                    raise Fail(f"{rel}: no mention of 0.{old} or newer to update; was it refactored?")
            elif not any(
                int(g) >= new for mt in pattern.finditer(text)
                for g in mt.groups()[0::2] if g is not None
            ):
                raise Fail(f"{rel}: does not mention 0.{new}")
            put(rel, text)

    put_text = changelog(new, oldest)
    (root / CHANGELOG).parent.mkdir(parents=True, exist_ok=True)
    if not (root / CHANGELOG).is_file() or (root / CHANGELOG).read_text() != put_text:
        changes[CHANGELOG] = put_text

    # Nothing may still name the previous newest minor.
    if delta:
        staged = {rel: text for rel, text in changes.items()}
        left = []
        for rel, text in staged.items():
            if rel.endswith((".md", ".rs")) and rel != CHANGELOG:
                pattern = RUST_VERSION if rel.endswith(".rs") else PROSE_VERSION
                for number, line in enumerate(text.splitlines(), 1):
                    if PROTECTED.search(line):
                        continue
                    for mt in pattern.finditer(line):
                        if any(g is not None and int(g) == old for g in mt.groups()[0::2]):
                            left.append(f"{rel}:{number}: {line.strip()}")
        for tree, pattern, kinds in (
            (SCAN_PROSE, PROSE_VERSION, "docs"), (SCAN_RUST, RUST_VERSION, "crates")
        ):
            left += [
                s for s in stale_mentions(root, tree, pattern, old)
                if s.split(":")[0] not in changes
            ]
        if left:
            raise Fail("still names 0.%d after the update:\n  %s" % (old, "\n  ".join(left)))

    for rel, text in changes.items():
        (root / rel).write_text(text)
    return old, new, sorted(changes)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("minor", help="the new newest minor, 0.Y")
    ap.add_argument("patch", help="its newest patch, 0.Y.Z")
    ap.add_argument("--root", default=Path(__file__).resolve().parents[1], type=Path)
    args = ap.parse_args(argv)
    try:
        old, new, changed = run(args.root, args.minor, args.patch)
    except Fail as error:
        print(f"solidcraft-window: {error}", file=sys.stderr)
        return 1
    if changed:
        print(f"Moved the newest SolidCraft minor from 0.{old} to 0.{new}:")
        print("\n".join(f"  {rel}" for rel in changed))
    else:
        print(f"Already at 0.{new}; nothing to change.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
