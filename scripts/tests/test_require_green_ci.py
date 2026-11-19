"""Release publication requires full CI on the exact tag and commit."""
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("green_ci", ROOT / "scripts/require-green-ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


class ReleaseCiTests(unittest.TestCase):
    def make_run(self, **values):
        return {"id": 10, "head_sha": "abc", "head_branch": "v1.0.0", "event": "push",
                "status": "completed", "conclusion": "success", "html_url": "https://example/run", **values}

    def test_wrong_commit_branch_or_event_cannot_authorize_release(self):
        for values in ({"head_sha": "old"}, {"head_branch": "main"}, {"event": "merge_group"}):
            with self.subTest(values=values):
                self.assertIsNone(ci.tag_run([self.make_run(**values)], "abc", "v1.0.0"))

    def test_newest_tag_run_wins(self):
        old, new = self.make_run(id=1), self.make_run(id=2, conclusion="failure")
        self.assertEqual(ci.tag_run([old, new], "abc", "v1.0.0"), new)

    def test_pass_and_fail_conclusions(self):
        for conclusion in ("success", "failure", "cancelled", "timed_out"):
            response = json.dumps([{"workflow_runs": [self.make_run(conclusion=conclusion)]}]).encode()
            with self.subTest(conclusion=conclusion), patch.object(ci.subprocess, "check_output", return_value=response):
                if conclusion == "success":
                    ci.wait("owner/repo", "abc", "v1.0.0")
                else:
                    with self.assertRaises(SystemExit):
                        ci.wait("owner/repo", "abc", "v1.0.0")

    def test_pending_ci_waits_and_missing_ci_times_out(self):
        responses = [json.dumps([{"workflow_runs": []}]).encode(),
                     json.dumps([{"workflow_runs": [self.make_run()]}]).encode()]
        with patch.object(ci.subprocess, "check_output", side_effect=responses), patch.object(ci.time, "sleep") as sleep:
            ci.wait("owner/repo", "abc", "v1.0.0")
            sleep.assert_called_once_with(30)
        with self.assertRaises(SystemExit):
            ci.wait("owner/repo", "abc", "v1.0.0", timeout=0)
