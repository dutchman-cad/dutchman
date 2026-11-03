#!/usr/bin/env python3
"""Compare a placement golden with explicitly identified Fusion display-scene matrices.

Usage: fusion_scene_placements.py source.f3z golden.json --mapping part-map.json [--out report.json]
       fusion_scene_placements.py source.f3z design.sc --mapping part-map.json \
           --relative-to ANCHOR [--path PART | --path-prefix PREFIX]
Also accepts a single .f3d. No conversion, extraction to disk, or network access occurs.
Matrix offsets identify parts independently of the golden being checked; nearest-position
matching would silently accept unrelated or stale scene entries. See corpus/placements/README.md.
Exit: 0 complete match, 1 moved parts, 2 input error, 3 no scene, 4 unmatched/not checked.
"""
import argparse
import hashlib
import gzip
import io
import json
import math
import struct
import sys
import zipfile

from placements import mul, world_transforms

SCENE = "FusionAssetName[Active]/OGS.BlobFolder/OGS/DefaultScene/world"
MAX_ROOT = 512 << 20
MAX_SCENE = 256 << 20
MAX_JSON = 16 << 20


def member(archive, name, limit):
    if len(archive.infolist()) > 100_000:
        raise ValueError("Archive has too many entries")
    info = archive.getinfo(name)
    if info.file_size > limit:
        raise ValueError("Archive member exceeds the size limit: " + name)
    with archive.open(info) as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("Archive member exceeds the size limit: " + name)
    return data


def scene_bytes(path):
    with zipfile.ZipFile(path) as archive:
        if "Manifest.json" in archive.namelist():
            manifest = json.loads(member(archive, "Manifest.json", MAX_JSON))
            root = member(archive, manifest["root"], MAX_ROOT)
            with zipfile.ZipFile(io.BytesIO(root)) as design:
                return member(design, SCENE, MAX_SCENE) if SCENE in design.namelist() else None
        return member(archive, SCENE, MAX_SCENE) if SCENE in archive.namelist() else None


def read_json(path):
    opener = gzip.open if str(path).endswith(".gz") else open
    with opener(path, "rb") as stream:
        data = stream.read(MAX_JSON + 1)
    if len(data) > MAX_JSON:
        raise ValueError("JSON input exceeds the size limit")
    return json.loads(data)


def digest(path):
    result = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            result.update(chunk)
    return result.hexdigest()


def scene_matrix(data, offset):
    if type(offset) is not int or offset < 0 or offset + 64 > len(data):
        raise ValueError("Scene matrix offset is outside the scene")
    matrix = struct.unpack_from("<16f", data, offset)
    if not all(math.isfinite(value) for value in matrix):
        raise ValueError("Scene matrix contains non-finite values")
    if any(abs(matrix[i] - expected) > 1e-6 for i, expected in [(3, 0), (7, 0), (11, 0), (15, 1)]):
        raise ValueError("Scene matrix is not affine")
    axes = [matrix[i:i + 3] for i in (0, 4, 8)]
    for i, first in enumerate(axes):
        for j, second in enumerate(axes):
            if abs(sum(a * b for a, b in zip(first, second)) - (i == j)) > 1e-3:
                raise ValueError("Scene matrix axes are not orthonormal")
    # Column-major float32, centimetres in the source scene, millimetres in goldens.
    result = [list(matrix[i:i + 4]) for i in range(0, 16, 4)]
    result[3][:3] = [value * 10 for value in matrix[12:15]]
    return result


def translation(data, offset):
    return scene_matrix(data, offset)[3][:3]


def rigid(matrix):
    """Remove float32 axis rounding before rigid inversion and angle measurement."""
    if len(matrix) != 4 or any(len(row) != 4 for row in matrix):
        raise ValueError("Expected a 4 by 4 transform")
    if not all(math.isfinite(value) for row in matrix for value in row):
        raise ValueError("Transform contains non-finite values")
    if any(abs(matrix[i][3] - (i == 3)) > 1e-6 for i in range(4)):
        raise ValueError("Transform is not affine")
    axes = [row[:3] for row in matrix[:3]]
    for i, first in enumerate(axes):
        for j, second in enumerate(axes):
            if abs(sum(a * b for a, b in zip(first, second)) - (i == j)) > 1e-3:
                raise ValueError("Transform axes are not orthonormal")
    x = [v / math.hypot(*axes[0]) for v in axes[0]]
    dot = sum(a * b for a, b in zip(x, axes[1]))
    y = [b - dot * a for a, b in zip(x, axes[1])]
    y = [v / math.hypot(*y) for v in y]
    z = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]]
    if sum(a * b for a, b in zip(z, axes[2])) < 0:
        raise ValueError("Reflected frames do not define a rotation")
    return [x + [0], y + [0], z + [0], matrix[3][:3] + [1]]


def inverse(matrix):
    """Inverse of an orthonormal row-vector transform."""
    result = [[matrix[j][i] for j in range(3)] + [0] for i in range(3)]
    result.append([-sum(matrix[3][j] * result[j][i] for j in range(3)) for i in range(3)] + [1])
    return result


def pose_error(actual, expected):
    delta = mul(actual, inverse(expected))
    sine = math.hypot(*(delta[i][j] - delta[j][i] for i, j in [(2, 1), (0, 2), (1, 0)])) / 2
    cosine = (sum(delta[i][i] for i in range(3)) - 1) / 2
    return math.dist(actual[3][:3], expected[3][:3]), math.atan2(sine, cosine)


def checked_scene(source, mapping):
    data = scene_bytes(source)
    if data is None:
        return None, ({"status": "no scene: not checked", "placements": []}, 3)
    if mapping is None:
        return None, ({"status": "no part mapping: not checked", "placements": []}, 4)
    if digest(source) != mapping["source_sha256"] or hashlib.sha256(data).hexdigest() != mapping["scene_sha256"]:
        raise ValueError("Part mapping does not identify this source and scene")
    return data, None


def compare_relative(source, worlds, mapping, anchor, paths=None, prefix=None, tolerance=0.1, angle_tolerance=0.001):
    """Compare full part-to-anchor poses, cancelling shared assembly movement."""
    data, unchecked = checked_scene(source, mapping)
    if unchecked:
        return unchecked
    if len(worlds) > 100_000:
        raise ValueError("Expected at most 100000 placements")

    def scene_pose(path):
        entry = mapping["entries"][path]
        matrices = [scene_matrix(data, offset) for offset in entry["offsets"]]
        if not matrices or any(matrix != matrices[0] for matrix in matrices):
            raise ValueError("Mapping must identify one unambiguous scene transform: " + path)
        return rigid(matrices[0])

    if anchor not in worlds or anchor not in mapping["entries"]:
        return {"status": "relative anchor unmatched: not checked", "anchor": anchor, "placements": []}, 4
    actual_inverse = inverse(rigid(worlds[anchor]))
    scene_inverse = inverse(scene_pose(anchor))
    selected = sorted(set(paths) if paths is not None else worlds)
    if prefix is not None:
        selected = [path for path in selected if path.startswith(prefix)]
    rows = []
    for path in selected:
        if path not in worlds or path not in mapping["entries"]:
            rows.append({"path": path, "status": "unmatched", "reason": "No converted transform or independent scene entry"})
            continue
        actual = mul(rigid(worlds[path]), actual_inverse)
        expected = mul(scene_pose(path), scene_inverse)
        distance, angle = pose_error(actual, expected)
        rows.append({"path": path, "status": "moved" if distance > tolerance or angle > angle_tolerance else "matched",
                     "distance_mm": distance, "angle_rad": angle,
                     "actual_relative": actual, "scene_relative": expected})
    compared = [row for row in rows if "distance_mm" in row]
    moved = sum(row["status"] == "moved" for row in rows)
    unmatched = len(rows) - len(compared)
    code = 1 if moved else 4 if unmatched or not rows else 0
    return {"status": "moved parts" if moved else "unmatched parts: not fully checked" if code == 4 else "match",
            "source": str(source), "scene_member": SCENE, "anchor": anchor,
            "tolerance_mm": tolerance, "tolerance_rad": angle_tolerance,
            "total": len(rows), "compared": len(compared), "over_tolerance": moved, "unmatched": unmatched,
            "max_distance_mm": max((row["distance_mm"] for row in compared), default=None),
            "max_angle_rad": max((row["angle_rad"] for row in compared), default=None), "placements": rows}, code


def compare(source, golden, mapping, tolerance=0.1):
    data, unchecked = checked_scene(source, mapping)
    if unchecked:
        return unchecked
    if golden["unit"] != "mm" or len(golden["placements"]) > 100_000:
        raise ValueError("Expected at most 100000 placements in millimetres")
    rows = []
    for path, actual in sorted(golden["placements"].items()):
        entry = mapping["entries"].get(path)
        if entry is None:
            reason = mapping.get("unmatched", {}).get(path, "No independent scene entry assigned")
            rows.append({"path": path, "status": "unmatched", "actual_mm": actual, "reason": reason})
            continue
        positions = [translation(data, offset) for offset in entry["offsets"]]
        if not positions or any(position != positions[0] for position in positions):
            raise ValueError("Mapping must identify one unambiguous scene translation: " + path)
        if len(actual) != 3 or not all(math.isfinite(value) for value in actual):
            raise ValueError("Expected three finite placement coordinates: " + path)
        distance = math.dist(actual, positions[0])
        rows.append({"path": path, "status": "moved" if distance > tolerance else "matched",
                     "actual_mm": actual, "scene_mm": positions[0], "distance_mm": distance,
                     "scene_offsets": entry["offsets"],
                     "source_occurrence": entry.get("source_occurrence"), "reason": entry.get("reason")})
    compared = [row for row in rows if "distance_mm" in row]
    moved = sum(row["status"] == "moved" for row in rows)
    unmatched = len(rows) - len(compared)
    code = 1 if moved else 4 if unmatched else 0
    return {"status": "moved parts" if moved else "unmatched parts: not fully checked" if unmatched else "match",
            "source": str(source), "scene_member": SCENE, "tolerance_mm": tolerance,
            "total": len(rows), "compared": len(compared), "over_tolerance": moved, "unmatched": unmatched,
            "max_distance_mm": max((row["distance_mm"] for row in compared), default=None), "placements": rows}, code


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("source")
    parser.add_argument("placements", help="Placement golden JSON, or a SolidCraft design for relative checks")
    parser.add_argument("--mapping", help="Independent part-to-scene offsets, bound to source and scene SHA-256")
    parser.add_argument("--relative-to", metavar="ANCHOR", help="Compare full transforms relative to this occurrence path; requires a design")
    parser.add_argument("--path", action="append", help="Check this path (repeatable); requires --relative-to")
    parser.add_argument("--path-prefix", help="Check paths starting with this prefix; requires --relative-to")
    parser.add_argument("--out", help="Write every placement result as JSON")
    args = parser.parse_args(argv)
    try:
        mapping = read_json(args.mapping) if args.mapping else None
        if args.relative_to:
            with zipfile.ZipFile(args.placements) as archive:
                document = json.loads(member(archive, "document.json", MAX_ROOT))
            if document.get("units", "mm") != "mm":
                raise ValueError("Expected a design in millimetres")
            report, code = compare_relative(args.source, world_transforms(document), mapping,
                                            args.relative_to, args.path, args.path_prefix)
        else:
            if args.path or args.path_prefix:
                raise ValueError("Path selection requires --relative-to")
            report, code = compare(args.source, read_json(args.placements), mapping)
        if args.out:
            with open(args.out, "w", encoding="utf-8") as stream:
                json.dump(report, stream, indent=2, ensure_ascii=False)
                stream.write("\n")
        print(report["status"])
        if "total" in report:
            print("placements: {total}; compared: {compared}; over tolerance: {over_tolerance}; unmatched: {unmatched}".format(**report))
            print("maximum compared translation error: %s mm" % report["max_distance_mm"])
            if "anchor" in report:
                print("relative to: %s; maximum rotation error: %s rad" % (report["anchor"], report["max_angle_rad"]))
            for row in report["placements"]:
                if row["status"] == "moved":
                    rotation = "; %.9g rad" % row["angle_rad"] if "angle_rad" in row else ""
                    print("  %.6f mm%s  %s" % (row["distance_mm"], rotation, row["path"]))
                elif row["status"] == "unmatched":
                    print("  unmatched  %s: %s" % (row["path"], row["reason"]))
        return code
    except (OSError, ValueError, KeyError, TypeError, AttributeError, zipfile.BadZipFile, struct.error, OverflowError, RuntimeError) as error:
        print("Scene check failed: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
