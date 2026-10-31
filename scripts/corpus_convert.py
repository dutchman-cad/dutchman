#!/usr/bin/env python3
"""Converts one chunk of a public corpus sweep (see corpus_plan.py).

  corpus_convert.py --plan plan.json --chunk 003 --dutchman ./dutchman --out out
                    [--keep-sc a,b] [--file-timeout 5400] [--budget 16800]

Fetches only the chunk's files (a blobless shallow fetch of the planned commit, then a checkout
of those paths), and runs per file `dutchman convert` under `/usr/bin/time -v` and `timeout`.
Writes out/<id>.json (the report), .err, .diag.jsonl and .meta.json; the converted design is
deleted unless the file matches --keep-sc, when it moves to sc/.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time

MEMORY_REFUSAL = "this conversion needs about "
OOM_MARKERS = ("memory allocation of", "out of memory", "cannot allocate memory")


def git(cwd, *args, stdin=None):
    return subprocess.run(["git", "-C", cwd, *args], input=stdin, capture_output=True, check=True)


def fetch_repo(root, repo, sha, paths):
    dest = os.path.join(root, repo.replace("/", "__"))
    os.makedirs(dest, exist_ok=True)
    git(dest, "init", "-q")
    git(dest, "remote", "add", "origin", f"https://github.com/{repo}")
    git(dest, "fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", sha)
    git(dest, "--literal-pathspecs", "checkout", "-q", sha, "--pathspec-from-file=-",
        "--pathspec-file-nul", stdin="\0".join(paths).encode())
    return dest


def parse_time(text):
    rss = wall = None
    m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", text)
    if m:
        rss = int(m.group(1))
    m = re.search(r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\): ([\d:.]+)", text)
    if m:
        wall = 0.0
        for part in m.group(1).split(":"):
            wall = wall * 60 + float(part)
    return rss, wall


def classify(code, report, err):
    """converted | failed | needs_memory | timeout | skipped"""
    if code == 0 and report and report.get("ok"):
        return "converted"
    if code == 124:
        return "timeout"
    message = ((report or {}).get("error") or {}).get("message") or ""
    if message.startswith(MEMORY_REFUSAL):
        return "needs_memory"
    low = err.lower()
    if code in (137, -9) or any(m in low for m in OOM_MARKERS):
        return "needs_memory"
    return "failed"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--plan", required=True)
    ap.add_argument("--chunk", required=True)
    ap.add_argument("--dutchman", required=True)
    ap.add_argument("--out", default="out")
    ap.add_argument("--work", default="work")
    ap.add_argument("--keep-sc", default="")
    ap.add_argument("--file-timeout", type=int, default=90 * 60)
    ap.add_argument("--budget", type=int, default=280 * 60, help="seconds for the whole chunk")
    args = ap.parse_args()

    with open(args.plan) as f:
        plan = json.load(f)
    chunk = next(c for c in plan["chunks"] if c["chunk"] == args.chunk)
    keep = [k.strip().lower() for k in args.keep_sc.split(",") if k.strip()]
    os.makedirs(args.out, exist_ok=True)
    sc_dir = os.path.join(os.path.dirname(os.path.abspath(args.out)), "sc")
    started = time.time()

    repos = {}
    for f in chunk["files"]:
        repos.setdefault((f["source"], f["sha"]), []).append(f)
    checkout = {}
    for (repo, sha), group in repos.items():
        try:
            checkout[repo] = fetch_repo(args.work, repo, sha, [f["path"] for f in group])
        except subprocess.CalledProcessError as err:
            print(f"::error::could not fetch {repo}: {err.stderr.decode()[:300]}")
            checkout[repo] = None

    for f in chunk["files"]:
        base = os.path.join(args.out, f["id"])
        meta = {"id": f["id"], "source": f["source"], "path": f["path"], "size": f["size"],
                "preset": f["preset"], "ext": os.path.splitext(f["path"])[1].lower().lstrip(".")}
        root = checkout.get(f["source"])
        source = os.path.join(root, f["path"]) if root else None
        remaining = args.budget - (time.time() - started)
        if not source or not os.path.exists(source):
            meta.update(status="failed", exit_code=None, note="could not fetch the file")
        elif remaining < 120:
            meta.update(status="skipped", exit_code=None, note="chunk time budget used up")
        else:
            limit = int(min(args.file_timeout, remaining - 60))
            out_sc = base + ".sc"
            cmd = ["/usr/bin/time", "-v", "-o", base + ".time", "timeout", "-k", "30", str(limit),
                   args.dutchman, "convert", source, "-o", out_sc, "--preset", f["preset"],
                   "--force", "--no-verify", "--diagnostics", base + ".diag.jsonl", "--json"]
            t0 = time.time()
            with open(base + ".json", "w") as so, open(base + ".err", "w") as se:
                code = subprocess.run(cmd, stdout=so, stderr=se).returncode
            wall = time.time() - t0
            try:
                time_text = open(base + ".time").read()
            except OSError:
                time_text = ""
            rss, timed = parse_time(time_text)
            try:
                report = json.load(open(base + ".json"))
            except (OSError, ValueError):
                report = None
            err = open(base + ".err", errors="replace").read()
            meta.update(status=classify(code, report, err), exit_code=code,
                        wall_s=round(timed if timed is not None else wall, 1), peak_rss_kb=rss,
                        limit_s=limit)
            if os.path.exists(out_sc):
                meta["sc_bytes"] = os.path.getsize(out_sc)
                if keep and any(k in f"{f['source']}/{f['path']}".lower() for k in keep):
                    os.makedirs(sc_dir, exist_ok=True)
                    shutil.move(out_sc, os.path.join(sc_dir, f["id"] + ".sc"))
                    meta["kept_sc"] = True
                else:
                    os.remove(out_sc)
            if os.path.exists(base + ".time"):
                os.remove(base + ".time")
            if os.path.getsize(base + ".err") > 200_000:
                with open(base + ".err", "rb") as fh:
                    data = fh.read()
                with open(base + ".err", "wb") as fh:
                    fh.write(data[:100_000] + b"\n[...]\n" + data[-100_000:])
        with open(base + ".meta.json", "w") as fh:
            json.dump(meta, fh)
        print(f"{meta['status']:<13} {meta.get('wall_s', '-')!s:>8}s  {f['source']}/{f['path']}", flush=True)
        # The checkout is not needed again: keep the runner's disk free.
        if source and os.path.exists(source):
            os.remove(source)


if __name__ == "__main__":
    sys.exit(main())
