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
local_code = r'([a-z][a-z0-9_-]*\.[a-z0-9_.-]+)'
constructor = r"(?:NamespacedLossKind::new(?:_owned)?|LossKind::namespaced)"
# Loss vocabularies can live anywhere, including shared IR helpers. Read public
# definitions and constructors, not filenames or dotted strings in comments.
for path in (source / "crates").rglob("*.rs"):
    if path.name == "tests.rs" or any(
        part in {"tests", "integration_tests", "test_support"} for part in path.parts
    ):
        continue
    contents = re.split(r"#\[cfg\(test\)\]\s*mod ", path.read_text())[0]
    contents = re.sub(r"//[^\n]*", "", contents)
    if re.search(constructor + r"\s*\(", contents):
        # Enum code()/spec() matches and loss_codes! vocabulary declarations.
        codes.update(re.findall(
            r'=>\s*(?:\{\s*)?(?:\(\s*)?"' + local_code + r'"', contents
        ))
        # Literal kinds emitted directly, such as unknown/part.rs's shared helper.
        codes.update(re.findall(
            constructor + r'\s*\((?:[^,"]|"[^"]*")*,\s*"' + local_code + r'"',
            contents
        ))
taxonomy = (source / "crates/cadmpeg-ir/src/report/loss.rs").read_text()
method = taxonomy.split("pub const fn as_str(self)")[1].split("pub const fn category")[0]
codes.update(re.findall(r'Self::\w+ => "([a-z_]+)"', method))
pathlib.Path(__file__).with_name("loss-codes.txt").write_text(
    f"# Public cadmpeg loss identifiers at {pin}; no design data.\n"
    + "\n".join(sorted(codes)) + "\n"
)
