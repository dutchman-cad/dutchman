#!/usr/bin/env python3
"""Invented 20 x 10 x 5 mm plate in the REST qCompressed response shape.

No private identifiers, coordinates or query strings are used. The compact value
stream is authored here, including dictionary, typed-value and value references.
"""
import argparse
import base64
import importlib.util
import json
from pathlib import Path
import zlib

spec = importlib.util.spec_from_file_location("fixtures", Path(__file__).with_name("onshape.py"))
fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixtures)


class Typed:
    def __init__(self, name, value):
        self.name, self.value = name, value


def plain(value):
    if isinstance(value, Typed):
        return plain(value.value)
    if isinstance(value, dict):
        return {k: plain(v) for k, v in value.items()}
    if isinstance(value, list):
        return [plain(v) for v in value]
    return value


class Encoder:
    def __init__(self):
        self.strings, self.values = [], []

    def fragment(self, value):
        if value in self.strings and self.strings.index(value) != 0:
            return f"-{self.strings.index(value):x}", ""
        self.strings.append(value)
        return f"{len(value):x}", value

    def string(self, value):
        if "." in value:
            first, second = value.rsplit(".", 1)
            n, a = self.fragment(first)
            m, b = self.fragment(second)
            self.strings.append(value)
            return f"{n}.{m}${a}{b}"
        n, data = self.fragment(value)
        return f"{n}${data}"

    def value(self, value):
        normalized = plain(value)
        if normalized in self.values:
            return f"R{self.values.index(normalized):x}"
        if isinstance(value, Typed):
            if value.name in self.strings:
                prefix = f"C{self.strings.index(value.name):x}"
            else:
                prefix = "B" + self.string(value.name)
            return prefix + self.value(value.value)
        if isinstance(value, str):
            result = "S" + self.string(value)
        elif isinstance(value, int):
            result = f"D{value:x}"
        elif isinstance(value, list):
            result = f"A{len(value):x}" + "".join(self.value(v) for v in value)
        elif isinstance(value, dict):
            result = f"M{len(value):x}" + "".join(self.value(k) + self.value(v) for k, v in value.items())
        else:
            raise ValueError("unsupported synthetic compact value")
        self.values.append(normalized)
        return result


def query_value(entity, kind, operation, **fields):
    return Typed("Query", {**fields, "entityType": Typed("EntityType", entity),
                           "historyType": "CREATION", "operationId": Typed("Id", [operation]), "queryType": kind})


def compressed(value, deterministic, zipped=True):
    raw = "%" + Encoder().value(value)
    encoded = "&" + f"{len(raw):x}$" + base64.b64encode(zlib.compress(raw.encode())).decode() if zipped else raw
    return {"btType": "BTMIndividualQuery-138", "deterministicIds": [deterministic],
            "queryStatement": None, "queryString": "query=qCompressed(1.0," + json.dumps(encoded) + ",id);", "nodeId": ""}


def region_query(sketch, entities, deterministic):
    boundaries = []
    for entity in entities:
        original = query_value("EDGE", "SKETCH_ENTITY", sketch + ".wireOp", sketchEntityId=entity)
        boundary = query_value("EDGE", "IMPRINT", sketch + ".imprint", derivedFrom=original)
        boundaries.append([boundary, 1])
    return compressed(query_value("FACE", "IMPRINT", sketch + ".imprint", disambiguationData=[
        {"disambiguationType": "TOPOLOGY", "entities": boundaries}]), deterministic)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", default="solidcraft-cli")
    args = parser.parse_args()
    sketch = fixtures.rectangle("drawn-profile")
    for i, entity in enumerate(sketch["entities"]):
        entity["entityId"] = f"outline.side{i}"
        entity.update(isConstruction=False, centerId="", internalIds=[], parameters=[], namespace="")
    sketch["entities"].append({"btType": "BTMSketchPoint-158", "entityId": "outline.origin", "isConstruction": True})
    sketch["constraints"] = []
    sketch["parameters"][0]["queries"] = [compressed(query_value("FACE", "DUMMY", "Top.planeOp"), "plane-pick", False)]
    sketch["parameters"].append(fixtures.param("disableImprinting", "BTMParameterBoolean-144", value=False))
    extrude = fixtures.feature("extrude", "raised-plate", [fixtures.enum("bodyType", "SOLID"), fixtures.enum("operationType", "NEW"),
        fixtures.param("entities", "BTMParameterQueryList-148", queries=[region_query("drawn-profile", [e["entityId"] for e in sketch["entities"][:4]], "region-pick")]),
        fixtures.enum("endBound", "BLIND"), fixtures.quantity("depth", "5 mm")])
    commands = fixtures.plate_commands()
    commands[-1]["params"]["distance"] = "5 mm"
    defaults = [fixtures.feature("origin", "Origin", [])] + [fixtures.feature("plane", plane, []) for plane in ["Top", "Front", "Right"]]
    fixtures.write_bundle(args.cli, "onshape-default-plane-regions", [sketch, extrude], commands,
                          {i: {"btType": "BTExportModelBodiesResponse-734", "microversionId": {"theId": "synthetic-microversion"}, "bodies": []} for i in [0, 1]}, defaults)


if __name__ == "__main__":
    main()
