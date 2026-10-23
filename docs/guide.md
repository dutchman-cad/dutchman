# Getting started

Dutchman converts native CAD files into [SolidCraft](https://github.com/tangency-labs/solidcraft)
designs (`.solidcraft`). This guide covers installing it, converting your first design,
reading what the conversion reports, and opening the result in SolidCraft.

- [Install](#install)
- [Your first conversion](#your-first-conversion)
- [Look before you convert: inspect](#look-before-you-convert-inspect)
- [Convert a whole folder: convert-all](#convert-a-whole-folder-convert-all)
- [Formats](#formats)
- [Reading the loss report](#reading-the-loss-report)
- [Feature history and the SolidCraft check](#feature-history-and-the-solidcraft-check)
- [Open the result in SolidCraft](#open-the-result-in-solidcraft)
- [Troubleshooting](#troubleshooting)

## Install

### macOS: the app

From 0.1.3 on, each release has `dutchman-VERSION-macos-universal.dmg`: **Dutchman.app**, for
Apple silicon and Intel Macs alike, with the `dutchman` and `dutchman-mcp` commands inside it.

1. Open the disk image and drag **Dutchman** onto **Applications**.
2. Open Dutchman from Applications.

#### Opening the app the first time

Dutchman is not signed with an Apple Developer ID, so the first time you open it macOS stops
it with this box:

<img alt="The macOS box: “Dutchman” Not Opened. Apple could not verify “Dutchman” is free of malware that may harm your Mac or compromise your privacy. Buttons: Move to Trash, Done." src="images/macos-not-opened.png" width="260">

Click **Done**, not Move to Trash, then:

1. Open **System Settings** › **Privacy & Security**.
2. Scroll down to **Security**. It says “Dutchman” was blocked to protect your Mac.
3. Click **Open Anyway**, enter your password, then click **Open Anyway** in the box that
   follows.

<img alt="System Settings, Privacy & Security, Security: “Dutchman” was blocked to protect your Mac, with an Open Anyway button." src="images/macos-open-anyway.png" width="450">

macOS remembers the choice; from then on Dutchman opens like any other app. Right-click ›
Open, which older macOS versions offered, no longer gets past the box.

Or clear the download's quarantine flag once, in Terminal:

```sh
xattr -dr com.apple.quarantine /Applications/Dutchman.app
```

The app is signed ad hoc, so macOS never calls it “damaged”. If it does, the download was
cut short or changed: download it again and check it against its `.sha256`
(`shasum -a 256 -c dutchman-VERSION-macos-universal.dmg.sha256`).

The commands inside the app (`dutchman`, `dutchman-mcp`) are covered by the same approval.
Run before it, from Terminal, they meet the same box: approve the app once, as above, and they
run too.

To use the commands from Terminal, link them onto your `PATH`:

```sh
sudo ln -sf /Applications/Dutchman.app/Contents/Helpers/dutchman /usr/local/bin/dutchman
sudo ln -sf /Applications/Dutchman.app/Contents/Helpers/dutchman-mcp /usr/local/bin/dutchman-mcp
```

### From a release archive

Each [release](https://github.com/dutchman-cad/dutchman/releases) has one archive per
platform. Each archive holds `dutchman` (the command), `dutchman-mcp` (the
[MCP server](mcp.md)), `dutchman-gui` (the desktop app, from 0.1.1 on), `LICENSE`, `NOTICE`,
`README.md` and the font licences, with a `.sha256` file beside it.

| Platform | Archive |
|---|---|
| Linux, x86_64 | `dutchman-VERSION-x86_64-unknown-linux-gnu.tar.gz` |
| Linux, ARM64 | `dutchman-VERSION-aarch64-unknown-linux-gnu.tar.gz` |
| macOS, Apple silicon | `dutchman-VERSION-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `dutchman-VERSION-x86_64-apple-darwin.tar.gz` |
| Windows, x86_64 | `dutchman-VERSION-x86_64-pc-windows-msvc.zip` |

Download the archive and its `.sha256` file, check one against the other, then unpack it and
put the programs on your `PATH`. On Linux x86_64, for release 0.1.5 (set `VERSION` and the
target for another release or machine):

```sh
VERSION=0.1.5 TARGET=x86_64-unknown-linux-gnu
base=https://github.com/dutchman-cad/dutchman/releases/download/v$VERSION
curl -fLO "$base/dutchman-$VERSION-$TARGET.tar.gz"
curl -fLO "$base/dutchman-$VERSION-$TARGET.tar.gz.sha256"
sha256sum -c dutchman-$VERSION-$TARGET.tar.gz.sha256
tar xzf dutchman-$VERSION-$TARGET.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 755 dutchman-$VERSION-$TARGET/dutchman* "$HOME/.local/bin/"
export PATH="$HOME/.local/bin:$PATH"
dutchman --version
```

The `export` lasts for this shell; add the same line to your shell's profile (`~/.bashrc`,
`~/.zshrc`) to keep it. The `dutchman*` pattern installs the desktop app, `dutchman-gui`, too.

On Linux, launch the desktop app with `dutchman-gui` from a terminal. It needs an X11 or
Wayland desktop session with working OpenGL. See [Troubleshooting](#troubleshooting) if no
window appears.

The window's title shows its version, and its **Report a problem** link opens the
[A file didn't convert](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form. `dutchman-gui --version` and `dutchman-gui --help` print and exit without opening a
window (on Windows, read the version with `dutchman --version`).

On macOS, `shasum -a 256 -c` takes the place of `sha256sum -c`. The programs are not signed
with a Developer ID, so macOS blocks each the first time, as it does
[the app](#opening-the-app-the-first-time); allow it in System Settings › Privacy & Security,
or clear the quarantine flag:

```sh
xattr -d com.apple.quarantine dutchman dutchman-mcp dutchman-gui
```

On Windows, in PowerShell, compare the hash with the one in the `.sha256` file, then unpack
the archive and add its folder to your `PATH`:

```powershell
Get-FileHash dutchman-VERSION-x86_64-pc-windows-msvc.zip -Algorithm SHA256
Get-Content dutchman-VERSION-x86_64-pc-windows-msvc.zip.sha256
Expand-Archive dutchman-VERSION-x86_64-pc-windows-msvc.zip -DestinationPath .
```

### Channels: stable and nightly

- **Stable** releases (`v0.1.5`, …) are the ones to use. Each is built against one tested
  commit of cadmpeg, the library that reads the native files.
- **Nightly** is one pre-release, [`nightly`](https://github.com/dutchman-cad/dutchman/releases/tag/nightly),
  rebuilt each day from Dutchman's latest code against cadmpeg's latest code, and published
  only when every test passes. Its notes give both commits and the date. Its archives are
  named `dutchman-nightly-TARGET` and install the same way. Use it when an issue says a fix is
  merged but not in a release yet; go back to stable once a stable release has it.

### From source

With Rust 1.88 or newer:

```sh
cargo install --git https://github.com/dutchman-cad/dutchman dutchman
```

This installs both `dutchman` and `dutchman-mcp`. Check the install:

```sh
dutchman --version
```

### SolidCraft (optional, recommended)

When a design's feature history converts, Dutchman checks it by replaying it in SolidCraft's
command-line tool, `solidcraft-cli`, which comes with every
[SolidCraft release](https://github.com/tangency-labs/solidcraft/releases). Installing the
SolidCraft app is enough: Dutchman finds the tool inside it (on macOS, the newest
`SolidCraft*.app` in `/Applications` or `~/Applications`; on Windows, `Program Files\SolidCraft`).
`--solidcraft PATH`, `SOLIDCRAFT_CLI`, or `solidcraft-cli` on your `PATH` come first. The
desktop app says which one it checks with. Without one, conversions still work; a converted
history is marked unverified.

Dutchman's designs open in **SolidCraft 0.18.2 or newer**, and are tested with 0.18.2 and
0.19.0. A `solidcraft-cli` older than 0.18.2 is not used: the conversion says so ("… is older
than SolidCraft 0.18.2, the oldest that opens and checks Dutchman's designs; install a newer
SolidCraft") and writes the history unverified. SolidCraft 0.19.0 saves designs in a newer
format that older versions do not open, so a design you save again in 0.19.0 opens in 0.19.0
or newer only.

Sheet-metal history includes base flanges (not of symmetric thickness) and full-edge flanges
on an unchanged rectangular base sheet. An edge flange needs its own CADIR input topology
with edge midpoint, tangent, and end vertices; each midpoint must identify exactly one edge
of the rebuilt base sheet. Fixed outer-face heights, angles between zero and 180 degrees,
inside/outside bend placement, and positive bend radii map. Inner-face heights, partial widths,
object-ended heights, adjacent/tangent bend placement, and picks on already modified sheets
keep the geometry and report the reason. Final-body edges alone never select a flange.
Hems keep geometry: CADIR specifies developed length while SolidCraft requires length to the
outer face, and rolled, teardrop, and unresolved hem forms are not supported yet. CADIR has no
contour-flange, bend, unfold/refold, or corner-relief operations, nor K-factor or relief
settings; sheet rules keep SolidCraft's defaults for those settings. The report names the
first feature that stops the history; the original geometry is still converted.

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
dutchman convert solidworks_cube.sldprt cube.solidcraft
```

```text
converted  solidworks_cube.sldprt -> cube.solidcraft (geometry without history)
dutchman: Degraded: sldprt/source.dialect-unverified: the document carries no swSolidWorks swVersion declaration, so no declared identity was verified. …
dutchman: Degraded: sldprt/config.inferred-without-native: 1 configuration state(s) are inferred from geometry partitions without native configuration definitions.
dutchman: Info: step/design.parametric-records-omitted: 1 parametric/design record(s) were not represented in STEP: configuration=1
dutchman: Info: step/native.passthrough-record-omitted: 1 uninterpreted passthrough record(s) were not represented in STEP
dutchman: Info: history: flattened: the design carries no feature history
dutchman: (notes are cut to their first sentence; --verbose prints them in full)
```

The synthetic SolidWorks cube lacks native version and configuration metadata, so those
warnings are expected for this sample. The internal records omitted from STEP do not change
its shape: the written geometry should be one 10 mm cube (1,000 mm³). The sample has no
editable feature history. Check the result in SolidCraft as [below](#open-the-result-in-solidcraft).

The exit code is 0, and the first line, on stdout, says what was written where: here the
geometry, as this file has no feature history (else `editable history, verified in SolidCraft`
or `editable history, unverified`). The lines on stderr are the loss report, explained
[below](#reading-the-loss-report). Long notes are cut to their first sentence, marked `…`;
`--verbose` prints them whole. An existing output file is
replaced (`--no-clobber` keeps it, and the conversion fails instead), and nothing is written
when the conversion fails.

The output must end in `.solidcraft`. To get one machine-readable report on stdout instead
of the text, add `--json` ([json.md](json.md) describes every field):

```sh
dutchman convert --json generated_unit_cube.f3d cube.solidcraft
```

```json
{"history":{"features":[],"mapped":0,"total":0},
 "input":{"format":"fusion_archive","format_name":"Fusion archive (.f3d)","path":"generated_unit_cube.f3d"},
 "losses":[
  {"code":"f3d/material.not-transferred","level":"Degraded","message":"Materials/appearances (.protein assets, ACT/design assignments) were not transferred.","subject":"f3d/material.not-transferred"},
  {"code":"step/attribute.source-record-not-written","level":"Info","message":"7 source attribute record(s) were not written to STEP","subject":"step/attribute.source-record-not-written"},
  {"code":"step/native.source-record-omitted","level":"Info","message":"45 source-native record(s) were not represented in STEP","subject":"step/native.source-record-omitted"},
  {"code":null,"level":"Info","message":"flattened: the design carries no feature history","subject":"history"}],
 "mode":"import","ok":true,
 "output":{"format":"solidcraft/23","path":"cube.solidcraft"},
 "schema":"dutchman.convert/1",
 "verification":{"reason":"the design carries no feature history","status":"not_applicable"}}
```

(Printed on one line; wrapped here.)

### Options

| Option | What it does |
|---|---|
| `--import-only` | Write the geometry as one non-parametric import, never the feature history. |
| `--solidcraft PATH` | The `solidcraft-cli` that checks a converted history. Default: `$SOLIDCRAFT_CLI`, else `solidcraft-cli` on `PATH`, else the one inside an installed SolidCraft app (macOS `SolidCraft*.app`, newest first; Windows `Program Files\SolidCraft`). |
| `--no-verify` | Write the history without checking it in SolidCraft. Cannot be combined with `--solidcraft`. |
| `--verify-timeout SECONDS` | How long each SolidCraft check may run before the history is written unverified. Default 120. |
| `--no-clobber` | Do not replace an existing `.solidcraft` file: that file's conversion fails instead. By default it is replaced. |
| `--project DIR` | The project folder an Inventor assembly's part files are searched in when they are not at their stored paths. Default: the nearest folder above the assembly holding an `.ipj`, else two folders up. See [Finding the part files](inventor.md#finding-the-part-files). |
| `--search DIR` | Another folder to search for part files the project folder does not hold, such as a shared library. May be given more than once. |
| `--json` | One JSON report on stdout instead of the text report. |
| `--verbose` | Print every loss, each note in full. Without it, a file with more than 12 losses is summarised by component (below), and long notes are cut to their first sentence. The JSON report always has them all in full. |
| `--log FILE` | Also write the whole text report, every loss in full, grouped by component, to `FILE`. |

## Look before you convert: inspect

`inspect` reads a file and says what converting it would carry across. It writes nothing and
does not run SolidCraft.

`design_sketch_constraints.f3d`, from the same folder, is for inspecting only: its history is a
single sketch, with no solid or surface to convert.

```sh
dutchman inspect design_sketch_constraints.f3d
```

```text
Fusion archive (.f3d): 1 body, 1 face
would write: nothing; converting it fails: the model has no solids or surfaces SolidCraft can import
  mapped     sketch "Sketch 1"
dutchman: Degraded: f3d/feature.edge-reference-lost: 1 source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and have no independent complete selection proof.
dutchman: Degraded: f3d/sketch.relation-native: 2 sketch relation(s) retain native operands because no unique neutral relation was resolved.
dutchman: Degraded: f3d/sketch.dimension-native: 1 sketch dimension(s) retain native operands because no unique neutral dimension was resolved.
```

The first line counts the bodies and faces read. The second says whether the design would keep
its feature history or be written as geometry, and why, or, as here, that converting it would
fail and why. Then comes each feature of the history
with its status (`mapped`, `not_needed`, `left_out` or `blocked`, with the reason for a
blocked one), and the loss report. `inspect --json` prints the same as a
`dutchman.inspect/1` report.

## Convert a whole folder: convert-all

`convert-all` converts every file named, and every file Dutchman knows inside the folders
named (with their subfolders; symbolic links are not followed). The designs go to
`--out-dir`, keeping the folder layout. A file that fails does not stop the others.

```sh
dutchman convert-all --jobs 2 --out-dir converted project
```

```text
converted  project/generated_unit_cube.f3d -> converted/generated_unit_cube.solidcraft (geometry without history)
           Degraded: f3d/material.not-transferred: Materials/appearances (.protein assets, ACT/design assignments) were not transferred.
           Info: step/attribute.source-record-not-written: 7 source attribute record(s) were not written to STEP
           Info: step/native.source-record-omitted: 45 source-native record(s) were not represented in STEP
           Info: history: flattened: the design carries no feature history
failed     project/sub/container_metadata_only.f3d: cannot read this design: no geometry could be read from this Fusion archive (.f3d) file
converted  project/sub/synthetic_cube.ipt -> converted/sub/synthetic_cube.solidcraft (geometry without history)
           Degraded: inventor/rse.segment-pair-untyped: Retained 1 structurally paired RSe segment(s) without record semantics.
           Info: step/attribute.source-record-not-written: 7 source attribute record(s) were not written to STEP
           Info: step/native.source-record-omitted: 67 source-native record(s) were not represented in STEP
           Info: history: flattened: the design carries no feature history
2 converted, 1 failed, 1 skipped (not in a format Dutchman knows)
```

Each converted file's loss report is listed beneath it, summarised by component when it has
more than 12 losses. The whole report, every loss in full, is written to `dutchman.log` in the
output folder (`--log FILE` puts it elsewhere). This log, ordinary JSON, and the journal hold
file names and paths; keep them private. For an issue, request `--report-anon FILE`, review
it, and share that report as [below](#troubleshooting). Files in the folders that are not in
a format Dutchman knows (`notes.txt`, say) are skipped and counted at the end. The exit code is 0
only when every file converted, else 1. `--jobs N` sets how many files
convert at once (default: the machine's cores, at most 8). It takes the same options as
`convert`, and `--json` prints one `dutchman.convert-all/1` report holding a
`dutchman.convert/1` report per file. One run converts at most 10,000 files and searches
folders at most 32 levels deep.

Large batches:

- **Each file in its own process.** A file that takes longer than `--timeout SECONDS` (default
  900) or holds more than `--max-memory MB` (default 8192; `0` for no limit) is stopped and
  reported as failed, and the rest carry on. A file that crashes the converter fails alone.
- **Carry on where you stopped.** `--journal FILE` records each file's result as it finishes.
  Run the same command again and the files already recorded, unchanged since, are not
  converted again; their results come from the journal. Files another version of Dutchman
  recorded, or an older journal format (before 0.1.4), are converted again.
- **A report you can share.** `--report-anon FILE` also writes an anonymous report: counts by
  format, every loss and failure by cause and stage, and timings, with no file names, paths or
  geometry. See [Testing Dutchman on your library](testing-your-library.md) and
  [the report's fields](report-anon.md).

Everything runs on your machine: Dutchman makes no network connections.

For an Inventor assembly (`.iam`), Dutchman finds its part files as Inventor does: at the
paths it stores, else anywhere under the project folder, the nearest folder above it holding a
`.ipj`. Give another with `--project DIR`; `convert-all` searches each folder it is given. See
[Finding the part files](inventor.md#finding-the-part-files).

## Formats

```sh
dutchman formats
```

```text
Fusion archive (.f3d)            converts  geometry; feature history (sketches, constraints, dimensions, extrudes, revolves, lofts, sweeps, spheres, tori, coils) when every feature maps; components when the file says which bodies they own
Fusion multi-document archive (.f3z) converts  as a Fusion archive, with the designs it references merged in by cadmpeg
Inventor part (.ipt)             converts  geometry; feature history (sketches, constraints, extrudes) when every feature maps
Inventor assembly (.iam)         converts  placed parts and sub-assemblies, read from their files in the project
SolidWorks part (.sldprt)        converts  geometry; feature history waits on cadmpeg binding sketches and extrude profiles
SolidWorks assembly (.sldasm)    not yet   nothing yet
FreeCAD document (.FCStd)        converts  geometry and components; feature history (sketches, pads, simple holes) when every feature maps

Written as a SolidCraft design (.solidcraft), or, experimental, as a FreeCAD rebuild macro (.FCMacro, --to freecad)
```

The state of each format, and what is still missing upstream, is in [fusion.md](fusion.md),
[inventor.md](inventor.md), [sldprt.md](sldprt.md) and [freecad.md](freecad.md). Reading
native files is done by
[cadmpeg](https://github.com/cadmpeg/cadmpeg); a format improves in Dutchman when it improves
there.

## Reading the loss report

An assembly's conversion first says how complete it is, under the line saying what was
written:

```text
converted  top.iam -> out/top.solidcraft (geometry without history)
assembly: 18 of 20 placements placed; 2 left out:
  pin.ipt ×2: not found at its stored path or in the project folder /work/Robot
```

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

Lines from the examples above, in plain words:

- `Info: history: flattened: the design carries no feature history`: the file has no
  history to convert, so the design is the solid geometry alone.
- `Info: step/native.source-record-omitted: 45 source-native record(s) were not represented
  in STEP`: internal records of the native file that have no place in the converted geometry.
  The shape is not affected.
- `Degraded: f3d/material.not-transferred: Materials/appearances … were not transferred.`:
  the shape converted, but the materials did not. Set them again in SolidCraft.
- `Degraded: f3d/sketch.dimension-native: 1 sketch dimension(s) retain native operands …`:
  a sketch dimension could not be tied to the sketch geometry it measures. The sketch is at
  the right size, but that dimension may not drive it.

`Dropped` lines name what was left out. For example, a part in an Inventor assembly whose
file cannot be found is reported as `Dropped: <part>: its part is not in a separate file
Dutchman can find`, and the rest of the assembly still converts.

The first part of a subject such as `f3d/material.not-transferred` is a stable code from
cadmpeg; in `--json` it is the `code` field, which scripts can match on.

## Feature history and the SolidCraft check

Constant-radius fillets and equal-setback chamfers keep editable history when their own
feature input topology supplies edge midpoints, tangents and endpoints, and each midpoint
can be proven to pick exactly one edge of the replayed pre-feature body. This currently
requires one blind, untapered extrusion of a single convex polygon and no earlier body
modifications. Equal two-distance chamfers also map; distance-and-angle chamfers map at
45 degrees on perpendicular faces. Unequal distances and other angles lack a source
reference-face ordering and are refused specifically. Variable fillets lack a proven
chain direction. Final-body edge selections never stand in for input geometry.

Hole history includes chamfered entries and tapped holes: the hole keeps its bore and an ISO
metric thread label such as `M6x1`. A thread's hand, tolerance class, and length don't carry
across; the report lists them as Degraded. Taper, exit treatments, modeled threads, and entry
recesses combined with drill points are not supported yet, and the report names the option.

When every feature in a design's history maps onto SolidCraft (sketches, extrudes, revolves,
patterns, body mirrors, and body booleans so far), the design keeps that history, editable, with
the original geometry kept beside it as a suppressed import named "Original geometry
(imported)". Otherwise the design is the geometry alone, and the report says which feature
stopped the history.

Sweeps accept ordered sketch curves, or a whole sketch containing a single non-construction
curve, for their path and optional guide rail. Taper, explicit right or round corners,
changing sections, tangent-path extension, and post-sweep simplification keep the original
geometry and report the unsupported option. Twist, end scale, and the supported orientations
remain editable; a guide rail requires the default orientation without twist or scale.

Revolves keep symmetric angles, equal angles on two sides, and full turns, including unequal
sides that add up to a full turn. An axis that is a line of the profile's sketch stays that
line; an edge or datum axis becomes a fixed line in the sketch plane. Surface revolves keep
their history short of a full turn. Partial one-sided angles, unequal partial two-sided
angles, ends at a face, vertex or body, full-turn surface revolves, and faces built other than
the way FreeCAD's default (bullseye) face maker builds them stop the history, and the report
names which.

Patterns can repeat earlier features or named bodies in one or two linear directions,
around an axis, or at fixed spacing along curves of an earlier planar sketch. Explicit
linear offsets and circular angles map when their intervals are equal; two linear composite
stages map to a rectangular grid. Patterns are limited to 10,000 instances. Nonuniform or
skipped instances, progressive scaling, face seeds, and unresolved body or path references
flatten the history with a specific reason. This CADIR version has no solid-pattern symmetry
or extent fields, so Dutchman cannot recover those settings independently of the decoded
spacing and locations.

Sketches retain bounded parabolas and hyperbolas, elliptical arcs up to one turn,
and rational quadratic Béziers as editable conics. Equal-weight rational splines become
polynomial control splines. Selected multi-piece elliptical arcs, unbounded conics, other
weighted or nonuniform splines, and text without solved outlines still prevent history replay.
Additional coordinate, intersection, tangency, curvature, symmetry, and repeated-dimension
relations carry across. Static sketch patterns retain point and radius associations;
driving count or span parameters are not supported yet. Constraints that cannot carry
across are counted in a `Degraded` sketch loss with a specific reason; they never by
themselves flatten a sketch whose solved geometry maps.

A converted history is checked by replaying it in `solidcraft-cli` and comparing the bodies
and their volumes with the original. The result is in the `history:` line of the report, and
in `verification.status` in `--json`:

| Status | Meaning |
|---|---|
| `verified` | The history replays in SolidCraft and matches the original. |
| `unverified` | The history was written without the check: no `solidcraft-cli` was found, it timed out, or `--no-verify`. The reason is given. |
| `flattened` | The design has a history, but it was not written: a feature does not map, or the replay differs from the original. The design keeps the imported geometry and any mapped sketches with resolved planes, hidden for reference, with no features built on them. |
| `not_applicable` | There was no history to write, or `--import-only`. |

For a flattened design, one Info line reports `sketches: kept N sketches for reference`,
with `; M left out` when any sketches did not map or their planes did not resolve.
`--import-only` continues to omit sketches.

When the design is written as geometry, the same `solidcraft-cli` opens it too. If SolidCraft
shows fewer faces than the source model has, or warns about it (for example faces it cannot
mesh), the report says so as `Degraded: geometry: …`, such as
`Degraded: geometry: SolidCraft opens 3 of the 6 faces converted: 3 faces are missing there; solid 'unit cube': 3 face(s) could not be meshed and are not shown`.
The geometry was written whole; SolidCraft does not show all of it. Look over those bodies in
SolidCraft before relying on them. When SolidCraft says why, the reason follows (faces it
cannot mesh, open shells, bodies inside out). `--no-verify`
skips this check too.

## Open the result in SolidCraft

In the SolidCraft app, use File, Open, or start it with the design:

```sh
solidcraft cube.solidcraft
```

From the command line, `solidcraft-cli eval` opens the design, rebuilds its timeline and
prints its bodies as JSON:

```sh
solidcraft-cli eval cube.solidcraft
```

Check that `body_count` is what you expect, that no `timeline` entry has an `error` or a
`warning`, and that each body's `volume_mm3` and `bbox` match the original design. An AI
agent can do the same through SolidCraft's MCP server: open the design with
`solidcraft-cli mcp --in cube.solidcraft` and call its `measure` tool.

## Troubleshooting

For a failed conversion, or a design that looks wrong in SolidCraft, start with an anonymous
report (replace `part.f3d` with your file or folder):

```sh
dutchman convert-all --out-dir converted --report-anon report-anon.json part.f3d
```

Review `report-anon.json`, then paste it into the
[**A file didn't convert**](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form on Dutchman's issues (for a whole library, the **Library test report** form). Keep
ordinary logs, JSON output, the journal, the report key, and the salt private: they contain
names, paths, or a way to identify files. Do not attach design files publicly. If you want to share a
small reproducer you made yourself, offer it privately in the issue. See
[Testing Dutchman on your library](testing-your-library.md#5-report-what-you-found).

On Linux, run `dutchman-gui` from a terminal and confirm that your X11 or Wayland desktop
session has working OpenGL and graphics drivers. Include the terminal diagnostics when
reporting startup trouble. A graphics startup error prints that advice and exits with
status 1, as does a start that stalls for 30 seconds. The command-line `dutchman convert` works without
a desktop session.

Every error is one line on stderr, and the exit code is 1 (2 for a mistake on the command
line). A failed file creates no design; a batch can still write successful designs, logs,
and requested reports.

| Message | What to do |
|---|---|
| `x.step: output must be a .solidcraft file` | Name the output `something.solidcraft`. |
| `a.step: Dutchman does not convert this kind of file. It converts …` | Dutchman converts the native formats listed; it does not read STEP or other exchange formats. Open STEP files in SolidCraft directly. |
| `SolidWorks assembly (.sldasm) files are not supported yet. Export a STEP file for SolidCraft to open; Dutchman's issues (…) say when …` | Export the assembly as STEP from the program that made it and open that in SolidCraft. |
| `cannot read this design: its contents are inventor data, not Fusion archive (.f3d); rename it to match` | The file's extension does not match its contents. Rename it with the right extension. |
| `cannot read this design: no geometry could be read from this Fusion archive (.f3d) file` | The file holds no geometry Dutchman can read yet, for example a design saved without bodies, or one whose geometry is stored in a form cadmpeg does not read yet. |
| `cannot read this design: malformed container: …` | Download or copy the file again, or check its extension. If the original opens in the program that made it, use the anonymous reporting route above. |
| `cannot convert this design: the model has no solids or surfaces SolidCraft can import` | The file was read but held nothing SolidCraft can hold as a solid or surface. |
| `missing.f3d: No such file or directory (os error 2)` | Check the path. |
| `part.f3d: the file is … bytes, over Dutchman's 1073741824-byte limit` | Dutchman opens files up to 1 GiB. |
| `history: …; unverified: …` | Not an error: the history was written without the SolidCraft check. Install `solidcraft-cli`, or point `--solidcraft` or `SOLIDCRAFT_CLI` at it, and convert again. |
| `history: flattened: replay differs (…)` | Not an error: the history did not rebuild the same shape in SolidCraft, so the geometry was written instead. `--no-verify` keeps the history, unchecked. |

## FreeCAD output (experimental)

**Experimental.** The macro, its options and what it rebuilds may change between releases,
including the headless verification result. Keep the original design for comparison.

Write a self-contained FreeCAD rebuild macro instead of a SolidCraft design:

```sh
dutchman convert in.f3d --to freecad -o out.FCMacro
```

The positional output path still works. `convert-all --to freecad --out-dir DIR` writes
`.FCMacro` files; the desktop app's **Output format** choice offers **FreeCAD macro** too.
This output works independently of the `freecad` feature used to read `.FCStd` inputs.

The macro makes a Body for each placed part, preserving nested assembly placements. It
uses Dutchman's existing history mapping and timeline order to rebuild planar sketches
(lines, circles, arcs, ellipses and supported B-splines), constraints, pads and pockets,
full-turn or symmetric revolves, unguided lofts, sweeps, simple holes, linear and circular
feature patterns, spheres, tori and coils. Numeric feature values are retained. A parameter
object keeps evaluated scalar values and source expressions as text; source expressions
are never executed. Coils are rebuilt with Part geometry and stored as a feature, with
numeric properties; editing those properties does not recompute the coil.

When a later feature cannot be rebuilt, the macro keeps the longest mapped prefix and
all mapped sketches in a hidden group named **Rebuilt history (partial)**. The embedded
STEP remains the visible final shape when the prefix differs; reference features are not
included in the final shape's measurements. The report says `rebuilt N of M features;
kept as reference` (sketches count as features; suppressed and organizational features do
not). Later body-building features are blocked, with their reasons; later mapped sketches
remain hidden references. Partial histories are still experimental and unverified.

A part whose history cannot be rebuilt is kept as an embedded STEP solid or surface in a
Body. Examples include unresolved source selections, intersect operations, fixed-orientation,
guided, twisted, scaled or binormal sweeps and lofts, lofts ending at a point, extrudes from
or to a point or face or with taper, counterbored, countersunk or face-ended holes, coils
about an axis other than Z, two-direction patterns, and histories spanning multiple bodies. The report gives one result per source feature, including the reason each feature
was not rebuilt. `--import-only` keeps every part as STEP and leaves out joints.
Assemblies retain placements and rebuild supported joints as described below.
Pattern counts are limited to 10,000. Solid lines drawn corner to
corner across a profile taken whole (a centre rectangle's diagonals, when the source does
not mark them construction) are left out of the padded profile, which is the same region.
Geometry whose STEP uses replicated trimming curves (`CURVE_REPLICA`) is refused, because
FreeCAD 1.1's STEP reader crashes on it.

Assembly workbench joints are experimental and exercised with FreeCAD 1.1.4. The fixed,
revolute, and slider joints the SolidCraft output writes are written to FreeCAD too, with the
same resolved parts, connector frames, frame sense, angles, and limits, through
sub-assemblies as well. Between parts placed directly in the assembly, cylindrical, ball,
distance, parallel, perpendicular, and angle joints are also written; axis coincidence uses
a cylindrical joint, and an insert uses a revolute joint with its plane offset. Their
attachment offsets are kept, angular limits use degrees and linear limits millimetres, and
either bound may be enabled alone. FreeCAD's static solve preserves free coordinates; limit
properties are kept for the workbench rather than used to drive a pose during conversion.
For a single revolute joint with one of its parts grounded, the macro also provides
`drive_joint_angle(assembly, joint, degrees)`, which clamps the requested angle to the
stored bounds and solves the other part's new pose. Use the workbench's simulation tools
for chains and loops.

A sub-assembly whose parts are joined gets its own Assembly, solved before the one that
holds it; FreeCAD moves a sub-assembly as one body in its parent, so a moving joint from a
part outside it to a part inside it turns the whole sub-assembly, with a note. Explicit
grounding is retained. In each assembly, a joint chain nothing grounds gets one inferred
grounded part (the second side of its first joint, as in the SolidCraft output), named in
the report. Suppressed joints remain suppressed. External CADIR operands, subelement
operands, joints between a sub-assembly and a part inside it, suppressed grounding, limits
on fixed joints, driving parameter expressions, rack/pinion, screw, gear, and belt
couplings, plane coincidence, point-on-axis, tangent, and native joint families get
specific report notes. Their stored placements are kept.

The macro checks the solver result and every part's stored pose. If the Assembly API is
unavailable, solving fails, or a part moves during reconstruction, it removes the new
joints and puts each part back where it was placed, as the macro does without joints.
FreeCAD's report view gives the reason. Conversion reports remain unverified until the
macro runs.

Open the macro in FreeCAD with **Macro > Macros… > Execute**, or run:

```sh
freecadcmd out.FCMacro
```

The macro creates a new document. Save it as `.FCStd` from FreeCAD, or have Dutchman
run FreeCAD headlessly and write the document directly:

```sh
dutchman convert in.f3d --to fcstd -o out.FCStd
dutchman convert in.f3d --to freecad --verify -o out.FCMacro
```

`--to fcstd` always checks the result. `--to freecad --verify` checks the macro without
saving a native document; the macro remains self-contained. Both compare each part's
resulting solids with its embedded source STEP: solid count, volume (relative tolerance
0.01%, the same as SolidCraft), and bounding-box coordinates (0.01% of the part's span,
with a 1 mm minimum scale). Assemblies are compared in each part's local coordinates.
The report says `verified` when every part matches, or `differs` and identifies the part.
A runtime rebuild failure restores embedded STEP and reports the specific reason as a
loss and a `differs` result, even when the restored solid matches. Sheets, wires and other
parts without solids are reported as not checked rather than verified.

Discovery uses `FREECAD_CMD` (one executable path, or the Flatpak form
`flatpak run --command=FreeCADCmd org.freecad.FreeCAD`; no shell is involved),
then `freecadcmd` or `FreeCADCmd` on `PATH`, the standard `/Applications/FreeCAD.app` or
`~/Applications/FreeCAD.app` bundle's `Contents/Resources/bin`, then an installed
`org.freecad.FreeCAD` Flatpak. Flatpak receives access only to the temporary execution
folder in addition to its existing permissions. `--freecad-timeout SECONDS` defaults to
300; the process is killed on timeout. Native output fails when FreeCAD cannot run or
produce measurements. A macro still writes successfully with verification `unverified`
and reason `not checked: FreeCAD not found` when discovery finds no installation.
`convert-all` forwards these options to each conversion worker.

No FreeCAD installation is needed to generate a macro without `--verify`. Original STEP
geometry remains in the document, hidden after a successful rebuild. The macro compares
rebuilt geometry with STEP; a rebuild failure or difference restores the original and
prints a warning in FreeCAD's report view. Temporary execution and STEP files are removed.
