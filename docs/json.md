# Machine-readable reports

The `dutchman` command also dispatches installed Dutchman CAD tools such as `pilot` and
`fixtures` through `dutchman-<tool>` beside the command or on `PATH`. `dutchman --help`
lists them. These external commands provide their own interfaces; the conversion MCP tools
and JSON reports described here retain their existing schemas.

The full CLI also accepts `dutchman part.f3d` (or `dutchman convert part.f3d`) and derives
the output beside the input from `--to`. An existing derived output is kept with exit 2;
choose `-o PATH` or use `--force` to replace it. `--force` conflicts with `--no-clobber`.
Explicit outputs retain their replacement behaviour. MCP output paths remain required.

`dutchman convert --json` and `dutchman inspect --json` print one JSON object on stdout and
nothing on stderr. The `dutchman-mcp` server's tools return the same objects, as structured
content and as text, with `isError` set when `ok` is false. The schema is named in `schema`. Within a schema version, fields are only
added: none is renamed, retyped or removed. A change that would break a reader gets a new
version (`dutchman.convert/2`).

## Exit codes

A geometry-only retry removes history, sketches, parameters and associated design records
before validation, including parameter links on joints. When it succeeds, `losses` includes
the existing `Degraded` loss with subject `history` and stage `project`, stating that parameters
were left out and giving the original check failure. Geometry and assembly checks still apply;
report schemas and field names are unchanged.

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
| `input.format` | `fusion_archive`, `fusion_multi_document`, `inventor_part`, `inventor_assembly`, `solidworks_part`, `solidworks_assembly`, `freecad_document`, `catia_part`, `creo_part`, `nx_part`, `creo_view`, `rhino`, `acis`, `step`, `iges`, `unclassified_part`, or `null` for an unknown extension. `format_name` is the same for people. Creo and NX share `.prt` and are distinguished by contents; inconclusive contents give `unclassified_part`. |
| `input.words` | Reader status: `reads geometry`, `opens, no geometry yet`, `recognised, not read yet`, or `open it in SolidCraft directly` (IGES); `null` for an unknown format. Present on successes and failures, including batch results and MCP reports. The conversion's history and verification describe what this file kept. |
| `input.program`, `input.extension` | Program or format family, and the input's actual suffix without a dot, used for status text (`prt.12` for a numbered save). Unknown programs are `null`. |
| `output.format` | The SolidCraft design format written. |
| `mode` | `history`: editable feature history, with the original geometry kept beside it, suppressed. `import`: the geometry alone. |
| `verification.status` | `verified`: the history was replayed in SolidCraft, or the FreeCAD output was measured, and matches the original. `differs`: a FreeCAD rebuild differs or restored STEP (`reason` names the part). `unverified`: the history was written without that check (`reason` says why: no `solidcraft-cli`, a timeout, `--no-verify`). `flattened`: the design has a history but it was not written (`reason`: the first feature that does not map, or `replay differs (…)`). `not_applicable`: there was no history to write (none in the source, or none cadmpeg decodes: every feature is kept in its native form), or `--import-only`. |
| `history.features` | Every source feature in timeline order. `status`: `mapped`, `not_needed` (the target needs no feature, e.g. a principal datum plane), `left_out` (suppressed in the source), `blocked` (does not map; `reason` says why). |
| `history.mapped`, `history.total` | Mapped or not-needed features, out of all but left-out ones. |
| `completeness` | For an assembly: how much of it was placed. `placed` and `placements` count every placement the assembly and its placed sub-assemblies record, a sub-assembly's as often as it is placed. `left_out` lists those not placed by cause, most first: `file` (the file placed, or `null`), `reason`, `stage` and `count`. Absent for a single part. |
| `joints` | For an assembly whose joints were written: how many of each kind, `{"revolute": 5, "rigid": 1}` (kinds `revolute`, `slider`, `rigid`). Grounded joints count as `rigid`, fixing their occurrence to the design at its stored placement. Absent when none were. |
| `names` | Every name the conversion met, sorted: the paths and folders it read from, and the names the design gives its parts, components, bodies, features, sketches, parameters and properties. For tools that take names out of a report before it is shared (the anonymous report does); absent when there were none. |
| `losses` | What did not survive intact. `level` is `Info` (no effect on geometry), `Degraded` (carried in a weaker form) or `Dropped` (left out). `code` is cadmpeg's stable loss code when cadmpeg reported it, else `null`; `subject` says what was affected; `message` is for people. `stage` and `upstream` say where it arose (below). |

IGES (`.igs`, `.iges`) is recognised as `input.format: "iges"` and refused before decoding
in conversion, inspection, batch results and MCP reports. Its `error.message` is
`Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import).`,
with `error.stage: "decode"` and `error.upstream: null`. Batch results count it as a refused
file (`ok: false`); the CLI and GUI do not offer a fixtures invitation for IGES.
The formats listing reports `status: "not_supported"`, `words: "open it in SolidCraft directly"`
and `converts: "SolidCraft imports IGES itself (File > Import)"`.

When SolidCraft has a quarantine attribute without the user-approved flag on macOS,
`verification.reason` explains how to allow its first run in Finder or System Settings
and try again; the history stays `unverified`. A geometry check that cannot run records
the same guidance in its `not checked` loss message.

For a blocked sweep, `history.features[].reason` names the unsupported option, such as
`sweeps with round corner transitions are not mapped yet` or
`sweeps with linear section interpolation are not mapped yet`. Tangent-path extension and
post-sweep simplification also block history. These are messages within the existing schema;
no report fields change.

For holes, a blocked feature's `reason` names the unsupported entry form, placement, or end
(for example, `a hole ending at the last encountered face is not mapped yet`). A tapped hole
maps with an ISO metric thread label; thread details it can't keep are a `Degraded` loss on
that feature. The wording is text for people, not a stable code.
Counterbores, countersinks, and chamfered entries with drill points report
`SolidCraft holes take a drill point only on a plain drilled hole`: SolidCraft 0.21.0 ignores
`tip_angle` on entry recesses. This uses the existing blocked feature and reason fields.

Sheet-metal edge flanges with proven input-edge picks and supported dimensions report
`mapped`. Other flanges report `blocked` with the specific selection or dimension reason
(for example, "its input edge midpoint does not identify exactly one rebuilt edge"). Hems
report the form or developed length SolidCraft cannot express. A blocked history falls back
to geometry. These use the existing report fields; no schema fields change.

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
 "skipped_by_extension": {"txt": 1}, "skipped_revisions": [], "resumed": 0,
 "results": [ {"schema": "dutchman.convert/1", "ok": true, "input": {…}, "output": {…}, "elapsed_ms": 412, …},
              {"schema": "dutchman.convert/1", "ok": false, "input": {…}, "error": {…}, "elapsed_ms": 96} ]}
```

`results` holds one `dutchman.convert/1` report per file (inputs that are neither a file nor a
folder come first), in the order found, each with `elapsed_ms`, the time it took. `skipped`
counts older numbered saves and files in the folders searched that are not in a format Dutchman knows; unknown formats are not
converted and not reported one by one, and `skipped_by_extension` counts them by extension
(lower case; `none`, or `other` for one that is not 1–8 letters and digits). `resumed` counts
the files whose reports came from `--journal` rather than a new conversion. `ok` is `true` only
when every file converted; the exit code is then 0, else 1.

For numbered `.prt.N` saves found in a directory, only the highest numeric revision per
part in that folder is converted. `skipped_revisions` lists each older save as
`{"path": "bracket.prt.3", "newer": "bracket.prt.7"}` (full paths as supplied).
These skips count in `skipped`, separately from `skipped_by_extension`. Explicit file
arguments are converted as named. A numbered save strips exactly one revision suffix
before deriving its base output name (`bracket.solidcraft`); existing outputs are kept.
These report additions retain `dutchman.convert-all/1`; existing fields keep their types.

Each file converts in a process of its own. One that runs past `--timeout` or holds more than
`--max-memory` is stopped and fails alone, its `error.stage` the stage it had reached; so does
one that crashes the converter. `--report-anon FILE` also writes the anonymous report,
[`dutchman.report-anon/1`](report-anon.md).

## `dutchman.formats/1`

`dutchman formats --json` lists the native formats Dutchman knows:

```json
{"schema": "dutchman.formats/1", "ok": true, "formats": [
  {"id": "fusion_archive", "name": "Fusion archive (.f3d)", "extension": "f3d",
   "extensions": ["f3d"], "words": "reads geometry", "level": "L5",
   "status": "converts", "converts": "geometry; feature history (…) when every feature maps; …"}
]}
```

`status` is `converts`, `coming` (in progress) or `not_supported`; `converts` says, for people,
what carries across.
`words` is `reads geometry`, `opens, no geometry yet`, `recognised, not read yet`, or
`open it in SolidCraft directly` (IGES).
`level` is cadmpeg's published headline in its format-support ladder (`L0`–`L9`),
or `null` when none is published. It supplements `words` and does not promise conversion
of a particular file. This additive field retains `dutchman.formats/1`.
`extensions` lists all accepted suffixes without dots (`prt.N` denotes a numbered save);
the existing `extension` remains the preferred suffix.
Dutchman attempts conversion when the format reads geometry and its codec is available in
this build. Creo and NX open without geometry support; SolidWorks assemblies and
unclassified `.prt` files are recognised without readers. Reader status does not promise
that a particular file will convert or that every feature will map.
`--quiet` suppresses only the text volunteer invitation after a conversion run; JSON
reports contain neither that invitation nor status prose outside the report.
The invitation appears once for programs with `ok: false`, `verification.status: "flattened"`,
or `history.mapped < history.total`, except IGES, and offers test-file recipes and private help
in Discord.

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
  "verification": {"status": "flattened", "reason": "feature \"Shell1\": shell waits on input face geometry"},
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

When a design falls back to imported geometry, a single Info loss with subject `sketches`
reports `kept N sketches for reference`, followed by `; M left out` when any sketches were
omitted. Only mapped sketches with resolved planes are retained, hidden after the import;
they do not change the body comparison or the report's `mode` and verification status.
`--import-only` continues to omit sketches.

### FreeCAD macro output (experimental)

For `convert --to freecad` (MCP `to: "freecad"`), `output.format` is
`"freecad-macro/1"` and `output.path` ends in `.FCMacro`. The schema remains
`dutchman.convert/1`; no fields are removed or renamed. Batch results use the same per-file
report. A macro with at least one rebuilt part has `mode: "history"` and
`verification.status: "unverified"` unless headless verification was requested.
An entirely imported macro has `mode: "import"` and `verification.status:
"not_applicable"`. In a mixed assembly the `history.features` and losses identify the parts
kept as STEP. A partial history keeps prefix features and all mapped sketches as `mapped`,
with reference reasons; later body-building features are `blocked`. A `Degraded` loss says
`rebuilt N of M features; kept as reference`, counting sketches but excluding suppressed
features (`left_out`) and organizational features (`not_needed`). These macros also use
`mode: "history"` and `verification.status: "unverified"` unless headless verification was requested; the visible final result remains
STEP when it differs from the prefix. `import_only` blocks all mapped features and retains
no prefix. Runtime rebuild failures restore embedded geometry and warn inside FreeCAD. Headless
execution also records these failures in the conversion report. Scalar parameter values are retained; their expressions are stored as
inert text and are not evaluated by the macro.

Assembly losses use the existing `subject: "joints"`: `Degraded` names each joint option
or operand that cannot be written, and `Info` records inferred grounding and the need to
run the macro. Supported workbench joints retain frames, limits, and suppression. A failed
runtime solve removes the joints, restores stored placements, and warns in FreeCAD;
it cannot modify this conversion report. Joint reconstruction does not change how
`mode` or `verification.status` describe the part histories. `import_only` omits joints.

The CLI also accepts `--to fcstd`: `output.format` is `"freecad-document/1"` and the
output is a native `.FCStd` document. This runs FreeCAD and always measures the result.
`--to freecad --verify` measures without saving a native document. Both use the existing
`verification` object: `status: "verified"` when every part's solid count, volume and bounds
match its source STEP, or the added status `"differs"`, with a `reason` naming the part
and differing measurement or runtime fallback. Runtime fallback reasons also appear as
`Degraded` losses with subject `FreeCAD rebuild`. Feature statuses continue to describe
mapping; a failed runtime rebuild is identified by verification and losses. Volumes use
SolidCraft's 1e-4 relative tolerance; bounds use the same tolerance relative to the largest
local span (minimum 1 mm). Parts without solids cannot be verified. Macro execution errors
use `"unverified"` and `"not checked: …"`; native execution errors fail the conversion.
The default timeout is 300 seconds, changed with `--freecad-timeout`. Output remains
experimental. These CLI additions do not change MCP's existing `to` choices.

For self-hosted browser conversions, [Dutchman Web](web.md#settings) documents the shared
zip budget, concurrent upload and saved-data limits, private data folder, and allowed browser
origins. Uploads may be refused with 403 (origin check) or 503 (capacity); the CLI and MCP
reports are unchanged.

## Watch events: `dutchman.watch-event/1`

`dutchman watch INBOX --out-dir DIR --json` streams newline-delimited JSON, one object
per completed attempt. There is no enclosing array or final batch report. Each object has:

| Field | Meaning |
|---|---|
| `schema` | `"dutchman.watch-event/1"` |
| `event` | `"converted"`, `"failed"` or `"skipped"` |
| `path` | Input path relative to the inbox, using platform path separators |
| `elapsed_ms` | Milliseconds spent converting and publishing this result |
| `message` | Empty on success; the error or skip reason otherwise |
| `report` | The `dutchman.convert/1` result, or null when the input changed during conversion |

For example:

```json
{"schema":"dutchman.watch-event/1","event":"skipped","path":"parts/cube.f3d","elapsed_ms":93,"message":"input changed during conversion","report":null}
```

An existing unrelated destination produces `skipped`; its nested report describes the
staged conversion, while `event` describes publication. Successful reports name the final
output path and are also saved as `<stem>.report.json`; failed reports are saved as
`<stem>.failed.json`, always under the output folder. Reports include `elapsed_ms`.
Ignored files, unsettled uploads and previously recorded hashes emit no event.
The private `.dutchman-watch.json` state has schema `dutchman.watch-state/1`, an `inbox`
identity, `files` mapping relative paths to SHA-256 hashes to `{ "outcome": ... }`, and
`owned` mapping relative output paths to `{ "sha256": "...", "input": "..." }`.
The ownership input is relative to the inbox; numbered `.prt.N` saves share the base `.prt`
owner. Outputs belonging to another input are protected. It is not a conversion report.
Watcher events and state contain paths; keep them private like batch journals.
