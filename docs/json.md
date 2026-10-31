# Machine-readable reports

The `dutchman` command also dispatches installed Dutchman CAD tools such as `pilot` and
`fixtures` through `dutchman-<tool>` beside the command or on `PATH`. `dutchman --help`
lists them. These external commands provide their own interfaces; the conversion MCP tools
and JSON reports described here retain their existing schemas.

The full CLI also accepts `dutchman part.f3d` (or `dutchman convert part.f3d`) and derives
the output beside the input from `--to`. An existing derived output is kept with exit 2;
choose `-o PATH` or use `--force` to replace it. `--force` conflicts with `--no-clobber`.
Explicit outputs retain their replacement behaviour. MCP output paths remain required except for `convert` with `dry_run: true`.

`dutchman convert --json` and `dutchman inspect --json` print one JSON object on stdout. During reading and conversion, progress JSON lines are written to
stderr unless `--quiet` is set. The `dutchman-mcp` server's tools return the same objects, as structured
content and as text, with `isError` set when `ok` is false. The schema is named in `schema`. Within a schema version, fields are only
added: none is renamed, retyped or removed. A change that would break a reader gets a new
version (`dutchman.convert/2`).

Sketch losses describe unresolved distance operands and ambiguous curve-only coincidences
specifically, including dimension values that match neither the line nor its finite endpoint. Cosmetic thread blockers distinguish missing diameter/pitch from historical
faces without input surface geometry. Unspecified modeled construction, when an annotation
can be placed, adds a Degraded loss explaining that only the cosmetic annotation is kept.
These use the existing loss fields; no report schema or field changes.

## `dutchman.progress/1`

`convert --json`, `convert-all --json`, `inspect --json`, and `watch --json` write progress
as one JSON object per line on stderr.
Stdout remains the existing final report (or watch-event stream), with no progress objects
embedded or appended.
`--quiet` suppresses progress. Stage changes and design completions emit events immediately;
a running read or conversion also emits a heartbeat every five seconds. Events are ordered per file;
files in a batch may interleave. No completion event is promised after a process is stopped.

```json
{"schema":"dutchman.progress/1","input":{"path":"part.f3d","format":"fusion_archive"},"stage":"decode","elapsed_ms":5000,"designs_read":0,"designs_total":1,"heartbeat":true}
```

`input` uses the same input object as the final report. `stage` is the current stage id:
`input`, `decode`, `project`, `map`, `write`, or `solidcraft`. The current conversion pipeline
includes output building and verification in `map`; inspection planning also uses `map`.
`elapsed_ms` is an integer duration since this file's conversion began. `designs_read` counts
successfully read unique documents, including assembly documents, rather than placements or
entities. `designs_total` is an integer when known, otherwise `null`. `heartbeat` is true for
timer updates and false for stage or count changes. A geometry-only retry does not count the
same document twice. Counts describe reading only, not the percentage of total conversion time.

The current cadmpeg API owns the Fusion `.f3z` member loop: its counts remain zero and `null`
respectively, even when the archive finishes reading. Separate assembly files report their
completed documents with an unknown total. A heartbeat says the worker is running, without
claiming that the decoder completed more work. Final report schemas remain unchanged.
MCP and the web app return their final reports without a progress stream.

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
| 2 | A usage error (unknown option, missing argument), or a memory-budget refusal. Usage errors print clap text on stderr. Memory refusals with `--json` return a failure report with `estimate` and `choices`. |

## MCP conversion pre-read

`convert` with `dry_run: true` returns `ok: true`, `estimate`, `choices` (as below),
and `fits`, without decoding or writing. `output` is optional. `fits` is a boolean
when both the estimate and available memory are known, otherwise `null`. A dry run
returns `fits: false` for an over-budget estimate instead of attempting conversion.
Standard's pre-read budgets any input without history when only its history-free
estimate fits. The decision is fixed before decoding. The dry run does
not promise that decoding or conversion will succeed. `no_memory_check` permits a run
without changing the physical-memory comparison in `fits`.

Memory refusals from `convert` or `convert_all` carry the matching schema, `ok: false`,
the offending `input`, `error`, `estimate`, and `choices`. A batch refuses before writing
its designs when a pre-read exceeds the budget; it also includes that file's refusal in
`results` with `converted: 0` and `failed: 1`.

## `dutchman.convert/1`

Repeated stderr notes of the same kind are grouped with a count. `--verbose`, `--log`,
and JSON retain each individual loss. Fusion’s material warning can also appear for a
default material: the decoded model does not establish that no source appearance was lost.

```json
{
  "schema": "dutchman.convert/1",
  "ok": true,
  "input": {"path": "part.f3d", "format": "fusion_archive", "format_name": "Fusion archive (.f3d)"},
  "output": {"path": "part.sc", "format": "solidcraft/23"},
  "estimate": {"peak_bytes": 1200000000, "seconds": 90, "seconds_exact": 89.5, "band": 0.5, "memory_band": 0.5, "time_band": null},
  "choices": {"preset": "standard", "history": true, "sketches": true, "joints": true},
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

RAM bands use every successful Windows and Mac measurement, re-evaluated with the
current coefficients: Fusion `.f3d` ±55%, multipart `.f3z` ±25%, Inventor parts ±30%,
SolidWorks parts ±35%, and FreeCAD ±50%. One percentage point of safety is added before
rounding up to a 5% step. These are empirical envelopes of this corpus, not guarantees for
unseen designs. Unmeasured formats retain their ±100% memory heuristic.
Time has its own band: multipart Fusion is ±20%; other time bands are uncalibrated.
Timing uses fractional `seconds_exact`; `seconds` remains its whole-second ceiling for
compatibility. Fixed startup is separate from content work: Windows has different fast,
history and verification startup terms; macOS has its own process floor. A possible
version-probe timeout is not included as normal conversion work. The estimator
never runs SolidCraft to measure a probe. History may probe its version even with
`--no-verify`. Old Mac probe durations were not recorded, Windows startup varies between runs, and
FreeCAD assembly worker startup is poorly predicted by archive bytes, so these four formats keep uncalibrated time bands. Verification
replay and explicit-version/no-memory-check variants also retain rough timing; their
startup or replay costs are not generally calibrated. Bands are not widened to hide this.
JSON adds `memory_band` and nullable `time_band`. The existing `band` stays as the legacy
shared heuristic for compatibility; use the new fields for separate calibrated coverage.
For multipart Fusion `band` is the larger of the two calibrated bands (25%).

For Fusion archives with multiple readable embedded `.f3d` designs, the time estimate
uses the requested threads (0 means auto), limited by the converter's shared CPU, item
count and free-memory worker policy. A serial component remains. Before native decoding,
the embedded design count and the converter's 256 MiB minimum worker budget are proxies;
actual body counts can reduce conversion workers further. Single-design files keep their
existing time model. The worker adjustment changes time estimates only.

`threads.used` is the largest bounded Dutchman part-worker limit selected across conversion
stages and embedded members. It excludes threads inside SolidCraft child processes and does
not measure simultaneously busy CPU cores. Failed worker tasks can retry serially.

Default SolidCraft paths use `.sc`; targets before 0.25.1 use `.solidcraft`.
Explicit output paths keep their given extension. The container format stays `solidcraft/23`.

| Field | Meaning |
|---|---|
| `ok` | `true` when the design was written. |
| `estimate` | Quick pre-read estimate, including on memory refusals: `peak_bytes` and `seconds` are unsigned integers; `seconds_exact` is an additive fractional-second elapsed estimate (`seconds` is its ceiling); time bands apply to `seconds_exact`; `band` is the legacy shared heuristic (`0.5` means ±50%); `memory_band` is the separate relative RAM band and `time_band` is a relative time band or `null` when uncalibrated. `band` remains the previous heuristic when timing is uncalibrated, and the larger of the two calibrated bands otherwise. `null` when unavailable. No design is decoded; a Fusion archive's designs are read one at a time for their own directories. Calibrated on measured conversions (see [Choosing what to keep](presets.md)). Memory adds fixed process overhead to a content cost; history scales the content cost. Memory and time use separate history factors. |
| `threads` | `{requested, used}` records the per-part worker limit (`0`: automatic, `1`: serial) and the bounded maximum used across component slicing and SolidCraft preparation. Counts are limited by part count and available memory, and never below `1`. Present on SolidCraft writes and on assembly reads that reach component slicing, including inspection and other output formats; omitted for pre-read estimates and reads/writers with no part-worker stage. Batch entries and watch reports carry it within each conversion report. Native decoding stays serial. |
| `solidcraft` | SolidCraft output only: `target` is the selected `x.y` minor; `found` is the discovered CLI release as `x.y.z`, or null when unavailable or not checked. `known_newest` is the newest known minor (`0.28`). `status` is `newer`, `too_old`, `older_loses_history`, `current`, or `none_found`. The history status requires an older installed release and a selected target whose writer gates retain less history than newest; `current` includes older releases with no gated history loss. Included even with verification off. An explicit target wins; otherwise the supported found version capped at known newest, else built-in newest (0.28), is targeted. |
| `choices` | Selected `preset` (`fast`, `standard`, or `full`) and effective `history`, `sketches`, and `joints` booleans after narrowing. `history` is false when Standard leaves history out to fit memory. These describe requested retention, not proof that every feature maps. FreeCAD output honours history only. |
| `input.format` | `fusion_archive`, `fusion_multi_document`, `inventor_part`, `inventor_assembly`, `solidworks_part`, `solidworks_assembly`, `freecad_document`, `catia_part`, `creo_part`, `nx_part`, `creo_view`, `rhino`, `acis`, `step`, `iges`, `unclassified_part`, or `null` for an unknown extension. `format_name` is the same for people. Creo and NX share `.prt` and are distinguished by contents; inconclusive contents give `unclassified_part`. |
| `input.words` | Reader status: `reads geometry`, `opens, no geometry yet`, `recognised, not read yet`, or `open it in SolidCraft directly` (IGES); `null` for an unknown format. Present on successes and failures, including batch results and MCP reports. The conversion's history and verification describe what this file kept. |
| `input.program`, `input.extension` | Program or format family, and the input's actual suffix without a dot, used for status text (`prt.12` for a numbered save). Unknown programs are `null`. |
| `output.format` | The SolidCraft design format written. |
| `mode` | `history`: editable feature history, with the original geometry kept beside it, suppressed. `partial_history`: retained editable features in a hidden component and the visible imported final shape. `import`: geometry, possibly with reference sketches. |
| `verification.status` | `verified`: the history was replayed in SolidCraft, or the FreeCAD output was measured, and matches the original. `differs`: a FreeCAD rebuild differs or restored STEP (`reason` names the part). `unverified`: the history was written without that check (`reason` says why: no `solidcraft-cli`, a timeout, `--no-verify`). `partial`: only partial history is retained; `reason` names the blocker, gives kept/total source feature counts, and states whether retained features replayed or remain unverified. This never claims the full history matches the original. `flattened`: no body-building prefix was retained (`reason`: the blocker or replay failure). `not_applicable`: there was no history to write (none in the source, or none cadmpeg decodes: every feature is kept in its native form), or `--import-only`. |
| `history.features` | Every source feature in timeline order. `status`: `mapped`, `not_needed` (the target needs no feature, e.g. a principal datum plane or an unused construction helper), `left_out` (suppressed in the source, or a reference sketch omitted by choice), `blocked` (does not map; `reason` says why). |
| `history.mapped`, `history.total` | Mapped or not-needed features, out of all but left-out ones. |
| `history.parts` | For an assembly written as SolidCraft components: each part's history, in the order written. `name`; `status` and `reason` as in `verification` (`not_applicable` when the part has no source features, else `flattened` with its first blocker, `partial`, `unverified` or `verified`); `mapped` and `total` as in `history`. A part placed several times is listed once. Absent for a single part. |
| `completeness` | For an assembly: how much of it was placed. `placed` and `placements` count every placement the assembly and its placed sub-assemblies record, a sub-assembly's as often as it is placed. `left_out` lists those not placed by cause, most first: `file` (the file placed, or `null`), `reason`, `stage` and `count`. Absent for a single part. |
| `joints` | For an assembly whose joints were written: how many of each kind, `{"revolute": 5, "rigid": 1}` (kinds `revolute`, `slider`, `rigid`). Grounded joints count as `rigid`, fixing their occurrence to the design at its stored placement. Absent when none were. |
| `names` | Every name the conversion met, sorted: the paths and folders it read from, and the names the design gives its parts, components, bodies, features, sketches, parameters and properties. For tools that take names out of a report before it is shared (the anonymous report does); absent when there were none. |
| `losses` | What did not survive intact. `level` is `Info` (no effect on geometry), `Degraded` (carried in a weaker form) or `Dropped` (left out). `code` is cadmpeg's stable loss code when cadmpeg reported it, else `null`; `subject` says what was affected; `message` is for people. `stage` and `upstream` say where it arose (below). |

SolidCraft version presentation uses minors: `solidcraft.target` and
`solidcraft.known_newest` now contain `x.y` strings (for example, `"0.28"`), changing their
previous `x.y.z` representation. `solidcraft.found` keeps the full CLI release (for example,
`"0.28.0"`). Status comparisons use minors, so all 0.28 patches are current; 0.29 is newer
and 0.17 is too old. Patch-sensitive writer gates and the internal 0.18.2 verification
floor remain unchanged. Explicit `solidcraft_version` inputs still accept `x.y.z` for
precise gate control.

`solidcraft.status` compares the installed minor with 0.18 and `known_newest`:
`newer` is above the newest minor, `too_old` is below the floor, and `none_found` has no readable
installed version. Within the supported range, `older_loses_history` requires an older
installed release and a selected target that disables editable writer capabilities;
otherwise it is `current`. Today those capabilities are sketch-profile coils and ordered
mirror-pattern chains, both introduced in 0.25 patch 1. The 0.22 patch 1 entry-recess tip representation
change preserves geometry and does not count as history loss. These fields use local
version discovery only.

SolidCraft history verification compares body count, area, and volume, using the measuring
mesh tolerances. A face-count difference (SolidCraft can merge faces on one surface) produces
no geometry loss, nor does an open-shell warning when area and volume agree. Geometry-only
output has no independent source measurements; evaluation errors, unmeshable faces, solids
without volume, solids opened as open shells, and bodies inside out still produce `Degraded` losses with subject `geometry` and stage `solidcraft`.
Sheets remain exempt from volume and closed-shell checks. The GUI, CLI, web app, and MCP
present these same losses; `verification.status` continues to describe history, and the
schema is unchanged.

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

Stored geometry (`history.features[].kind: "stored_geometry"`) becomes a mapped base-body
Import feature. When the mapped features are only stored geometry, `mode` is `"import"`
and `verification.status` is `"not_applicable"`, with reason “This part holds an imported
solid; there is no feature history to bring over”. The same message appears in `losses`
with level `"Info"`; nothing was dropped. Later mappable features keep the usual history
mode and verification. The desktop app shows the same Info message from this report.
These use existing fields in the schema; `inspect` reports the same stored-only outcome.

FreeCAD external links use the existing assembly fields: missing or unreadable referenced
`.FCStd` files appear in `completeness.left_out` with `file`, `reason`, `stage`, and `count`,
and in `losses`. Available linked Bodies and Parts convert as placed components; link-array
elements count once. Grounded parts appear as rigid joints, and unsupported joints have
loss notes while stored placements are retained. No report schema fields are added.

For FreeCAD fillets and chamfers, `history.features[].reason` reports missing stored input
edge geometry or a failed replay proof. Resolved edges without proven membership in an
earlier stored feature result are refused. This uses the existing feature status and reason
fields; no report fields change.

For a blocked sweep, `history.features[].reason` names the unsupported option, such as
`sweeps with round corner transitions are not mapped yet` or
`sweeps with linear section interpolation are not mapped yet`. Tangent-path extension and
post-sweep simplification also block history. These are messages within the existing schema;
no report fields change.

A mapped `helical_sweep` becomes a sketch-profile coil only for targets 0.25 patch 1 or newer.
For older targets, `history.features[].reason` says
`needs SolidCraft 0.25 patch 1 or newer (target 0.18)` (with the actual target), and the history
line explains the blocker. `--solidcraft-version x.y.z` selects the target on `convert`,
`convert-all`, and `watch`; it must be at least 0.18.2. By default the found verification
CLI's version is used, even with `--no-verify`, else the newest release known at build
time (currently 0.28). The supported rolling 14-day window is currently 0.18–0.28;
older targets down to 0.18.2 retain the existing feature simplifications.
When a written coil or flat entry-recess tip uses the newer mapping, the history line
includes the actual target, such as `target SolidCraft 0.25 patch 1`. No new report fields are added. Counterbore and countersink holes use an explicit flat tip
for targets 0.22.1 or newer, retaining the source's flat bottom.

Conical and spiral helices report
`conical and spiral helical sweeps are not supported yet: SolidCraft's sketch-profile coil
has no radial growth or cone angle` in `history.features[].reason`. Dimensions without
feature parameter bindings produce a `Degraded` loss saying their resolved values were
kept. These use existing schema fields.

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

Renamed parameter names are included in `names` so anonymous reports can remove them.
Fusion consumed tools and removed bodies are omitted only when cadmpeg marks them hidden
and no component owns them, with an Info `bodies` note giving the count. User-hidden
component members stay available; mixed shown/hidden parts are shown whole, with an Info
note. See [Fusion visibility](fusion.md#parts-and-history).

Parameter expression fallbacks use the existing `losses` records: `subject` is
`parameter <written name>`, `level` is `Degraded` when a scalar value is retained, and
`Dropped` when neither expression nor scalar value can be written. The message names the
unsupported function, conditional, unresolved or ambiguous reference, cycle, or syntax
limitation. Spreadsheet text and empty cells produce no parameter loss; unused numeric
cells produce an `Info` note. These are mapping observations, not additional schema fields.
`verification.status: verified` compares initial geometry and does not certify that every
source expression was decoded or that edits reproduce the source design's dependencies.

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

The desktop app shows Info messages with subject `history` directly on result cards,
including Standard's explanation when history is left out to fit in free memory.
These are existing `losses` entries; desktop estimates and preset labels add no report fields.
The CLI and desktop display small estimates as “under 0.1 GB” and “under a minute”;
JSON `estimate.peak_bytes` and `estimate.seconds` remain numeric and unrounded.

Appearance bindings that cannot reach written component geometry, resolve an appearance, or
select a mixed-colour face are recorded in `losses` with subject `appearance` and level
`Dropped`. Their messages identify the binding. Uniform face appearances carried as a body
colour are preserved and do not produce a loss.

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
before deriving its base output name (`bracket.sc`); existing outputs are kept.
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
The invitation appears once for programs with `ok: false`, `verification.status: "flattened"` or `"partial"`,
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
  "mode": "partial_history",
  "verification": {"status": "partial", "reason": "history stopped at feature \"Shell1\": shell waits on input face geometry (2 of 3 features kept); the rest is the imported final shape; prefix unverified: inspect writes nothing"},
  "history": {"mapped": 2, "total": 3, "features": [ … ]},
  "losses": [ … ]
}
```

`conversion.possible` is `false` when converting the file would fail, with `reason` saying
why, for example a design whose only geometry is a sketch: "the model has no solids or surfaces
SolidCraft can import". `geometry` counts what was read, whether or not it can be written.

`inspect` writes nothing and runs no SolidCraft, so a history that would be written reports
`verification.status` `unverified`, or `partial` when mapping identifies a retained prefix
(the reason states that the prefix is unverified). Both reports look at every feature;
`blocked` includes operations after the first blocker, and `verification.reason` names the
first blocker.

Sketches that fail replay are retried without constraints and reported at `Degraded` level;
their mapped feature reason records that loss. If the solved geometry still cannot replay,
the sketch is left out and reported as `blocked`.

Signed horizontal and vertical sketch distances that map to positive magnitudes retain
their constraints and expression dependencies without a constraint-loss message. Report
fields and verification statuses are unchanged.

Periodic sketch B-splines use the existing feature status and reason fields. Uniform curves
(including equal-weight rational curves) map by exact endpoint knot insertion. A periodic
curve with unequal weights is blocked with `weighted periodic splines are not supported yet:
SolidCraft control splines have no weights`; nonuniform or repeated interior knots name
SolidCraft's missing custom knots. No report fields or schema versions change.

Sketch constraint losses keep the existing report fields and schema. Their `message`
counts source constraints, for example `2 of 2 constraints not carried (angle difference:
solver scalar identities have no sketch entity binding, scalar equality: solver scalar
identities have no sketch entity binding); its geometry is exact`. Repeated reasons are
counted with `×N`. Unsupported constraints are `Degraded` losses at `map`; they do not
change a mapped sketch's feature status. Inactive and disabled relations do not create a
loss. Relations in virtual sketch space are counted separately because SolidCraft has no
separate virtual solver space.

Whole-sketch hole placements exclude construction entities; explicit entity selections
can still use construction points or circle centres.

For mirror-containing composite patterns and repeated full-turn endpoints, an attempted
replay that cannot finish keeps the original geometry and reports why. Turning verification
off, or having no usable CLI, still writes unverified history.

Composite feature patterns containing resolved mirrors can map to several ordered timeline
features while retaining one result per source feature in the history report. Each stage
repeats the previous instance set. Chained stages require a target of SolidCraft 0.25 patch 1
or newer; older targets report `needs SolidCraft 0.25 patch 1 or newer` and keep the original
geometry. Single-stage mirrors and patterns still map on every supported target.
A full-turn circular pattern with a repeated seed endpoint
keeps each distinct location once. Replay differences still flatten the history and report
why; body-seeded composite mirrors and unresolved planes remain unsupported.

Body mirror patterns and join/cut/intersect body combines (optionally keeping the tool
bodies) can keep editable history; their bodies must be named by earlier mapped features.
Shells and drafts are refused, since their faces are known only on the final body, where
the feature has already removed or tilted them; they wait on input face geometry.
Replacing a source shape with its mirror is not supported yet: SolidCraft's mirror keeps
the original. Cosmetic threads lack a resolved standard and pitch in CADIR, so assigning
SolidCraft's ISO metric designation would invent thread data; these stop the editable prefix.

When a design falls back to imported geometry, a single Info loss with subject `sketches`
reports `kept N sketches for reference`, followed by `; M left out` when any sketches were
omitted. Only mapped sketches with resolved planes are retained, hidden after the import;
they do not change the body comparison or the report's `mode` and verification status.
`--import-only` leaves out history while retaining reference sketches when the preset
keeps sketches. Use `--no-sketches` as well, or `--preset fast`, for geometry alone.

Assembly timeline folders are reported as Info losses with subject `timeline folders`,
`code: null`, and message `"Name": N feature(s) grouped`, one per folder added. Assemblies
with multiple top-level designs group each design's features, including the root's own
features when present; joints remain outside folders, and feature order is unchanged.
Parts and assemblies with one top-level design have no folders and no such notes.

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

For `dutchman-web --solidcraft`, converted SolidCraft files also have `output.open`, the
same-origin link to open the design in the configured web app. FreeCAD macros have no `open`
field. The link is built when the report is returned, from the current server settings; see
[Open in SolidCraft](web.md#open-in-solidcraft).

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

Unused construction helpers have `history.features[].status: "not_needed"` and an Info
loss `helper not converted: nothing uses it`. Helper Info notes remain present when another
feature or a replay mismatch flattens the history. A used datum whose placement is already
resolved in its dependent sketch, pattern, or datum also has status `not_needed`. Used helpers
that cannot be resolved keep status `blocked` and a specific `reason`. These statuses
measure mapping eligibility; `mode` and `verification` still determine whether history
was written and replayed successfully.

Partial history adds `mode: "partial_history"` and `verification.status: "partial"` within the
existing schema; existing values retain their meanings. `history.mapped` counts retained
source features marked `mapped` or `not_needed`, not mapping eligibility after the blocker.
Later body-building features have status `blocked` and a reason saying they follow the
blocker. Resolved later sketches can still be `mapped`. Counts exclude suppressed features.
Orphan sketches without source feature rows are retained but do not increase source feature
counts. An assembly with partial component history also reports `partial_history` / `partial`;
the reason identifies each affected part. Hidden bodies are included in SolidCraft's numeric
evaluation, so evaluating the whole saved design is not a visible-body comparison.

Assembly visibility reports count source-hidden placements separately from retained
editable history. An Info `visibility` note, `N editable-history component placement(s)
are placed hidden`, counts the latter across repeated and nested placements. Both use
the same bounded hidden-occurrence paths; a Degraded note reports paths left shown
past the 4096-path limit.

Assembly placement validation uses the existing `losses` and `completeness` fields.
No placement-specific schema fields are added. An Info loss with subject `placements`
says `N occurrence(s) placed by a reflection were written with a mirrored copy of their
component`; the count covers occurrence records in the written component definitions.
Mirrored definitions share reflected geometry and keep imported geometry only; a Degraded
`history` loss explains discarded history, and a Dropped `joints` loss explains joints
touching mirrored components. Non-finite, scaled, or sheared placements outside the 1e-6
orthogonality/determinant tolerance produce Dropped `placements` losses with a zero-based
occurrence index local to the containing assembly, name, and kind. They are excluded from
`completeness.placed` and counted in `completeness.left_out` at stage `write`. Tiny numeric
drift is re-orthonormalised before writing.

### DXF pages (additive to `dutchman.convert/1`)

`convert --to dxf --json` adds `pages`, in source page order. Each entry has
`name` (FreeCAD object name), `path`, `format` (`dxf/AC1015`), `units` (`mm`),
`entities_per_layer` (a map including empty declared layers),
`dimension_representation` (`lines_and_text` when all visible dimensions are written,
`partial_lines_and_text` when some have only text or are omitted, `text_only`
when all dimensions have labels but no lines, `partial_text_only` when some labels
are omitted and none have lines, `omitted_missing_ir` when none can be written,
or `none` when the page has no visible dimension), and
`losses` with the same fields as conversion losses. Info notes describe kept content;
Degraded notes describe presentation changes; Dropped notes name unavailable view,
dimension and template content and unmapped metadata/resources. IR gaps use
`stage: "project"`, `upstream: "cadmpeg"`; writer omissions use `stage: "map"`. The top-level
`losses` includes every page's mapping losses prefixed by page name, plus decode
and unassigned-object losses. `output.path` names the first page and `output.format`
is `dxf/AC1015`; consumers must use `pages` to find every output. Failures use the
existing error envelope and write no page results. Publication I/O failures can
leave already-written pages on disk.

`dutchman.formats/1` adds `drawings`, an array of drawing capabilities with `input`,
`name`, `output` and `converts`; `input` uses the existing native format id
(`freecad_document`). The FreeCAD TechDraw entry describes DXF
page output and its reported limitations. Existing `formats` fields keep their meaning.

## `dutchman.diagnostics/1`

Live diagnostics are opt-in on CLI `convert`, `convert-all`, and `watch`, MCP `convert`
and `convert_all`, and the desktop app. See [locations and sharing](troubleshooting.md#diagnostic-logs-for-unfinished-runs).
`--diagnostics-default` creates a unique UTC timestamp file in the platform's diagnostic
folder, retaining ten logs. `--diagnostics PATH` appends to an explicit file without retention.
MCP uses `diagnostics_default: true` or `diagnostics: "/absolute/path/run.jsonl"`.

Each line is one JSON object. Common fields are `schema: "dutchman.diagnostics/1"`,
`type`, `file` (the anonymous report's salted file id), and `elapsed_ms` since that
conversion's start. A batch/watch log interleaves conversions; group by `file` and each
`start`/`end` pair. Repeated conversions of one file can reuse its id within a run.

| `type` | Other fields |
|---|---|
| `start` | `dutchman` version; `cadmpeg` version and pinned revision; `os: {family, name, version}`; `cpu_count`; `total_memory_bytes`; `available_memory_bytes`; `choices` (the shared preset/history/sketches/joints choices object); `threads` (chosen concurrent conversions); `input: {format, size_bytes}`; `members` (Fusion ZIP member list, or null). |
| `stage` | `stage`: `input`, `decode`, `project`, `map`, `write`, or `solidcraft`. Written when the stage changes, including the initial stage. |
| `design` | `count` of successful designs read and `total` (null while unknown). Written when these counts change. |
| `heartbeat` | `stage`; `rss_bytes`; `cpu_ticks` (process user + system CPU ticks); `cpu_ticks_per_second`; `cpu_ms` (derived process CPU time); `member` (last started member id, or null). Every 30 seconds from an independent timer. Linux reads `/proc`; unavailable resource measurements are null. Windows and macOS use sysinfo; their CPU ticks are milliseconds (`cpu_ticks_per_second`: 1000). |
| `member` | `member` (salted id); `event`: `start`, `finish`, or `fail`; `ms` (0 at start, duration at finish/failure); `code` (fixed decoder error category on failure, else null). |
| `phase` | `phase` (fixed category); `event`: `start` or `finish`; `ms` (0 at start, elapsed duration at finish); `bytes` (source container length, not inflated bytes). |
| `end` | `error` (anonymous failure facts, or null on success); `ok` (boolean); `stage` last reached; `losses` (code-to-count object); `repeated_losses` (total extra occurrences of identical losses); `member_decodes` (member-id-to-start-count object); `redecoded` (sum of starts after each member's first start). |

`end.error` contains `stage` (failure stage, or null if unavailable), `kind` (fixed error
category, or `other`), `findings` (cadmpeg check-code counts), and `curve_kinds` (counts of
referenced carriers for parameter-domain, carrier-reachability, and geometry-consistency
errors). Only error/blocking findings count. Curve categories are `procedural`, `nurbs`,
`analytic`, `other`, and `unavailable`. Each carrier counts once per finding; counts are
empty when no structured reference can be resolved. These fields contain no finding text,
entity ids, names, paths, or geometry values. CLI/MCP JSON errors from the conversion core
also add `kind` and, for check failures, `findings` and `curve_kinds`; their existing
`message` stays private. Other failures use diagnostic kind `other`.
Model check failures have failure stage `project`, independently of the last progress stage.

Phase categories are `codec_selection`, `container_prepare` (codec entry to the first
member start), `container_compose` (between member attempts and after the last one), and
`model_validation` (including any geometry-only recheck). Without member callbacks,
`container_prepare` covers the whole codec call. A finish closes the timed scope, including
error returns; it does not claim success. Start records identify a phase even if the
process is killed before it finishes. Cadmpeg currently supplies no callbacks within
container preparation, so inflation, scanning, classification, and reference metadata
work share that phase rather than claiming separate measured durations.

Each `members` entry is `{id, compressed_bytes, uncompressed_bytes, nested_content_bytes}`.
The last value is the sum of a nested design archive's declared uncompressed content sizes,
or null when unavailable or beyond the bounded metadata-read budget (64 MiB total).
Directories and metadata entries are included with anonymous ids. Archive directories are
bounded to 10,000 members and the existing memory estimator's directory size limits.

Per-member records appear when the decoder reports them. The pinned Fusion decoder reports
actual `.f3z` member decode attempts, including re-decodes during composition; these are
not unique design completion counts. Error categories are `wrong_format`, `malformed`,
`invalid_input`, `truncated`, `resource_limit`, `unsupported_dialect`, `unsupported_target`,
`insufficient_depth`, `not_implemented`, `io`, or `other`. They contain no exception text.
`member_decodes` tracks at most 100,000 distinct ids per conversion.

```json
{"schema":"dutchman.diagnostics/1","type":"member","file":"f0123456789abcdef","elapsed_ms":245,"member":"fabcdef0123456789","event":"fail","ms":23,"code":"resource_limit"}
{"schema":"dutchman.diagnostics/1","type":"end","file":"f0123456789abcdef","elapsed_ms":246,"ok":false,"stage":"decode","losses":{},"repeated_losses":0,"member_decodes":{"fabcdef0123456789":2},"redecoded":1}
```

Names, paths, geometry, file contents, messages, and the salt are never written. Loss codes
use the same generic code/subject mapping as [report-anon](report-anon.md); messages are
used only in memory to count repeated identical losses. A combined anonymous report and
diagnostic log share the run's salt and file ids. Member ids use that same derivation on
literal member paths without resolving them on the host filesystem, so the `start` list,
member events, and heartbeats agree.

Every record is appended and flushed as it occurs. `sync_data()` runs at least every five
seconds, at start/end, and on stage changes. Complete lines remain readable after a kill;
ignore a trailing incomplete line. A missing `end` means that conversion did not finish
reporting. A supervised worker's timeout/cancellation writes an error `end` if its parent
is still alive. Fields may be added within this schema version.

Default diagnostic filenames use UTC, computed from `SystemTime` without a subprocess.
Start-record OS and total-memory metadata are collected only with diagnostics enabled.
On Windows `os.version` is `"windows"`. Windows and macOS read total and available memory
through sysinfo without launching a memory-probe command. Linux reads `/proc` and caps
available memory by remaining cgroup headroom, including ancestor limits. Memory-budget
refusals happen before decode, never at `map`; Standard accepts the history-free estimate
when it fits.

## `dutchman.diagnose/1`

`dutchman diagnose INPUT [--out FOLDER] [--time-limit T] [--memory-limit GB] [--json]` writes
`FOLDER/diagnose/report.json` in this schema, rewriting it after each part so an interrupted run
keeps its results. An existing `diagnose` folder is refused. Every field is a number, a fixed
category or a code; the report holds no name, path, identity, message or geometry, and is safe
to share. See [Find what breaks in a large private file](diagnose.md).

| Field | Meaning |
|---|---|
| `schema` | `dutchman.diagnose/1`. |
| `versions` | `dutchman` release and `cadmpeg` version. |
| `input` | `format` (Dutchman's format id) and `size_class` (as in `dutchman.repro/1`). |
| `status` | `running` (in progress, or interrupted), `complete`, or `read_failed` (the design could not be read within the limits, so no part was tried). |
| `limits` | `time_limit_s` per part and `memory_limit_mb` (null for none). |
| `read` | How reading the design went: `outcome` (`ok`, `failed`, or null while reading), `time_ms`, `peak_memory_mb`, and, if it failed, `stage`, `error_kind` and `findings`. |
| `summary` | Counts: `parts`, `finished`, `ok`, `failed`, `timed_out`, `out_of_memory`, `container`, `not_run`, `restarts` (how often the design was read again after a part stopped the converter) and `elapsed_ms`. |
| `parts` | Every part, ranked: failures first (slowest first), then parts not run, then parts that converted (slowest first, biggest first among equals), then sub-assemblies with nothing of their own. |
| `advice` | Fixed next steps: `{"code": "convert_part", "part": N}`, `{"code": "repro", "part": N}`, `every_part_converts` or `read_failed`. |

Each of `parts`:

| Field | Meaning |
|---|---|
| `rank` | Position in this ranking, from 1. |
| `part` | The part's number, from 1: the number `dutchman convert --part` takes and `dutchman repro` uses. |
| `parent` | The number of the sub-assembly holding it, or null. |
| `kind` | `part`, or `assembly` for a sub-assembly. |
| `depth` | How many sub-assemblies it sits inside. |
| `outcome` | `ok`, `failed`, `timed_out`, `out_of_memory`, `container` (a sub-assembly, or a component, with no geometry of its own: nothing to convert), or `not_run` (given up after too many restarts, or the run was stopped). |
| `stage` | The stage it stopped at (`map`, `write`, ...), or null. A part stopped for time or memory is at `map`. |
| `error_kind` | A fixed category such as `restate`, `encode`, `cadmpeg_checks`, `no_geometry`, `panic` or `crash`, or null. |
| `findings`, `curve_kinds` | cadmpeg check codes with counts, and referenced curve categories, as in `dutchman.repro/1`. |
| `time_ms` | How long the part took (until it was stopped, if it was). |
| `peak_memory_mb`, `extra_memory_mb` | The most memory the converter held while on the part, and how much of that came on top of what it held before the part. |
| `bodies`, `entities` | Size: bodies, and all the entities (faces, edges, features, sketches and the rest) the part holds. |

`FOLDER/diagnose/local-key.json` uses `dutchman.diagnose-local/1`, with `warning` and the
private `input` path and each `parts` entry's `part` number and `name`. It must stay local, and
is written readable by its owner only.

The `--json` result also uses `dutchman.diagnose/1` with `ok`, `summary` (text), `status`,
`paths` (`report`, `text`, `local_key`), `share` and `private` (which files are which) and
`report` (the content of `report.json`). It holds local paths and stays local.

`dutchman convert --part N` and the part-stop message of a conversion that cannot fit in memory
use the same numbers.

## `dutchman.repro/1`

`dutchman repro INPUT --out FOLDER [--json]` and the MCP `repro` tool write a local kit.
An existing `FOLDER/kit` is refused. Temporary conversion output and diagnostics live in a
random temporary directory under `FOLDER` and are removed when the call returns normally,
including on errors. A killed process can leave that directory behind; keep it private.
The command performs the default SolidCraft conversion and its normal checks and recovery.
It makes no network requests.

`kit/anonymous.json` is the shareable artifact. It is built from fixed categories and
counts, using the diagnostic log's allowlist for the failure object. It contains no source
names, paths, entity identities, file hashes, geometry, parameter values, raw messages,
exact input byte lengths, or timings.

| Field | Meaning |
|---|---|
| `schema` | `dutchman.repro/1`. |
| `versions` | `dutchman` release and pinned `cadmpeg` version plus commit. |
| `input.format` | Stable Dutchman format id, or null. |
| `input.size_class` | `under_1_mib`, `1_to_16_mib`, `16_to_256_mib`, `256_mib_or_more`, or null if unavailable. Boundaries are powers of two. |
| `status` | `failure` or `no_failure_to_reproduce`. A recovered conversion is a success. |
| `failure` | Null on success; otherwise fixed `stage`, `kind`, error `findings` by code and referenced `curve_kinds` by category, as in diagnostics. Counts describe the original failure. |
| `narrowing.members` | Independent F3Z member trials with one-based `ordinal`, `total`, fixed `status` (`decoded` or `decode_failed`) and `trial_failure` (anonymous stage/category/check facts, or null). Includes tested members only. |
| `narrowing.path` | Ordered steps with `unit` (`part`, `member`, `component`, `occurrence`, `body`, `feature`, `sketch`), one-based `ordinal` and `total`, and for a `feature` its `kind` (a fixed category). `part` is the first step when the kit followed a [`dutchman diagnose`](diagnose.md) report, and its ordinal is diagnose's part number. Each step is scoped to the previously retained unit. |
| `narrowing.from_diagnose` | True when the kit went straight to the failing part of a diagnose report (found in `--out`, or given with `--from-diagnose`). |
| `narrowing.smallest_failing_unit_isolated` | True when a single body or smaller independent feature/sketch subset retained the same stage, category and complete finding-code set. Does not prove the globally smallest model or identify the native operation responsible. |
| `narrowing.limits` | Fixed explanatory sentences for unavailable levels, unsuccessful trials or a trial budget. |
| `retained_unit_kinds` | Null when no checked subset is available; otherwise `surface_kinds`, `feature_kinds` (fixed enum categories; unlisted kinds become `other`), `finding_entity_kinds` (arena names cited by structured errors, or null when unavailable), and `use_curve_outside_carrier` (structurally detected coedge range/carrier-domain mismatch count). Counts cover the retained unit, without carrier values. |

For model check failures, only cadmpeg's checks run again on the retained model. The full
code set must match; counts may decrease. For F3Z, cadmpeg's bounded independent-member
session decodes members before testing their models. Then dependency-closed component,
occurrence, body, feature and sketch subsets are tried in arena order. Dependencies are
retained, so a subset may require several bodies or features. A feature referencing the
same final geometry does not isolate the feature that produced the fault. No native
records are edited and feature regeneration is not supported. At most 256 trials and
100,000 entities per subset are considered. Member composition errors and unstructured
decoder/encoder errors have no check-code signature: no isolated unit is claimed for them.
Writer failures are retried on component/body/feature/sketch subsets. Matching only their
stage and category produces candidate steps, with isolation false and an explicit limit;
the broad category alone does not establish equivalent failures. Mapping, restatement and
any replay run together as one writer operation; this API cannot retry them separately.
Unsupported stages are stated in `limits`.

`kit/local-key.json` uses `dutchman.repro-local/1`, with `warning: "Do not share"` and
the private `input` path, `source_document` name (or null), `members` (tested archive ordinals and names), and `ordinals`: selected steps with `unit`, `ordinal`, `total`, `parent_step` (zero-based index
of the preceding scope, or null), `name` (or null) and a local lookup hint. Ordinals use
CADIR arena order; native CAD programs may show a different order. Names come directly
from the source model. This file must stay local.

`kit/next-steps.md` contains fixed recipes chosen by finding code and carrier kinds,
anonymous ordinal lookup instructions, limits, and exactly what may be sent. A recipe
suggests a candidate; the rebuilt design must reproduce the same stage and code set.
Unknown codes get the body-copy/rebuild recipe. Neither instructions nor anonymous JSON
include local-key content.

The CLI `--json` and MCP response envelope also uses `dutchman.repro/1`, with `ok` (kit
creation succeeded), `summary`, `paths` (`anonymous`, `local_key`, `next_steps`), and
`anonymous` (the artifact content). This envelope contains local output paths and stays
local. A conversion failure still has `ok: true` when the kit was written. MCP includes
`local_key` content only with explicit `include_local_key: true`; keep that response local.
