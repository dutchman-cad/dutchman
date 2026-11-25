"""Exercise the required status check with real event/result combinations."""
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]


class CiResultTests(unittest.TestCase):
    def check_results(self, event, release=False, failed=None, results=None, docs=False, platform=False, floor=False,
                      placements=False, lockfile=False):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        script = workflow.split('echo "$RESULTS" | python3 -c \'\n', 1)[1]
        script = textwrap.dedent(script.rsplit("\n          '", 1)[0])
        full = event not in ("pull_request", "merge_group")
        active = {"changes", "release-notes", "fmt", "clippy", "test"}
        if full:
            active |= {"wasm", "msrv", "solidcraft-roundtrip", "test-macos",
                       "docker", "test-arm", "placements"}
        if event == "workflow_dispatch" and release:
            active.add("release-build")
        jobs = active | {"docs", "test-arm-again", "release-build", "wasm", "msrv",
                         "solidcraft-roundtrip", "test-macos", "docker", "test-arm", "placements"}
        needs = {job: {"result": "success" if job in active else "skipped"} for job in jobs}
        needs["changes"]["outputs"] = {key: "true" for key in ("code", "rust", "docker")}
        needs["changes"]["outputs"]["full"] = str(full).lower()
        if event == "merge_group":
            active |= {"placements", "solidcraft-roundtrip"}
            needs["placements"]["result"] = "success"
            needs["solidcraft-roundtrip"]["result"] = "success"
        needs["changes"]["outputs"].update(platform=str(platform).lower(), floor=str(floor).lower())
        if lockfile:
            needs["changes"]["outputs"]["lockfile"] = "true"
            active.add("msrv")
            needs["msrv"]["result"] = "success"
        if event == "pull_request" and placements:
            needs["changes"]["outputs"]["placements"] = "true"
            active.add("placements")
            needs["placements"]["result"] = "success"
        if event == "pull_request":
            if platform:
                needs["test-macos"]["result"] = "success"
            if floor:
                needs["solidcraft-roundtrip"]["result"] = "success"
        if docs:
            for job in ("fmt", "clippy", "test", "placements", "solidcraft-roundtrip"):
                needs[job]["result"] = "skipped"
            needs["docs"]["result"] = "success"
            needs["changes"]["outputs"].update(code="false", rust="false", docker="false")
        if event == "pull_request" and platform:
            needs["test"]["result"] = "success"
        for job, result in (results or {}).items():
            needs[job]["result"] = result
        if failed:
            needs[failed] = {"result": "skipped"}
        return subprocess.run(
            [sys.executable, "-c", script], input=json.dumps(needs), text=True,
            capture_output=True,
            env={**os.environ, "EVENT": event, "RELEASE_BUILDS": str(release).lower(),
                 "GITHUB_REF": "refs/heads/test"},
        )

    def test_queue_does_not_require_release_packaging(self):
        result = self.check_results("merge_group")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_queue_still_requires_tests(self):
        result = self.check_results("merge_group", failed="test")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("test", result.stderr)

    def test_dispatch_without_release_packaging(self):
        result = self.check_results("workflow_dispatch")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_dispatch_requires_requested_release_packaging(self):
        result = self.check_results("workflow_dispatch", release=True, failed="release-build")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release-build", result.stderr)

    def test_dispatch_accepts_successful_release_packaging(self):
        result = self.check_results("workflow_dispatch", release=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_fast_pr(self):
        result = self.check_results("pull_request")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_queue_skips_platform_and_image_checks(self):
        for job in ("test-arm", "test-macos", "wasm", "msrv"):
            with self.subTest(job=job):
                result = self.check_results("merge_group", results={job: "success"})
                self.assertNotEqual(result.returncode, 0)

    def test_a_pin_or_assembly_pr_requires_the_placement_sets(self):
        result = self.check_results("pull_request", placements=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.check_results("pull_request", placements=True, failed="placements")
        self.assertNotEqual(result.returncode, 0)
        # Without the filter a pull request does not run them, and a run would be unexpected.
        result = self.check_results("pull_request", results={"placements": "success"})
        self.assertNotEqual(result.returncode, 0)

    def test_a_cargo_lock_change_requires_the_msrv_build(self):
        for event in ("pull_request", "merge_group"):
            with self.subTest(event=event):
                result = self.check_results(event, lockfile=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                result = self.check_results(event, lockfile=True, failed="msrv")
                self.assertNotEqual(result.returncode, 0)

    def test_queue_requires_the_solidcraft_replay(self):
        result = self.check_results("merge_group", failed="solidcraft-roundtrip")
        self.assertNotEqual(result.returncode, 0)

    def test_queue_requires_linux_tools_and_smoke_placements(self):
        result = self.check_results("merge_group", failed="placements")
        self.assertNotEqual(result.returncode, 0)

    def test_docs_only_queue_requires_focused_docs_checks(self):
        result = self.check_results("merge_group", docs=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_main_and_tags_require_full_matrix(self):
        for event in ("push", "schedule", "workflow_dispatch"):
            for job in ("test-arm", "test-macos", "wasm", "msrv", "solidcraft-roundtrip", "placements"):
                with self.subTest(event=event, job=job):
                    result = self.check_results(event, failed=job)
                    self.assertNotEqual(result.returncode, 0)

    def test_image_build_is_reported_not_required(self):
        # release-image.yml checks the published image before pushing it.
        for event in ("push", "schedule", "workflow_dispatch"):
            for outcome in ("failure", "cancelled", "skipped"):
                with self.subTest(event=event, outcome=outcome):
                    result = self.check_results(event, results={"docker": outcome})
                    self.assertEqual(result.returncode, 0, result.stderr)

    def test_merge_group_does_not_schedule_moved_jobs(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for job in ("test-arm", "test-arm-again", "test-macos", "msrv", "wasm", "solidcraft-roundtrip", "docker"):
            block = workflow.split("  " + job + ":\n", 1)[1]
            condition = next(line.strip()[4:] for line in block.splitlines() if line.startswith("    if: "))
            values = {"always()": True, "cancelled()": False, "github.event_name": "merge_group",
                      "needs.changes.outputs.full": "false", "needs.changes.outputs.code": "true",
                      "needs.changes.outputs.rust": "true", "needs.changes.outputs.docker": "true",
                      "needs.changes.outputs.platform": "true", "needs.changes.outputs.floor": "true",
                      "needs.changes.outputs.lockfile": "false",
                      "needs.placements.result": "success", "needs.test-arm.result": "failure"}
            for key, value in values.items():
                condition = condition.replace(key, repr(value))
            condition = re.sub(r"!(?!=)", "not ", condition.replace("&&", "and").replace("||", "or"))
            with self.subTest(job=job):
                # The SolidCraft replay is the one job the queue runs for every code change.
                scheduled = eval(condition, {"__builtins__": {}})
                self.assertEqual(scheduled, job == "solidcraft-roundtrip")

    def test_full_ci_accepts_retry_after_timeout_or_shutdown(self):
        # A timeout can appear as failure or cancelled in the needs context.
        # The retry itself verifies the Actions job conclusion before testing.
        for first in ("failure", "cancelled"):
            with self.subTest(first=first):
                result = self.check_results("push", results={
                    "test-arm": first, "test-arm-again": "success",
                })
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_full_ci_rejects_timeout_without_successful_retry(self):
        for first in ("failure", "cancelled"):
            for retry in ("failure", "cancelled", "skipped"):
                with self.subTest(first=first, retry=retry):
                    result = self.check_results("push", results={
                        "test-arm": first, "test-arm-again": retry,
                    })
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("test-arm", result.stderr)

    def test_selected_pr_requires_platform_and_floor_results(self):
        result = self.check_results("pull_request", platform=True, floor=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for job in ("test-macos", "solidcraft-roundtrip"):
            result = self.check_results("pull_request", platform=True, floor=True, failed=job)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(job, result.stderr)

    def test_queue_keeps_linux_coverage_even_when_selector_matches(self):
        result = self.check_results("merge_group", platform=True, floor=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_test_documentation_still_runs_selected_platform_checks(self):
        result = self.check_results("pull_request", docs=True, platform=True)
        self.assertEqual(result.returncode, 0, result.stderr)
