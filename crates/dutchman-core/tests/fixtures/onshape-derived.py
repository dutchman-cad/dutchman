#!/usr/bin/env python3
"""Synthetic two-studio Derived + cut bundle. No account or source CAD files.

Only this fixture is generated. Run with --cli pointing at SolidCraft 0.44.1.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile
import onshape as shapes

ROOT = Path(__file__).resolve().parent
# Repeated digits are invented identifiers, never document/account identities.
D, V, E = "1" * 24, "2" * 24, "4" * 24
MID = "synthetic-microversion"


def generate(cli):
    source_sketch = shapes.rectangle("Base sketch", .005, .002, .015, .007)
    source_extrude = shapes.extrude()
    source_extrude["name"] = "Tool"
    source_extrude["parameters"][-1]["expression"] = "4 mm"
    host = [shapes.rectangle("Base sketch"), shapes.extrude(),
            shapes.feature("importDerived", "Derived", [shapes.param(
                "partStudio", "BTMParameterReferencePartStudio", value={
                    "documentId": D, "versionId": V, "elementId": E})]),
            shapes.feature("boolean", "Cut", [shapes.enum("operationType", "SUBTRACTION"),
                shapes.query("targets", deterministicIds=["base"]),
                shapes.query("tools", deterministicIds=["inserted"]),
                shapes.param("keepTools", "BTMParameterBoolean", value=False)])]
    files = {}
    def studio(folder, eid, features, states, step):
        prefix = f"{folder}partstudios/{eid}"
        files[f"{prefix}/features.json"] = {"sourceMicroversion": MID, "features": features}
        files[f"{prefix}/bodydetails/index.json"] = list(states)
        for i, parts in states.items():
            files[f"{prefix}/bodydetails/{i}.json"] = {"microversionId": MID, "bodies": [
                {"id": pid, "type": "SOLID", "properties": {"name": name},
                 "faces": [], "edges": [], "vertices": []} for pid, name in parts]}
        files[f"{prefix}/export.step"] = step
    def manifest(eid):
        return {"schema": "dutchman.onshape-bundle/1", "consistent": True, "root_element": eid,
                "document": {"id": D, "workspace_or_version": "v", "wvid": V, "microversion": MID},
                "elements": [{"id": eid, "type": "PARTSTUDIO"}]}
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        source = [shapes.command("solid.box", length=10, width=5, height=4,
                                 corner=[5, 2, 0], body_name="Tool", operation="new")]
        host_commands = [shapes.command("solid.box", length=20, width=10, height=2,
                                        body_name="Plate", operation="new"), *source,
                         shapes.command("solid.combine", target="Plate", tools=["Tool"], operation="cut")]
        def export(commands, name):
            script = tmp / f"{name}.json"
            script.write_text(json.dumps(commands))
            step = tmp / f"{name}.step"
            subprocess.run([cli, "run", str(script), "--out", str(step), "--quiet"], check=True)
            return step.read_bytes()
        studio("", "studio", host, {0: [], 1: [], 2: [("base", "Plate")],
               3: [("base", "Plate"), ("inserted", "Tool")], 4: [("base", "Plate")]}, export(host_commands, "host"))
        prefix = f"references/{D}/{V}/{E}/"
        studio(prefix, E, [source_sketch, source_extrude], {0: [], 1: [], 2: [("tool", "Tool")]}, export(source, "source"))
        files[f"{prefix}manifest.json"] = manifest(E)
        files["manifest.json"] = manifest("studio")
    path = ROOT / "onshape-derived.onshape-bundle"
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as bundle:
        for name, data in sorted(files.items()):
            bundle.writestr(name, data if isinstance(data, bytes) else json.dumps(data, sort_keys=True).encode())
    print(path.name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True)
    generate(parser.parse_args().cli)
