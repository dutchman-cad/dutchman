import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import placements  # noqa: E402


def shift(x=0.0, y=0.0, z=0.0):
    return [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [x, y, z, 1]]


def occ(id, name, parent, component, transform):
    return {"id": id, "name": name, "parent": parent, "component": component, "transform": transform}


def doc(second_x=30.0):
    # root(0) holds "Frame"(component 1) at x=10; Frame holds two "Bolt"s (component 2) and
    # "Plate" (component 3, rotated 90 degrees about z) whose child "Pin" sits at x=5 in it.
    rot = [[0, 1, 0, 0], [-1, 0, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]
    plate = [row[:] for row in rot]
    plate[3] = [0, 20, 0, 1]
    return {"units": "mm", "occurrences": [
        occ(1, "Frame", 0, 1, shift(10)),
        occ(2, "Bolt", 1, 2, shift(1, 2, 3)),
        occ(3, "Bolt", 1, 2, shift(second_x, 2, 3)),
        occ(4, "Plate", 1, 3, plate),
        occ(5, "Pin", 3, 4, shift(5)),
    ]}


def make_sc(directory, name, document):
    path = os.path.join(directory, name)
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("document.json", json.dumps(document))
    return path


class PlacementsTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = tmp.name

    def test_nested_composition_and_unique_names(self):
        p = placements.placements(doc())
        self.assertEqual(p["Frame"], [10.0, 0.0, 0.0])
        self.assertEqual(p["Frame/Bolt"], [11.0, 2.0, 3.0])
        self.assertEqual(p["Frame/Bolt#2"], [40.0, 2.0, 3.0])
        self.assertEqual(p["Frame/Plate"], [10.0, 20.0, 0.0])
        # Pin at x=5 in the plate's frame; the plate is turned 90 degrees (row vectors: x -> y).
        self.assertEqual(p["Frame/Plate/Pin"], [10.0, 25.0, 0.0])

    def test_history_stand_ins_are_left_out(self):
        d = doc()
        d["occurrences"].append(occ(6, "Editable history (partial)", 1, 5, shift(7)))
        d["occurrences"].append(occ(7, "Inside", 5, 6, shift(1)))
        self.assertEqual(placements.placements(d), placements.placements(doc()))

    def test_check_passes_within_tolerance_and_fails_on_a_move(self):
        golden = placements.golden_of(doc())
        near = placements.golden_of(doc(second_x=30.005))
        self.assertFalse(placements.report(golden, near)[0])
        far = placements.golden_of(doc(second_x=31.0))
        failed, text = placements.report(golden, far)
        self.assertTrue(failed)
        self.assertIn("Frame/Bolt#2", text)
        self.assertIn("moved more than 0.01 mm: 1", text)

    def test_missing_and_extra_paths_fail(self):
        golden = placements.golden_of(doc())
        smaller = doc()
        smaller["occurrences"].pop()
        failed, text = placements.report(golden, placements.golden_of(smaller))
        self.assertTrue(failed)
        self.assertIn("disappeared  Frame/Plate/Pin", text)
        failed, text = placements.report(placements.golden_of(smaller), golden)
        self.assertTrue(failed)
        self.assertIn("appeared  Frame/Plate/Pin", text)

    def test_worst_twenty_only(self):
        many = {"units": "mm", "occurrences": [occ(i, "P%d" % i, 0, 9, shift(i)) for i in range(1, 40)]}
        moved = {"units": "mm", "occurrences": [occ(i, "P%d" % i, 0, 9, shift(i + i)) for i in range(1, 40)]}
        _, text = placements.report(placements.golden_of(many), placements.golden_of(moved))
        self.assertEqual(text.count("  moved "), 20)
        self.assertIn("and 19 more moved", text)

    def test_command_line_round_trip(self):
        sc = make_sc(self.dir, "a.sc", doc())
        golden = os.path.join(self.dir, "golden.json")
        def run(*argv):
            with contextlib.redirect_stdout(io.StringIO()):
                return placements.main(list(argv))

        self.assertEqual(run(sc, "--out", golden), 0)
        self.assertEqual(run(sc, "--check", golden), 0)
        self.assertEqual(run(make_sc(self.dir, "b.sc", doc(second_x=99)), "--check", golden), 1)
        gz = golden + ".gz"
        self.assertEqual(run(sc, "--out", gz), 0)
        self.assertEqual(run(sc, "--check", gz), 0)


if __name__ == "__main__":
    unittest.main()
