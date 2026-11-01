# Getting started

## Project inputs

Files whose names start with `~$` are lock files. They are ignored in project planning
and excluded from job, failure and skipped-file counts.

ZIP directory data and ZIP64 extensible records are limited to 16 MiB each before
the ZIP directory is allocated; entry budgets count every declared entry.

Folders and project ZIPs can be passed directly to `dutchman convert` or `convert-all`.
The default output is a new `Project (SolidCraft)` folder beside the input. Project planning
selects top-level Inventor assemblies and unused parts. SolidWorks assemblies are not read
yet, so their parts convert independently. Same-name adjacent STEP exports are fallbacks
for failed native conversions. See [Project inputs](command-line.md#project-inputs).

Dutchman Convert turns native CAD files into [SolidCraft](https://github.com/tangency-labs/solidcraft)
designs (`.sc`). This page takes you from install to an open design in four steps: install, convert, read the report, and open the result; the
[docs list](../README.md#docs) has everything else.

What changed in each release: the [0.2.5 release notes](../release-notes/0.2.5.md) and the
[changelog](../CHANGELOG.md).

## Install

- **Mac:** `brew install --cask dutchman-cad/tap/dutchman-convert` for the app, or
  `brew install dutchman-cad/tap/dutchman` for the commands.
- **Windows:** `scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket`,
  then `scoop install dutchman-cad/dutchman`.
- **Any system:** a [release archive](install.md#from-a-release-archive), [Docker](docker.md),
  or [from source](install.md#from-source).

[Install](install.md) has every option, checksums, and the one-time
[first-run approval](install.md#opening-the-app-the-first-time) on macOS. Install
[SolidCraft](install.md#solidcraft-optional-recommended) too: Dutchman uses it to check each
converted history. The supported SolidCraft window is currently 0.18–0.29.

## Your first conversion

You need a native CAD file. Dutchman has no public sample designs yet, but its tests use
small synthetic files that are fine to try: they are in
[`crates/dutchman-core/tests/fixtures/`](../crates/dutchman-core/tests/fixtures/), with
where each came from in that folder's README. None is a real design. `solidworks_cube.sldprt`
is a 10 mm cube; `generated_unit_cube.f3d` and `synthetic_cube.ipt` are the same cube as
Fusion and Inventor files. To download them:

```sh
fixtures=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/crates/dutchman-core/tests/fixtures
curl -fLO "$fixtures/solidworks_cube.sldprt"
curl -fLO "$fixtures/generated_unit_cube.f3d"
curl -fLO "$fixtures/synthetic_cube.ipt"
```

```sh
dutchman convert solidworks_cube.sldprt cube.sc
```

```text
SolidWorks .sldprt: reads geometry
converted  solidworks_cube.sldprt -> cube.sc (geometry without history)
dutchman: Degraded: sldprt/source.dialect-unverified: the document carries no swSolidWorks swVersion declaration, so no declared identity was verified. …
dutchman: Degraded: sldprt/config.inferred-without-native: 1 configuration state(s) are inferred from geometry partitions without native configuration definitions.
dutchman: Info: step/design.parametric-records-omitted: 1 parametric/design record(s) were not represented in STEP: configuration=1
dutchman: Info: step/native.passthrough-record-omitted: 2 uninterpreted passthrough record(s) were not represented in STEP
dutchman: Info: step/native.source-record-omitted: 1 source-native record(s) were not represented in STEP
dutchman: Info: history: flattened: the design carries no feature history
dutchman: (repeated notes are grouped and long notes shortened; --verbose prints every note in full)
Want to help Dutchman read SolidWorks better? If you have SolidWorks, you can make a few small test files from short recipes: https://dutchman-cad.github.io/fixtures/ or type /volunteer in our Discord (https://discord.gg/yTcseBEdeu).
```

The synthetic SolidWorks cube lacks native version and configuration metadata, so those
warnings are expected for this sample. The internal records omitted from STEP do not change
its shape: the written geometry should be one 10 mm cube (1,000 mm³). The sample has no
editable feature history. Check the result in SolidCraft as [below](#open-the-result-in-solidcraft).

The exit code is 0, and the first line, on stdout, says what was written where: here the
geometry, as this file has no feature history (else `editable history, verified in SolidCraft`
or `editable history, unverified`). After progress updates, the lines on stderr are the loss report, explained
[below](#reading-the-loss-report). Long notes are cut to their first sentence, marked `…`;
`--verbose` prints them whole. An explicitly named existing output file is
replaced (`--no-clobber` keeps it, and the conversion fails instead), and nothing is written
when the conversion fails.

SolidCraft output accepts `.sc`, `.solidcraft`, and `.solvecraft`; each uses the same container.
Other targets use their own extension.
To get one machine-readable report on stdout instead
of the text, add `--json` ([json.md](json.md) describes every field):

```sh
dutchman convert --json generated_unit_cube.f3d cube.sc
```

```json
{"history":{"features":[],"mapped":0,"total":0},
 "input":{"format":"fusion_archive","format_name":"Fusion archive (.f3d)","path":"generated_unit_cube.f3d"},
 "losses":[
  {"code":"f3d/material.not-transferred","level":"Degraded","message":"Materials and appearances were not carried over.","subject":"f3d/material.not-transferred"},
  {"code":"step/attribute.source-record-not-written","level":"Info","message":"7 source attribute record(s) were not written to STEP","subject":"step/attribute.source-record-not-written"},
  {"code":"step/native.source-record-omitted","level":"Info","message":"45 source-native record(s) were not represented in STEP","subject":"step/native.source-record-omitted"},
  {"code":null,"level":"Info","message":"flattened: the design carries no feature history","subject":"history"}],
 "mode":"import","ok":true,
 "output":{"format":"solidcraft/23","path":"cube.sc"},
 "schema":"dutchman.convert/1",
 "verification":{"reason":"the design carries no feature history","status":"not_applicable"}}
```

(Printed on one line; wrapped here.)

SolidWorks assemblies (`.sldasm`) convert with their external part files. Keep parts beside
the assembly or give `--project DIR` and `--search DIR`. Missing or unreadable occurrences
are reported and left out; unavailable referenced configurations use the saved active
configuration with a Degraded loss. See [SolidWorks assemblies](sldprt.md#assemblies-sldasm).

## Reading the loss report

An assembly's conversion first says how complete it is, under the line saying what was
written:

```text
converted  top.iam -> out/top.sc (geometry without history)
assembly: 18 of 20 placements placed; 2 left out:
  pin.ipt ×2: not found at its stored path or in the project folder /work/Robot
```

Empty leaf parts are counted separately as `placed empty`, and make the desktop and
web result card Incomplete. For example, `67 of 88 placements placed; 13 placed empty;
8 left out` means 13 component placements retain their joints but have no bodies.
Each empty leaf adds a Degraded line naming its anonymous part number, as used by
`diagnose` and `--part`. An assembly holder of children is not an empty leaf.
When a solid region becomes an open sheet, a Degraded geometry line counts the
affected regions. Intended surface bodies are not reported as degraded for being open.
Doctor includes these findings and the assembly's placement counts.

Mirrored assembly placements use a shared mirrored copy of the component's geometry;
ordinary copies keep the original. An Info `placements` note counts the converted
reflections. Mirrored copies retain imported geometry only, with a history note when
needed. Non-finite, scaled, or sheared placements beyond tolerance are left out with a
Dropped `placements` note naming the occurrence index and kind. Check those placements
in the source and convert again. Tiny numeric drift is corrected.

The count takes in every placement the assembly and its sub-assemblies record, each
sub-assembly as often as it is placed. Each cause names the file and how many placements it
cost. A part not found usually means its project folder is elsewhere: give it with
`--project`, or a library with `--search` (see
[Finding the part files](inventor.md#finding-the-part-files)). `inspect` says the same as
`would place: …`.

Every conversion lists what it could not carry across intact. A design with more than 12
losses, an assembly's usually, gets them by component instead, the design itself first, then
the parts with the most that matters:

```text
losses: 182 in 14 components
  the design: 1 Degraded, 3 Info
  pin.ipt: 1 Dropped
  base.ipt: history flattened (feature "Shell1": its shell input faces are not resolved yet); 2 Degraded, 9 Info
  …
```

`--verbose` prints every line, and `--log FILE` writes them all to a file. In the desktop app,
each part's losses fold away under its own heading, and **Save the full log…** writes them
out.

Each loss line reads `LEVEL: SUBJECT: MESSAGE`:

| Level | Meaning | What to do |
|---|---|---|
| `Info` | No effect on the geometry. Something was left out that SolidCraft does not use, or the line just says what happened. | Nothing. |
| `Degraded` | Carried across in a weaker form. | Check that part of the design in SolidCraft. |
| `Dropped` | Left out of the design. | Compare with the original; redraw what is missing, or convert again when Dutchman or cadmpeg supports it. |

When cadmpeg's model fails its checks, Dutchman retries with geometry and assembly structure
alone. It removes history, sketches, parameters, and associated design records before checking
again, including parameter links on joints. A `Degraded: history` loss says these were left out
and gives the original check failure. Duplicate parameter names therefore cannot block this
retry; geometry checks still apply.

Repeated stderr notes of the same kind are grouped with a count. `--verbose`, `--log`,
and JSON retain each individual loss. Fusion’s material warning can also appear for a
default material: the decoded model does not establish that no source appearance was lost.

Lines from the examples above, in plain words:

- `Info: history: flattened: the design carries no feature history`: the file has no
  history to convert, so the design is the solid geometry alone.
- `Info: step/native.source-record-omitted: 45 source-native record(s) were not represented
  in STEP`: internal records of the native file that have no place in the converted geometry.
  The shape is not affected.
- `Degraded: f3d/material.not-transferred: Materials and appearances were not carried over.`:
  the decoder could not confirm that materials carried over. Set any missing materials
  again in SolidCraft.
- `Degraded: f3d/sketch.dimension-native: 1 sketch dimension(s) retain native operands …`:
  a sketch dimension could not be tied to the sketch geometry it measures. The sketch is at
  the right size, but that dimension may not drive it.

`Dropped` lines name what was left out. For example, a part in an Inventor assembly whose
file cannot be found is reported as `Dropped: <part>: its part is not in a separate file
Dutchman can find`, and the rest of the assembly still converts.

The first part of a subject such as `f3d/material.not-transferred` is a stable code from
cadmpeg; in `--json` it is the `code` field, which scripts can match on.

Sketch dimensions keep parameter expressions when their operands map, including
point-to-line distances and parallel-line spacing. A loss saying a historical thread face
has no input surface geometry means the thread annotation could not be placed safely;
see [Feature history](history.md#feature-by-feature). Check size edits on the editable body
when the result also contains the original imported shape.

## Open the result in SolidCraft

In the SolidCraft app, use File, Open, or start it with the design:

```sh
solidcraft cube.sc
```

From the command line, `solidcraft-cli eval` opens the design, rebuilds its timeline and
prints its bodies as JSON:

```sh
solidcraft-cli eval cube.sc
```

Check that `body_count` is what you expect, that no `timeline` entry has an `error` or a
meshing warning, and compare each body's `area_mm2` and `volume_mm3` with the original design
within the measuring mesh tolerances. A face-count difference alone does not establish a
defect. An AI agent can do the same through SolidCraft's MCP server: open the design with
`solidcraft-cli mcp --in cube.sc` and call its `measure` tool.

Which SolidCraft a design is written for, and how Dutchman finds the installed one, is in
[SolidCraft versions](command-line.md#solidcraft-versions).

## Where next

- [The desktop app](desktop.md): presets, estimates, the SolidCraft version, and updates.
- [The command line](command-line.md): options, `inspect`, whole folders, watching a folder.
- [Choosing what to keep](presets.md): Fast, Standard, Full, and the memory and time estimate.
- [Formats](formats.md): what converts from each program.
- [Feature history](history.md): what maps onto SolidCraft, and what stops a history.
- [Troubleshooting](troubleshooting.md): common problems and diagnostic logs.

## Moved sections

This page used to hold everything. Each part now has its own page:

### Assembly workers

[Assembly workers](command-line.md#assembly-workers)

### Choosing what to keep

[Choosing what to keep](presets.md)

### Homebrew or Scoop

[Homebrew or Scoop](install.md#homebrew-or-scoop)

### macOS: the app

[macOS: the app](install.md#macos-the-app)

### Opening the app the first time

[Opening the app the first time](install.md#opening-the-app-the-first-time)

### From a release archive

[From a release archive](install.md#from-a-release-archive)

### Channels: stable and nightly

[Channels: stable and nightly](install.md#channels-stable-and-nightly)

### From source

[From source](install.md#from-source)

### Docker

[Docker](docker.md)

### Dutchman Web, in a browser

[Dutchman Web, in a browser](install.md#dutchman-web-in-a-browser)

### SolidCraft (optional, recommended)

[SolidCraft (optional, recommended)](install.md#solidcraft-optional-recommended)

### Desktop app

[Desktop app](desktop.md)

### Options

[Options](command-line.md#options)

### Look before you convert: inspect

[Look before you convert: inspect](command-line.md#look-before-you-convert-inspect)

### Progress during conversion

[Progress during conversion](command-line.md#progress-during-conversion)

### Convert a whole folder: convert-all

[Convert a whole folder: convert-all](command-line.md#convert-a-whole-folder-convert-all)

### Watch a folder

[Watch a folder](command-line.md#watch-a-folder)

### Formats

[Formats](formats.md)

### Pilot

[Pilot](pilot.md)

### Feature history and the SolidCraft check

[Feature history and the SolidCraft check](history.md)

Offset datum planes need no building feature when their support resolves or all their
consumers carry independent resolved sketch placements. Otherwise the history reason
names the unresolved plane reference; resolved skipped planes count as `not_needed`.

Shells require removed faces from their own historical input topology, a resolved
thickness and inward or outward direction, and one earlier named body. A shell input-face
blocker means the source selection or historical boundary is not supported yet; final-body
faces cannot establish which face existed before shelling.

A through-all cut with no earlier named body is `blocked` before replay, with a reason
asking for its body-producing feature to resolve first. Source storage order does not
change replay order: the neutral ordinal controls it.

### Diagnostic logs for unfinished runs

[Diagnostic logs for unfinished runs](troubleshooting.md#diagnostic-logs-for-unfinished-runs)

### Troubleshooting

[Troubleshooting](troubleshooting.md)

### FreeCAD output (experimental)

[FreeCAD output (experimental)](freecad.md#freecad-output-experimental)

### Conversion details

[Conversion details](command-line.md#solidcraft-versions)

### Presets and estimates

[Presets and estimates](presets.md#presets-and-estimates)

Anonymous diagnostic logs include timed decoder preparation phases and, on failure, the
failure stage and error category. Model check failures add check-code counts and available
referenced curve-kind counts. A geometry-only recovery rechecks the decoded model in memory
only when failing checks involve history, sketches, or parameters; geometry failures stop
without repeating the decode. See [diagnostic fields](json.md#dutchmandiagnostics1).

## Make a minimum reproduction

When a failing design must stay private, run:

```sh
dutchman repro part.f3d --out reproduction
```

Use a fresh output folder each time. This runs the default SolidCraft conversion with
local diagnostics and writes `reproduction/kit/anonymous.json`, `local-key.json` and
`next-steps.md`. `--json` returns the summary, kit paths and anonymous content. Exit 0
means the kit was written, including when conversion failed; exit 1 means kit creation
failed. A successful conversion says **no failure to reproduce**.

Only `anonymous.json` is intended for sharing. `local-key.json` maps anonymous ordinals
to your own names and is marked **Do not share**. Follow `next-steps.md` in your CAD
program to rebuild a small design and compare the failure stage and finding-code set.
Temporary conversion output and diagnostic logs are removed after the command returns.
No files are uploaded. Narrowing uses retained CADIR subsets and stops where an independent
subset cannot reproduce the same structured check codes; it does not regenerate features.
The [kit schema](json.md#dutchmanrepro1) describes these limits.

## Rebuild a small failing example

`dutchman diagnose --narrow FILE --out RESULTS` now includes a numbered rebuild recipe for
Fusion, Inventor, SolidWorks and FreeCAD in `RESULTS/diagnose/kit/next-steps.md`. Recipes
use round starting sizes and the signature's options and profile counts. Unknown feature
classes get a generic recipe. An exact match in the release's known-problem list gives a
workaround or next-version fix advice before further narrowing when enough fields are known.

In the desktop app, choose **Diagnose this file** on the failure card for progress, resume,
the recipe, **Check my small file**, and **Make the report**. The report ZIP stays local
and contains the small file's geometry. Review it before choosing to share it.
The [MCP tools](mcp.md#anonymous-diagnostic-tools) provide the same steps for an assistant.

## Check a folder locally

`dutchman doctor designs --out checked --jobs 2 --memory-mb 8192 --json` plans
folders and project ZIPs with the same rules as `convert-all`: top-level assemblies,
their referenced parts, and unused parts. It converts in isolated workers, then runs
part diagnosis. `--memory-mb` is the total converter budget, divided across concurrent
designs; CPU availability caps `--jobs`. `--time-limit 5m` limits each diagnose part.
Run the same command again to resume. Completed checks are reused when the project
inventory, choices, budgets and matching STEP metadata agree. Keep the output outside
the input folder. Default output is a stable sibling named `designs (Doctor)`.

Only `checked/report.json` is anonymous. Converted designs, `local-key.json` and
`private-checkpoints/` stay private. Findings are grouped locally by signature with
counts. A broad error category is a candidate group, not proof of the same defect.
Nothing is sent. Review advisory findings before making a report.

`dutchman diagnose big.f3z --step big.step --out checked` compares stored geometry
against an export of the same design. An unambiguous same-name `.step` or `.stp`
beside a native file is found automatically, including inside a folder or ZIP.
`doctor --step reference.step` accepts a project with one conversion job. Use
`diagnose --part 17` or `doctor --part 17` to record a report target. This does not
change the geometry being compared or replace `convert --part`.

STEP results use evaluated **body numbers**, which can differ from diagnose part
numbers for assemblies and repeated occurrences. Matching uses geometry, placement
and stable order, never names. Results are worst first and contain volume, area,
bounding box, face counts where available, and placement differences. Current
thresholds are 1% volume/area. Bounds/placement allow 0.1% of the body diagonal,
with a 0.1 mm floor and a 0.5 mm ceiling, so a 1 mm move still flags on large bodies.
Face counts alone never flag a difference. Missing measurements are explicit. Checks remain advisory; unavailable comparisons do not count as passes.
No automatic report is sent. The observed false-positive rate for the two
placement-golden assemblies is 0/329 evaluated body rows (0%);
[calibration results and limits](doctor-calibration.md) give the coverage and denominators.
The STEP itself stays local; use the private-send card if you choose to share it.

The structural checks also flag `geometry_untrimmed_torus`: a toroidal face without
trim loops sharing a shell with other faces. Review its boundary before reporting a
defect. A whole torus in its own shell is allowed, including a separate cavity shell.
An isolated part cannot be called floating without an independent placement reference;
the comparison checks detect movement relative to stored geometry or a matching STEP.

Every successful SolidCraft result also gets an advisory body comparison against the
source's stored geometry, using the same joint setting and assembly pose as the
conversion. Partial-history outputs skip this comparison because eval
includes intentionally hidden editable bodies; the normal history verification still
runs. Where supported, SolidCraft writes private whole-design snapshot pairs for
flagged comparisons under `diagnose/preview/`. Their PNGs and staged designs carry
geometry. Review them locally; they are excluded from anonymous tool results.

Memory supervision includes kernel subprocesses on Linux, macOS and Windows; a stopped
worker stops its process group/tree too. These are sampled resident-memory limits,
not an operating-system reservation. The folder planner runs before worker supervision.
Resume fingerprints include resolved external assembly references (up to 10,000 files);
an incomplete dependency inventory is checked afresh.

Real SolidCraft controls on the three published synthetic cube fixtures (Fusion,
Inventor and SolidWorks) produced 0 flags in 3 comparisons against matching STEP.
A planted 1 mm move in a two-instance assembly was detected, and snapshot pairs were
produced. These small controls establish basic behavior, not a false-positive rate
for large assemblies or arbitrary designs.

Report consent is per report. In the desktop failed-file card, **Review and send report**
opens a review with bundle or summary (bundle is the default). Until minimal repro
extraction is available, explicitly choose summary: version, OS, format and failure
signature only. Whole files use the private-send card on Discord. A separate box,
**You may quote these bytes in a public test case**, starts unticked.
Nothing is sent before clicking Send. A report is sent only when a matching receipt
arrives. A temporary failure gets one short retry, then keeps the report locally;
retrying later requires a click. A daily cap asks you to try again tomorrow.
GitHub accepts the anonymous summary only; never attach a geometry bundle there.

Release builds send to Dutchman's report service; `DUTCHMAN_REPORT_ENDPOINT` at compile
time replaces it. A debug build has no default: clicking Send keeps the report locally
and offers the same fallbacks, and development tests use a loopback HTTP stub.
