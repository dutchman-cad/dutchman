#!/usr/bin/env python3
"""Fail when user-facing text uses British spellings.

Dutchman's text is American English: color, behavior, recognize, organize, normalize, initialize,
center, license, gray, canceled, favor, modeling, millimeter, meter. This scans the user-facing
text: the Markdown files at the top (README, CONTRIBUTING, AGENTS, CHANGELOG) and NOTICE, docs/, the
READMEs in assets/ and corpus/, release-notes/, changelog.d/, skills/, and the string literals in
crates/*/src (comments are not checked). A justified exception (an identifier or JSON key that
is part of a stable interface, a quote of another program's own text, a proper name) goes in
scripts/american-english-allowlist.txt: one line per file and word, with the reason.

    python3 scripts/check-american-english.py            # check
    python3 scripts/check-american-english.py --list     # print every hit, allowed or not
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / "scripts" / "american-english-allowlist.txt"
# Stems match inside a word (recognise, recognised, recognising, normalisation ...); the rest are whole words.
STEMS = ("colour", "behaviour", "recognis", "organis", "normalis", "initialis", "centre", "licence",
         "favour", "modelling", "millimetre", "centimetre", "honour", "artefact", "catalogue",
         "summaris", "optimis(?!t)", "serialis", "neighbour", "labelled", "modelled", "travell")
WORDS = ("grey", "cancelled", "cancelling", "metre", "metres", "programme", "judgement", "fulfil",
         "analyse", "analysed")
PATTERN = re.compile(
    r"(?:" + "|".join(STEMS) + r")|\b(?:" + "|".join(WORDS) + r")\b", re.IGNORECASE
)
TEXT_DIRS = ("docs", "release-notes", "changelog.d", "skills")
TEXT_SUFFIXES = {".md", ".txt", ".toml", ".json", ".yml", ".yaml", ".html", ".sh", ".py", ".rs"}


def string_literals(source: str):
    """Yield (line, start, end) for every string literal in Rust source; comments and chars are skipped."""
    i, n, line = 0, len(source), 1
    while i < n:
        c = source[i]
        if c == "\n":
            line += 1
            i += 1
        elif source.startswith("//", i):
            while i < n and source[i] != "\n":
                i += 1
        elif source.startswith("/*", i):
            depth = 0
            while i < n:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                    if depth == 0:
                        break
                else:
                    line += source[i] == "\n"
                    i += 1
        elif c == "r" and re.match(r'r#*"', source[i:i + 8]) and (i == 0 or not (source[i - 1].isalnum() or source[i - 1] == "_")):
            hashes = len(re.match(r"r(#*)", source[i:]).group(1))
            start = i + 2 + hashes
            end = source.find('"' + "#" * hashes, start)
            end = n if end < 0 else end
            yield line, start, end
            line += source.count("\n", i, end)
            i = end + 1 + hashes
        elif c == '"':
            start = i + 1
            i += 1
            while i < n and source[i] != '"':
                i += 2 if source[i] == "\\" else 1
            yield line, start, i
            line += source.count("\n", start, min(i, n))
            i += 1
        elif c == "'":
            m = re.match(r"'(?:\\.[^']*|[^\\'])'", source[i:i + 12])
            i += len(m.group(0)) if m else 1
        else:
            i += 1


def text_files():
    for name in sorted(p.name for p in ROOT.iterdir() if p.suffix == ".md" or p.name == "NOTICE"):
        yield ROOT / name
    for folder in TEXT_DIRS:
        for path in sorted((ROOT / folder).rglob("*")):
            if path.is_file() and path.suffix in TEXT_SUFFIXES - {".rs"}:
                yield path
    for path in sorted((ROOT / "assets").rglob("*.md")) + sorted((ROOT / "corpus").rglob("*.md")):
        yield path


def spans(text: str):
    """The (start, end, word) of every British spelling in `text`, leaving out an ALL-CAPS token
    (a STEP keyword such as COLOUR_RGB, or a constant)."""
    for m in PATTERN.finditer(text):
        a, b = m.start(), m.end()
        while a > 0 and (text[a - 1].isalnum() or text[a - 1] == "_"):
            a -= 1
        while b < len(text) and (text[b].isalnum() or text[b] == "_"):
            b += 1
        if text[a:b].isupper():
            continue
        yield m.start(), m.end(), m.group(0).lower()


def hits():
    """(relative path, line, word, text, start, end) for every British spelling in user-facing text."""
    found = []
    for path in text_files():
        rel = path.relative_to(ROOT).as_posix()
        for number, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            found += [(rel, number, w, line, s, e) for s, e, w in spans(line)]
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        source = path.read_text(encoding="utf-8")
        for number, a, b in string_literals(source):
            text = source[a:b]
            found += [(rel, number + text.count("\n", 0, s), w, text, s, e) for s, e, w in spans(text)]
    return found


def allowed():
    """{path: [fragment]} from the allowlist: path, fragment, reason separated by tabs. A hit inside an
    occurrence of the fragment is allowed; a fragment starting with `=` must be the whole line or string."""
    result = {}
    for line in ALLOWLIST.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        name, fragment, reason = line.split("\t", 2)
        if not reason.strip():
            raise SystemExit(f"{ALLOWLIST.name}: no reason for {name} {fragment}")
        result.setdefault(name, []).append(fragment)
    return result


def is_allowed(rel, text, start, end, allow, used=None):
    for fragment in allow.get(rel, ()):
        if fragment.startswith("="):
            if text.strip() == fragment[1:]:
                if used is not None:
                    used.add((rel, fragment))
                return True
            continue
        at = text.find(fragment)
        while at >= 0:
            if at <= start and end <= at + len(fragment):
                if used is not None:
                    used.add((rel, fragment))
                return True
            at = text.find(fragment, at + 1)
    return False


def violations(found, allow, used=None):
    return [h for h in found if not is_allowed(h[0], h[3], h[4], h[5], allow, used)]


def first_line(text):
    return text.strip().splitlines()[0][:100] if text.strip() else ""


def main() -> int:
    found = hits()
    allow = allowed()
    if "--list" in sys.argv:
        for rel, number, word, text, *_ in found:
            print(f"{rel}:{number}: {word}: {first_line(text)}")
        return 0
    used = set()
    bad = violations(found, allow, used)
    for rel, number, word, text, *_ in bad:
        print(f"{rel}:{number}: British spelling '{word}': {first_line(text)}")
    for name, fragments in sorted(allow.items()):
        for fragment in fragments:
            if (name, fragment) not in used:
                print(f"note: {name} no longer needs the allowlist entry '{fragment}'")
    if bad:
        print(
            "\nDutchman's text is American English (color, behavior, recognize, center, license, gray,"
            "\ncanceled, modeling, millimeter). A justified exception (a stable identifier or JSON key, a quote"
            "\nof another program's own text, a proper name) goes in scripts/american-english-allowlist.txt."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
