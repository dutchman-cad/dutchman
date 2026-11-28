"""Release publication requires the ci-result job of full CI on the exact tag and commit."""
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

    def make_job(self, name="ci-result", status="completed", conclusion="success", **values):
        return {"id": 1, "name": name, "status": status, "conclusion": conclusion, **values}

    def fake_gh(self, run, jobs):
        def check_output(cmd):
            if "/jobs?" in cmd[2]:
                return json.dumps([{"jobs": jobs}]).encode()
            return json.dumps([{"workflow_runs": [run] if run else []}]).encode()
        return patch.object(ci.subprocess, "check_output", side_effect=check_output)

    def test_ci_result_success_passes_while_docker_still_runs(self):
        run = self.make_run(status="in_progress", conclusion=None)
        jobs = [self.make_job(), self.make_job("docker", "in_progress", None, id=2)]
        with self.fake_gh(run, jobs), patch.object(ci.time, "sleep") as sleep:
            ci.wait("owner/repo", "abc", "v1.0.0")
            sleep.assert_not_called()

    def test_ci_result_not_success_is_refused(self):
        for conclusion in ("failure", "cancelled", "skipped", "timed_out"):
            jobs = [self.make_job(conclusion=conclusion), self.make_job("docker", conclusion="success", id=2)]
            with self.subTest(conclusion=conclusion), self.fake_gh(self.make_run(), jobs):
                with self.assertRaises(SystemExit):
                    ci.wait("owner/repo", "abc", "v1.0.0")

    def test_completed_run_without_ci_result_is_refused(self):
        with self.fake_gh(self.make_run(), [self.make_job("docker", id=2)]):
            with self.assertRaises(SystemExit):
                ci.wait("owner/repo", "abc", "v1.0.0")

    def test_no_run_is_refused(self):
        with self.fake_gh(None, []), patch.object(ci.time, "sleep"):
            with self.assertRaises(SystemExit):
                ci.wait("owner/repo", "abc", "v1.0.0", timeout=0)

    def test_wrong_commit_run_is_refused(self):
        with self.fake_gh(self.make_run(head_sha="old"), [self.make_job()]), patch.object(ci.time, "sleep"):
            with self.assertRaises(SystemExit):
                ci.wait("owner/repo", "abc", "v1.0.0", timeout=0)

    def test_running_ci_result_waits(self):
        run = self.make_run(status="in_progress", conclusion=None)
        states = [[self.make_job(status="in_progress", conclusion=None)], [self.make_job()]]
        def check_output(cmd):
            if "/jobs?" in cmd[2]:
                return json.dumps([{"jobs": states.pop(0)}]).encode()
            return json.dumps([{"workflow_runs": [run]}]).encode()
        with patch.object(ci.subprocess, "check_output", side_effect=check_output), patch.object(ci.time, "sleep") as sleep:
            ci.wait("owner/repo", "abc", "v1.0.0")
            sleep.assert_called_once_with(30)
