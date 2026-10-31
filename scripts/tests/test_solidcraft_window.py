"""solidcraft-window.py on a small synthetic tree; no network, no real repository."""
import importlib.util
import io
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "solidcraft_window", Path(__file__).parents[1] / "solidcraft-window.py"
)
window = importlib.util.module_from_spec(spec)
spec.loader.exec_module(window)

FILES = {
    window.VERIFY: """\
pub const OLDEST_SOLIDCRAFT: [u64; 3] = [0, 18, 2];
pub const NEWEST_SOLIDCRAFT: [u64; 2] = [0, 28];
pub const WINDOW_OLDEST: [u64; 3] = [0, 18, 2];
const SCALE: f64 = 0.35;
        for version in ["0.18.2", "0.25.1", "0.27.1", "0.28.0", "0.28.9", "0.29.0"] {}
            ("0.28.0", "current"),
            ("0.29.0", "newer"),
""",
    window.CI: """\
        solidcraft:
          - version: 0.18.2
            sha256: 9d0c922ac72204f34ad22ca7a171be78016ab002e304ace3c07d34d187d830ff
          - version: newest
""",
    "crates/dutchman/src/json.rs": """\
(Some("0.29.0"), "newer", Some("SolidCraft 0.29 is newer. Designs are written for 0.28 and open fine in 0.29."))
(Some("0.28.9"), "current", None)
json!({"known_newest": "0.28"})
"Target SolidCraft 0.28 (found 0.28.1)", "Target SolidCraft 0.28 (found 0.26.0)"
("solidcraft-cli 0.28.0", Some("0.28.0"))
let eframe = "0.33"; let opacity = "0.35";
""",
    "crates/dutchman/src/main.rs": 'for version in ["0.18.2", "0.25.1", "0.28.0"] {}\n',
    "crates/dutchman-gui/tests/ui.rs": '(Some("0.28.9"), "current"), (Some("0.29.0"), "newer"), "SolidCraft 0.28 is installed."\n',
    "docs/ARCHITECTURE.md": """\
format provides the 0.18.2 floor. This stays `solidcraft/23` for 0.28
targets too: its format 45 additions are unused.
`NEWEST_SOLIDCRAFT` (currently 0.28), unless
""",
    "docs/command-line.md": "(currently 0.28). The current window is 0.18–0.28.\n`Target SolidCraft 0.28 (found 0.28.0)`\n",
    "docs/desktop.md": "0.25 patch 1 through 0.28 currently need no notice. built-in newest (currently 0.28), even\n",
    "docs/freecad.md": "(currently 0.28). The window is currently\n0.18–0.28. Older targets\n",
    "docs/guide.md": "The window is currently 0.18–0.28.\n",
    "docs/install.md": "The window is 0.18–0.28; roundtrip\n(currently 0.28).\n",
    "docs/json.md": "minor (`0.28`), `\"0.28.0\"`). all 0.28 patches are current; 0.29 is newer\n",
    "docs/mcp.md": "currently 0.28). is currently 0.18–0.28;\n",
    "docs/releasing.md": "currently `[0, 28]`; `solidcraft-window.sh 0.18.2 0.28` with\n",
    "docs/testing-your-library.md": "currently 0.18–0.28). Older\n",
    "docs/web.md": "(currently 0.28). is currently\n0.18–0.28; older\n",
    "skills/dutchman-converting/SKILL.md": '(currently 0.28), even\n0.18–0.28); older\nsuch as `"0.28"`.\n',
    "README.md": "Dutchman converts CAD.\n",
    window.CHANGELOG: "old\n",
}


def build(root, **edits):
    for rel, text in {**FILES, **edits}.items():
        if text is None:
            continue
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def snapshot(root):
    return {str(p.relative_to(root)): p.read_text() for p in sorted(root.rglob("*")) if p.is_file()}


class Window(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def read(self, rel):
        return (self.root / rel).read_text()

    def cli(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            code = window.main([*args, "--root", str(self.root)])
        return code, out.getvalue(), err.getvalue()

    def test_moves_every_place_and_keeps_the_oldest_end(self):
        build(self.root)
        code, out, err = self.cli("0.29", "0.29.0")
        self.assertEqual((code, err), (0, ""), out)
        verify = self.read(window.VERIFY)
        self.assertIn("NEWEST_SOLIDCRAFT: [u64; 2] = [0, 29];", verify)
        self.assertIn("OLDEST_SOLIDCRAFT: [u64; 3] = [0, 18, 2];", verify)
        self.assertIn("WINDOW_OLDEST: [u64; 3] = [0, 18, 2];", verify)
        self.assertIn('"0.25.1", "0.27.1", "0.29.0", "0.29.9", "0.30.0"', verify)
        self.assertIn("SCALE: f64 = 0.35", verify)
        self.assertIn('("0.29.0", "current")', verify)
        self.assertIn('("0.30.0", "newer")', verify)
        json_rs = self.read("crates/dutchman/src/json.rs")
        self.assertIn("SolidCraft 0.30 is newer. Designs are written for 0.29 and open fine in 0.30.", json_rs)
        self.assertIn('"known_newest": "0.29"', json_rs)
        self.assertIn("(found 0.29.1)", json_rs)
        self.assertIn("(found 0.26.0)", json_rs)
        self.assertIn('"0.33"', json_rs)
        self.assertIn('"0.35"', json_rs)
        self.assertIn("0.18–0.29", self.read("docs/guide.md"))
        self.assertIn("currently `[0, 29]`", self.read("docs/releasing.md"))
        self.assertIn("all 0.29 patches are current; 0.30 is newer", self.read("docs/json.md"))
        self.assertIn('`"0.29"`', self.read("skills/dutchman-converting/SKILL.md"))
        # The design format decision is left alone.
        self.assertIn("`solidcraft/23` for 0.28", self.read("docs/ARCHITECTURE.md"))
        self.assertIn("(currently 0.29)", self.read("docs/ARCHITECTURE.md"))
        # The workflow file is never edited (pushes with the default token cannot).
        self.assertEqual(self.read(window.CI), FILES[window.CI])
        changelog = self.read(window.CHANGELOG)
        self.assertIn("window is 0.18–0.29; CI checks the newest 0.29 patch", changelog)
        self.assertIn("pinned 0.18.2", changelog)
        self.assertIn("Designs keep format 23", changelog)

    def test_idempotent(self):
        build(self.root)
        self.assertEqual(self.cli("0.29", "0.29.0")[0], 0)
        first = snapshot(self.root)
        code, out, _ = self.cli("0.29", "0.29.0")
        self.assertEqual(code, 0)
        self.assertIn("Already at 0.29", out)
        self.assertEqual(snapshot(self.root), first)

    def test_skipping_a_minor_shifts_by_the_gap(self):
        build(self.root)
        self.assertEqual(self.cli("0.30", "0.30.1")[0], 0)
        self.assertIn("[0, 30]", self.read(window.VERIFY))
        self.assertIn('("0.31.0", "newer")', self.read(window.VERIFY))

    def test_missing_places_fail_loudly_and_change_nothing(self):
        cases = {
            "constant": {window.VERIFY: FILES[window.VERIFY].replace("NEWEST_SOLIDCRAFT", "NEWEST")},
            "matrix": {window.CI: FILES[window.CI].replace("newest", '"0.28"')},
            "doc": {"docs/guide.md": "Nothing about versions.\n"},
            "docs file": {"docs/web.md": None},
            "fixtures": {"crates/dutchman/src/main.rs": "fn main() {}\n"},
            "stale": {"docs/new-page.md": "The newest is currently 0.28 here.\n"},
        }
        for name, edits in cases.items():
            with self.subTest(name):
                self.dir.cleanup()
                self.dir = tempfile.TemporaryDirectory()
                self.root = Path(self.dir.name)
                build(self.root, **edits)
                before = snapshot(self.root)
                code, _, err = self.cli("0.29", "0.29.0")
                self.assertEqual(code, 1)
                self.assertIn("solidcraft-window:", err)
                self.assertEqual(snapshot(self.root), before)

    def test_rejects_bad_arguments(self):
        build(self.root)
        for args in [("0.29", "0.28.1"), ("0.29", "0.29"), ("29", "29.0.0"), ("0.27", "0.27.0")]:
            with self.subTest(args):
                self.assertEqual(self.cli(*args)[0], 1)


if __name__ == "__main__":
    unittest.main()
