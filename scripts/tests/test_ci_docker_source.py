"""Prove source recipes and their inputs keep pre-merge Docker coverage."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('docker_source', ROOT / 'scripts/ci-docker-source.py')
SOURCE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SOURCE)


class DockerSourceTests(unittest.TestCase):
    def test_recipes_and_inputs_require_source_build(self):
        for path in ('Dockerfile', 'docker/release.Dockerfile', '.dockerignore',
                     'Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml',
                     'crates/dutchman/build.rs', 'crates/dutchman/assets/report-policy.json',
                     'licences/cadmpeg.txt', 'LICENSE', 'NOTICE', 'README.md',
                     '.github/workflows/ci.yml', 'unknown-build-input'):
            with self.subTest(path=path):
                self.assertTrue(SOURCE.required([path]))

    def test_unrelated_changes_keep_runtime_fast_path(self):
        self.assertFalse(SOURCE.required(['docs/docker.md', 'changelog.d/fix.md', 'skills/guide.md']))

    def test_rust_sources_and_tests_keep_runtime_fast_path(self):
        # Every other Linux job compiles and tests these; the daily Docker workflow rebuilds the
        # source image, so a code-only change does not pay for a cold source image build.
        self.assertFalse(SOURCE.required(['crates/dutchman/src/main.rs',
                                          'crates/dutchman-solidcraft/tests/history.rs',
                                          'crates/dutchman-core/tests/fixtures/cube.f3d']))
        self.assertTrue(SOURCE.required(['crates/dutchman/src/main.rs', 'Cargo.lock']))

    def test_missing_base_fails_open(self):
        with patch.object(SOURCE.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1)):
            self.assertTrue(SOURCE.select('missing'))

    def test_whole_group_diff_includes_dockerfile(self):
        with patch.object(SOURCE.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0)), \
             patch.object(SOURCE.subprocess, 'check_output', return_value=b'docs/guide.md\0Dockerfile\0') as diff:
            self.assertTrue(SOURCE.select('base'))
            diff.assert_called_once_with(['git', 'diff', '--name-only', '-z', 'base', 'HEAD'])

    def test_workflow_builds_source_and_runtime_when_required(self):
        text = (ROOT / '.github/workflows/ci.yml').read_text()
        source = text.split('      - name: Build the source Dockerfile\n', 1)[1].split('        with:', 1)[0]
        condition = next(x.strip()[4:] for x in source.splitlines() if x.strip().startswith('if:'))
        for full, required, expected in [('true', 'true', True), ('true', 'false', False), ('false', '', True)]:
            expression = condition.replace('needs.changes.outputs.full', repr(full)).replace('steps.source.outputs.required', repr(required))
            self.assertEqual(eval(expression.replace('||', 'or'), {'__builtins__': {}}), expected)
        self.assertIn("if: needs.changes.outputs.full == 'true'", text.split('      - name: Build the image from the placement programs', 1)[1])
