"""The plugin archive carries the vector images referenced by its manifest."""
import hashlib
import json
import tomllib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


class PluginPackage(unittest.TestCase):
    def test_icon_ships_in_unversioned_and_release_archives(self):
        # Component assembly is tested separately. Isolate the packaging step with
        # a synthetic wasm-tools stand-in; no compiler or CAD files are needed.
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            core = folder / "core.wasm"
            core.write_bytes(b"\0asm\1\0\0\0")
            tool = folder / "wasm-tools"
            tool.write_text(
                "#!/usr/bin/env python3\n"
                "from pathlib import Path\n"
                "import sys\n"
                "args = sys.argv[1:]\n"
                "if args[0] == 'print': print('(module)')\n"
                "elif args[:2] == ['component', 'new']:\n"
                "    Path(args[args.index('-o') + 1]).write_bytes(b'component fixture')\n"
                "elif args[:2] == ['component', 'wit']:\n"
                "    print('  import craft:plugin/source@0.1.0;')\n"
                "else: sys.exit(2)\n"
            )
            tool.chmod(0o700)
            for version in (None, "0.3.1"):
                with self.subTest(version=version):
                    out = folder / (version or "unversioned")
                    args = ["bash", str(ROOT / "scripts/package-plugin.sh"), str(core), str(out)]
                    if version:
                        args.append(version)
                    subprocess.run(args, env={**os.environ, "WASM_TOOLS": str(tool)},
                                   check=True, capture_output=True, text=True)
                    name = f"dutchman-plugin-{version}.zip" if version else "dutchman-plugin.zip"
                    archive = out / name
                    with zipfile.ZipFile(archive) as package:
                        manifest = json.loads(package.read("plugin.json"))
                        entries = manifest["runtime"]["entries"] + manifest["runtime"]["commands"]
                        self.assertTrue(entries)
                        for entry in entries:
                            icon = entry["input_schema"]["x-solidcraft-icon"]
                            self.assertEqual(icon, "icons/dutchman.svg")
                            expected = (ROOT / "crates/dutchman-plugin" / icon).read_bytes()
                            self.assertEqual(package.read(icon), expected)
                            self.assertEqual((out / icon).read_bytes(), expected)
                        self.assertEqual(package.read("dutchman.wasm"), b"component fixture")
                        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
                        expected_version = version or workspace["workspace"]["package"]["version"]
                        self.assertEqual(manifest["version"], expected_version)
                    if version:
                        digest = (out / f"{name}.sha256").read_text().split()[0]
                        self.assertEqual(digest, hashlib.sha256(archive.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
