"""Select platform-sensitive PRs without expanding the merge queue."""
import importlib.util
from pathlib import Path
import re
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('platform_tests', ROOT / 'scripts/ci-platform-tests.py')
SELECTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SELECTOR)


class PlatformTests(unittest.TestCase):
    def test_test_layouts_and_manifests(self):
        for path in ('crates/a/tests/http.rs', 'crates/a/src/tests.rs', 'crates/a/src/http_tests.rs',
                     'scripts/tests/test_guard.py', 'Cargo.toml', 'Cargo.lock', 'crates/a/Cargo.toml'):
            with self.subTest(path=path):
                self.assertEqual(SELECTOR.required({path: ('', '')}), (True, False))

    def test_platform_lines_in_either_revision(self):
        for code in ('#[cfg(target_os = "macos")]', '#[cfg(windows)]', '#[cfg(unix)]',
                     '#[cfg(any(windows, unix))]', 'use std::fs;', 'std::process::Command',
                     'use std :: net::TcpStream;', 'use std::{fs, path::Path};',
                     'std::path::PathBuf', '#[cfg(\n target_os = "windows"\n)]', 'rustix::fs::open()', '#[cfg(test)] mod tests;',
                     '#[test] fn smoke() {}', '#[cfg(\n test\n)] mod tests;'):
            for texts in ((code, ''), ('', code)):
                with self.subTest(texts=texts):
                    self.assertEqual(SELECTOR.required({'crates/a/src/lib.rs': texts}), (True, False))

    def test_network_and_platform_paths(self):
        for path in ('onshape.rs', 'onshape/http.rs', 'share_links.rs', 'share_links/network.rs',
                     'share_scratch/tests.rs', 'report_send.rs', 'report_send/client.rs',
                     'platform.rs', 'platform/windows.rs'):
            with self.subTest(path=path):
                self.assertTrue(SELECTOR.required({'crates/dutchman/src/' + path: ('', '')})[0])

    def test_history_and_changed_version_gates_select_floor(self):
        history = 'crates/dutchman-solidcraft/tests/history.rs'
        self.assertEqual(SELECTOR.required({history: ('', '')}), (True, True))
        path = 'crates/dutchman-solidcraft/src/document.rs'
        for before, after in (('', 'const FLOOR: Version = V0_18_2;'),
                              ('const FLOOR: Version = V0_18_2;', ''),
                              ('if target >= V0_25_1 { old(); }', 'if target >= V0_25_2 { old(); }'),
                              ('if target >= V0_25_1 { old(); }', 'if target > V0_25_1 { old(); }'),
                              ('VersionGate::ThinExtrude.supported(target)', 'VersionGate::Other.supported(target)'),
                              ('target >= self.minimum()', 'target > self.minimum()'),
                              ('gate.supported(target)', '!gate.supported(target)'),
                              ('if gate.supported(target)\n && allowed { old(); }',
                               'if gate.supported(target)\n || allowed { old(); }')):
            with self.subTest(before=before, after=after):
                self.assertTrue(SELECTOR.required({path: (before, after)})[1])

    def test_other_writer_and_docs_changes_keep_existing_coverage(self):
        for path in ('docs/guide.md', 'crates/a/src/math.rs', 'crates/dutchman-solidcraft/src/document.rs'):
            self.assertEqual(SELECTOR.required({path: ('fn old() {}', 'fn new() {}')}), (False, False))
        code = 'if target >= V0_25_1 { old(); }'
        self.assertEqual(SELECTOR.required({'crates/dutchman-solidcraft/src/document.rs':
                                          (code, code + '\n// explanation')}), (False, False))

    def test_unchanged_inline_tests_do_not_expand_math_changes(self):
        before = 'fn number() -> u8 { 1 }\n#[cfg(test)] mod checks { #[test] fn smoke() {} }'
        after = before.replace('{ 1 }', '{ 2 }')
        self.assertEqual(SELECTOR.required({'crates/a/src/math.rs': (before, after)}), (False, False))
        after = before.replace('fn smoke() {}', 'fn smoke() { assert!(true); }')
        self.assertEqual(SELECTOR.required({'crates/a/src/math.rs': (before, after)}), (True, False))

    def test_docs_examples_do_not_select_platform_coverage(self):
        self.assertEqual(SELECTOR.required({'docs/guide.md': ('', 'Use std::fs or #[cfg(windows)]')}), (False, False))

    def test_missing_base_fails_open(self):
        with patch.object(SELECTOR.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1)):
            self.assertEqual(SELECTOR.select('missing'), (True, True))

    def test_deletion_and_rename_read_both_trees(self):
        def run(args, **kwargs):
            if args[1] == 'cat-file':
                return subprocess.CompletedProcess(args, 0)
            revision = args[-1]
            return subprocess.CompletedProcess(args, 0 if revision.startswith('base:') else 1,
                                               stdout=b'use std::net;')
        with patch.object(SELECTOR.subprocess, 'run', side_effect=run), \
             patch.object(SELECTOR.subprocess, 'check_output', return_value=b'crates/a/src/old.rs\0') as diff:
            self.assertEqual(SELECTOR.select('base'), (True, False))
            diff.assert_called_once_with(['git', 'diff', '--no-renames', '--name-only', '-z', 'base', 'HEAD'])

    def test_workflow_conditions_and_matrices(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        for event in ('pull_request', 'merge_group', 'push'):
            for selected in (False, True):
                values = {'github.event_name': event, 'needs.changes.outputs.full': str(event == 'push').lower(),
                          'needs.changes.outputs.code': 'true', 'needs.changes.outputs.rust': 'true',
                          'needs.changes.outputs.platform': str(selected).lower(),
                          'needs.changes.outputs.floor': str(selected).lower()}
                def evaluate(expression):
                    for key, value in values.items():
                        expression = expression.replace(key, repr(value))
                    return eval(expression.replace('&&', 'and').replace('||', 'or'), {'__builtins__': {}})
                for job in ('test-macos', 'solidcraft-roundtrip'):
                    block = workflow.split('  ' + job + ':\n', 1)[1]
                    condition = next(line[8:] for line in block.splitlines() if line.startswith('    if: '))
                    self.assertEqual(evaluate(condition), event == 'push' or (event == 'pull_request' and selected))
                test = workflow.split('  test:\n', 1)[1].split('  test-arm:', 1)[0]
                expression = re.search(r'fromJSON\((.*)\)', test).group(1)
                self.assertEqual('windows-latest' in evaluate(expression),
                                 event == 'push' or (event == 'pull_request' and selected))
                roundtrip = workflow.split('  solidcraft-roundtrip:\n', 1)[1].split('  placements:', 1)[0]
                # The runner choice is a fromJSON too; the version matrix is the other one.
                expression = next(m.group(1) for m in re.finditer(r'fromJSON\((.*)\)', roundtrip)
                                  if 'self-hosted' not in m.group(1))
                self.assertIn('0.18.2', evaluate(expression))
                self.assertEqual('newest' in evaluate(expression), event == 'push')
