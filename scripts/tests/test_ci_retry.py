"""Run the workflow's ARM retry gate against recorded job outcomes."""
import os
import re
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"


class ArmRetryTests(unittest.TestCase):
    def gate(self, conclusion, notes="", api_failure=False):
        workflow = WORKFLOW.read_text()
        block = workflow.split("      - name: Retry only after a timeout or runner shutdown\n", 1)[1]
        # The script ends where the next step starts, whatever that step's first key.
        script = textwrap.dedent(re.split(r"^      - ", block.split("        run: |\n", 1)[1], maxsplit=1, flags=re.M)[0])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gh = root / "gh"
            gh.write_text("#!/usr/bin/env python3\n" + textwrap.dedent('''
                import os, sys
                from pathlib import Path
                with Path(os.environ["CALLS"]).open("a") as calls:
                    calls.write(sys.argv[2] + "\\n")
                if os.environ["API_FAILURE"] == "1":
                    sys.exit(1)
                if sys.argv[2].endswith("/annotations"):
                    print(os.environ["NOTES"])
                elif os.environ["CONCLUSION"]:
                    print("123\\t" + os.environ["CONCLUSION"] + "\\thttps://api.github.com/repos/example/repo/check-runs/456")
            '''))
            gh.chmod(0o755)
            calls = root / "calls"
            result = subprocess.run(
                ["bash", "-eo", "pipefail", "-c", script],
                text=True, capture_output=True,
                env={**os.environ, "PATH": directory + os.pathsep + os.environ["PATH"],
                     "CALLS": str(calls), "CONCLUSION": conclusion, "NOTES": notes,
                     "API_FAILURE": "1" if api_failure else "0",
                     "GITHUB_REPOSITORY": "example/repo", "GITHUB_RUN_ID": "789",
                     "GITHUB_RUN_ATTEMPT": "2"},
            )
            return result, calls.read_text()

    def test_timeout_retries_without_logs_or_annotations(self):
        result, calls = self.gate("timed_out")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("timed out", result.stdout)
        self.assertNotIn("/annotations", calls)
        self.assertIn("/attempts/2/jobs", calls)

    def test_runner_shutdown_retries(self):
        for notes in ("Process completed with exit code 143.", "The runner has received a shutdown signal."):
            with self.subTest(notes=notes):
                result, calls = self.gate("failure", notes)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("check-runs/456/annotations", calls)

    def test_test_failure_is_not_retried(self):
        result, _ = self.gate("failure", "Test assertion failed")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("failed on its own", result.stderr)

    def test_cancellation_or_other_conclusion_is_not_retried(self):
        for conclusion in ("cancelled", "success", "skipped"):
            with self.subTest(conclusion=conclusion):
                result, calls = self.gate(conclusion, "shutdown signal")
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("/annotations", calls)

    def test_missing_job_or_api_failure_fails_closed(self):
        for conclusion, api_failure in (("", False), ("timed_out", True)):
            with self.subTest(conclusion=conclusion, api_failure=api_failure):
                result, _ = self.gate(conclusion, api_failure=api_failure)
                self.assertNotEqual(result.returncode, 0)

    def test_retry_scheduling(self):
        block = WORKFLOW.read_text().split("  test-arm-again:\n", 1)[1]
        condition = next(line.strip()[4:] for line in block.splitlines() if line.startswith("    if: "))
        for first in ("success", "failure", "cancelled", "skipped"):
            for cancelled in (False, True):
                for event in ("merge_group", "pull_request", "push", "schedule", "workflow_dispatch"):
                    for code in ("true", "false"):
                        expression = condition
                        values = {"always()": True, "cancelled()": cancelled,
                                  "github.event_name": event, "needs.changes.outputs.code": code,
                                  "needs.changes.outputs.full": str(event not in ("merge_group", "pull_request")).lower(),
                                  "needs.test-arm.result": first}
                        for key, value in values.items():
                            expression = expression.replace(key, repr(value))
                        expression = re.sub(r"!(?!=)", "not ", expression.replace("&&", "and").replace("||", "or"))
                        with self.subTest(first=first, cancelled=cancelled, event=event, code=code):
                            self.assertEqual(
                                eval(expression, {"__builtins__": {}}),
                                not cancelled and event not in ("pull_request", "merge_group") and code == "true"
                                and first in ("failure", "cancelled"),
                            )

    def test_every_merge_group_runner_job_has_an_explicit_timeout(self):
        # Reusable calls cannot set a job timeout; cover their runner jobs here.
        for name in ("ci.yml", "placements.yml"):
            workflow = (ROOT / ".github/workflows" / name).read_text()
            jobs = re.split(r"^  ([\w-]+):\n", workflow.split("\njobs:\n", 1)[1], flags=re.M)
            for job, block in zip(jobs[1::2], jobs[2::2]):
                if re.search(r"^    runs-on:", block, flags=re.M):
                    with self.subTest(workflow=name, job=job):
                        self.assertRegex(block, r"(?m)^    timeout-minutes: ")
