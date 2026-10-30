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

    def test_download_choices(self):
        body = self.run_generator("0.2.0").decode()
        mac_rows = [line for line in body.splitlines() if line.startswith("| Mac")]
        self.assertEqual(len(mac_rows), 2)
        for label, extension in [
            ("Mac (any Mac): the desktop app", "dmg"),
            ("Mac command line (any Mac)", "tar.gz"),
        ]:
            name = f"dutchman-0.2.0-macos-universal.{extension}"
            url = f"{RELEASE_NOTES.REPO}/releases/download/v0.2.0/{name}"
            self.assertIn(f"| {label} | [{name}]({url}) | [SHA-256]({url}.sha256) |", mac_rows)
        self.assertNotIn("apple-darwin", body)
        self.assertNotIn("Not sure which Mac", body)

        # The install page links the latest release's version-less copies of the same files.
        from downloads import END, START, table
        install = (ROOT / "docs/install.md").read_text(encoding="utf-8")
        self.assertEqual(install.split(START, 1)[1].split(END, 1)[0], "\n" + table("latest"))
        other_rows = [line for line in table("0.2.0").splitlines()
                      if line.startswith(("| Linux", "| Windows"))]
        self.assertEqual([line for line in body.splitlines()
                          if line.startswith(("| Linux", "| Windows"))], other_rows)

    def test_nightly_download_choices(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/downloads.py"), "nightly"],
            capture_output=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertIn(b"dutchman-nightly-macos-universal.tar.gz.sha256", result.stdout)
        self.assertNotIn(b"apple-darwin", result.stdout)
        self.assertNotIn(b"Not sure which Mac", result.stdout)

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
            self.assertIn("#123-2027-01-02", RELEASE_NOTES.body("1.2.3", root))
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
