import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "unproven-placements.py"
SPEC = importlib.util.spec_from_file_location("unproven_placements", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class UnprovenPlacementsTests(unittest.TestCase):
    def test_new_counts_and_both_legacy_codes(self):
        report = {"completeness": {"unplaced": {"Panel": 3}}, "losses": [
            {"code": "f3d/xref.placement-identity-assumed", "message":
             "xref r: member panel.f3d occurrence placement did not decode; it was placed at the identity without proof"},
            {"code": "f3d/xref.placement-undecoded", "message":
             "external occurrence frame.f3d has typed placement carrier 3 for roles []"},
        ]}
        self.assertEqual(MODULE.unproven_placements(report),
                         {"Panel": 3, "panel.f3d": 1, "frame.f3d": 1})
        self.assertEqual(MODULE.unproven_placements({"ok": True}), {})
        self.assertEqual(self.run_check({"ok": True, "completeness": {"unplaced": {}}}).returncode, 0)

    def run_check(self, report, allowances="{}"):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(json.dumps(report))
            return subprocess.run([sys.executable, str(SCRIPT), str(path),
                                   "--allowances", allowances], capture_output=True, text=True)

    def test_gate_fails_and_prints_names_unless_explicitly_allowed(self):
        report = {"ok": True, "completeness": {"unplaced": {"Panel": 3}}}
        result = self.run_check(report)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Panel ×3", result.stdout)
        result = self.run_check(report, '{"Panel":"Synthetic placement failure under investigation"}')
        self.assertEqual(result.returncode, 0)
        self.assertIn("allowed:", result.stdout)
        self.assertEqual(self.run_check(report, '{"Panel":""}').returncode, 1)
        self.assertEqual(self.run_check(report, '{"Other":"Unrelated"}').returncode, 1)

    def run_with_file(self, report, unplaced):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(json.dumps(report))
            allow = Path(directory) / "allow.json"
            allow.write_text(json.dumps({"unplaced": unplaced}))
            return subprocess.run([sys.executable, str(SCRIPT), str(path), "--allowances-file", str(allow)],
                                  capture_output=True, text=True)

    def test_allowlist_file_pins_names_and_counts(self):
        report = {"ok": True, "completeness": {"unplaced": {"Panel": 3, "Rail": 1}}}
        entry = lambda n: {"count": n, "reason": "synthetic; tracking issue"}
        exact = {"Panel": entry(3), "Rail": entry(1)}
        self.assertEqual(self.run_with_file(report, exact).returncode, 0)
        # A part becoming unplaced more often fails like a new part.
        grown = {"ok": True, "completeness": {"unplaced": {"Panel": 4, "Rail": 1}}}
        result = self.run_with_file(grown, exact)
        self.assertEqual(result.returncode, 1)
        self.assertIn("expects ×3", result.stdout)
        added = {"ok": True, "completeness": {"unplaced": {"Panel": 3, "Rail": 1, "Bolt": 1}}}
        result = self.run_with_file(added, exact)
        self.assertEqual(result.returncode, 1)
        self.assertIn("Bolt ×1: position could not be read; not in the allowlist", result.stdout)
        # Progress is not a failure: a part that is placed now is only reported as stale.
        fewer = {"ok": True, "completeness": {"unplaced": {"Panel": 3}}}
        result = self.run_with_file(fewer, exact)
        self.assertEqual(result.returncode, 0)
        self.assertIn("Rail: now placed", result.stdout)
        # Every file entry needs a positive count and a reason.
        self.assertEqual(self.run_with_file(report, {"Panel": {"count": 3}, "Rail": entry(1)}).returncode, 1)
        self.assertEqual(self.run_with_file(report, {"Panel": {"count": 0, "reason": "x"}, "Rail": entry(1)}).returncode, 1)

    def test_shipped_allowlists_are_well_formed(self):
        folder = SCRIPT.parent / "gate-allowances"
        for path in sorted(folder.glob("*.json")):
            data = json.loads(path.read_text())
            self.assertIn("tracking", data, path.name)
            MODULE.load_allowances("{}", path)

    def test_each_legacy_loss_fails_even_when_geometry_was_verified(self):
        for code in MODULE.CODES:
            with self.subTest(code=code):
                result = self.run_check({"ok": True, "verification": {"status": "verified"},
                                         "losses": [{"code": code, "message": "Panel"}]})
                self.assertEqual(result.returncode, 1)
                self.assertIn("Panel", result.stdout)

    def test_identity_assumptions_are_rejected_but_stored_poses_are_not(self):
        report = {"ok": True, "part_positions": {"assumed": [
            {"part": "Panel", "reason": "identity"},
            {"part": "Frame", "reason": "only_stored_pose"},
        ]}}
        self.assertEqual(MODULE.unproven_placements(report), {"Panel": 1})
        self.assertEqual(self.run_check(report).returncode, 1)

    def test_bad_report_fails_closed(self):
        self.assertEqual(self.run_check({}).returncode, 1)
        self.assertEqual(self.run_check({"ok": True, "completeness": {"unplaced": {"Panel": 0}}}).returncode, 1)
        self.assertEqual(self.run_check({"ok": True, "completeness": None}).returncode, 1)
