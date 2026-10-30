---
name: dutchman-converting
description: Use when someone wants a CAD file (Fusion, Inventor, SolidWorks, FreeCAD, CATIA, Creo View, Rhino, ACIS or STEP) turned into a SolidCraft design (.solidcraft) with Dutchman, through the dutchman-mcp tools or the dutchman command. Covers inspecting first, reader status and refusals (including IGES, Creo and NX), converting one file or a folder, checking the result in SolidCraft, and explaining losses in plain words.
---

# Converting CAD files with Dutchman

The `dutchman` command also dispatches installed Dutchman CAD tools such as `pilot` and
`fixtures` through `dutchman-<tool>` beside the command or on `PATH`. `dutchman --help`
lists them. These external commands provide their own interfaces; the conversion MCP tools
and JSON reports described here retain their existing schemas.

The full CLI also accepts `dutchman part.f3d` (or `dutchman convert part.f3d`) and derives
the output beside the input from `--to`. An existing derived output is kept with exit 2;
choose `-o PATH` or use `--force` to replace it. `--force` conflicts with `--no-clobber`.
Explicit outputs retain their replacement behaviour. MCP output paths remain required.

Dutchman converts native CAD files into SolidCraft designs. Use the `dutchman-mcp` tools when
they are connected (`inspect`, `convert`, `convert_all`, `formats`); otherwise run the
`dutchman` command with `--json`. Both return the same reports, described in Dutchman's
`docs/json.md`.

A part that holds an imported solid becomes a base-body Import feature at its place in the
timeline. Later features keep editable history when they map. Stored geometry has no recipe
to lose: the report says, at Info level, “This part holds an imported solid; there is no
feature history to bring over”. A part made only of stored geometry is geometry without
history, with no flattened-history warning.

For `convert`, `convert-all`, `inspect`, and `watch`, progress goes to stderr: stage and
design updates plus a still-working heartbeat every five seconds with elapsed time.
`--json` makes these `dutchman.progress/1` JSON lines; parse stdout as the final report
(or the existing watch-event stream). `--quiet` suppresses progress. Desktop file cards
show the same information, with a determinate bar when the design total is known. Fusion
`.f3z` updates show stage and elapsed time without a percentage; diagnostic member decode
attempts can repeat and do not establish a unique design completion count. MCP and web conversions return only their final reports; no
progress stream is available.

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

## Conversion choices and memory

Use `threads: 0` (default) for automatic assembly part concurrency, or
`threads: 1` for serial preparation; a positive number bounds workers. Part count and
available memory cap the actual count. The report records `threads: {requested, used}`.
The core slices components in parallel and reuses identical empty holder models. SolidCraft
parts prepare independently and enter the design in the original order, preserving the
design and loss order; worker failures retry serially. Native decoding remains serial.
Batch `jobs` limits simultaneous files, while `threads` limits part workers per file.
The CLI equivalent is `--threads N` on `convert`, `convert-all`, and `watch`.
`no_memory_check` does not remove the part-worker memory cap.

Use `preset: "standard"` by default. `fast` keeps geometry at stored placements,
without history, reference sketches, or joints; `full` keeps everything that maps and
never drops history to fit memory. Standard drops only assembly history when its
estimate exceeds free memory and the history-free estimate fits, with one Info line
under `history` explaining the choice. `history: false` (also `import_only: true`),
`sketches: false`, and `joints: false` narrow any preset. Sketches needed by retained
history stay; with joints off, stored placements stay. FreeCAD output honours history
only, not the sketch or joint choices.

Before a costly conversion, call `convert` with `dry_run: true`; omit `output` if desired.
This pre-reads metadata and bounded ZIP directory information without decoding or
writing. Read `estimate` (`peak_bytes`, `seconds`, `band`, or `null`), `choices`
(`preset`, `history`, `sketches`, `joints`), and `fits` (boolean or unknown `null`).
Estimates are calibrated on measured conversions; the band is ±60 % for Fusion, ±40 % for
SolidWorks, ±50 % for Inventor parts, and ±100 % for FreeCAD and other formats. Fixed process
overhead does not grow with history; memory and time have separate history factors. Times
leave out the SolidCraft check and round up to whole seconds. Assembly parts
stored in separate files are not pre-read, and archive assembly structure is confirmed
only during normal decoding. A dry run does not promise that conversion will succeed.

For Fusion archives with multiple readable embedded `.f3d` designs, the time estimate
uses the requested threads (0 means auto), limited by the converter's shared CPU, item
count and free-memory worker policy. A serial component remains. Before native decoding,
the embedded design count and the converter's 256 MiB minimum worker budget are proxies;
actual body counts can reduce conversion workers further. Single-design files keep their
existing time model. Memory estimates and bands are unchanged by this time refit.

Both converting tools refuse an over-budget run with a message naming the estimate,
free memory, and lighter preset. Prefer that lighter preset. Only use
`no_memory_check: true` when running despite the estimate is intended; it bypasses the
refusal and Standard's history budget, while input and batch worker limits remain.
The CLI equivalents are `--preset`, `--no-history`, `--no-sketches`, `--no-joints`, and
`--no-memory-check`. CLI scripts should use `--yes` to forbid interactive preset
switches; `--yes` and `--json` refuse over-budget runs with exit 2 rather than accepting
extra memory use. Normal JSON reports include `estimate` and effective `choices`.


## Ground rules

- **The user's designs stay on the user's machine.** Never upload, paste, attach or send a
  design file, or its contents, anywhere: not to a web service, an issue tracker, a chat or
  another tool. Work with local paths only. If a bug report needs a file, say so and let the
  user decide what to share; suggest a small design they made themselves for the purpose.
- **Use absolute paths.** Paths are on the machine running the server; a relative one is read
  against the server's working directory.
- **Don't overwrite without asking.** `convert` replaces an existing output file and
  `convert_all` replaces same-named designs in `out_dir`. Write to a new file or folder
  (for example `converted/` next to the originals) unless the user said otherwise.
- **Say "convert", not "import".** The result is a SolidCraft design; the user's file is not
  changed.

## 1. Check the format

Call `formats` for the installed build's formats, status `words` and `converts`.
Its `level` adds cadmpeg's published headline (`L0`–`L9`), or `null` when none is
published; reader status still determines whether conversion can be attempted.
Fusion, Inventor, SolidWorks parts, FreeCAD, CATIA, Creo View, Rhino, ACIS and STEP
can convert geometry. Creo and NX parts are identified by contents
(they share `.prt`) and currently open without geometry, so conversion is refused. SolidWorks
assemblies are recognised without a reader. `input.words` appears on
conversion successes and failures. Reader status describes capability; the actual
history and verification report says what the file kept. CLI `--quiet` hides the volunteer
invitation; JSON and MCP reports never include it.

Directory conversion keeps the highest numbered `.prt.N` revision per part, reports older
saves as skipped, and uses the part's base output name without replacing an existing file.
After a refused conversion (except IGES) or a history that did not fully map, the invitation
links to test-file recipes and offers private help: send the file and its STEP export to
Dutchy in Discord.

What the installed build converts is what its `formats` says; check it rather than this
list when unsure.

For anything else, don't try workarounds such as renaming the file. Tell the user plainly:

- **STEP (`.step`, `.stp`)**: converts through the geometry path; SolidCraft also opens STEP directly.
- **IGES (`.igs`, `.iges`)**: recognised with `input.words: "open it in SolidCraft directly"`
  and refused before decoding: "Dutchman doesn't convert IGES yet: open it in SolidCraft
  directly (File > Import)." Use SolidCraft's own importer. Batch results retain the same
  refusal; do not invite the user to make IGES test files.
- **SolidWorks assemblies (`.sldasm`)**, Creo and NX: reading geometry is not supported yet.
  Suggest exporting STEP from the program that made the design and converting that.
- **Inventor assemblies (`.iam`)** convert, but need their part files next to the assembly
  (or at the paths it stores). If parts are missing, the report lists them as `Dropped`.

## 2. Inspect first

Call `inspect` with `input`. It writes nothing. Read:

- `conversion.possible` and `conversion.reason`: whether conversion can succeed and why it
  cannot. `ok: true` means inspection succeeded, even when conversion is impossible.
- `geometry.bodies`, `geometry.faces`: what was read.
- `mode`: `history` (editable feature history) or `import` (solid geometry only).
- `verification.reason`: why there is no history, when there is none.
- `history.features`: each feature's `status`; a `blocked` one has a `reason`.
- `losses`: everything that will not carry across intact.

Sweep histories support twist, end scale, supported orientations, and one guide rail, with
single-curve whole sketches accepted as paths or rails. A blocked sweep's reason names
unsupported corner transitions, section interpolation, tangent-path extension or
simplification. Explain that the converted design keeps its original geometry.

Holes keep chamfered entries and tapped holes (the bore plus an ISO metric thread label; a
thread's hand, class, and length are listed as losses). Taper, exit treatments, modeled
threads, and entry recesses with drill points block history: explain the reported reason.
"SolidCraft holes take a drill point only on a plain drilled hole" means SolidCraft 0.21.0
ignores drill points on counterbores and countersinks; conversion keeps the original geometry.

Tell the user what to expect in one or two sentences before converting, for example: "This
part has one body. Its history won't carry across (the fillet's input edges can't be proven),
so you'll get the solid shape without the feature history."

Sheet-metal histories map base flanges and full-edge flanges on unchanged rectangular base
sheets when their own input-edge midpoint, tangent, and vertices prove the pick. Fixed
outer-face heights, inside/outside placement, angles, and bend radii map. Other dimensions
or unproven selections keep geometry and explain the blocked feature. CADIR's hem developed
length has no exact mapping to SolidCraft's outer-face length. Do not claim
that K-factor or relief settings were preserved: CADIR does not supply them, so sheet rules
use SolidCraft defaults. Contour flange, bend, unfold/refold and corner-relief operations
are not present in the pinned CADIR version.

## 3. Convert

- One file: `convert` with `input` and `output` (ending in `.solidcraft`).
- Many files or a folder: `convert_all` with `inputs` and `out_dir`. One failure does not stop
  the rest; the call is an error when any file failed, and each file has its own report in
  `results`.
  Designs and the default log go inside `out_dir`; optional `journal`, `report_anon`, and
  `report_key` files go to their specified paths, which may be outside it.
- `solidcraft_version: "0.25.1"` (CLI `--solidcraft-version 0.25.1` on `convert`,
  `convert-all`, or `watch`) chooses the target release. It must be x.y.z and at least
  0.18.2. The default is the found CLI's version, else the newest known at build time
  (currently 0.27.0), even with CLI `--no-verify` or MCP `verify: false`.
  Dutchman targets the current SolidCraft with a rolling 14-day window (currently
  0.18.2–0.27.0); older targets get simplified features with a named reason.
- `preset: "fast"` when the user wants only the shape. `history: false` (also
  `import_only: true`) removes history while keeping the preset's reference sketches and
  joints. Leave `verify` at its default.

A failed conversion comes back with `ok: false` and `error.message`. The message already says
what to do next; pass it on in plain words. A failed file creates no design. A mixed batch
may still write successful designs, logs, and requested reports.

For unreadable archives, suggest downloading or copying again and checking the extension.

### Reporting a problem

When a file fails, loses something it should not, or looks wrong in SolidCraft, and the
original opens in its CAD program, point the user to Dutchman's
[**A file didn't convert**](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form (for a whole library, the **Library test report** form and
`docs/testing-your-library.md`). It asks for the format, the versions, the operating system,
what was expected and what happened, and an anonymous report for that one file:
`convert_all` with only that file in `inputs` and a `report_anon` path (or `dutchman
convert-all --out-dir converted --report-anon report-anon.json /abs/part.f3d`). The user
reads that report before pasting it; ordinary JSON, logs, the journal, the key, and the salt
stay private. Do not attach designs publicly; a user who wants to share a reproducer can
offer it privately in the issue. The issue goes on Dutchman even when the report says
`upstream: cadmpeg` or `upstream: solidcraft`. Draft the issue for the user to post; don't
post it yourself.

When conversion hangs or gets killed, enable a live diagnostic log **before** retrying:
MCP `convert`/`convert_all` accept `diagnostics: "/abs/run.jsonl"`, or
`diagnostics_default: true` for an automatically named log. CLI `convert`, `convert-all`,
and `watch` accept `--diagnostics PATH` or `--diagnostics-default`. Explicit files append;
default folders retain ten timestamp logs. They are Windows
`%LOCALAPPDATA%\Dutchman\diagnostics\`, macOS `~/Library/Logs/Dutchman/`, and Linux
`$XDG_STATE_HOME/dutchman/diagnostics/` (default `~/.local/state/dutchman/diagnostics/`).
The app has **Save a diagnostic log (no names or contents from your file)** and
**Open diagnostics folder** settings, **Show the log** after Stop/completion, and an
unfinished-run notice on next launch. Logs flush every record and sync at least every five
seconds and at stage changes. They hold stages, resource heartbeats, anonymous member
sizes and decoder events, and loss-code counts; no names, paths, contents, geometry, or
messages. Per-member records appear when the decoder reports them. Ask the user to review
and share this JSONL log with the Dutchman issue; keep ordinary logs private. A combined
`report_anon` shares its salt and file ids with diagnostics; keep the salt/key private.
See `docs/guide.md#diagnostic-logs-for-unfinished-runs` and `docs/json.md` for the schema.

## 4. Verify in SolidCraft

Never report success on the reply alone.

If macOS blocks SolidCraft, follow the report's first-run guidance: open SolidCraft once
from Applications, then click Open Anyway in System Settings > Privacy & Security, and
try again. Dutchman checks whether the quarantine attribute has the user-approved flag.
Never remove or change the quarantine attribute.

- `verification.status` `verified`: SolidCraft replayed the history and it matches the
  original in area and volume within the measuring mesh tolerances. `unverified`: written
  without that check (the reason says why, usually no `solidcraft-cli` was found); mention
  that the history has not been checked. `partial`: an editable prefix and resolved
  sketches are hidden beside the visible imported shape; the reason says whether retained
  features replayed. `flattened`: no body-building prefix could be retained, so the design
  keeps the imported shape and any
  mapped sketches with resolved planes, hidden for reference with no features built on them.
  An Info `sketches` line counts sketches kept and, when applicable, left out.
- Assemblies with multiple top-level designs group each design's features in uniquely named
  timeline folders, keeping feature order and leaving joints outside folders. Each folder
  added has an Info `timeline folders` note: `"Name": N feature(s) grouped`. Parts and
  assemblies with one top-level design get no folders.
- Open the design and look at it: `solidcraft-cli eval /abs/path/design.solidcraft`, or with
  SolidCraft's MCP server, start it on the design (`solidcraft-cli mcp --in
  /abs/path/design.solidcraft`) and call `measure`.
- For partial history, use SolidCraft's `document.inspect` to check each body's `visible`
  flag; numerical `eval` includes hidden editable bodies in its body count and totals.
  Compare the visible bodies with the source's final bodies.
- For full history or an import, check `body_count` against `geometry.bodies` from
  `inspect`, that no timeline entry has an `error` or meshing warning, and that area
  and volume are plausible for the part (ask
  the user for a known dimension if you have nothing to compare with).
- A face-count difference alone does not establish a defect: SolidCraft can merge faces on
  one surface. Geometry-only output has no independent source measurements; timeline
  errors, unmeshable faces, bodies inside out, solids without volume, and solids opened as
  open shells still matter. Sheets remain exempt from volume and closed-shell checks.
- If something is off, say exactly what ("SolidCraft could not mesh 3 faces"), and don't
  claim the conversion is fine.

## 5. Explain the loss report

The raw messages are technical. Group them by level and translate:

- **Info**: no effect on the shape. Usually say nothing, or "a few internal records that
  SolidCraft doesn't use were left out."
- **Degraded**: carried across in a weaker form. Name the thing affected:
  `f3d/material.not-transferred` means "the shape converted, but its materials did not
  (check the colours too); you can set them again in SolidCraft." A sketch dimension "retaining native
  operands" means "the sketch is the right size, but that dimension may not drive it."
- **Dropped**: left out. Always mention these, with the part or feature named.
- History: say which feature stopped it, from `verification.reason` or the first `blocked`
  feature, and that the shape itself is complete.

Keep it short, lead with what the user got ("You have an editable design with 7 features"
or "You have the solid shape, without its feature history"), then what is missing. Don't
call the user's file corrupt or invalid; when Dutchman cannot read something, it is not
supported yet.

## Command line equivalents

```sh
dutchman formats --json
dutchman inspect --json /abs/part.f3d
dutchman convert --json /abs/part.f3d /abs/converted/part.solidcraft
dutchman convert-all --json --out-dir /abs/converted /abs/project
```

Exit codes: 0 converted, 1 could not convert (report has `ok: false`), 2 usage error.

FreeCAD signed horizontal and vertical sketch distances convert to positive dimension
magnitudes at their solved positions. Their signed parameters and expressions remain
available; a dependent magnitude parameter drives each negative distance in SolidCraft.

When a sketch reports `constraints not carried`, translate the count and reason as lost
editability: the solved geometry remains exact. Coordinate, intersection, coradial,
endpoint tangency, curvature, point symmetry, regular polygon, and repeated dimensions can
now carry across. Static patterns keep point and radius associations, but driving count
or span parameters still need a loss. Bounded parabolas and hyperbolas, elliptical arcs
up to one turn, rational quadratic Béziers, equal-weight rational splines, and uniform
periodic splines map. Periodic splines are clamped exactly by endpoint knot insertion;
unequal weights or nonuniform/repeated interior knots name the weights or custom knots
SolidCraft control splines cannot store. FreeCAD compact periodic sketch knots currently
need normalization in cadmpeg before reaching the mapper. Selected multi-piece elliptical
arcs, unbounded conics, other weighted/nonuniform splines, and text without solved outlines still prevent the sketch's history from mapping.

FreeCAD numeric spreadsheet cells and supported formulas can remain editable parameters.
Qualified object properties bind through decoded parameter identities; renaming preserves
links. Text cells are omitted silently, and unused numeric cells get `Info` notes.
A value-only expression loss gives a specific cause: unsupported function or conditional,
unresolved or ambiguous reference, cycle, or unsupported syntax. A verified initial shape
does not certify edit behavior. The current decoder omits named sketch constraint bindings
such as `.Constraints.width`; their dimensions stay at their evaluated values. Object-label
references require decoded label identities. Check geometry after editing parameters; see
[FreeCAD expressions](../../docs/freecad.md#spreadsheet-parameters-and-expressions).

Constant-radius fillets and equal-setback chamfers map when the feature's own input edge
geometry proves which edge each one acts on in SolidCraft's replay before the blend.
Unused FreeCAD binders and datum helpers are `not_needed`, with the Info note
`helper not converted: nothing uses it`. Used datum points and coordinate systems can be
carried by resolved sketch placements, pattern axes, or another resolved datum. A used helper without the required
resolved placement or binder source geometry remains a blocker, with its reason in the report.
These Info notes remain when another feature or a replay mismatch flattens the history.
A body inside a hidden container is hidden even when its own flag says it is shown.

FreeCAD can supply that geometry from an earlier base feature's stored shape, including
when its intermediate body is hidden. Unproven source membership or an ambiguous replay
pick stops body-building history before the blend, with the reason reported. Earlier editable
features and resolved sketches remain hidden beside the imported final shape when possible.
`--no-verify` cannot prove blends.

Whole-sketch hole placements exclude construction entities; explicit entity selections
can still use construction points or circle centres.

For mirror-containing composite patterns and repeated full-turn endpoints, an attempted
replay that cannot finish keeps the original geometry and reports why. Turning verification
off, or having no usable CLI, still writes unverified history.

Composite feature patterns containing resolved mirrors can map to several ordered timeline
features while retaining one result per source feature in the history report. Each stage
repeats the previous instance set. Chained stages require a target of SolidCraft 0.25.1
or newer; older targets report `needs SolidCraft 0.25.1 or newer` and keep the original
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

Constant-radius helical sweeps (FreeCAD AdditiveHelix and SubtractiveHelix) keep their
sketch profile, axis, pitch/height/turn law, handedness, reversed travel, and Boolean
operation as sketch-profile coils only for targets SolidCraft 0.25.1 or newer. Older
targets block the feature with `needs SolidCraft 0.25.1 or newer` and the target version,
preserving the original geometry. Feature parameter
bindings drive dimensions when available; otherwise their resolved values are kept.
Conical helices and spirals remain blocked because SolidCraft's coil has no cone-angle
or radial-growth option. Check replay verification as for other mapped history.

When cadmpeg's model fails its checks, Dutchman retries after removing history, sketches,
parameters and associated design records, including parameter links on joints. A successful
retry retains geometry and assembly structure, validates them, and reports a `Degraded`
`history` loss explaining that parameters were left out and naming the original check failure.
Duplicate parameter names cannot block this geometry-only validation.

FreeCAD assemblies may hold only external `App::Link` references. Keep their sibling
`.FCStd` parts beside the assembly, or supply `project` and `search` folders. Missing links
are named losses in `completeness.left_out`; the remaining components still convert.
Link arrays place each element once. Grounding fixes the part, and unsupported assembly
joints are reported while stored placements are retained.

### FreeCAD output (experimental)

When FreeCAD is the requested destination, pass `to: "freecad"` to `convert` or
`convert_all`; use an output path ending in `.FCMacro` for a single conversion. The CLI
form is `dutchman convert INPUT --to freecad -o OUTPUT.FCMacro`. This does not require the
FreeCAD input feature. Tell the user to run the result in FreeCAD with **Macro > Macros… >
Execute**, or `freecadcmd OUTPUT.FCMacro`, then save the document as `.FCStd` if needed.
The macro embeds original STEP geometry; parts whose features cannot be rebuilt remain
imported geometry, with the mapped prefix and mapped sketches hidden in **Rebuilt history
(partial)** when available. Explain `rebuilt N of M features; kept as reference`,
`history.features` and their reasons, and that the history is unverified until the macro
runs. The original remains available for comparison; a runtime
failure restores it and warns in FreeCAD. Parameter values are kept; source expressions
remain inert text. See `docs/guide.md#freecad-output-experimental` for the supported cases and limits.
Assembly macros also write supported joints (through sub-assemblies too) with frames,
limits, and grounding. Explain `joints` losses: unsupported operands/options retain stored
placements, and an ungrounded joint chain gets an inferred grounded part (`Info`). A failed
Assembly solve removes the joints, restores stored placements, and warns in FreeCAD.
`import_only` leaves joints out.

For CLI conversions, `--to fcstd -o OUTPUT.FCStd` runs FreeCAD headlessly, saves the native
document and verifies every part against embedded STEP. `--to freecad --verify` checks a
macro without saving a native document. Set `FREECAD_CMD` to one headless executable path (or the Flatpak form)
when discovery cannot find it; `--freecad-timeout SECONDS` defaults to 300 and kills a slow
process. Explain `differs` using the named part and reason; a runtime fallback restores
STEP and adds a specific loss. A macro may be written with `not checked: FreeCAD not found`;
native output requires working FreeCAD. These extra flags are CLI-only and experimental.

For browser conversions, consult [Dutchman Web](../../docs/web.md) for upload and storage
limits and HTTPS setup. A 403 means the browser origin was refused; a 503 means capacity is
unavailable. Retry after saved jobs expire or adjust the documented server settings.

## Watch an inbox with the CLI

For a local, self-hosted inbox use `dutchman watch INBOX --out-dir OUTPUT --json`.
Polling defaults to 5 seconds and settling to 10 seconds (`--interval`, `--settle`).
`--once` handles one scan for cron, and uses the usual bounded batch child workers and
conversion flags. The inbox is only read. Results and `dutchman.convert/1` report sidecars
(`.report.json` or `.failed.json`) go under OUTPUT with relative folders preserved.
Existing unrelated outputs are skipped. SHA-256 state in `.dutchman-watch.json` prevents
retries of recorded content, including failures, after restart. `--json` is an NDJSON stream
of `dutchman.watch-event/1`, not a batch report. Ctrl-C/SIGTERM allows running work a
30-second grace period and saves state. This command has no MCP counterpart.


FreeCAD TechDraw drawing output is available from the CLI:
`dutchman convert part.FCStd --to dxf [--page NAME]`. It writes one
`part-PageName.dxf` per page; `-o` accepts an existing directory or a file base
(an exact file path with `--page`). Defaults require `--force` to overwrite;
explicit outputs replace unless `--no-clobber` is set. Page names are FreeCAD
object names. Pages keep neutral view curves, dimension labels, annotations, template frames, and
resolved title-block text in millimetre page space. Unsupported content is reported
in per-page JSON `pages` results. No page is an error. See [FreeCAD coverage](../../docs/freecad.md#techdraw-to-dxf).
DXF conversion is not supported yet by convert-all, the MCP convert tool, desktop,
or web. The MCP formats report includes the additive `drawings` capability list.

The desktop app also offers Fast, Standard (default), and Full presets, with Advanced
checkboxes that leave history, sketches, or joints out. It shows available estimates and
checks memory before converting; choosing Convert anyway applies only to that run. History
Info messages, including assembly history left out by Standard, appear on its result cards.
