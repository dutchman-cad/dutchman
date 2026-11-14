#!/usr/bin/env python3
"""Synthetic bundle v1 fixtures, built from the public OpenAPI response shapes.

No Onshape account, API requests, vendor files or private designs are used. STEP
references are built from the same elementary geometry with solidcraft-cli.
Run: python3 onshape.py [--cli /path/to/solidcraft-cli]
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent
PREFIX = "partstudios/studio"


def param(key, kind, **values):
    return {"btType": kind, "parameterId": key, **values}


def enum(key, value):
    return param(key, "BTMParameterEnum-145", value=value)


def quantity(key, expression):
    return param(key, "BTMParameterQuantity-147", expression=expression)


def query(key, **values):
    return param(key, "BTMParameterQueryList-148", queries=[values])


def sketch_query(key, sketch):
    return query(key, btType="BTMIndividualSketchRegionQuery-140", featureId=sketch)


def feature(kind, fid, parameters, **values):
    return {"btType": "BTMFeature-134", "featureType": kind, "featureId": fid,
            "name": fid, "parameters": parameters, **values}


def rectangle(fid, x0=0, y0=0, x1=.02, y1=.01):
    corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    entities = []
    for i, (a, b) in enumerate(zip(corners, corners[1:] + corners[:1])):
        # Parametric line is origin + t*direction, t in metres.
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = (dx*dx + dy*dy)**.5
        entities.append({"btType": "BTMSketchCurveSegment-155", "entityId": f"line{i}",
                         "startPointId": f"line{i}.start", "endPointId": f"line{i}.end",
                         "startParam": 0, "endParam": length,
                         "geometry": {"btType": "BTCurveGeometryLine-117", "pntX": a[0], "pntY": a[1],
                                      "dirX": dx/length, "dirY": dy/length}})
    return feature("newSketch", fid, [query("sketchPlane", btType="BTMIndividualCreatedByQuery-137", featureId="Top")],
                   btType="BTMSketch-151", entities=entities, constraints=[
                       {"btType": "BTMSketchConstraint-2", "entityId": "horizontal", "constraintType": "HORIZONTAL",
                        "parameters": [param("localFirst", "BTMParameterString-149", value="line0")]},
                       {"btType": "BTMSketchConstraint-2", "entityId": "length", "constraintType": "LENGTH",
                        "parameters": [param("localFirst", "BTMParameterString-149", value="line0"), quantity("length", f"{(x1-x0)*1000} mm")]},
                   ])


def extrude(negative=False):
    parameters = [enum("bodyType", "SOLID"), enum("operationType", "NEW"), sketch_query("entities", "Base sketch"),
                  enum("endBound", "BLIND"), quantity("depth", "2 mm")]
    if negative:
        parameters.append(param("oppositeDirection", "BTMParameterBoolean-144", value=True))
    return feature("extrude", "Plate", parameters)


def vec(x, y, z):
    return {"x": x, "y": y, "z": z}


def edge(eid, a, b):
    direction = [q-p for p, q in zip(a, b)]
    length = sum(v*v for v in direction)**.5
    direction = [v/length for v in direction]
    return {"btType": "BTExportModelEdge-1782", "id": eid, "vertices": [],
            "curve": {"btType": "BTLineDescription-1559", "type": "LINE", "origin": vec(*a), "direction": vec(*direction)},
            "geometry": {"btType": "BTExportModelEdgeGeometry-1125", "startPoint": vec(*a), "endPoint": vec(*b),
                         "midPoint": vec(*[(p+q)/2 for p, q in zip(a, b)]), "startVector": vec(*direction),
                         "endVector": vec(*direction), "length": length}}


def bodydetails(edges):
    return {"btType": "BTExportModelBodiesResponse-734", "microversionId": {"theId": "synthetic-microversion"},
            "bodies": [{"btType": "BTExportModelBody-1272", "id": "body", "type": "SOLID", "edges": edges, "faces": [], "vertices": []}]}


def command(command_id, **params):
    return {"command": command_id, "params": params}


def plate_commands(negative=False):
    return [command("sketch.create", plane="XY", name="Base sketch"),
            command("sketch.line", points=[[0, 0], [20, 0], [20, 10], [0, 10]], closed=True),
            command("sketch.finish"), command("solid.extrude", distance="2 mm", direction="negative" if negative else "positive", operation="new", name="Plate")]


def write_bundle(cli, name, features, commands, details=None):
    details = details or {}
    with tempfile.TemporaryDirectory(prefix="onshape-fixture-") as folder:
        script = Path(folder) / "commands.json"
        step = Path(folder) / "export.step"
        script.write_text(json.dumps(commands))
        subprocess.run([cli, "run", str(script), "--out", str(step), "--quiet"], check=True)
        manifest = {"schema": "dutchman.onshape-bundle/1", "fetched_at": "2026-11-11T00:00:00Z", "fetcher": "synthetic fixture generator",
                    "api_version": "v1", "document": {"id": "synthetic-document", "name": name, "workspace_or_version": "w", "wvid": "synthetic-workspace", "microversion": "synthetic-microversion"},
                    "elements": [{"id": "studio", "name": name, "type": "PARTSTUDIO", "files": {"features": f"{PREFIX}/features.json", "step": f"{PREFIX}/export.step", "bodydetails": f"{PREFIX}/bodydetails/index.json"}}], "calls": 0}
        members = {"manifest.json": manifest, f"{PREFIX}/features.json": {"btType": "BTFeatureListResponse-2457", "features": features, "defaultFeatures": [], "sourceMicroversion": "synthetic-microversion"},
                   f"{PREFIX}/bodydetails/index.json": sorted(details)}
        members.update({f"{PREFIX}/bodydetails/{i}.json": response for i, response in details.items()})
        with zipfile.ZipFile(ROOT / f"{name}.onshape-bundle", "w", zipfile.ZIP_DEFLATED) as bundle:
            for path, value in members.items():
                info = zipfile.ZipInfo(path, (2026, 11, 11, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                bundle.writestr(info, json.dumps(value, sort_keys=True))
            info = zipfile.ZipInfo(f"{PREFIX}/export.step", (2026, 11, 11, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            bundle.writestr(info, step.read_bytes())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", default="solidcraft-cli")
    args = parser.parse_args()
    fillet = feature("fillet", "Corner round", [query("entities", btType="BTMIndividualQuery-138", deterministicIds=["upright"]), quantity("radius", "1 mm"), enum("crossSection", "CIRCULAR")])
    write_bundle(args.cli, "onshape-plate-fillet", [rectangle("Base sketch"), extrude(), fillet],
                 plate_commands() + [command("solid.fillet", edges=[[0, 0, 1]], radius="1 mm", tangent_chain=False, name="Corner round")],
                 {2: bodydetails([edge("upright", [0, 0, 0], [0, 0, .002])])})
    revolve = feature("revolve", "Ring", [enum("bodyType", "SOLID"), enum("operationType", "NEW"), sketch_query("entities", "Ring sketch"),
                                         query("axis", btType="BTMIndividualQuery-138", deterministicIds=["axis"]), enum("revolveType", "FULL")])
    write_bundle(args.cli, "onshape-revolve", [rectangle("Ring sketch", .005, 0, .01, .01), revolve],
                 [command("sketch.create", plane="XY", name="Ring sketch"), command("sketch.line", points=[[5, 0], [10, 0], [10, 10], [5, 10]], closed=True),
                  command("sketch.finish"), command("solid.revolve", axis="Y", angle="360 deg", operation="new", name="Ring")],
                 {1: bodydetails([edge("axis", [0, 0, 0], [0, .01, 0])])})
    points = feature("newSketch", "Hole locations", [query("sketchPlane", btType="BTMIndividualCreatedByQuery-137", featureId="Top")],
                     btType="BTMSketch-151", entities=[{"btType": "BTMSketchPoint-158", "entityId": "point", "x": .005, "y": .005}], constraints=[])
    hole = feature("hole", "Bore", [query("entities", btType="BTMIndividualSketchUniqueVerticesQuery-1472", featureId="Hole locations"), enum("style", "SIMPLE"), enum("endStyle", "THROUGH"), quantity("holeDiameter", "2 mm")])
    write_bundle(args.cli, "onshape-hole", [rectangle("Base sketch"), extrude(True), points, hole],
                 plate_commands(True) + [command("solid.hole", position=[5, 5, 0], diameter="2 mm", type="simple", name="Bore")])
    write_bundle(args.cli, "onshape-unsupported", [rectangle("Base sketch"), extrude(), feature("fixtureCustom", "Custom feature", [], namespace="synthetic")], plate_commands())


if __name__ == "__main__":
    main()
