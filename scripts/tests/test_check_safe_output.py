import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent.parent / "check-safe-output.py"
spec = importlib.util.spec_from_file_location("check_safe_output", SCRIPT)
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class SafeOutputCheck(unittest.TestCase):
    def count(self, text):
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "x.rs"
            path.write_text(text, encoding="utf-8")
            return check.count(path)

    def test_a_planted_direct_write_is_counted(self):
        self.assertEqual(self.count('fn f() { std::fs::write("a", b"b").unwrap(); }\n'), 1)
        self.assertEqual(self.count("let f = std::fs::File::create(p)?;\n"), 1)
        self.assertEqual(self.count("let o = std::fs::OpenOptions::new();\n"), 1)
        self.assertEqual(self.count("std::fs::remove_file(p).ok();\n"), 1)

    def test_tests_and_comments_are_not(self):
        self.assertEqual(self.count("// std::fs::write(a, b)\n"), 0)
        self.assertEqual(self.count("fn a() {}\n#[cfg(test)]\nmod t { fn f() { std::fs::write(a, b); } }\n"), 0)

    def test_the_repository_is_within_its_allowlist(self):
        allow = check.allowed()
        for name, n in check.counts().items():
            self.assertLessEqual(n, allow.get(name, 0), name)


if __name__ == "__main__":
    unittest.main()
