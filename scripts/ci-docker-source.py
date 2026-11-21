#!/usr/bin/env python3
"""Keep source Docker builds required when their recipes or inputs change."""
import argparse
from pathlib import PurePosixPath
import subprocess


def required(paths):
    # These paths neither configure the image build nor supply its programs/runtime files.
    # Unknown paths fail open: COPY . . can introduce a new build input at any time.
    # Rust sources and tests are compiled and tested by every other Linux job, so a change to
    # them cannot break only the source image; the image's own inputs (recipes, manifests,
    # lock file, build scripts, toolchain, copied assets and licences) still require it.
    # The daily Docker workflow rebuilds the source image from scratch either way.
    def unrelated(path):
        name = PurePosixPath(path).name
        return (path.startswith(('docs/', 'changelog.d/', 'release-notes/', 'skills/', 'assets/'))
                or (path.endswith('.md') and name != 'README.md')
                or path.startswith('.github/ISSUE_TEMPLATE/')
                # CI's own configuration: the daily Docker workflow builds the source image from
                # scratch, and release-image.yml checks the published image before pushing it.
                or path.startswith(('.github/workflows/', 'scripts/tests/'))
                or (path.startswith('scripts/ci-') and path.endswith('.py'))
                or (path.endswith('.rs') and name != 'build.rs')
                or (path.startswith('crates/') and '/tests/' in path))
    return any(not unrelated(path) for path in paths)


NEW_REF = '0' * 40


def git(*args):
    found = subprocess.run(['git', *args], capture_output=True, text=True)
    return found.stdout.strip() if found.returncode == 0 else ''


def tested_bases(base):
    """Commits whose CI already covered the image's inputs.

    A pushed tag (or a new branch) has no previous commit: GitHub sends forty zeros. Compare it
    with the nearest earlier release tag on its line, whose own run checked the image, and with
    where it left main. A tag adding only docs, Rust sources or CI changes beyond either takes
    the fast path, so release candidates build as quickly as main.
    """
    if base != NEW_REF:
        return [base]
    candidates = [git('describe', '--tags', '--abbrev=0', '--match', 'v*', 'HEAD^'),
                  git('merge-base', 'HEAD', 'origin/main')]
    return [found for found in candidates if found]


def changed(base):
    if subprocess.run(['git', 'cat-file', '-e', f'{base}^{{commit}}'], capture_output=True).returncode:
        return None
    diff = subprocess.check_output(['git', 'diff', '--name-only', '-z', base, 'HEAD'])
    return [path.decode('utf-8', errors='surrogateescape') for path in diff.split(b'\0') if path]


def select(base):
    # The source build is skipped when any tested base shows only unrelated changes since then.
    for candidate in tested_bases(base):
        paths = changed(candidate)
        if paths is not None and not required(paths):
            return False
    return True


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True)
    args = parser.parse_args()
    print(f'required={str(select(args.base)).lower()}')
