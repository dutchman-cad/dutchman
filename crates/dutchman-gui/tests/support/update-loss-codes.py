"""Refresh the title-coverage inventory from a public checkout at Cargo.toml's cadmpeg pin.

Usage: python3 crates/dutchman-gui/tests/support/update-loss-codes.py /path/to/cadmpeg
Only loss identifiers are copied. This reads no design files.
"""
import pathlib
import re
import subprocess
import sys

repo = pathlib.Path(__file__).resolve().parents[4]
manifest = (repo / "Cargo.toml").read_text()
pin = re.search(r'cadmpeg-registry.*rev = "([0-9a-f]+)"', manifest)[1]
source = pathlib.Path(sys.argv[1])
actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
if actual != pin:
    raise SystemExit(f"Use the public cadmpeg checkout at {pin}; found {actual}")
codes = set()
for path in (source / "crates").rglob("loss.rs"):
    for code in re.findall(r'"([a-z][a-z0-9_-]*\.[a-z0-9_.-]+)"', path.read_text()):
        if code not in {"crate.value-not-read", "crate.tests", "loss.rs"}:
            codes.add(code)
taxonomy = (source / "crates/cadmpeg-ir/src/report/loss.rs").read_text()
method = taxonomy.split("pub const fn as_str(self)")[1].split("pub const fn category")[0]
codes.update(re.findall(r'Self::\w+ => "([a-z_]+)"', method))
pathlib.Path(__file__).with_name("loss-codes.txt").write_text(
    f"# Public cadmpeg loss identifiers at {pin}; no design data.\n"
    + "\n".join(sorted(codes)) + "\n"
)
