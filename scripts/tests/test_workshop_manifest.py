import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class WorkshopManifest(unittest.TestCase):
    """solidcraft-workshop.json is what workshop.solidcraft.io reads when the plugin is listed."""

    def setUp(self):
        self.listing = json.loads((ROOT / "solidcraft-workshop.json").read_text())
        self.plugin = json.loads((ROOT / "crates/dutchman-plugin/plugin.json").read_text())
        cargo = (ROOT / "Cargo.toml").read_text()
        self.version = re.search(r'^version = "([^"]+)"', cargo, re.MULTILINE).group(1)

    def test_version_follows_the_workspace(self):
        self.assertEqual(self.listing["version"], self.version)

    def test_minimum_version_matches_the_plugin(self):
        self.assertEqual(self.listing["min_app_version"], self.plugin["min_host_version"])

    def test_package_is_the_release_zip(self):
        packages = [f for f in self.listing["files"] if f["role"] == "package"]
        self.assertEqual([f["asset"] for f in packages], [f"dutchman-plugin-{self.version}.zip"])

    def test_workshop_field_rules(self):
        self.assertEqual(self.listing["schema"], 1)
        self.assertEqual(self.listing["kind"], "plugin")
        self.assertEqual(self.listing["id"], self.plugin["id"])
        self.assertTrue(4 <= len(self.listing["summary"]) <= 160)
        self.assertLessEqual(len(self.listing["tags"]), 10)
        self.assertNotIn("network", " ".join(self.listing["permissions"]))
        self.assertTrue((ROOT / self.listing["description"]).is_file())
        for f in self.listing["files"]:
            if "path" in f:
                self.assertTrue((ROOT / f["path"]).is_file(), f["path"])


if __name__ == "__main__":
    unittest.main()
