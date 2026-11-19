"""Exercise main failure reporting without creating GitHub issues."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("main_signal", ROOT / "scripts/ci-main-signal.py")
signal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signal)


class MainSignalTests(unittest.TestCase):
    def report(self, failed=False, issue=None, sha="abc", main="abc", run="100", arm_retry=False, arm_result="failure"):
        calls = []
        def api(path, payload=None, paginate=False):
            calls.append((path, payload))
            if path.endswith("/commits/main"):
                return {"sha": main}
            if paginate:
                return [[issue] if issue else []]
            return {}
        results = {job: {"result": "success"} for job in ("test", "test-arm", "test-macos")}
        results["test-arm-again"] = {"result": "skipped"}
        if failed:
            results["test-macos"]["result"] = "failure"
        if arm_retry:
            results["test-arm"]["result"] = arm_result
            results["test-arm-again"]["result"] = "success"
        signal.update(results, "owner/repo", sha, run, request=api)
        return [(path, payload) for path, payload in calls if payload is not None]

    def issue(self, state="open", run="90"):
        return {"number": 42, "state": state, "title": "main CI: test-macos failing since first",
                "body": signal.MARKER + f"\n<!-- run-id: {run} -->"}

    def test_creates_single_issue_on_failure(self):
        calls = self.report(failed=True)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1]["title"], "main CI: test-macos failing since abc")

    def test_updates_issue_preserving_first_failure(self):
        calls = self.report(failed=True, issue=self.issue())
        self.assertEqual(calls[0][0], "repos/owner/repo/issues/42")
        self.assertIn("since first", calls[0][1]["title"])

    def test_closes_on_green_and_reopens_on_new_failure(self):
        self.assertEqual(self.report(issue=self.issue())[0][1]["state"], "closed")
        reopened = self.report(failed=True, issue=self.issue(state="closed"))[0][1]
        self.assertEqual(reopened["state"], "open")
        self.assertIn("since abc", reopened["title"])

    def test_green_without_issue_does_nothing(self):
        self.assertEqual(self.report(), [])

    def test_successful_arm_retry_is_green(self):
        for arm_result in ("failure", "cancelled"):
            self.assertEqual(self.report(arm_retry=True, arm_result=arm_result), [])

    def test_stale_commit_or_run_cannot_overwrite_current_signal(self):
        self.assertEqual(self.report(failed=True, main="new"), [])
        self.assertEqual(self.report(issue=self.issue(run="101")), [])
