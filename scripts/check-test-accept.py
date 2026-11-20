#!/usr/bin/env python3
"""Require the portable accept helper in Rust test code."""
import argparse
from bisect import bisect_right
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
HELPER = 'crates/dutchman/tests/support/net.rs'
# Strings and comments are inert, including raw strings and nested block comments.
LEX = re.compile(r"//[^\n]*|/\*|r(?P<hash>\#*)\".*?\"(?P=hash)|\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])'|\w+|[^\s]", re.S)


def tokens(source):
    end = 0
    result = []
    for match in LEX.finditer(source):
        if match.start() < end:
            continue
        word = match.group()
        if word == '/*':
            depth, end = 1, match.end()
            while depth:
                marker = re.search(r'/\*|\*/', source[end:])
                if marker is None:
                    end = len(source)
                    break
                depth += 1 if marker.group() == '/*' else -1
                end += marker.end()
        elif not word.startswith(('//', '"', 'r"', 'r#', "'")):
            result.append((word, match.start()))
    return result


def test_file(path):
    path = Path(path)
    return 'tests' in path.parts or path.name == 'tests.rs' or path.name.endswith('_tests.rs')


def test_ranges(words):
    ranges = []
    # cfg(test) can guard an inline module, function, or a sequence of attributes.
    for i in range(len(words)):
        if words[i:i + 3] not in (['#', '[', 'cfg'], ['#', '[', 'test']):
            continue
        end = i + 3
        while end < len(words) and words[end] != ']':
            end += 1
        if 'test' not in words[i:end]:
            continue
        start = end
        while end < len(words) and words[end] not in ('{', ';'):
            end += 1
        if end < len(words) and words[end] == '{':
            depth = 1
            end += 1
            while end < len(words) and depth:
                depth += (words[end] == '{') - (words[end] == '}')
                end += 1
        elif end < len(words):
            end += 1
        ranges.append((start, end))
    return ranges


def violations(path, source, external_test=False):
    if str(path) == HELPER:
        return []
    ts = tokens(source)
    words = [word for word, _ in ts]
    ranges = test_ranges(words)
    newlines = [m.start() for m in re.finditer('\n', source)]
    return [bisect_right(newlines, ts[i][1]) + 1 for i in range(len(words))
            if words[i:i + 4] == ['.', 'accept', '(', ')']
            and (external_test or test_file(path) or any(start <= i < end for start, end in ranges))]


def module_directory(path, words, index):
    parent = path.parent if path.name in ('lib.rs', 'main.rs', 'mod.rs') else path.parent / path.stem
    scopes = []
    for i, word in enumerate(words[:index]):
        if word == '{':
            scopes.append(words[i - 1] if i >= 2 and words[i - 2] == 'mod' else None)
        elif word == '}' and scopes:
            scopes.pop()
    return parent.joinpath(*(name for name in scopes if name))


def check(root):
    root = root.resolve()
    files = {path: path.read_text(encoding='utf-8') for path in (root / 'crates').rglob('*.rs')}
    external = {path for path in files if test_file(path.relative_to(root))}
    # Follow out-of-line test modules and their children, even with custom names.
    pending = list(files)
    while pending:
        path = pending.pop()
        ts = tokens(files[path])
        words = [word for word, _ in ts]
        ranges = [(0, len(words))] if path in external else test_ranges(words)
        for start, end in ranges:
            for i in range(start, end - 2):
                if words[i] != 'mod' or words[i + 2] != ';':
                    continue
                name = words[i + 1]
                parent = module_directory(path, words, i)
                candidates = [parent / (name + '.rs'), parent / name / 'mod.rs']
                # Explicit paths are relative to the containing file's directory.
                prefix = files[path][:ts[i][1]]
                override = re.search(r'#\[path\s*=\s*"([^"]+)"\]\s*$', prefix)
                if override:
                    candidates = [(path.parent / override[1]).resolve()]
                for child in candidates:
                    if child in files and child not in external:
                        external.add(child)
                        pending.append(child)
    errors = []
    for path, source in sorted(files.items()):
        relative = path.relative_to(root).as_posix()
        errors.extend(f'{relative}:{line}: Use accept_blocking from {HELPER} for portable mock sockets.'
                      for line in violations(relative, source, path in external))
    return errors


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    errors = check(args.root)
    print('\n'.join(errors) if errors else 'Test socket accepts use the portable helper.')
    sys.exit(bool(errors))
