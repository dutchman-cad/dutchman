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
                     'scripts/package-archive.sh', 'unknown-build-input'):
            with self.subTest(path=path):
                self.assertTrue(SOURCE.required([path]))

    def test_unrelated_changes_keep_runtime_fast_path(self):
        self.assertFalse(SOURCE.required(['docs/docker.md', 'changelog.d/fix.md', 'skills/guide.md']))

    def test_ci_configuration_keeps_runtime_fast_path(self):
        self.assertFalse(SOURCE.required(['.github/workflows/ci.yml', '.github/workflows/release-image.yml',
                                          'scripts/ci-docker-source.py', 'scripts/tests/test_ci_result.py']))

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

    @staticmethod
    def fake_git(tag='v1.0.0-rc.3', main='abc123'):
        def run(args, **kwargs):
            if args[:2] == ['git', 'describe']:
                return subprocess.CompletedProcess(args, 0 if tag else 128, stdout=(tag or '') + '\n')
            if args[:2] == ['git', 'merge-base']:
                return subprocess.CompletedProcess(args, 0 if main else 1, stdout=(main or '') + '\n')
            return subprocess.CompletedProcess(args, 0)
        return run

    def diffs(self, by_base):
        return lambda args: by_base[args[4]]

    def test_new_tag_compares_with_the_previous_release_tag(self):
        # rc.4 after rc.3 on a release branch: main is far behind, the previous tag is not.
        with patch.object(SOURCE.subprocess, 'run', side_effect=self.fake_git()), \
             patch.object(SOURCE.subprocess, 'check_output', side_effect=self.diffs(
                 {'v1.0.0-rc.3': b'.github/workflows/ci.yml\0docs/docker.md\0',
                  'abc123': b'Dockerfile\0'})):
            self.assertFalse(SOURCE.select('0' * 40))

    def test_new_tag_on_main_compares_with_main(self):
        with patch.object(SOURCE.subprocess, 'run', side_effect=self.fake_git()), \
             patch.object(SOURCE.subprocess, 'check_output', side_effect=self.diffs(
                 {'v1.0.0-rc.3': b'Cargo.lock\0', 'abc123': b''})):
            self.assertFalse(SOURCE.select('0' * 40))

    def test_new_tag_changing_image_inputs_since_both_builds_the_source(self):
        with patch.object(SOURCE.subprocess, 'run', side_effect=self.fake_git()), \
             patch.object(SOURCE.subprocess, 'check_output', side_effect=self.diffs(
                 {'v1.0.0-rc.3': b'Dockerfile\0', 'abc123': b'Dockerfile\0'})):
            self.assertTrue(SOURCE.select('0' * 40))

    def test_new_ref_without_tag_or_main_fails_open(self):
        with patch.object(SOURCE.subprocess, 'run', side_effect=self.fake_git(tag=None, main=None)):
            self.assertTrue(SOURCE.select('0' * 40))

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
