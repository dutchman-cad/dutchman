#!/usr/bin/env python3
"""Plans a public corpus sweep: reads corpus/public.toml, checks each repository's licence,
lists its CAD files and groups them into chunks, one per matrix job of the workflow.

  corpus_plan.py --corpus corpus/public.toml --out plan.json [--sources a/b,c/d] [--step true]

Needs the `gh` CLI (GH_TOKEN) and git. Writes `plan.json`; with GITHUB_OUTPUT set, also writes
`matrix` (a GitHub matrix object) and `chunks` (their count).
"""
import argparse
import fnmatch
import json
import os
import subprocess
import sys
import tempfile
import tomllib

BIG_BYTES = 40 * 1024 * 1024
GROUP = 20
MAX_CHUNKS = 256
# A chunk is checked out on one runner, which has about 14 GB free; stay well within it.
CHUNK_BYTES = 1500 * 1024 * 1024
STEP_GLOBS = ("*.step", "*.stp")


def gh_api(path, jq=None):
    cmd = ["gh", "api", path]
    if jq:
        cmd += ["--jq", jq]
    done = subprocess.run(cmd, capture_output=True, text=True)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip() or done.stdout.strip())
    return done.stdout.strip()


def matches(path, patterns):
    lower = path.lower()
    return any(fnmatch.fnmatchcase(lower, p.lower()) for p in patterns)


def list_tree(repo, sha):
    """(path, size) for every file at `sha`; sizes are None when the tree is too big for the API."""
    tree = json.loads(gh_api(f"repos/{repo}/git/trees/{sha}?recursive=1"))
    if not tree.get("truncated"):
        return [(e["path"], e.get("size")) for e in tree["tree"] if e["type"] == "blob"]
    with tempfile.TemporaryDirectory() as tmp:
        run = lambda *a: subprocess.run(["git", "-C", tmp, *a], check=True, capture_output=True, text=True)
        run("init", "-q")
        run("remote", "add", "origin", f"https://github.com/{repo}")
        run("fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", sha)
        out = run("ls-tree", "-r", "-z", "--name-only", sha).stdout
    return [(p, None) for p in out.split("\0") if p]


def file_id(index, path):
    base = os.path.basename(path)
    safe = "".join(c if c.isalnum() or c in "._-" else "_" for c in base)[:60]
    return f"{index:05d}-{safe}"


def chunk_files(files):
    """Big files alone, small files in groups, never more than MAX_CHUNKS."""
    big = [f for f in files if f["size"] > BIG_BYTES or f["path"].lower().endswith(".f3z")]
    small = [f for f in files if f not in big]
    chunks = [[f] for f in big]
    by_repo = {}
    for f in small:
        by_repo.setdefault(f["source"], []).append(f)
    group = GROUP
    while True:
        smalls = []
        for repo_files in by_repo.values():
            current, total = [], 0
            for f in repo_files:
                if current and (len(current) >= group or total + f["size"] > CHUNK_BYTES):
                    smalls.append(current)
                    current, total = [], 0
                current.append(f)
                total += f["size"]
            if current:
                smalls.append(current)
        if len(chunks) + len(smalls) <= MAX_CHUNKS or group > 10000:
            break
        group *= 2
    chunks += smalls
    if len(chunks) > MAX_CHUNKS:
        # Even whole repositories at a time do not fit: pack by size, heaviest first.
        flat = sorted((f for c in chunks for f in c), key=lambda f: -f["size"])
        bins = [[] for _ in range(MAX_CHUNKS)]
        for f in flat:
            min(bins, key=lambda b: sum(x["size"] for x in b)).append(f)
        chunks = [b for b in bins if b]
    return chunks


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default="corpus/public.toml")
    ap.add_argument("--out", default="plan.json")
    ap.add_argument("--sources", default="")
    ap.add_argument("--step", default="true")
    ap.add_argument("--preset", default="standard")
    args = ap.parse_args()

    with open(args.corpus, "rb") as f:
        corpus = tomllib.load(f)
    default_globs = corpus.get("defaults", {}).get("globs", [])
    wanted = {s.strip().lower() for s in args.sources.split(",") if s.strip()}
    include_step = args.step.lower() == "true"

    listed = {s["repo"].lower() for s in corpus["source"]}
    unknown = sorted(wanted - listed)
    if unknown:
        sys.exit(f"not in {args.corpus}: {', '.join(unknown)}")

    sources, files = [], []
    for src in corpus["source"]:
        repo = src["repo"]
        if wanted and repo.lower() not in wanted:
            continue
        entry = {"repo": repo, "license": src["license"], "status": "planned", "reason": None,
                 "sha": None, "files": 0, "bytes": 0}
        sources.append(entry)
        try:
            actual = gh_api(f"repos/{repo}/license", ".license.spdx_id")
            if actual != src["license"]:
                entry.update(status="skipped", reason=f"licence is {actual or 'none'}, listed as {src['license']}")
                continue
            branch = gh_api(f"repos/{repo}", ".default_branch")
            sha = gh_api(f"repos/{repo}/commits/{branch}", ".sha")
            entry["sha"] = sha
            globs = src.get("globs", default_globs)
            if not include_step:
                globs = [g for g in globs if g.lower() not in STEP_GLOBS]
            exclude = src.get("exclude", [])
            presets = src.get("file_presets", {})
            for path, size in list_tree(repo, sha):
                if not matches(path, globs) or matches(path, exclude):
                    continue
                preset = next((p for g, p in presets.items() if matches(path, [g])), args.preset)
                files.append({"source": repo, "path": path, "size": size or 0,
                              "size_known": size is not None, "preset": preset, "sha": sha})
                entry["files"] += 1
                entry["bytes"] += size or 0
        except (RuntimeError, subprocess.CalledProcessError) as err:
            entry.update(status="skipped", reason=f"could not list: {str(err)[:200]}")

    for i, f in enumerate(files, 1):
        f["id"] = file_id(i, f["path"])
    chunks = []
    for n, chunk in enumerate(chunk_files(files), 1):
        repos = sorted({f["source"] for f in chunk})
        label = repos[0].split("/")[1] if len(repos) == 1 else f"{len(repos)} repositories"
        chunks.append({"chunk": f"{n:03d}", "label": f"{label} ({len(chunk)} files)", "files": chunk})

    plan = {"sources": sources, "chunks": chunks, "files": len(files)}
    with open(args.out, "w") as f:
        json.dump(plan, f, indent=1)
    for s in sources:
        print(f"{s['repo']}: {s['status']} {s['reason'] or ''} {s['files']} files, {s['bytes'] / 1e6:.0f} MB")
    print(f"{len(files)} files in {len(chunks)} chunks")

    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        matrix = {"include": [{"chunk": c["chunk"], "label": c["label"]} for c in chunks]}
        with open(out, "a") as f:
            f.write(f"matrix={json.dumps(matrix)}\nchunks={len(chunks)}\n")


if __name__ == "__main__":
    main()
