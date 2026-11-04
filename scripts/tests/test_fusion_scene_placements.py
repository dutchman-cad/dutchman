import contextlib
import hashlib
import io
import json
import math
import os
import struct
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import fusion_scene_placements as scene  # noqa: E402
from placements import IDENTITY, mul  # noqa: E402


def matrix(x):
    # Column-major float32; x is centimetres.
    return struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, 0, 0, 1)


def pose(x=0, angle=0):
    c, s = math.cos(angle), math.sin(angle)
    return [[c, s, 0, 0], [-s, c, 0, 0], [0, 0, 1, 0], [x, 0, 0, 1]]


def packed(pose):
    values = [value for row in pose for value in row]
    values[12:15] = [value / 10 for value in values[12:15]]
    return struct.pack("<16f", *values)


class ScenePlacementsTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = temp.name

    def source(self, data):
        inner = io.BytesIO()
        with zipfile.ZipFile(inner, "w") as archive:
            if data is not None:
                archive.writestr(scene.SCENE, data)
        path = os.path.join(self.directory, "synthetic.f3z")
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("Manifest.json", json.dumps({"root": "part.f3d"}))
            archive.writestr("part.f3d", inner.getvalue())
        return path

    def mapping(self, source, data, offset=0):
        return {"source_sha256": scene.digest(source), "scene_sha256": hashlib.sha256(data).hexdigest(),
                "entries": {"Part:1": {"offsets": [offset]}}}

    def golden(self, x=20):
        return {"count": 1, "unit": "mm", "placements": {"Part:1": [x, 0, 0]}}

    def test_column_major_centimetres_and_explicit_entry(self):
        # A stale base pose exactly matches the wrong golden. Only the independently
        # assigned second scene entry may be used; nearest-position matching is forbidden.
        data = matrix(2) + matrix(3)
        source = self.source(data)
        mapping = self.mapping(source, data, 64)
        report, code = scene.compare(source, self.golden(), mapping)
        self.assertEqual(code, 1)
        self.assertEqual(report["max_distance_mm"], 10)
        self.assertEqual(report["placements"][0]["scene_mm"], [30, 0, 0])
        self.assertEqual(scene.compare(source, self.golden(30), mapping)[1], 0)

    def test_overrides_ignore_plain_transforms_and_check_translation_and_axes(self):
        name = "OverrideTransformAttribute"
        marker = struct.pack("<I", len(name)) + name.encode("utf-16le") + bytes(5)
        data = matrix(2) + marker + packed(pose(30, 0.5)) + bytes(12)
        report, code = scene.compare_overrides(self.source(data), {
            "stored": pose(20), "current": pose(30, 0.5),
            "wrong rotation": pose(30), "outside tolerance": pose(30.11, 0.5),
            "duplicate": pose(30, 0.5),
        })
        self.assertEqual(code, 4)  # No named-occurrence binding, even for matching poses.
        self.assertEqual(report["overrides"], 1)
        self.assertEqual(report["matched"], 2)
        self.assertEqual(report["total"], 5)
        self.assertIn("identical parts could swap", report["limitation"])

    def test_missing_or_truncated_overrides_are_not_a_pass(self):
        self.assertEqual(scene.compare_overrides(self.source(None), {})[1], 3)
        self.assertEqual(scene.compare_overrides(self.source(matrix(2)), {})[1], 4)
        name = "OverrideTransformAttribute"
        data = struct.pack("<I", len(name)) + name.encode("utf-16le") + bytes(5)
        with self.assertRaises(ValueError):
            scene.overrides(data)

    def test_no_scene_is_not_a_pass_in_the_command(self):
        source = self.source(None)
        golden = os.path.join(self.directory, "golden.json")
        with open(golden, "w") as stream:
            json.dump(self.golden(), stream)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = scene.main([source, golden])
        self.assertEqual(code, 3)
        self.assertEqual(output.getvalue().strip(), "no scene: not checked")

    def test_unmatched_holders_and_missing_mapping_are_not_passes(self):
        data = matrix(2)
        source = self.source(data)
        golden = self.golden()
        golden["placements"]["Holder:1"] = [0, 0, 0]
        report, code = scene.compare(source, golden, self.mapping(source, data))
        self.assertEqual(code, 4)
        self.assertEqual(report["unmatched"], 1)
        self.assertEqual(scene.compare(source, golden, None)[1], 4)

    def test_wrong_source_hash_and_ambiguous_entries_are_rejected(self):
        data = matrix(2) + matrix(3)
        source = self.source(data)
        mapping = self.mapping(source, data)
        mapping["source_sha256"] = "different"
        with self.assertRaises(ValueError):
            scene.compare(source, self.golden(), mapping)
        mapping = self.mapping(source, data)
        mapping["entries"]["Part:1"]["offsets"] = [0, 64]
        with self.assertRaises(ValueError):
            scene.compare(source, self.golden(), mapping)

    def test_matrix_bounds_and_nonfinite_values_are_rejected(self):
        with self.assertRaises(ValueError):
            scene.translation(matrix(2), -1)
        with self.assertRaises(ValueError):
            scene.translation(matrix(2), 1)
        with self.assertRaises(ValueError):
            scene.translation(matrix(float("nan")), 0)

    def test_relative_comparison_cancels_common_translation_and_rotation(self):
        # Noncommuting rotations: scene anchor turns about x; the relative part
        # and the converted anchor turn about z.
        anchor = [[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [100, 0, 0, 1]]
        relative = pose(20, math.pi / 3)
        data = packed(anchor) + packed(mul(relative, anchor))
        source = self.source(data)
        mapping = self.mapping(source, data)
        mapping["entries"]["Anchor:1"] = {"offsets": [0]}
        mapping["entries"]["Part:1"]["offsets"] = [64]
        # Different absolute poses, identical part-to-anchor transform.
        worlds = {"Anchor:1": pose(-300, -0.5), "Part:1": mul(relative, pose(-300, -0.5))}
        report, code = scene.compare_relative(source, worlds, mapping, "Anchor:1")
        self.assertEqual(code, 0)
        self.assertEqual(report["compared"], 2)
        self.assertLess(report["max_distance_mm"], 1e-5)
        self.assertLess(report["max_angle_rad"], 1e-7)

    def test_relative_half_turn_fails_even_when_origin_matches(self):
        data = packed(IDENTITY) + packed(pose(20))
        source = self.source(data)
        mapping = self.mapping(source, data, 64)
        mapping["entries"]["Anchor:1"] = {"offsets": [0]}
        worlds = {"Anchor:1": IDENTITY, "Part:1": pose(20, math.pi)}
        report, code = scene.compare_relative(source, worlds, mapping, "Anchor:1", paths=["Part:1"])
        self.assertEqual(code, 1)
        self.assertEqual(report["total"], 1)
        self.assertEqual(report["max_distance_mm"], 0)
        self.assertAlmostEqual(report["max_angle_rad"], math.pi)

    def test_relative_check_does_not_pass_missing_empty_or_ambiguous_mapping(self):
        # Same translation but different rotation is ambiguous for a relative check.
        data = packed(pose()) + packed(pose(angle=math.pi))
        source = self.source(data)
        mapping = self.mapping(source, data)
        worlds = {"Part:1": IDENTITY, "Holder:1": IDENTITY}
        self.assertEqual(scene.compare_relative(source, worlds, mapping, "Part:1")[1], 4)
        self.assertEqual(scene.compare_relative(source, worlds, mapping, "Holder:1")[1], 4)
        self.assertEqual(scene.compare_relative(source, worlds, mapping, "Part:1", prefix="absent/")[1], 4)
        mapping["entries"]["Part:1"]["offsets"] = [0, 64]
        with self.assertRaises(ValueError):
            scene.compare_relative(source, worlds, mapping, "Part:1")

    def test_relative_command_and_absent_scene(self):
        data = matrix(2)
        source = self.source(data)
        mapping = os.path.join(self.directory, "map.json")
        with open(mapping, "w") as stream:
            json.dump(self.mapping(source, data), stream)
        design = os.path.join(self.directory, "design.sc")
        with zipfile.ZipFile(design, "w") as archive:
            archive.writestr("document.json", json.dumps({"units": "mm", "occurrences": [
                {"parent": 0, "component": 1, "name": "Part:1", "transform": pose(500)}]}))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(scene.main([source, design, "--mapping", mapping,
                                         "--relative-to", "Part:1", "--path-prefix", "Part:"]), 0)
        source = self.source(None)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = scene.main([source, design, "--relative-to", "Part:1"])
        self.assertEqual(code, 3)
        self.assertEqual(output.getvalue().strip(), "no scene: not checked")


if __name__ == "__main__":
    unittest.main()
