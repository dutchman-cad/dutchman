# Machine-readable reports

`dutchman convert --json` and `dutchman inspect --json` print one JSON object on stdout and
nothing on stderr. The `dutchman-mcp` server's tools return the same objects, as structured
content and as text, with `isError` set when `ok` is false. The schema is named in `schema`. Within a schema version, fields are only
added: none is renamed, retyped or removed. A change that would break a reader gets a new
version (`dutchman.convert/2`).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | The design was converted (`convert`) or read (`inspect`). Losses do not change this. |
| 1 | It could not be: unsupported or unreadable input, no geometry, an output that could not be written. With `--json`, the report has `"ok": false`. |
| 2 | A usage error (unknown option, missing argument). clap's usage text is printed on stderr; there is no JSON. |

## `dutchman.convert/1`

```json
{
  "schema": "dutchman.convert/1",
  "ok": true,
  "input": {"path": "part.f3d", "format": "fusion_archive", "format_name": "Fusion archive (.f3d)"},
  "output": {"path": "part.solidcraft", "format": "solidcraft/23"},
  "mode": "history",
  "verification": {"status": "verified", "reason": null},
  "history": {
    "mapped": 2,
    "total": 2,
    "features": [
      {"name": "Base sketch", "kind": "sketch", "status": "mapped", "reason": null},
      {"name": "Block", "kind": "extrude", "status": "mapped", "reason": null}
    ]
  },
  "losses": [
    {"level": "Info", "code": "step/native.source-record-omitted", "subject": "step/native.source-record-omitted",
     "message": "45 source-native record(s) were not represented in STEP"}
  ]
}
```

| Field | Meaning |
|---|---|
| `ok` | `true` when the design was written. |
| `input.format` | `fusion_archive`, `fusion_multi_document`, `inventor_part`, `inventor_assembly`, `solidworks_part`, `solidworks_assembly`, `freecad_document`, or `null` for an unknown extension. `format_name` is the same for people. |
| `output.format` | The SolidCraft design format written. |
| `mode` | `history`: editable feature history, with the original geometry kept beside it, suppressed. `import`: the geometry alone. |
| `verification.status` | `verified`: the history was replayed in SolidCraft and matches the original. `unverified`: the history was written without that check (`reason` says why: no `solidcraft-cli`, a timeout, `--no-verify`). `flattened`: the design has a history but it was not written (`reason`: the first feature that does not map, or `replay differs (…)`). `not_applicable`: there was no history to write (none in the source, or none cadmpeg decodes: every feature is kept in its native form), or `--import-only`. |
| `history.features` | Every source feature in timeline order. `status`: `mapped`, `not_needed` (the target needs no feature, e.g. a principal datum plane), `left_out` (suppressed in the source), `blocked` (does not map; `reason` says why). |
| `history.mapped`, `history.total` | Mapped or not-needed features, out of all but left-out ones. |
| `completeness` | For an assembly: how much of it was placed. `placed` and `placements` count every placement the assembly and its placed sub-assemblies record, a sub-assembly's as often as it is placed. `left_out` lists those not placed by cause, most first: `file` (the file placed, or `null`), `reason`, `stage` and `count`. Absent for a single part. |
| `joints` | For an assembly whose joints were written: how many of each kind, `{"revolute": 5, "rigid": 1}` (kinds `revolute`, `slider`, `rigid`). Grounded joints count as `rigid`, fixing their occurrence to the design at its stored placement. Absent when none were. |
| `names` | Every name the conversion met, sorted: the paths and folders it read from, and the names the design gives its parts, components, bodies, features, sketches, parameters and properties. For tools that take names out of a report before it is shared (the anonymous report does); absent when there were none. |
| `losses` | What did not survive intact. `level` is `Info` (no effect on geometry), `Degraded` (carried in a weaker form) or `Dropped` (left out). `code` is cadmpeg's stable loss code when cadmpeg reported it, else `null`; `subject` says what was affected; `message` is for people. `stage` and `upstream` say where it arose (below). |

For a blocked sweep, `history.features[].reason` names the unsupported option, such as
`sweeps with round corner transitions are not mapped yet` or
`sweeps with linear section interpolation are not mapped yet`. Tangent-path extension and
post-sweep simplification also block history. These are messages within the existing schema;
no report fields change.

For holes, a blocked feature's `reason` names the unsupported entry form, placement, or end
(for example, `a hole ending at the last encountered face is not mapped yet`). A tapped hole
maps with an ISO metric thread label; thread details it can't keep are a `Degraded` loss on
that feature. The wording is text for people, not a stable code.

A failure carries `"ok": false`, `input` and `error`, and exits 1:

```json
{"schema": "dutchman.convert/1", "ok": false,
 "input": {"path": "assembly.sldasm", "format": "solidworks_assembly", "format_name": "SolidWorks assembly (.sldasm)"},
 "error": {"message": "SolidWorks assembly (.sldasm) files are not supported yet. …",
           "stage": "decode", "upstream": "cadmpeg"}}
```

`error.message` is for people; do not parse its wording. Unreadable-container errors retain
cadmpeg's cause and add recovery advice and a link to the
**A file didn't convert** issue form. The CLI and MCP return the same message, stage, and upstream.
Normal JSON reports contain file names and paths. For sharing, request `--report-anon FILE`
(MCP `report_anon`) and review that file first; keep logs, journals, keys, and salts private.

### Stages

Every loss, and every failure's `error`, says where in the conversion it arose, so a report of
it reaches the code that can fix it:

| `stage` | Where | `upstream` |
|---|---|---|
| `input` | The file itself, or how it was asked for: missing, too large, the wrong kind, an assembly's parts not beside it. | `null` |
| `decode` | cadmpeg reading the file's bytes: a record or stream it could not decode. | `cadmpeg` |
| `project` | cadmpeg turning what it read into its neutral model: something read but not carried into it (unresolved, unprojected, kept native). | `cadmpeg` |
| `map` | Dutchman mapping the neutral model onto SolidCraft's: a feature, sketch, parameter or joint it does not carry. | `null` |
| `write` | Writing the design: Dutchman's writer, and cadmpeg's STEP restatement within it (a loss with a `step/…` code). | `cadmpeg` for a `step/…` loss, else `null` |
| `solidcraft` | SolidCraft opening or checking the written design. | `solidcraft` |

`upstream` names the project whose code it arose in when that is not Dutchman's. Report it on
Dutchman all the same: Dutchman's maintainers take it upstream.

A cadmpeg loss is at `decode` when its code says the bytes could not be read (`…-undecoded`,
`…-unparsed`, `…-untyped`, framing, a missing or unreadable stream, `…not-transferred`), else
at `project`.

## `dutchman.convert-all/1`

`dutchman convert-all --json --out-dir DIR <files and folders>…` converts every file named, and
every file Dutchman knows in the folders named (with their subfolders; symbolic links are not
followed), several at a time (`--jobs N`, default: the machine's cores, at most 8). Each
design goes to `DIR`, keeping the folder layout below each folder named. A file that fails
does not stop the others.

```json
{"schema": "dutchman.convert-all/1", "ok": false, "converted": 2, "failed": 1, "skipped": 1,
 "skipped_by_extension": {"txt": 1}, "resumed": 0,
 "results": [ {"schema": "dutchman.convert/1", "ok": true, "input": {…}, "output": {…}, "elapsed_ms": 412, …},
              {"schema": "dutchman.convert/1", "ok": false, "input": {…}, "error": {…}, "elapsed_ms": 96} ]}
```

`results` holds one `dutchman.convert/1` report per file (inputs that are neither a file nor a
folder come first), in the order found, each with `elapsed_ms`, the time it took. `skipped`
counts the files in the folders searched that are not in a format Dutchman knows; they are not
converted and not reported one by one, and `skipped_by_extension` counts them by extension
(lower case; `none`, or `other` for one that is not 1–8 letters and digits). `resumed` counts
the files whose reports came from `--journal` rather than a new conversion. `ok` is `true` only
when every file converted; the exit code is then 0, else 1.

Each file converts in a process of its own. One that runs past `--timeout` or holds more than
`--max-memory` is stopped and fails alone, its `error.stage` the stage it had reached; so does
one that crashes the converter. `--report-anon FILE` also writes the anonymous report,
[`dutchman.report-anon/1`](report-anon.md).

## `dutchman.formats/1`

`dutchman formats --json` lists the native formats Dutchman knows:

```json
{"schema": "dutchman.formats/1", "ok": true, "formats": [
  {"id": "fusion_archive", "name": "Fusion archive (.f3d)", "extension": "f3d",
   "status": "converts", "converts": "geometry; feature history (…) when every feature maps; …"}
]}
```

`status` is `converts`, `coming` (in progress) or `not_supported`; `converts` says, for people,
what carries across.

## `dutchman.inspect/1`

The same fields as `convert` except `output` (with `completeness` for an assembly), plus
`geometry`, `conversion`, and for an assembly
`assembly: {"parts": <part files read>, "placements": <parts placed at the top level>}`:

```json
{
  "schema": "dutchman.inspect/1",
  "ok": true,
  "input": {"path": "part.f3d", "format": "fusion_archive", "format_name": "Fusion archive (.f3d)"},
  "geometry": {"bodies": 1, "faces": 6},
  "conversion": {"possible": true, "reason": null},
  "mode": "import",
  "verification": {"status": "flattened", "reason": "feature \"Fillet1\": fillet is not mapped yet"},
  "history": {"mapped": 2, "total": 3, "features": [ … ]},
  "losses": [ … ]
}
```

`conversion.possible` is `false` when converting the file would fail, with `reason` saying
why, for example a design whose only geometry is a sketch: "the model has no solids or surfaces
SolidCraft can import". `geometry` counts what was read, whether or not it can be written.

`inspect` writes nothing and runs no SolidCraft, so a history that would be written reports
`verification.status` `unverified`. Both reports look at every feature, so every `blocked` one
is listed; `verification.reason` names the first.

Sketch constraint losses keep the existing report fields and schema. Their `message`
counts source constraints, for example `2 of 2 constraints not carried (angle difference:
solver scalar identities have no sketch entity binding, scalar equality: solver scalar
identities have no sketch entity binding); its geometry is exact`. Repeated reasons are
counted with `×N`. Unsupported constraints are `Degraded` losses at `map`; they do not
change a mapped sketch's feature status. Inactive and disabled relations do not create a
loss. Relations in virtual sketch space are counted separately because SolidCraft has no
separate virtual solver space.

Body mirror patterns and join/cut/intersect body combines (optionally keeping the tool
bodies) can keep editable history; their bodies must be named by earlier mapped features.
Shells and drafts are refused, since their faces are known only on the final body, where
the feature has already removed or tilted them; they wait on input face geometry.
Replacing a source shape with its mirror is not supported yet: SolidCraft's mirror keeps
the original. Cosmetic threads lack a resolved standard and pitch in CADIR, so assigning
SolidCraft's ISO metric designation would invent thread data; these remain flattened.

### FreeCAD macro output (experimental)

For `convert --to freecad` (MCP `to: "freecad"`), `output.format` is
`"freecad-macro/1"` and `output.path` ends in `.FCMacro`. The schema remains
`dutchman.convert/1`; no fields are removed or renamed. Batch results use the same per-file
report. A macro with at least one rebuilt part has `mode: "history"` and
`verification.status: "unverified"`, because FreeCAD is run by the recipient afterwards.
An entirely imported macro has `mode: "import"` and `verification.status:
"not_applicable"`. In a mixed assembly the `history.features` and losses identify the parts
kept as STEP. Every source feature in a part kept as STEP is `blocked` with a reason, except
suppressed features (`left_out`) and organizational features (`not_needed`). Runtime rebuild
failures restore embedded geometry and warn inside FreeCAD; they cannot update the earlier
conversion report. Scalar parameter values are retained; their expressions are stored as
inert text and are not evaluated by the macro.
