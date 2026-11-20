#!/usr/bin/env python3
"""Select additional pull request coverage for platform and writer compatibility changes."""
import argparse
from bisect import bisect_right
import difflib
from collections import Counter
import importlib.util
from pathlib import Path, PurePosixPath
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('version_gates', ROOT / 'scripts/check-version-gates.py')
gates = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gates)
SPEC = importlib.util.spec_from_file_location('test_accept', ROOT / 'scripts/check-test-accept.py')
tests = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tests)
PLATFORM = re.compile(r'\bcfg\s*\([^\]]*(?:target_os|windows|unix)\b|\bstd\s*::\s*(?:(?:fs|path|process|net)\b|\{[^}]*\b(?:fs|path|process|net)\b)|\brustix\b')
GATE = re.compile(r'\bVersionGate\b|\.\s*(?:supported|minimum)\s*\(|\btarget\b.*(?:[<>]=?|==|!=)|\b(?:NEWEST_SOLIDCRAFT|WINDOW_OLDEST|OLDEST_SOLIDCRAFT)\b')


def changed_tests(before, after):
    revisions = (before.splitlines(), after.splitlines())
    changed = (set(), set())
    for tag, a, b, c, d in difflib.SequenceMatcher(None, *revisions, autojunk=False).get_opcodes():
        if tag != 'equal':
            changed[0].update(range(a, b))
            changed[1].update(range(c, d))
    for source, lines in zip((before, after), changed):
        ts = tests.tokens(source)
        words = [word for word, _ in ts]
        newlines = [m.start() for m in re.finditer('\n', source)]
        for start, end in tests.test_ranges(words):
            # Include the cfg/test attribute itself, even when it spans several lines.
            start = next((i for i in range(start, -1, -1) if words[i] == '#'), start)
            first = bisect_right(newlines, ts[start][1])
            last = bisect_right(newlines, ts[min(end, len(ts)) - 1][1])
            if lines.intersection(range(first, last + 1)):
                return True
    return False


def required(files):
    """files maps each changed path to its before/after text, including deletions."""
    platform = False
    floor = False
    for path, (before, after) in files.items():
        name = PurePosixPath(path)
        text = before + '\n' + after
        platform |= ('tests' in name.parts or name.name == 'tests.rs'
                     or name.name.endswith('_tests.rs') or name.name.startswith('test_')
                     or name.name in ('Cargo.toml', 'Cargo.lock')
                     or (name.suffix == '.rs' and (changed_tests(before, after) or bool(PLATFORM.search(text))))
                     or bool(re.match(r'^crates/dutchman/src/(?:onshape(?:/|\.rs$)|share_|report_send(?:/|\.rs$)|platform(?:/|\.rs$))', path)))
        floor |= path == 'crates/dutchman-solidcraft/tests/history.rs'
    # Use the same inventory as the version policy check, so aliases and capability tables
    # receive coverage too. Compare keys, excluding line numbers and whitespace differences.
    before = {p: texts[0] for p, texts in files.items() if p.startswith('crates/dutchman-solidcraft/src/') and p.endswith('.rs')}
    after = {p: texts[1] for p, texts in files.items() if p in before}
    floor |= Counter(row[0] for row in gates.inventory(before)) != Counter(row[0] for row in gates.inventory(after))
    for path in before:
        # Also cover changes to capability uses and comparison logic whose release numbers
        # live in another module. Tokenization excludes comments and test-only items.
        old = [' '.join(word for word, _ in group) for group in _statements(before[path])]
        new = [' '.join(word for word, _ in group) for group in _statements(after[path])]
        changed = [line[2:] for line in difflib.ndiff(old, new) if line.startswith(('- ', '+ '))]
        floor |= any(GATE.search(line) for line in changed)
    return bool(platform), bool(floor)


def _statements(source):
    statement = []
    for token, line in gates.tokens(source):
        statement.append((token, line))
        if token in ('{', '}', ';'):
            yield statement
            statement = []
    if statement:
        yield statement


def select(base):
    if subprocess.run(['git', 'cat-file', '-e', f'{base}^{{commit}}'], capture_output=True).returncode:
        return True, True
    paths = subprocess.check_output(['git', 'diff', '--no-renames', '--name-only', '-z', base, 'HEAD']).split(b'\0')
    files = {}
    for raw in filter(None, paths):
        path = raw.decode('utf-8', errors='surrogateescape')
        texts = []
        for revision in (base, 'HEAD'):
            result = subprocess.run(['git', 'show', f'{revision}:{path}'], capture_output=True)
            texts.append(result.stdout.decode('utf-8', errors='replace') if result.returncode == 0 else '')
        files[path] = texts
    return required(files)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True)
    args = parser.parse_args()
    platform, floor = select(args.base)
    print(f'platform={str(platform).lower()}')
    print(f'floor={str(floor).lower()}')
