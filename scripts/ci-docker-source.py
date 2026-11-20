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
                or (path.endswith('.rs') and name != 'build.rs')
                or (path.startswith('crates/') and '/tests/' in path))
    return any(not unrelated(path) for path in paths)


def select(base):
    if subprocess.run(['git', 'cat-file', '-e', f'{base}^{{commit}}'], capture_output=True).returncode:
        return True
    diff = subprocess.check_output(['git', 'diff', '--name-only', '-z', base, 'HEAD'])
    return required([path.decode('utf-8', errors='surrogateescape') for path in diff.split(b'\0') if path])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', required=True)
    args = parser.parse_args()
    print(f'required={str(select(args.base)).lower()}')
