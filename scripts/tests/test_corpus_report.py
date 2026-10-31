import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import corpus_report  # noqa: E402


def write(directory, meta, report=None, err=""):
    stem = os.path.join(directory, meta["id"])
    with open(stem + ".meta.json", "w") as f:
        json.dump(meta, f)
    if report is not None:
        with open(stem + ".json", "w") as f:
            json.dump(report, f)
    with open(stem + ".err", "w") as f:
        f.write(err)


def meta(id, source, path, status, **extra):
    return {"id": id, "source": source, "path": path, "ext": path.rsplit(".", 1)[1], "size": 1000,
            "status": status, "exit_code": 0 if status == "converted" else 1, "wall_s": 12.0,
            "peak_rss_kb": 2097152, **extra}


class ReportTest(unittest.TestCase):
    def build(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        d = tmp.name
        a, b = "owner/alpha", "owner/beta"
        write(d, meta("1-a", a, "cad/a.f3d", "converted"),
              {"ok": True, "mode": "history", "verification": {"status": "unverified"},
               "history": {"mapped": 3, "total": 3, "features": []}})
        write(d, meta("2-b", a, "cad/b.iam", "converted"),
              {"ok": True, "mode": "partial_history",
               "verification": {"status": "partial", "reason": "history stopped at feature Loft 3: lofts are not mapped"},
               "completeness": {"placed": 8, "placements": 10}})
        write(d, meta("3-c", b, "c.f3d", "failed"),
              {"ok": False, "error": {"message": "unreadable record 12 in stream 'BREP'", "stage": "decode", "upstream": "cadmpeg"}})
        write(d, meta("4-d", b, "d.f3d", "failed"),
              {"ok": False, "error": {"message": "unreadable record 99 in stream 'XYZ'", "stage": "decode", "upstream": "cadmpeg"}})
        write(d, meta("5-e", b, "e.f3z", "needs_memory", exit_code=2, peak_rss_kb=None),
              {"ok": False, "error": {"message": "this conversion needs about 20.0 GB and 8.0 GB are free", "stage": "input", "upstream": None}})
        return d

    def test_rows_and_classes(self):
        rows = corpus_report.collect(self.build())
        self.assertEqual(len(rows), 5)
        by_id = {r["id"]: r for r in rows}
        self.assertEqual(by_id["1-a"]["history"], "full")
        self.assertEqual(by_id["2-b"]["history"], "partial")
        self.assertEqual(by_id["3-c"]["owner"], "cadmpeg")
        self.assertEqual(by_id["5-e"]["status"], "needs_memory")

    def test_report_sections(self):
        rows = corpus_report.collect(self.build())
        text = corpus_report.build_report(rows, {"sources": [{"repo": "owner/gone", "status": "skipped", "reason": "licence is MIT, listed as GPL-3.0"}]})
        self.assertIn("5 files**: 2 converted, 2 failed", text)
        self.assertIn("1 needs more memory (runner)", text)
        self.assertIn("owner/alpha", text)
        self.assertIn("owner/beta", text)
        self.assertIn("8 / 10", text)
        self.assertIn("1 / 1 / 0", text)
        # The two cadmpeg failures differ only in numbers and names: one cause, count 2.
        self.assertIn("| 2 | cadmpeg | unreadable record N in stream <name> |", text)
        self.assertIn("stopped at feature <feature>", text)
        self.assertIn("owner/gone: licence is MIT", text)

    def test_memory_is_not_a_failure(self):
        rows = corpus_report.collect(self.build())
        text = corpus_report.build_report(rows, None)
        self.assertNotIn("this conversion needs about", text.split("## Top failure causes")[1].split("##")[0])

    def test_crash_without_report(self):
        with tempfile.TemporaryDirectory() as d:
            write(d, meta("1", "o/r", "x.ipt", "failed", exit_code=139), None, "thread panicked\nSegmentation fault\n")
            rows = corpus_report.collect(d)
            self.assertEqual(rows[0]["owner"], "Dutchman")
            self.assertIn("Segmentation fault", rows[0]["error"]["message"])


if __name__ == "__main__":
    unittest.main()
