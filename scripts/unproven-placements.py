#!/usr/bin/env python3
"""Reject reports with unreadable occurrence positions, including older reports.

Allowances map each returned part name to a documented reason. Unmatched legacy
loss explanations must also be listed explicitly; they are retained as evidence.
A missing allowance, blank reason, or unmatched name does not waive a loss.
"""

import argparse
import json
import sys
from pathlib import Path

CODES = {
    "f3d/xref.placement-identity-assumed", "f3d/xref.placement-undecoded",
    "f3d/assembly.local-occurrence-unplaced", "f3d/assembly.stored-frame-occurrence",
    "f3d/assembly.local-occurrence-envelope-native",
}


def unproven_placements(report):
    """Return part names and counts; legacy losses retain their full explanation.

    New reports count occurrences, so don't add loss rows to their counts. Keep
    unmatched loss rows as evidence: an undecoded reference may have no occurrence.
    """
    parts = dict(report.get("completeness", {}).get("unplaced", {}))
    if any(not isinstance(name, str) or type(count) is not int or count < 1
           for name, count in parts.items()):
        raise ValueError("unplaced must map part names to positive occurrence counts")
    assumed = report.get("part_positions", {}).get("assumed", [])
    if isinstance(assumed, list):
        for placement in assumed:
            if placement.get("reason") == "identity":
                parts.setdefault(placement.get("part") or "unnamed part", 1)
    for loss in report.get("losses", []):
        if loss.get("code") in CODES:
            message = loss.get("message", "")
            # Decode losses give the target path in these two stable forms.
            name = message
            if message.startswith("external occurrence "):
                name = message.removeprefix("external occurrence ").split(
                    " has typed placement carrier", 1
                )[0]
            elif ": member " in message:
                name = message.split(": member ", 1)[1].split(
                    " occurrence placement did not decode", 1
                )[0]
            parts.setdefault(name or "unnamed part", 1)
    return parts


def load_allowances(inline, path):
    """Return {part name: (expected count or None, reason)}.

    Inline allowances map names to reasons (any count). A reviewed allowlist file pins each
    part's count too, so a part becoming unplaced more often fails like a new part does.
    """
    result = {}
    data = json.loads(inline) or {}
    if not isinstance(data, dict):
        raise ValueError("allowances must map part names to nonempty reasons")
    for name, reason in data.items():
        if not isinstance(name, str) or not isinstance(reason, str) or not reason.strip():
            raise ValueError("allowances must map part names to nonempty reasons")
        result[name] = (None, reason)
    if path:
        listed = json.loads(Path(path).read_text()).get("unplaced")
        if not isinstance(listed, dict):
            raise ValueError("an allowlist file needs an unplaced object")
        for name, entry in listed.items():
            count, reason = (entry or {}).get("count"), (entry or {}).get("reason")
            if (not isinstance(name, str) or type(count) is not int or count < 1
                    or not isinstance(reason, str) or not reason.strip()):
                raise ValueError(f"allowlist entry {name!r} needs a positive count and a reason")
            result[name] = (count, reason)
    return result


def only_unplaced(report_path, design_path):
    """Return 0 when a failed conversion wrote its design and failed only for unplaced parts."""
    try:
        report = json.loads(Path(report_path).read_text())
    except (OSError, ValueError):
        return 1
    if not Path(design_path).is_file() or report.get("ok") is not False:
        return 1
    unplaced = (report.get("completeness") or {}).get("unplaced") or {}
    reason = (report.get("verification") or {}).get("reason") or ""
    return 0 if unplaced and "couldn't be placed" in reason else 1


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--only-unplaced":
        return only_unplaced(sys.argv[2], sys.argv[3])
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--allowances", default="{}", help="part name to reason JSON object")
    parser.add_argument("--allowances-file", type=Path,
                        help="reviewed JSON file: {\"unplaced\": {part name: {\"count\": N, \"reason\": text}}}")
    args = parser.parse_args()
    try:
        report = json.loads(args.report.read_text())
        if not isinstance(report, dict) or "ok" not in report:
            raise ValueError("expected a conversion report with an ok field")
        allowances = load_allowances(args.allowances, args.allowances_file)
        parts = unproven_placements(report)
        failed = False
        for name, count in sorted(parts.items()):
            allowed = allowances.get(name)
            if allowed is None:
                print(f"{name} ×{count}: position could not be read; not in the allowlist")
                failed = True
            elif allowed[0] is not None and allowed[0] != count:
                print(f"{name} ×{count}: position could not be read; the allowlist expects ×{allowed[0]}")
                failed = True
            else:
                print(f"{name} ×{count}: position could not be read; allowed: {allowed[1]}")
        for name in sorted(set(allowances) - set(parts)):
            print(f"{name}: now placed; remove it from the allowlist")
        return int(failed)
    except (OSError, ValueError, TypeError, AttributeError) as error:
        print(f"Could not check placement report: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
