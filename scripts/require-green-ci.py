#!/usr/bin/env python3
"""Require successful full CI for the exact release tag and commit."""
import argparse
import json
import subprocess
import time


def tag_run(runs, sha, tag):
    matches = [run for run in runs if run["head_sha"] == sha and run["event"] == "push"
               and run["head_branch"] == tag]
    return max(matches, key=lambda run: run["id"], default=None)


def wait(repo, sha, tag, timeout=21600):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        pages = json.loads(subprocess.check_output([
            "gh", "api", f"repos/{repo}/actions/workflows/ci.yml/runs?head_sha={sha}&event=push&per_page=100",
            "--paginate", "--slurp",
        ]))
        run = tag_run([run for page in pages for run in page["workflow_runs"]], sha, tag)
        if run and run["status"] == "completed":
            if run["conclusion"] != "success":
                raise SystemExit(f"Full CI for {tag} at {sha} is {run['conclusion']}: {run['html_url']}")
            print(f"Full CI passed for {tag} at {sha}: {run['html_url']}")
            return
        time.sleep(30)
    raise SystemExit(f"No successful full CI completed for {tag} at {sha} within {timeout}s")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repo")
    parser.add_argument("sha")
    parser.add_argument("tag")
    args = parser.parse_args()
    wait(args.repo, args.sha, args.tag)
