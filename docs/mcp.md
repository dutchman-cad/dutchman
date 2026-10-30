# The MCP server

The `dutchman` command also dispatches installed Dutchman CAD tools such as `pilot` and
`fixtures` through `dutchman-<tool>` beside the command or on `PATH`. `dutchman --help`
lists them. These external commands provide their own interfaces; the conversion MCP tools
and JSON reports described here retain their existing schemas.

The full CLI also accepts `dutchman part.f3d` (or `dutchman convert part.f3d`) and derives
the output beside the input from `--to`. An existing derived output is kept with exit 2;
choose `-o PATH` or use `--force` to replace it. `--force` conflicts with `--no-clobber`.
Explicit outputs retain their replacement behaviour. MCP output paths remain required except for `convert` with `dry_run: true`.

`dutchman-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server. It lets
an AI assistant convert CAD files with Dutchman on your machine: it gives the assistant four
tools, and each returns the same JSON report as the matching `dutchman … --json` command
([json.md](json.md)).

It comes in every release archive and with `cargo install` (see the [guide](guide.md#install)).
It speaks JSON-RPC over stdio, one message per line, and takes no arguments: the client starts
it and talks to it.

A part that holds an imported solid becomes a base-body Import feature at its place in the
timeline. Later features keep editable history when they map. Stored geometry has no recipe
to lose: the report says, at Info level, “This part holds an imported solid; there is no
feature history to bring over”. A part made only of stored geometry is geometry without
history, with no flattened-history warning.

Sheet-metal history follows the same mapping limits in the [guide](guide.md) as the CLI:
base flanges and full-edge flanges on unchanged rectangular base sheets map when their own
input-edge geometry proves each pick. Unsupported flange dimensions or selections, and hems,
appear as the first blocked feature's reason; the conversion keeps the geometry. K-factor and
relief use the target rule defaults because CADIR does not supply them.

FreeCAD signed horizontal and vertical sketch distances convert to positive dimension
magnitudes at their solved positions. Their signed parameters and expressions remain
available; a dependent magnitude parameter drives each negative distance in SolidCraft.

CLI progress is separate from tool results: `convert --json`, `convert-all --json`,
`inspect --json`, and `watch --json` emit `dutchman.progress/1` JSON lines on stderr
while keeping the final report or watch-event stream on stdout (`--quiet` hides
progress). The MCP server has no progress notification mechanism and continues to return
only the final report. The web app also has no streaming conversion progress mechanism.
Desktop file cards show stage, elapsed time, and five-second heartbeats, with design
counts and a determinate bar when the total is known. Fusion `.f3z` member counts are
not available yet.

When a feature blocks SolidCraft history conversion, earlier editable features and
resolved sketches are kept in a hidden **Editable history (partial)** component. The
original final shape remains visible as an Import after the kept history. Show the
hidden component to continue editing it. The report names the blocker and counts
retained source features; `mode: partial_history` and verification `partial` distinguish
this from full verified history. The reason says whether retained features replayed or
remain unverified. Later sketches with unresolved planes or external geometry are
reported and left out. A sketch that fails replay is retried without constraints, with
the loss reported. If there is no body-building prefix, resolved sketches remain hidden
beside the imported shape for reference. A final shape mismatch preserves the mapped
timeline for correction when it replays; it does not identify which feature needs
correction. SolidCraft's numerical eval includes hidden bodies as well as the visible
final shape.

Assembly visibility reports count source-hidden placements separately from retained
editable history. An Info `visibility` note, `N editable-history component placement(s)
are placed hidden`, counts the latter across repeated and nested placements. Both use
the same bounded hidden-occurrence paths; a Degraded note reports paths left shown
past the 4096-path limit.

Both `convert` and `convert_all` accept `threads`, a whole number at least zero.
`0` (default) selects available CPU parallelism for assembly parts; `1`
uses the serial path. Part count and available memory cap the actual worker count.
Component slices prepare independently and identical empty holders share a model.
SolidCraft parts also prepare independently, including checks in separate temporary folders,
and enter the design in their original order. Designs and ordered reports are identical
apart from `threads: {requested, used}`. Failed preparations retry serially. Native
decoding remains serial; other writers use shared component slicing only. Batch `jobs` limits
files, while `threads` limits part workers per file. `no_memory_check` leaves this
worker cap enabled.

## Choosing what to keep and estimating cost

Both `convert` and `convert_all` accept `preset` (`fast`, `standard`, or `full`, default
`standard`), `history`, `sketches`, `joints`, and `no_memory_check`. The three retention
booleans default to true and only narrow the preset: true cannot enable history in
`fast`. `history: false` is also available as the existing `import_only: true`.
Fast keeps shape only and skips the memory guard. Standard keeps history, sketches,
and joints, leaving history out when its estimate exceeds free memory and
the history-free estimate fits. Full never drops history for memory. Reference sketches
are controlled by `sketches`; sketches needed by retained history stay. Disabling
joints retains stored placements and adds an Info report line. FreeCAD output honours
history only; sketch and joint choices do not control that writer.

A conversion that exceeds available memory refuses with the same message as the CLI:
`this conversion needs about N GB and M GB are free; try --preset <lighter> (or
--no-memory-check to run anyway)`. Set a lighter `preset`, or explicitly set
`no_memory_check: true` to bypass the guard and Standard's history budget. Existing
input and batch worker limits still apply. Standard's automatic history decision is
reported as one Info line under `history`, as described in the [guide](guide.md#choosing-what-to-keep).

Use `convert` with `dry_run: true` for a quick estimate without decoding or writing;
`output` is optional:

```json
{"name":"convert","arguments":{"input":"/models/assembly.f3z","preset":"standard","dry_run":true}}
```

The result contains `ok: true`, `estimate: {peak_bytes, seconds, band}`,
`choices: {preset, history, sketches, joints}`, and `fits` (boolean, or `null` when
memory or the estimate is unknown). `estimate` itself can be `null`. ZIP directory
reads are bounded and no design is decoded. The band is ±60 % for Fusion, ±40 % for
SolidWorks, ±50 % for Inventor parts, and ±100 % for FreeCAD and formats without measurements
(see guide.md). Memory includes fixed process overhead that is not multiplied by history;
its content cost and the time estimate have separate history factors. A pre-read cannot confirm archive assembly structure,
so Standard's budgeted choices are provisional until normal decoding. An over-budget
dry run returns `fits: false`; it writes nothing and does not attempt conversion.
Normal convert reports, including batch results, carry the same `estimate` and
`choices` fields ([json.md](json.md)).


## Set up a client

### Claude Code

```sh
claude mcp add dutchman -- dutchman-mcp
```

On macOS, a blocked SolidCraft check explains how to open SolidCraft once and allow it
with **Open Anyway** in **System Settings > Privacy & Security**, then try again.
Dutchman checks the quarantine attribute for the user-approved flag without changing it.

Optional: if Dutchman cannot find `solidcraft-cli` on `PATH` or inside an installed SolidCraft
app, pass its path. Replace `/path/to/solidcraft-cli` with the installed executable's absolute
path before running this command:

```sh
claude mcp add dutchman --env SOLIDCRAFT_CLI=/path/to/solidcraft-cli -- dutchman-mcp
```

`claude mcp list` shows whether it connected.

### Codex

In `~/.codex/config.toml`:

```toml
[mcp_servers.dutchman]
command = "dutchman-mcp"
```

Optional, inside that same table, add the following line only after replacing the placeholder
with the installed executable's absolute path:

```toml
env = { SOLIDCRAFT_CLI = "/path/to/solidcraft-cli" }
```

### Other clients

Most clients (Claude Desktop, Cursor, Windsurf, and others) take an `mcpServers` entry:

```json
{
  "mcpServers": {
    "dutchman": {
      "command": "dutchman-mcp"
    }
  }
}
```

If needed, add an optional `"env": {"SOLIDCRAFT_CLI": "/path/to/solidcraft-cli"}` member beside
`command`, with a comma between them. Replace the placeholder with the installed executable's
absolute path; on Windows, use `.exe` and escape backslashes as `\\` in JSON.

If the client cannot find `dutchman-mcp`, give its full path as `command` (on Windows,
including `.exe`). `SOLIDCRAFT_CLI` is optional: without it, Dutchman looks on the server's
`PATH` and inside an installed SolidCraft app, and reports the history unverified if neither
provides a usable CLI.

## Tools

`formats` lists all recognised formats, their status `words`,
and what Dutchman converts. Inputs without geometry support or a codec in this build
return a failure describing the reader's current capability. Creo and NX `.prt` inputs
are distinguished by contents. CATIA, Creo View, Rhino, ACIS and STEP use the geometry
path. Each conversion report's `input` also carries `words`, `program` and `extension`.
Status words are `reads geometry`, `opens, no geometry yet`, `recognised, not read yet`,
or `open it in SolidCraft directly` for IGES (`.igs`, `.iges`). IGES is recognised and
refused before decoding by `inspect`, `convert` and `convert_all`, with `error.message`:
`Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import).`
Batch results retain that refusal; IGES does not trigger the CLI or GUI fixtures invitation.
Numbered `.prt.N` saves are identified by contents too; a directory conversion keeps only
the highest revision per part and returns older saves in `skipped_revisions` (`path`, `newer`).
Numbered saves use the part's base output name and keep existing output files.
The CLI and desktop volunteer invitation is absent from MCP and `--json` output.

For `inspect`, `ok: true` means the file was read. Check `conversion.possible` and
`conversion.reason` before converting: inspection can succeed even when conversion cannot.

Unused FreeCAD binders and datum helpers are `not_needed`, with the Info note
`helper not converted: nothing uses it`. Used datum points and coordinate systems can be
carried by resolved sketch placements, pattern axes, or another resolved datum. A used helper without the required
resolved placement or binder source geometry remains a blocker, with its reason in the report.
These Info notes remain when another feature or a replay mismatch flattens the history.
A body inside a hidden container is hidden even when its own flag says it is shown.

FreeCAD external Body and Part links are resolved beside the assembly first, then through
`project` and `search`. A missing linked `.FCStd` file is left out and named in losses and
`completeness.left_out`; available components still convert. Link arrays place each element
once. Grounding and supported Assembly-workbench joints map like other assembly joints.

FreeCAD fillets and chamfers use their base feature's stored edges only when the earlier
result and each pick on SolidCraft's replay are proven. Otherwise `history.features[].reason`
says why the edge selection is refused, and conversion keeps the original geometry.

For sweeps, `history.features[].reason` identifies unsupported corner transitions, section
interpolation, tangent-path extension, or simplification. Paths and guide rails may select
ordered sketch curves or a whole sketch with one non-construction curve. A blocked sweep
keeps the original geometry when converted.

Constant-radius helical sweeps, including FreeCAD AdditiveHelix and SubtractiveHelix,
map to sketch-profile coils when targeting SolidCraft 0.25 patch 1 or newer. Older targets block
the helix with `needs SolidCraft 0.25 patch 1 or newer` and the target version, preserving the
original geometry. The CLI equivalent is `--solidcraft-version x.y.z` on `convert`,
`convert-all`, and `watch` (default: found CLI version, even with `--no-verify`, else
the newest release known at build time, currently 0.27). The rolling 14-day window
is currently 0.18–0.27; older targets down to 0.18.2 get simplified features.
Pitch/height/turn laws, handedness, axis, reversed travel, and Boolean operation carry across. Feature parameter
bindings are retained when available, otherwise dimensions use resolved values. Conical
and spiral sweeps report the missing SolidCraft cone-angle/radial-growth option.

Hole history keeps chamfered entries and tapped holes (the bore and an ISO metric thread
label); thread details it can't keep are `Degraded` losses. Unsupported hole forms,
placements, and ends appear in `history.features[].reason`, and conversion keeps the
geometry when the history can't map.
Entry recesses with drill points report "SolidCraft holes take a drill point only on a
plain drilled hole"; SolidCraft 0.21.0 ignores `tip_angle` on counterbores and countersinks. SolidCraft 0.22.1 and newer support those drill
points, but Dutchman retains this limit for compatibility with older releases.

| Tool | Arguments | Returns | Writes |
|---|---|---|---|
| `inspect` | `input`, and optionally `project`, `search` | `dutchman.inspect/1` | nothing |
| `convert` | `input`, `output` (optional for `dry_run`), and optionally `threads`, `preset`, `history`, `sketches`, `joints`, `no_memory_check`, `dry_run`, `import_only`, `verify`, `solidcraft`, `solidcraft_version`, `verify_timeout`, `overwrite`, `project`, `search` | `dutchman.convert/1` | the `output` file |
| `convert_all` | `inputs`, `out_dir`, and optionally `jobs`, `timeout`, `max_memory_mb`, `journal`, `report_anon`, `report_salt`, `report_key`, `threads`, `preset`, `history`, `sketches`, `joints`, `no_memory_check`, `import_only`, `verify`, `solidcraft`, `solidcraft_version`, `verify_timeout`, `overwrite`, `project`, `search` | `dutchman.convert-all/1` | designs and the default log inside `out_dir`, plus `journal`, `report_anon`, and `report_key` at their specified paths |
| `formats` | none | `dutchman.formats/1` | nothing |

The formats report includes each reader's status and cadmpeg's published headline
`level` (`L0`–`L9`, or `null` when none is published). The level supplements the status.

| Argument | Type | Meaning |
|---|---|---|
| `input` | string | Absolute path of a native CAD file: `.f3d`, `.f3z`, `.ipt`, `.iam` or `.sldprt` or `.FCStd`. |
| `output` | string | Absolute path of the `.solidcraft` file to write. An existing file is replaced, unless `overwrite` is false. |
| `inputs` | array of strings, at least one | Absolute paths of files, and of folders to search (with their subfolders) for files Dutchman knows. |
| `out_dir` | string | Absolute path of the folder for the designs, keeping each input folder's layout. Created if missing. |
| `jobs` | integer, at least 1 | How many files to convert at once. Default: the machine's cores, at most 8. |
| `timeout` | integer, at least 1 | Seconds one file may take before it is stopped and reported as failed. Default 900. |
| `max_memory_mb` | integer | Megabytes one file may hold before it is stopped and reported as failed. Default 8192; 0 for no limit. |
| `journal` | path | A file recording each file's result as it finishes; files it already records, unchanged since, are not converted again. |
| `report_anon` | path | Also write the anonymous report ([`dutchman.report-anon/1`](report-anon.md)) there. The result then carries `report_anon: {"path", "salt"}`; the salt stays private. |
| `report_salt` | 32 hex digits | The salt for the anonymous report's file ids, to get the same ids as an earlier run. |
| `report_key` | path | Write the key from each anonymous id to its file's path there. Keep it private. |
| `preset` | string | `fast`, `standard` (default), or `full`; see [Choosing what to keep](#choosing-what-to-keep-and-estimating-cost). |
| `history`, `sketches`, `joints` | boolean | Narrow the preset; each defaults to true. |
| `no_memory_check` | boolean | Bypass the estimate refusal and Standard's history budget; default false. |
| `dry_run` | boolean | `convert` only: return estimate, choices, and fits without decoding or writing; default false. `output` is optional when true. |
| `import_only` | boolean | Leave feature history out; keep geometry and the preset's reference sketches. |
| `verify` | boolean | Check a converted history in SolidCraft. Default `true`; skipped, and the history marked unverified, when no `solidcraft-cli` is found. |
| `solidcraft` | string | Path of `solidcraft-cli` for the check. Default: `$SOLIDCRAFT_CLI`, else `solidcraft-cli` on `PATH`, else the one inside an installed SolidCraft app (macOS `SolidCraft*.app`, newest first; Windows `Program Files\SolidCraft`). |
| `solidcraft_version` | string | Target release as x.y.z, at least 0.18.2. Default: the found CLI's version capped at newest known, else the newest known at build time (currently 0.27), including when `verify` is false. |
| `verify_timeout` | integer, at least 1 | Seconds each SolidCraft check may run before the history is written unverified. Default 120. |
| `overwrite` | boolean | Replace an existing `.solidcraft` file (default true). When false, an existing file is left alone and that file's conversion fails with "already exists". |
| `project` | string | Absolute path of the project folder an assembly's part files are searched in when they are not at their stored paths. Default: the nearest folder above the assembly holding an Inventor `.ipj`, else two folders up; for `convert_all`, each folder given. See [Finding the part files](inventor.md#finding-the-part-files). |
| `search` | array of strings | Absolute paths of more folders to search, with their subfolders, for part files the project folder does not hold. |

An unknown argument is refused (`unknown argument bogus`), as is a missing required one
(`output is required`).

Paths are on the machine running the server. Give absolute ones: a relative path is read
against the server's working directory, which the client chooses.

### Diagnostic logs during conversion

Both `convert` and `convert_all` accept optional `diagnostics` (an absolute file path), or
`diagnostics_default: true` for a unique timestamp file in the platform's default diagnostics
folder. They cannot be combined. Explicit files are appended without retention; the default
folder keeps the newest ten timestamp logs. [The guide](guide.md#diagnostic-logs-for-unfinished-runs)
lists each OS folder and how to review and send a log.

For example, call `convert` with `{"input":"/abs/part.f3d","output":"/abs/part.solidcraft",
"diagnostics":"/abs/run.jsonl"}`. `dry_run` writes no diagnostic log.
The log is [`dutchman.diagnostics/1`](json.md#dutchmandiagnostics1): records are flushed as
conversion proceeds, with disk syncs at least every five seconds and at stage changes,
and resource heartbeats every 30 seconds. It survives an interrupted process and contains
no names, paths, design contents, geometry, or messages. Per-member records appear when the
decoder reports them. `convert_all` with `report_anon` shares its salt and ids with diagnostics;
keep the salt and any `report_key` private. These extra files do not change conversion report
schemas or the output format.

## Example calls and replies

Each reply carries the report twice: as `structuredContent`, and as the same JSON in a text
`content` item for clients that read only text. The replies below show `structuredContent`,
with `content` shortened to `…`.

Inspect a file:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect","arguments":{"input":"/home/me/designs/solidworks_cube.sldprt"}}}
```

```json
{"jsonrpc":"2.0","id":2,"result":{"isError":false,"content":"…","structuredContent":{
  "schema":"dutchman.inspect/1","ok":true,
  "input":{"format":"solidworks_part","format_name":"SolidWorks part (.sldprt)","path":"/home/me/designs/solidworks_cube.sldprt"},
  "geometry":{"bodies":1,"faces":6},
  "conversion":{"possible":true,"reason":null},
  "mode":"import",
  "verification":{"status":"not_applicable","reason":"the design carries no feature history"},
  "history":{"features":[],"mapped":0,"total":0},
  "losses":[
    {"level":"Degraded","stage":"project","upstream":"cadmpeg","code":"sldprt/source.dialect-unverified","subject":"sldprt/source.dialect-unverified","message":"the document carries no swSolidWorks swVersion declaration, …"},
    {"level":"Degraded","stage":"project","upstream":"cadmpeg","code":"sldprt/config.inferred-without-native","subject":"sldprt/config.inferred-without-native","message":"1 configuration state(s) are inferred from geometry partitions without native configuration definitions."}]}}}
```

Convert one file:

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"convert","arguments":{"input":"/home/me/designs/synthetic_cube.ipt","output":"/home/me/designs/cube.solidcraft"}}}
```

```json
{"jsonrpc":"2.0","id":3,"result":{"isError":false,"content":"…","structuredContent":{
  "schema":"dutchman.convert/1","ok":true,
  "input":{"format":"inventor_part","format_name":"Inventor part (.ipt)","path":"/home/me/designs/synthetic_cube.ipt"},
  "output":{"format":"solidcraft/23","path":"/home/me/designs/cube.solidcraft"},
  "mode":"import",
  "verification":{"status":"not_applicable","reason":"the design carries no feature history"},
  "history":{"features":[],"mapped":0,"total":0},
  "losses":[
    {"level":"Degraded","code":"inventor/rse.segment-pair-untyped","subject":"inventor/rse.segment-pair-untyped","message":"Retained 1 structurally paired RSe segment(s) without record semantics."},
    {"level":"Info","code":"step/attribute.source-record-not-written","subject":"step/attribute.source-record-not-written","message":"7 source attribute record(s) were not written to STEP"},
    {"level":"Info","code":"step/native.source-record-omitted","subject":"step/native.source-record-omitted","message":"67 source-native record(s) were not represented in STEP"},
    {"level":"Info","code":null,"subject":"history","message":"flattened: the design carries no feature history"}]}}}
```

SolidCraft history checks compare area and volume within the measuring mesh tolerances.
Face-count differences alone produce no geometry loss, nor do open-shell warnings when area
and volume agree. Evaluation errors, unmeshable faces, bodies inside out, solids without
volume, and solids that geometry-only output opens as open shells are still reported;
sheets remain exempt from volume and closed-shell checks. Geometry-only output has no
independent source measurements. These use the existing report fields described in
[JSON reports](json.md).

If cadmpeg's model fails its checks, the reader retries after removing history, sketches,
parameters and associated design records, including parameter links on joints. Geometry and
assembly structure still undergo validation. A successful retry reports a `Degraded` loss
with subject `history`, saying parameters were left out and giving the original failure.

A conversion that fails is a tool error (`isError: true`) carrying its failure report:

```json
{"jsonrpc":"2.0","id":4,"result":{"isError":true,"content":"…","structuredContent":{
  "schema":"dutchman.convert/1","ok":false,
  "input":{"format":"solidworks_assembly","format_name":"SolidWorks assembly (.sldasm)","path":"/home/me/designs/a.sldasm"},
  "error":{"message":"SolidWorks assembly (.sldasm) files are not supported yet. Export a STEP file for SolidCraft to open; Dutchman's issues (https://github.com/dutchman-cad/dutchman/issues) say when SolidWorks files will convert."}}}}
```

Arguments a tool cannot take (a missing `output`, an unknown argument, a mistyped value) fail
the same way: a tool error carrying a failure report of the tool's schema, whose `error.message`
says what is wrong. `input` is described when the call names one, else `null`:

```json
{"jsonrpc":"2.0","id":6,"result":{"isError":true,"content":"…","structuredContent":{
  "schema":"dutchman.convert/1","ok":false,
  "input":{"format":"fusion_archive","format_name":"Fusion archive (.f3d)","path":"/home/me/designs/part.f3d"},
  "error":{"message":"output is required"}}}}
```

So a client checks `isError`, and finds the reason in `structuredContent.error.message` (or the
same report as text in `content`) whichever way the call failed.

Convert a folder. When any file fails, the whole call is a tool error, but the other files are
still converted and each has its own report (shortened here):

```json
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"convert_all","arguments":{"inputs":["/home/me/designs/project"],"out_dir":"/home/me/designs/converted","jobs":2}}}
```

```json
{"jsonrpc":"2.0","id":5,"result":{"isError":true,"content":"…","structuredContent":{
  "schema":"dutchman.convert-all/1","ok":false,"converted":3,"failed":1,
  "results":[
    {"ok":true,"input":{"path":"/home/me/designs/project/generated_unit_cube.f3d", …},"output":{"path":"/home/me/designs/converted/generated_unit_cube.solidcraft", …},"mode":"import", …},
    {"ok":false,"input":{"path":"/home/me/designs/project/sub/container_metadata_only.f3d", …},"error":{"message":"cannot read this design: no geometry could be read from this Fusion archive (.f3d) file"}},
    {"ok":true,"input":{"path":"/home/me/designs/project/sub/solidworks_cube.sldprt", …},"output":{"path":"/home/me/designs/converted/sub/solidworks_cube.solidcraft", …},"mode":"import", …},
    {"ok":true,"input":{"path":"/home/me/designs/project/sub/synthetic_cube.ipt", …},"output":{"path":"/home/me/designs/converted/sub/synthetic_cube.solidcraft", …},"mode":"import", …}]}}}
```

FreeCAD conversions retain numeric spreadsheet cells and supported formulas as parameters,
resolving qualified object properties to parameter names. Text cells are omitted silently;
unused numeric cells produce `Info` notes. Expression fallback losses name the cause,
such as an unsupported function or an unresolved reference. The CLI and MCP use the same
losses and verification fields. Verification compares the initial geometry; it cannot
certify source dependencies that the decoder did not expose. See
[FreeCAD expressions](freecad.md#spreadsheet-parameters-and-expressions) before relying on
parameter edits, particularly named sketch constraint expressions and object-label references.

## Safety

- **Everything stays on your machine.** The server reads and writes local files only and
  sends nothing over the network. It runs `solidcraft-cli` locally for the history check.
- **What it writes.** `inspect` and `formats` write nothing. `convert` writes only its
  `output` file, matching the selected format. `convert_all` writes converted designs and
  its ordinary log inside `out_dir`. If supplied, `diagnostics`, `journal`, `report_anon`,
  and `report_key` are also written at their specified paths. `diagnostics_default` writes
  in the platform diagnostics folder and retains its newest ten timestamp logs. A failed file creates no design; a mixed batch
  can still write successful designs, logs, and requested reports. A design is written only
  once it is complete. The server has no list of allowed folders: it can read and write
  wherever the account running it can. Point `output` and `out_dir` at a folder for converted
  designs, and check what your assistant asks for before you approve it.
- **Existing files.** `convert` replaces an existing `output` file, and `convert_all`
  replaces designs of the same name in `out_dir`. Use a fresh folder when that matters.
- **Caps.** Input files over 1 GiB are refused before they are read. One `convert_all`
  converts at most 10,000 files and searches folders at most 32 levels deep. An assembly
  brings in at most 1,000 part files and 100,000 placements, nested at most 32 deep. Each
  SolidCraft check stops after `verify_timeout` seconds. A message over 1 MiB is answered
  with an error and skipped.
- **Hostile files.** A damaged or unexpected file comes back as an error report, never as a
  crash of the server.

For an unreadable archive, the error keeps cadmpeg's cause and adds advice
to download or copy it again, check its extension, and report it on the
[A file didn't convert](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form with the file's anonymous report if the original opens in its CAD program.
For failure reports, request `report_anon`, review that file before sharing, and keep normal
JSON, logs, the journal, the key, and the salt private. Do not attach design files publicly.

Sketch history uses the same conic and constraint mappings as the CLI. Bounded parabolas,
hyperbolas, elliptical arcs up to one turn, and rational quadratic Béziers retain editable
geometry. Uniform periodic sketch B-splines also retain their exact shape through endpoint
knot insertion into a clamped control spline. Unequal periodic weights or nonuniform/repeated
interior knots name SolidCraft's missing weights or custom knots in the blocker reason.
An unsupported relation appears in `losses` as a counted `Degraded` sketch
message with its reason, while the solved sketch remains mapped. Static sketch patterns
retain point and radius associations; driving count and span parameters still produce a
constraint loss.

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

A flattened conversion keeps mapped sketches with resolved planes hidden after the imported
geometry for reference, with no features built on them. Its losses include one Info line,
`sketches: kept N sketches for reference`, with `; M left out` if any were omitted. Body
verification is unchanged. `import_only` leaves out history and retains reference sketches when the preset keeps
sketches. Use `sketches: false` as well, or `preset: "fast"`, for geometry alone.

SolidCraft assemblies with multiple top-level designs group each design's features in a
uniquely named timeline folder, keeping feature order and leaving joints outside folders.
Each folder added is listed in an Info loss with subject `timeline folders` and message
`"Name": N feature(s) grouped`. Parts and assemblies with one top-level design get no folders.

### FreeCAD output (experimental)

`convert` and `convert_all` accept `to: "freecad"` (default: `"solidcraft"`). For example:

```json
{"input":"/designs/part.f3d","output":"/designs/part.FCMacro","to":"freecad"}
```

Use a `.FCMacro` output path for `convert`; `convert_all` chooses that extension for its
jobs. The GUI uses the same output setting. The result uses `output.format:
"freecad-macro/1"` and the existing history and loss fields. Each macro embeds original STEP
geometry and rebuilds supported features through FreeCAD's public scripting API; parts
whose histories cannot be fully rebuilt retain their mapped prefix and mapped sketches
in a hidden **Rebuilt history (partial)** group, with STEP as the visible final shape when
the prefix differs. The report says `rebuilt N of M features; kept as reference`.
FreeCAD output is unverified and does not invoke SolidCraft or FreeCAD. `import_only` writes STEP geometry alone. See
[FreeCAD output](guide.md#freecad-output-experimental) for feature coverage and running the macro.

Experimental Assembly output keeps supported joints, through sub-assemblies too, with
resolved connector frames, limits, and grounding. Unsupported operands or joint options
produce specific `joints` losses. An inferred grounded part is reported as `Info`. The macro
checks that solving preserves stored placements; failure removes the joints, restores those
placements, and warns in FreeCAD. `import_only` leaves joints out.

The CLI additionally supports experimental `--to fcstd` to run FreeCAD headlessly, save
and verify a native document, and `--to freecad --verify` to check a macro without saving
a document. These options are currently CLI-only; the MCP FreeCAD macro output above
remains unverified. `FREECAD_CMD` selects the headless executable and
`--freecad-timeout SECONDS` sets its 300-second default limit. See the guide for discovery,
measurement tolerances, fallback reporting and the `differs` verification status.

For self-hosted browser conversions, [Dutchman Web](web.md#settings) documents the shared
zip budget, concurrent upload and saved-data limits, private data folder, and allowed browser
origins. Uploads may be refused with 403 (origin check) or 503 (capacity); the CLI and MCP
reports are unchanged.


FreeCAD TechDraw drawing output is available from the CLI:
`dutchman convert part.FCStd --to dxf [--page NAME]`. It writes one
`part-PageName.dxf` per page; `-o` accepts an existing directory or a file base
(an exact file path with `--page`). Defaults require `--force` to overwrite;
explicit outputs replace unless `--no-clobber` is set. Page names are FreeCAD
object names. Pages keep neutral view curves, dimension labels, annotations, template frames, and
resolved title-block text in millimetre page space. Unsupported content is reported
in per-page JSON `pages` results. No page is an error. See [FreeCAD coverage](freecad.md#techdraw-to-dxf).
DXF conversion is not supported yet by convert-all, the MCP convert tool, desktop,
or web. The MCP formats report includes the additive `drawings` capability list.

The desktop app also offers Fast, Standard (default), and Full presets, with Advanced
checkboxes that leave history, sketches, or joints out. Estimates below 0.1 GB show
**under 0.1 GB**, and times below 60 seconds show **under a minute**, using the CLI’s text
formatter. Numeric MCP estimates stay unchanged. The app checks memory before converting; choosing Convert anyway applies only to that run. History
Info messages, including history left out by Standard, appear on its result cards.

SolidCraft conversion results include `solidcraft: {target, found, known_newest, status}`: the selected
target release, discovered CLI release (or null), newest known release, and installed-version
status, even when verification is disabled.
Standard fixes its history budget before decoding for parts and assemblies; if only the
history-free estimate fits, conversion proceeds with one Info explanation. Memory refusals
happen before decoding. See [JSON reports](json.md) for diagnostic platform limits and UTC
log filenames.

With an explicit `solidcraft_version` and `verify: false`, version discovery uses only
cached results; `solidcraft.found` is null when not checked. Otherwise the CLI version
probe has a three-second limit and runs at most once per resolved CLI path per process.
Successful versions are cached across runs until the CLI's size or modification time
changes; see [the guide](guide.md) for cache locations.

SolidCraft reports also include `solidcraft.known_newest` (a version string) and
`solidcraft.status`: `newer`, `too_old`, `older_loses_history`, `current`, or
`none_found`. History notices follow the selected target’s writer gates. These
fields require no network check. See [JSON reports](json.md).

SolidCraft version presentation uses minors: `solidcraft.target` and
`solidcraft.known_newest` now contain `x.y` strings (for example, `"0.27"`), changing their
previous `x.y.z` representation. `solidcraft.found` keeps the full CLI release (for example,
`"0.27.1"`). Status comparisons use minors, so all 0.27 patches are current; 0.28 is newer
and 0.17 is too old. Patch-sensitive writer gates and the internal 0.18.2 verification
floor remain unchanged. Explicit `solidcraft_version` inputs still accept `x.y.z` for
precise gate control.
