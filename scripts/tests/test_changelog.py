"""Exercise changelog assembly in temporary repositories."""

from datetime import date
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "changelog", Path(__file__).resolve().parents[1] / "changelog.py"
)
CHANGELOG = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHANGELOG)


class Changelog(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.path = self.root / "CHANGELOG.md"
        self.original = "# Changelog\n\nIntro.\n\n## 0.2.0 (2026-10-25)\n\n### Formats\n\n- Old.\n"
        self.path.write_text(self.original, encoding="utf-8")
        (self.root / "changelog.d").mkdir()
        self.readme = self.root / "changelog.d/README.md"
        self.readme.write_text("# Instructions\n", encoding="utf-8")

    def fragment(self, name, text):
        path = self.root / "changelog.d" / f"{name}.md"
        path.write_text(text, encoding="utf-8")
        return path

    def test_check_accepts_same_heading_in_different_versions(self):
        self.path.write_text(self.original + "\n## 0.1.0\n\n### Formats\n\n- Older.\n",
                             encoding="utf-8")
        self.fragment("good", "### History\n\n- New.\n  More detail.\n- Another.\n")
        CHANGELOG.check(self.root)

    def test_check_rejects_duplicate_heading_in_any_version(self):
        for version in ("0.2.0 (2026-10-25)", "0.1.0"):
            with self.subTest(version=version):
                self.path.write_text(self.original + f"\n## {version}\n\n"
                                     "### History\n\n- One.\n\n### History\n\n- Two.\n",
                                     encoding="utf-8")
                with self.assertRaises(ValueError) as caught:
                    CHANGELOG.check(self.root)
                self.assertIn(version, str(caught.exception))
                self.assertIn("duplicate heading ### History", str(caught.exception))

    def test_check_rejects_bad_fragments(self):
        for text, heading in (
            ("", "''"),
            ("- Missing heading.\n", "Missing heading"),
            ("\n### History\n\n- Leading blank.\n", "''"),
            ("### Unknown\n\n- Change.\n", "Unknown"),
            ("### Known issue\n\n- Change.\n", "Known issue"),
            ("### History\n\nOnly prose.\n", "History"),
            ("### History\n\n- Change.\n\n### Docs\n\n- Another.\n", "History"),
            ("### History\n\n- Change.\nUnindented continuation.\n", "History"),
        ):
            with self.subTest(text=text):
                self.fragment("bad", text)
                with self.assertRaises(ValueError) as caught:
                    CHANGELOG.check(self.root)
                self.assertIn("bad.md", str(caught.exception))
                self.assertIn(heading, str(caught.exception))

    def test_preview_orders_topics_then_filenames(self):
        self.fragment("a-docs", "### Docs\n\n- Docs.\n")
        self.fragment("z-history", "### History\n\n- Later.\n")
        self.fragment("b-history", "### History\n\n- First.\n  Continued.\n- Second.\n")
        self.fragment("z-formats", "### Formats\n\n- Format.\n")
        self.assertEqual(CHANGELOG.preview(self.root),
                         "## Unreleased\n\n### Formats\n\n- Format.\n\n"
                         "### History\n\n- First.\n  Continued.\n- Second.\n- Later.\n\n"
                         "### Docs\n\n- Docs.\n")
        self.assertEqual(self.path.read_text(encoding="utf-8"), self.original)
        self.assertTrue((self.root / "changelog.d/z-history.md").exists())

    def test_empty_preview(self):
        self.assertEqual(CHANGELOG.preview(self.root), "## Unreleased\n")

    def test_release_inserts_above_newest_and_deletes_only_fragments(self):
        self.fragment("history", "### History\n\n- New.\n")
        self.fragment("docs", "### Docs\n\n- Guide.\n")
        CHANGELOG.release("0.3.0", "2026-11-01", self.root)
        intro, old = self.original.split("## 0.2.0", 1)
        self.assertEqual(self.path.read_text(encoding="utf-8"), intro
                         + "## 0.3.0 (2026-11-01)\n\n### History\n\n- New.\n\n"
                         "### Docs\n\n- Guide.\n\n## 0.2.0" + old)
        self.assertEqual(list((self.root / "changelog.d").iterdir()), [self.readme])
        CHANGELOG.check(self.root)

    def test_release_defaults_to_today(self):
        self.fragment("docs", "### Docs\n\n- Guide.\n")
        CHANGELOG.release("0.3.0", root=self.root)
        self.assertIn(f"## 0.3.0 ({date.today().isoformat()})",
                      self.path.read_text(encoding="utf-8"))

    def test_release_refusals_leave_files_untouched(self):
        fragment = self.fragment("good", "### History\n\n- New.\n")
        for version, stamp in (("0.2.0", None), ("bad", None),
                               ("0.3.0", "2026-02-30"), ("0.3.0", "20261101")):
            with self.subTest(version=version, stamp=stamp):
                with self.assertRaises(ValueError):
                    CHANGELOG.release(version, stamp, self.root)
                self.assertEqual(self.path.read_text(encoding="utf-8"), self.original)
                self.assertTrue(fragment.exists())
        fragment.write_text("### Unknown\n\n- New.\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            CHANGELOG.release("0.3.0", root=self.root)
        self.assertEqual(self.path.read_text(encoding="utf-8"), self.original)
        self.assertTrue(fragment.exists())

    def test_release_does_not_confuse_version_prefix(self):
        self.fragment("good", "### History\n\n- New.\n")
        CHANGELOG.release("0.2.00", root=self.root)
        self.assertIn("## 0.2.00 (", self.path.read_text(encoding="utf-8"))

    def test_release_refuses_existing_undated_version(self):
        self.path.write_text("# Changelog\n\n## 0.3.0\n", encoding="utf-8")
        self.fragment("good", "### History\n\n- New.\n")
        with self.assertRaisesRegex(ValueError, "already has an entry"):
            CHANGELOG.release("0.3.0", root=self.root)

    def test_release_refuses_no_fragments(self):
        with self.assertRaisesRegex(ValueError, "add a fragment"):
            CHANGELOG.release("0.3.0", root=self.root)
        self.assertEqual(self.path.read_text(encoding="utf-8"), self.original)

    def test_cli_check_reports_file_and_heading(self):
        scripts = self.root / "scripts"
        scripts.mkdir()
        script = scripts / "changelog.py"
        script.write_bytes(Path(CHANGELOG.__file__).read_bytes())
        self.fragment("bad", "### Wrong\n\n- Change.\n")
        result = subprocess.run([sys.executable, str(script), "check"],
                                capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("bad.md", result.stderr)
        self.assertIn("Wrong", result.stderr)
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
