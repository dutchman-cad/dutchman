#!/usr/bin/env python3
"""Maintain one issue for failing checks on main."""
import json
import os
import re
import subprocess

MARKER = "<!-- dutchman-main-ci -->"


def api(path, payload=None, paginate=False):
    command = ["gh", "api", path]
    if paginate:
        command += ["--paginate", "--slurp"]
    if payload is not None:
        command += ["--method", "POST" if path.endswith("/issues") else "PATCH", "--input", "-"]
    return json.loads(subprocess.check_output(
        command, input=json.dumps(payload).encode() if payload is not None else None,
    ))


def update(results, repo, sha, run_id, request=api):
    pages = request(
        f"repos/{repo}/actions/workflows/ci.yml/runs?branch=main&event=push&status=completed",
        paginate=True,
    )
    if any(int(run["id"]) > int(run_id) for page in pages for run in page["workflow_runs"]):
        return  # A newer completed main run already supplies the current result.
    failed = sorted(job for job, value in results.items()
                    if value["result"] != "success"
                    and not (job == "test-arm-again" and value["result"] == "skipped")
                    and not (job == "test-arm" and value["result"] in ("failure", "cancelled")
                             and results["test-arm-again"]["result"] == "success"))
    pages = request(f"repos/{repo}/issues?state=all&per_page=100", paginate=True)
    matches = [issue for page in pages for issue in page
               if "pull_request" not in issue and MARKER in (issue.get("body") or "")]
    issue = max(matches, key=lambda x: x["number"], default=None)
    if issue:
        previous = re.search(r"<!-- run-id: (\d+) -->", issue.get("body") or "")
        if previous and int(previous[1]) > int(run_id):
            return  # An older main run must not overwrite a newer result.
    body = (f"{MARKER}\n<!-- run-id: {run_id} -->\n"
            f"Commit: `{sha}`\n\n[CI run](https://github.com/{repo}/actions/runs/{run_id})\n\n"
            + ("Failing checks: " + ", ".join(failed) if failed else "All main CI checks passed."))
    if failed:
        title = f"main CI: {failed[0]} failing since {sha}"
        if issue:
            request(f"repos/{repo}/issues/{issue['number']}", {
                "state": "open", "body": body,
                "title": issue["title"] if issue["state"] == "open" and issue["title"].startswith(
                    f"main CI: {failed[0]} failing since ") else title,
            })
        else:
            request(f"repos/{repo}/issues", {"title": title, "body": body})
    elif issue and issue["state"] == "open":
        request(f"repos/{repo}/issues/{issue['number']}", {"state": "closed", "body": body})


if __name__ == "__main__":
    update(json.loads(os.environ["RESULTS"]), os.environ["GITHUB_REPOSITORY"],
           os.environ["GITHUB_SHA"], os.environ["GITHUB_RUN_ID"])
