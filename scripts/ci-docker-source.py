#!/usr/bin/env python3
"""Keep source Docker builds required when their recipes or inputs change."""
import argparse
from pathlib import PurePosixPath
import subprocess


def required(paths):
    # These paths neither configure the image build nor supply its programs/runtime files.
    # Unknown paths fail open: COPY . . can introduce a new build input at any time.
    def unrelated(path):
        return (path.startswith(('docs/', 'changelog.d/', 'release-notes/', 'skills/', 'assets/'))
                or (path.endswith('.md') and PurePosixPath(path).name != 'README.md')
                or path.startswith('.github/ISSUE_TEMPLATE/'))
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
