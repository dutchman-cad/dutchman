"""Reject direct socket accepts across Rust test layouts."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('test_accept', ROOT / 'scripts/check-test-accept.py')
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


class TestAcceptTests(unittest.TestCase):
    def test_test_file_layouts(self):
        for path in ('crates/a/tests/http.rs', 'crates/a/src/tests.rs',
                     'crates/a/src/http_tests.rs', 'crates/a/tests/support/http.rs'):
            with self.subTest(path=path):
                self.assertEqual(GUARD.violations(path, 'listener . accept ( );'), [1])

    def test_inline_modules_and_production_after_tests(self):
        source = '''fn server() { listener.accept(); }
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    fn mock() { listener.accept(); }
    mod nested { fn mock() { listener .\n accept(); } }
}
fn other_server() { listener.accept(); }
'''
        self.assertEqual(GUARD.violations('crates/a/src/lib.rs', source), [5, 6])

    def test_combined_cfg(self):
        self.assertEqual(GUARD.violations('crates/a/src/lib.rs',
                                         '#[cfg(all(test, unix))] mod mocks { l.accept(); }'), [1])

    def test_comments_and_strings_are_inert(self):
        source = '''// listener.accept()
/* outer /* listener.accept() */ still commented */
let text = "listener.accept()";
let raw = r###"listener.accept() { }"###;
l.accept();
'''
        self.assertEqual(GUARD.violations('crates/a/tests/http.rs', source), [5])

    def test_only_shared_helper_is_exempt(self):
        self.assertEqual(GUARD.violations(GUARD.HELPER, 'l.accept();'), [])
        self.assertEqual(GUARD.violations('crates/a/tests/net.rs', 'l.accept();'), [1])

    def test_repository_scan_reports_location_and_next_step(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            path = root / 'crates/a/tests/http.rs'
            path.parent.mkdir(parents=True)
            path.write_text('l.accept();\n')
            errors = GUARD.check(root)
            self.assertEqual(len(errors), 1)
            self.assertIn('crates/a/tests/http.rs:1: Use accept_blocking', errors[0])

    def test_external_cfg_test_modules_are_scanned(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            src = root / 'crates/a/src'
            src.mkdir(parents=True)
            (src / 'lib.rs').write_text('#[cfg(test)] mod mock;')
            (src / 'mock.rs').write_text('mod child; fn server() { l.accept(); }')
            (src / 'mock').mkdir()
            (src / 'mock/child.rs').write_text('fn server() { l.accept(); }')
            self.assertEqual(len(GUARD.check(root)), 2)

    def test_external_module_inside_inline_test_module(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            src = root / 'crates/a/src'
            (src / 'checks').mkdir(parents=True)
            (src / 'lib.rs').write_text('#[cfg(test)] mod checks { mod mock; }')
            (src / 'checks/mock.rs').write_text('fn server() { l.accept(); }')
            self.assertEqual(len(GUARD.check(root)), 1)

    def test_standalone_test_functions_are_scanned(self):
        self.assertEqual(GUARD.violations('crates/a/src/lib.rs', '#[test] fn mock() { l.accept(); }'), [1])

    def test_character_literals_do_not_close_test_modules(self):
        self.assertEqual(GUARD.violations('crates/a/src/lib.rs',
                                         "#[cfg(test)] mod tests { let brace = '}'; l.accept(); }"), [1])

    def test_workflow_runs_guard(self):
        self.assertIn('run: python3 scripts/check-test-accept.py',
                      (ROOT / '.github/workflows/ci.yml').read_text())
