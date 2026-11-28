#!/usr/bin/env python3
"""Require the ci-result job of full CI for the exact release tag and commit.

ci-result encodes the required set of jobs. Reported-only jobs (the Docker image build) are
neither waited for nor required, because no release step consumes them.
"""
import argparse
import json
import subprocess
import time


def tag_run(runs, sha, tag):
    matches = [run for run in runs if run["head_sha"] == sha and run["event"] == "push"
               and run["head_branch"] == tag]
    return max(matches, key=lambda run: run["id"], default=None)


def ci_result_job(jobs):
    matches = [job for job in jobs if job["name"] == "ci-result"]
    return max(matches, key=lambda job: job["id"], default=None)


def run_jobs(repo, run_id):
    pages = json.loads(subprocess.check_output([
        "gh", "api", f"repos/{repo}/actions/runs/{run_id}/jobs?per_page=100&filter=latest",
        "--paginate", "--slurp",
    ]))
    return [job for page in pages for job in page["jobs"]]


def wait(repo, sha, tag, timeout=21600):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        pages = json.loads(subprocess.check_output([
            "gh", "api", f"repos/{repo}/actions/workflows/ci.yml/runs?head_sha={sha}&event=push&per_page=100",
            "--paginate", "--slurp",
        ]))
        run = tag_run([run for page in pages for run in page["workflow_runs"]], sha, tag)
        if run:
            job = ci_result_job(run_jobs(repo, run["id"]))
            if job and job["status"] == "completed":
                if job["conclusion"] != "success":
                    raise SystemExit(f"ci-result for {tag} at {sha} is {job['conclusion']}: {run['html_url']}")
                print(f"ci-result passed for {tag} at {sha}: {run['html_url']}")
                return
            if not job and run["status"] == "completed":
                raise SystemExit(f"Full CI for {tag} at {sha} has no ci-result job: {run['html_url']}")
        time.sleep(30)
    raise SystemExit(f"No passing ci-result for {tag} at {sha} within {timeout}s")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repo")
    parser.add_argument("sha")
    parser.add_argument("tag")
    args = parser.parse_args()
    wait(args.repo, args.sha, args.tag)
