# The anonymous report: `dutchman.report-anon/1`

`dutchman convert-all --report-anon FILE …` (or the MCP `convert_all` tool's `report_anon`)
writes, beside the usual results, one JSON file that says how a batch went without saying
which files were in it. It is made to be shared: posted in an issue, sent to the maintainers.

## What it never holds

- **No file or folder names, no paths.** Each file appears only as an id, `f` and 16 hex
  digits: a SHA-256 hash of the file's full path and a secret salt. Nobody can turn an id back
  into a path without the salt and the path.
- **No part, component, feature or sketch names.** Messages keep their wording, but every path,
  file name, quoted name (`"…"`, `'…'`, `` `…` ``, `“…”`), each file's own feature names and
  every long hexadecimal id (a GUID, a hash) in them are replaced by `<path>`, `<file>`,
  `<name>` or `<id>`.
- **No geometry**: no coordinates, sizes, volumes or models.
- **No salt.** The salt is printed on your screen when the run ends, and written to the key
  file if you ask for one. It is never in the report.

Read the report before you share it. It is plain JSON. If you find a name in it, leave that
line out and say so in your issue; it is a bug in Dutchman.

## Matching an id to your file

The ids depend on the salt, so the same file gets the same id whenever you use the same salt:

- `--report-key FILE` writes the key, `{"salt": "…", "files": {"f…": "/path/to/file.f3d"}}`.
  Keep it to yourself.
- `--report-salt HEX` runs with a salt you already have (the one printed last time), so the
  ids match an earlier report.

When a maintainer asks about `f3a9c0e1d2b4f5a6`, look it up in your key.

## Fields

```json
{
  "schema": "dutchman.report-anon/1",
  "versions": {"dutchman": "0.1.2", "cadmpeg": "0.6.3", "solidcraft": "0.18.2",
               "os": "linux", "arch": "x86_64"},
  "run": {"verify": true, "import_only": false, "jobs": 8, "timeout_s": 900,
          "max_memory_mb": 8192, "resumed": 0},
  "totals": {"files": 105, "converted": 77, "history_verified": 3, "history_unverified": 0,
             "geometry_only": 74, "degraded": 77, "incomplete": 3, "failed": 28,
             "skipped": 2},
  "formats": {"fusion_archive": {"files": 40, "converted": 38, …, "timing": {…}}, …},
  "skipped_by_extension": {"txt": 1, "pdf": 1},
  "stages": {"decode": {"failed": 25, "losses": 119, "files": 83}, …},
  "errors": [{"message": "cannot read this design: …", "stage": "decode",
              "upstream": "cadmpeg", "files": 23, "occurrences": 23,
              "formats": {"fusion_archive": 23}, "file_ids": ["f…", …]}],
  "losses": [{"code": "f3d/geometry.not-transferred", "level": "Dropped", "stage": "decode",
              "upstream": "cadmpeg", "example": "ASM BREP geometry was not transferred: …",
              "files": 4, "occurrences": 4, "formats": {…}, "file_ids": […]}],
  "blocked_features": [{"kind": "fillet", "reason": "fillet is not mapped yet",
                        "stage": "map", "files": 2, …}],
  "verification": [{"status": "flattened", "reason": "feature \"<name>\": …", "files": 6, …}],
  "timing": {"count": 105, "p50_ms": 657, "p90_ms": 1386, "p99_ms": 2061, "max_ms": 2557,
             "total_ms": 77854},
  "slowest": [{"file": "f…", "format": "fusion_archive", "ms": 2557,
               "outcome": "geometry_only", "stage": null}]
}
```

| Field | Meaning |
|---|---|
| `versions` | Dutchman's version, the cadmpeg it was built with, the SolidCraft CLI that checked the designs (`null` when none did), and the operating system and processor. |
| `run` | How the batch ran: whether designs were checked in SolidCraft, `--import-only`, files at once, one file's time and memory limits, and how many results came from `--journal`. |
| `totals` | Every file: `converted`, then how: `history_verified` (editable history, checked against the original in SolidCraft), `history_unverified`, `geometry_only`; `degraded` counts converted files with at least one `Degraded` or `Dropped` loss; `incomplete` counts converted assemblies with placements left out; `failed`; `skipped` (not in a format Dutchman knows). |
| `formats` | The same counts per input format (`fusion_archive`, `fusion_multi_document`, `inventor_part`, `inventor_assembly`, `solidworks_part`, `solidworks_assembly`, `freecad_document`, `unknown`), each with its `timing`. |
| `skipped_by_extension` | The skipped files by extension: lower case, `none`, or `other` for one that is not 1–8 letters and digits. |
| `stages` | For each stage (`input`, `decode`, `project`, `map`, `write`, `solidcraft`; see [Stages](json.md#stages)): the files that `failed` there, the `losses` that arose there, and the `files` with either. |
| `errors` | Every distinct failure, most files first. Messages that differ only in their numbers are one cause. |
| `losses` | Every distinct loss: by cadmpeg's loss `code`, or for Dutchman's own by `dutchman/<kind>` (what it is about: `history`, `feature`, `sketch`, `joints`, …) and message. `example` is one message, cleaned. |
| `blocked_features` | The source features that kept a history from being written, by `kind` and `reason`. |
| `verification` | Why histories were written unchecked (`unverified`), not written (`flattened`), or not applicable, by reason. |
| `timing` | Times per file, in milliseconds: the nearest-rank 50th, 90th and 99th percentiles, the longest and the total. Files that were never started (a path that does not exist) are not timed. |
| `slowest` | The 20 slowest files, slowest first: id, format, time, outcome and, for a failure, its stage. |

Every cause carries `files` (how many files show it), `occurrences` (how many times in all),
`formats` (files per format) and `file_ids` (up to 20 of those files' ids).

The report's fields are added to, never renamed or removed, while its schema stays
`dutchman.report-anon/1`.
