"""Synthetic release metadata; no network or SolidCraft installation."""
import importlib.util
from pathlib import Path
import unittest
import hashlib
import io
import json
import os
import subprocess
import tarfile
import tempfile

spec = importlib.util.spec_from_file_location(
    "solidcraft_release", Path(__file__).parents[1] / "solidcraft-release.py"
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class Releases(unittest.TestCase):
    def test_selects_highest_patch_not_api_order_or_another_minor(self):
        tags = ["v0.27.1", "v0.28.0", "v0.27.9", "v0.27.0", "v0.27.10", "v0.27.11-rc1"]
        rows = [{"tag_name": tag} for tag in tags]
        rows += [{"tag_name": "v0.27.12", "draft": True},
                 {"tag_name": "v0.27.13", "prerelease": True}]
        self.assertEqual(release.newest("0.27", rows), "0.27.10")

    def test_missing_minor_fails(self):
        with self.assertRaises(ValueError):
            release.newest("0.27", [{"tag_name": "v0.26.9"}])
        with self.assertRaises(ValueError):
            release.newest("0.27.0", [])

    def test_checksum_requires_one_exact_archive(self):
        digest = "a" * 64
        self.assertEqual(release.checksum("archive", f"{digest} *archive\n"), digest)
        for contents in [f"{digest} other", f"{digest} archive\n{digest} archive", "bad archive"]:
            with self.assertRaises(ValueError):
                release.checksum("archive", contents)

    def test_fetch_resolves_and_verifies_before_extracting(self):
        script = Path(__file__).resolve().parents[1] / "solidcraft-window.sh"
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            fixtures = work / "fixtures"
            fixtures.mkdir()
            archive = fixtures / "solidcraft-0.27.9-linux-x86_64.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                entry = tarfile.TarInfo("verified-marker")
                entry.size = 2
                output.addfile(entry, io.BytesIO(b"ok"))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            (fixtures / "SHA256SUMS.txt").write_text(f"{digest}  {archive.name}\n")
            (fixtures / "releases.json").write_text(json.dumps([
                {"tag_name": tag} for tag in ["v0.27.1", "v0.28.0", "v0.27.9"]
            ]))
            curl = work / "curl"
            curl.write_text("""#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(a for a in args if a.startswith("https://"))
name = "releases.json" if "api.github.com" in url else url.rsplit("/", 1)[1]
shutil.copyfile(pathlib.Path(os.environ["RELEASE_FIXTURES"]) / name, args[args.index("-o") + 1])
""")
            curl.chmod(0o755)
            env = dict(os.environ, PATH=f"{work}:{os.environ['PATH']}",
                       RELEASE_FIXTURES=str(fixtures), GITHUB_TOKEN="synthetic-token")
            command = ["bash", str(script), "--fetch", "0.27", ""]
            result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "0.27.9")
            marker = work / "verified-marker"
            self.assertEqual(marker.read_text(), "ok")
            marker.unlink()
            matrix = subprocess.run(["bash", str(script), "0.27.9", "0.27"],
                                    cwd=work, env=env, capture_output=True, text=True)
            self.assertEqual(matrix.returncode, 0, matrix.stderr)
            self.assertIn("NEWEST_SOLIDCRAFT: [u64; 2] = [0, 27]", matrix.stdout)
            self.assertIn('- version: "0.27"', matrix.stdout)
            archive.write_bytes(b"wrong checksum")
            result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()
