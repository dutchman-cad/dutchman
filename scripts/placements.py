#!/usr/bin/env python3
"""World placement of every occurrence of a SolidCraft design, and a check against a golden.

    placements.py design.sc [--out golden.json]     write (or print) the placements
    placements.py design.sc --check golden.json      fail when a part moved, vanished or appeared

Transforms are row-vector 4x4 matrices with the translation in the last row. An occurrence's
world transform is its own transform followed by its holder's, up to the root component (0).
Paths join occurrence names with "/"; a name repeated among siblings gets "#2", "#3", ... in
document order, so a path names the same part from run to run. Positions are in the design's
unit (millimetres), rounded to 0.001.

The "Editable history" stand-ins the converter adds beside a part's shape (whether a part keeps
its history depends on the preset and the memory free) are not parts of the design and are left
out, with whatever they hold.
"""
import argparse
import gzip
import json
import sys
import zipfile

TOLERANCE_MM = 0.01
STAND_IN_PREFIX = "Editable history"
IDENTITY = [[1.0, 0, 0, 0], [0, 1.0, 0, 0], [0, 0, 1.0, 0], [0, 0, 0, 1.0]]


def load_document(path):
    with zipfile.ZipFile(path) as z:
        return json.loads(z.read("document.json"))


def mul(a, b):
    """Row-vector product: apply a, then b."""
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def placements(doc):
    """Map of occurrence path -> world origin [x, y, z], rounded to 0.001."""
    children = {}
    for occ in doc["occurrences"]:
        children.setdefault(occ["parent"], []).append(occ)
    out = {}
    stack = [(0, IDENTITY, "", 0)]
    while stack:
        comp, world, prefix, depth = stack.pop()
        if depth > 256:
            raise ValueError("occurrences nest deeper than 256 levels (a cycle?)")
        seen = {}
        for occ in children.get(comp, []):
            name = str(occ["name"]).replace("/", "∕")
            seen[name] = seen.get(name, 0) + 1
            if seen[name] > 1:
                name = "%s#%d" % (name, seen[name])
            if name.startswith(STAND_IN_PREFIX):
                continue
            w = mul(occ["transform"], world)
            path = prefix + "/" + name if prefix else name
            out[path] = [round(v, 3) + 0.0 for v in w[3][:3]]
            stack.append((occ["component"], w, path, depth + 1))
    return dict(sorted(out.items()))


def golden_of(doc):
    p = placements(doc)
    return {"count": len(p), "unit": doc.get("units", "mm"), "placements": p}


def dump(golden, f):
    """One placement per line, so a changed golden reads as a diff."""
    f.write('{"count":%d,"unit":%s,"placements":{\n' % (golden["count"], json.dumps(golden["unit"])))
    items = ['%s:%s' % (json.dumps(k, ensure_ascii=False), json.dumps(v)) for k, v in golden["placements"].items()]
    f.write(",\n".join(items))
    f.write("\n}}\n")


def read_golden(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return json.load(f)


def compare(golden, current, tolerance=TOLERANCE_MM):
    """(moved, missing, extra): moved is a list of (distance, path, old, new), worst first."""
    old, new = golden["placements"], current["placements"]
    moved = []
    for path in old.keys() & new.keys():
        d = sum((a - b) ** 2 for a, b in zip(old[path], new[path])) ** 0.5
        if d > tolerance:
            moved.append((d, path, old[path], new[path]))
    moved.sort(key=lambda m: (-m[0], m[1]))
    return moved, sorted(old.keys() - new.keys()), sorted(new.keys() - old.keys())


def report(golden, current, tolerance=TOLERANCE_MM, worst=20):
    moved, missing, extra = compare(golden, current, tolerance)
    shared = len(golden["placements"].keys() & current["placements"].keys())
    lines = ["occurrences: golden %d, now %d, shared %d" % (golden["count"], current["count"], shared),
             "moved more than %g mm: %d, disappeared: %d, appeared: %d"
             % (tolerance, len(moved), len(missing), len(extra))]
    for d, path, a, b in moved[:worst]:
        lines.append("  moved %.3f mm  %s  %s -> %s" % (d, path, a, b))
    if len(moved) > worst:
        lines.append("  ... and %d more moved" % (len(moved) - worst))
    for label, paths in (("disappeared", missing), ("appeared", extra)):
        for path in paths[:worst]:
            lines.append("  %s  %s" % (label, path))
        if len(paths) > worst:
            lines.append("  ... and %d more %s" % (len(paths) - worst, label))
    return bool(moved or missing or extra), "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("design")
    ap.add_argument("--out", help="write the placements here (.gz is compressed)")
    ap.add_argument("--check", metavar="GOLDEN", help="compare with this golden; exit 1 on a difference")
    ap.add_argument("--tolerance", type=float, default=TOLERANCE_MM)
    args = ap.parse_args(argv)
    current = golden_of(load_document(args.design))
    if args.out:
        opener = gzip.open if args.out.endswith(".gz") else open
        with opener(args.out, "wt") as f:
            dump(current, f)
        print("wrote %d placements to %s" % (current["count"], args.out))
    if args.check:
        failed, text = report(read_golden(args.check), current, args.tolerance)
        print(text)
        if failed:
            print("FAIL: placements differ from %s" % args.check)
            return 1
        print("ok: placements match %s" % args.check)
    if not args.out and not args.check:
        json.dump(current, sys.stdout, indent=0, separators=(",", ":"))
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
