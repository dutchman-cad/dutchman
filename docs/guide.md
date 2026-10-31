# Getting started

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
converted history. The supported SolidCraft window is currently 0.18–0.28.

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

## Reading the loss report

An assembly's conversion first says how complete it is, under the line saying what was
written:

```text
converted  top.iam -> out/top.sc (geometry without history)
assembly: 18 of 20 placements placed; 2 left out:
  pin.ipt ×2: not found at its stored path or in the project folder /work/Robot
```

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
  base.ipt: history flattened (feature "Shell1": shell waits on input face geometry); 2 Degraded, 9 Info
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
