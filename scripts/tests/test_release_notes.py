"""Check release bodies with Python's standard library."""

import importlib.util
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("release_notes", ROOT / "scripts/release-notes.py")
RELEASE_NOTES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE_NOTES)


class ReleaseNotes(unittest.TestCase):
    def run_generator(self, version):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/release-notes.py"), version],
            cwd=tempfile.gettempdir(), capture_output=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        return result.stdout

    def test_current_workspace_version(self):
        manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        package = manifest.split("[workspace.package]", 1)[1].split("\n[", 1)[0]
        version = re.search(r'^version = "([^"]+)"', package, re.MULTILINE).group(1)
        if not (ROOT / "release-notes" / f"{version}.md").exists():
            self.skipTest(f"No release notes yet for {version}")
        self.run_generator(version)

    def test_020_exact_body(self):
        expected = (Path(__file__).parent / "fixtures/v0.2.0-body.md").read_bytes()
        self.assertEqual(self.run_generator("0.2.0"), expected)

    def test_required_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            notes = root / "release-notes/1.2.3.md"
            notes.parent.mkdir()
            changelog = root / "CHANGELOG.md"
            changelog.write_text("## 1.2.3 (2027-01-02)\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "notes file is missing"):
                RELEASE_NOTES.body("1.2.3", root)
            for text, message in [
                ("Intro only.", "What's new section"),
                ("## What's new\n\n- **Change.** Details.", "intro paragraph"),
                ("## Known issue\n\nIssue.\n\n## What's new", "intro paragraph"),
                ("Intro.\n\n## What's new\n\n" + "- **Change.** Details.\n" * 8,
                 "more than 7 bullets"),
            ]:
                with self.subTest(message=message):
                    notes.write_text(text, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, message):
                        RELEASE_NOTES.body("1.2.3", root)
            image = ("![A converted part](https://raw.githubusercontent.com/dutchman-cad/dutchman"
                     "/v1.2.3/docs/images/releases/1.2.3/part.png)\n\n")
            complete = ("Intro.\n\n" + image + "## What's new\n\n"
                        + "- **Change.** Details.\n" * 7 + "\n## Known issue\n\n- An issue.\n")
            notes.write_text(complete.replace(image, ""), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "at least one new image"):
                RELEASE_NOTES.body("1.2.3", root)
            notes.write_text(complete.replace("[A converted part]", "[ ]"), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "alt text"):
                RELEASE_NOTES.body("1.2.3", root)
            notes.write_text(complete, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "commit the image"):
                RELEASE_NOTES.body("1.2.3", root)
            picture = root / "docs/images/releases/1.2.3/part.png"
            picture.parent.mkdir(parents=True)
            picture.write_bytes(b"png")
            self.assertIn("#123-2027-01-02", RELEASE_NOTES.body("1.2.3", root))
            for text in ["## Unreleased\n", "## 1.2.3\n", "## 1.2.30 (2027-01-02)\n"]:
                changelog.write_text(text, encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "dated entry for 1.2.3"):
                    RELEASE_NOTES.body("1.2.3", root)

    def test_cli_error(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/release-notes.py"), "999.999.999"],
            capture_output=True, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"notes file is missing", result.stderr)
        self.assertEqual(result.stdout, b"")


if __name__ == "__main__":
    unittest.main()
