<h1>
  <img alt="Dutchman Convert" src="assets/logo/dutchman-lockup.png#gh-light-mode-only" width="252">
  <img alt="Dutchman Convert" src="assets/logo/dutchman-lockup-night.png#gh-dark-mode-only" width="252">
</h1>

[![Discord](https://img.shields.io/badge/Discord-Dutchman%20CAD-5865F2?logo=discord&logoColor=white)](https://discord.gg/yTcseBEdeu)

Dutchman Convert, part of Dutchman CAD, ferries CAD designs between formats: it converts
native CAD designs into open, editable models. Reading the native file is
done by [cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman maps cadmpeg's model onto a
target format, starting with [SolidCraft](https://github.com/tangency-labs/solidcraft) designs.
**Help it convert more:** make a small test file from a [fixtures recipe](https://dutchman-cad.github.io/fixtures/),
or send a file that converts wrong to Dutchy in [Discord](https://discord.gg/yTcseBEdeu).

```sh
dutchman part.f3d                 # writes part.solidcraft
dutchman part.f3d out.solidcraft
dutchman convert part.f3d out.solidcraft
```

Dutchman is made for the current SolidCraft (any release from the last two weeks);
older versions get simplified features, or pass `--solidcraft-version x.y.z` on
`convert`, `convert-all`, or `watch` (at least 0.18.2).
The target defaults to the found CLI's version, even with `--no-verify`, else the
newest release known at build time (currently 0.27.0). The current window is 0.18.2–0.27.0.
Constant-radius FreeCAD helices keep editable sketch-profile coils only for targets
0.25.1 or newer; older targets preserve the original geometry and report the version needed.
Designs open in their target release and newer releases.

With no output given, Dutchman writes beside the input and won't replace a file that is
already there unless you pass `--force`. `dutchman convert --help` lists every option, and
[Getting started](docs/guide.md) walks through a first conversion.

Conversions show their stage and elapsed time, with a still-working update every five
seconds. The desktop app shows design counts and a progress bar when the reader knows
the total. `convert`, `convert-all`, `inspect`, and `watch` write progress to stderr;
`--quiet` hides it. With `--json`, progress is JSON lines on stderr and stdout keeps its
existing report format. Fusion `.f3z` member counts are not available yet; their
progress shows stage and elapsed time.

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

Assembly parts prepare in parallel by default; use `--threads N` to bound workers, or
`--threads 1` for serial conversion with the same design and ordered report.

## Choosing what to keep

Use `--preset fast` for shape only, `--preset standard` (the default) for history,
sketches, and joints with assembly history limited by free memory, or `--preset full`
to keep everything that maps. Narrow any preset with `--no-history` (also
`--import-only`), `--no-sketches`, or `--no-joints`. Without joints, stored placements
are retained. FreeCAD output honours the history choice only.

Before conversion, Dutchman shows an approximate memory/time estimate. An over-budget
run offers a lighter preset in a terminal, or refuses with exit 2 in scripts, with
`--yes`, or with `--json`. `--no-memory-check` bypasses the guard and Standard's history
budget. See [the guide](docs/guide.md#choosing-what-to-keep) for limits and examples.

## Install

With [Homebrew](https://brew.sh) on macOS or Linux:

```sh
brew install dutchman-cad/tap/dutchman
```

With [Scoop](https://scoop.sh) on Windows:

```powershell
scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket
scoop install dutchman-cad/dutchman
```

Both install `dutchman`, `dutchman-mcp` and the desktop app (`dutchman-gui`; Scoop also adds a
Start menu shortcut), and follow each release: `brew upgrade` or `scoop update` brings the
newest. Homebrew pours a ready-made bottle, so it needs no Xcode or Command Line Tools, and on
macOS the commands need no first-run approval, because Homebrew's downloads are not
quarantined. For **Dutchman Convert.app** in Applications and Launchpad, add
`brew install --cask dutchman-cad/tap/dutchman-convert`; the app needs the
[first-run approval](docs/guide.md#opening-the-app-the-first-time) once. Or download a release
below, or run it in [Docker](docs/docker.md) (`docker pull ghcr.io/dutchman-cad/dutchman:latest`).

## Download

<!-- downloads -->
| Your computer | Download | Checksum |
|---|---|---|
| Mac (any Mac): the desktop app | [dutchman-macos-universal.dmg](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.dmg) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.dmg.sha256) |
| Mac command line (any Mac) | [dutchman-macos-universal.tar.gz](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.tar.gz.sha256) |
| Windows, 64-bit | [dutchman-x86_64-pc-windows-msvc.zip](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-pc-windows-msvc.zip) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-pc-windows-msvc.zip.sha256) |
| Linux, x86_64 | [dutchman-x86_64-unknown-linux-gnu.tar.gz](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-unknown-linux-gnu.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-unknown-linux-gnu.tar.gz.sha256) |
| Linux, ARM 64-bit (aarch64) | [dutchman-aarch64-unknown-linux-gnu.tar.gz](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-aarch64-unknown-linux-gnu.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-aarch64-unknown-linux-gnu.tar.gz.sha256) |
<!-- /downloads -->

Each archive holds `dutchman`, `dutchman-mcp` and the desktop app, Dutchman Convert
(`dutchman-gui`).
[Getting started](docs/guide.md) shows how to check it, install it and convert a sample in a
minute. Every release is on the [releases page](https://github.com/dutchman-cad/dutchman/releases).

## What converts

Early development. Reader status says whether geometry can be read; the conversion report
says what this file kept. Formats with geometry readers attempt conversion through STEP.
Creo and NX share `.prt` and are identified by contents, including numbered `.prt.N` saves.
SolidWorks assemblies are recognised without a reader.
IGES (`.igs`, `.iges`) is recognised and refused before decoding: "Dutchman doesn't convert
IGES yet: open it in SolidCraft directly (File > Import)."
`dutchman formats` lists the current reader status, cadmpeg's published headline level
when available, and what Dutchman converts:

<!-- formats: generated from dutchman formats -->
```text
Fusion archive (.f3d)  reads geometry  L5  converts: geometry; feature history when every feature maps; components and mapped joints
Fusion multi-document archive (.f3z)  reads geometry  L5  converts: as a Fusion archive, with referenced designs merged in by cadmpeg
Inventor part (.ipt)  reads geometry  L3  converts: geometry; feature history when every feature maps
Inventor assembly (.iam)  reads geometry  L3  converts: placed parts and sub-assemblies, read from their files in the project
SolidWorks part (.sldprt)  reads geometry  L3  converts: geometry; feature history waits on cadmpeg binding sketches and extrude profiles
SolidWorks assembly (.sldasm)  recognised, not read yet  not yet: nothing yet; reading geometry is not supported yet
FreeCAD document (.FCStd)  reads geometry  L5  converts: geometry and components; feature history and joints when they map
CATIA part (.CATPart)  reads geometry  L3  converts: geometry through STEP; solids and surfaces SolidCraft can import
Creo part (.prt, .prt.N)  opens, no geometry yet  L1  not yet: nothing yet; reading geometry is not supported yet
NX part (.prt)  opens, no geometry yet  L1  not yet: nothing yet; reading geometry is not supported yet
Creo View (.pvz)  reads geometry  L3  converts: geometry through STEP; solids and surfaces SolidCraft can import
Rhino (.3dm)  reads geometry  L2  converts: geometry through STEP; solids and surfaces SolidCraft can import
ACIS (.sat, .sab, .smt, .smb)  reads geometry  L3  converts: geometry through STEP; solids and surfaces SolidCraft can import
STEP (.step, .stp)  reads geometry  L8  converts: geometry through STEP; solids and surfaces SolidCraft can import
IGES (.igs, .iges)  open it in SolidCraft directly  L9  not yet: SolidCraft imports IGES itself (File > Import)
Creo or NX part (.prt)  recognised, not read yet  not yet: nothing yet; reading geometry is not supported yet

FreeCAD TechDraw → DXF  drawings: page curves, dimension labels, annotations, and templates; unsupported content reported as Dropped

Written as a SolidCraft design (.solidcraft), or, experimental, as a FreeCAD rebuild macro (.FCMacro, --to freecad)
```
<!-- /formats -->

After a refused conversion (except IGES) or a history that did not fully map, Dutchman points
to the same [fixtures recipes](https://dutchman-cad.github.io/fixtures/) and `/volunteer` in
[Discord](https://discord.gg/yTcseBEdeu); `--quiet` hides that, and `--json` keeps the final
report on stdout. For a file that converts wrong, DM it to Dutchy with its STEP export.

A part that holds an imported solid becomes a base-body Import feature at its place in the
timeline. Later features keep editable history when they map. Stored geometry has no recipe
to lose: the report says, at Info level, “This part holds an imported solid; there is no
feature history to bring over”. A part made only of stored geometry is geometry without
history, with no flattened-history warning.

- When every feature in a design's history maps onto SolidCraft, the SolidCraft design keeps
  that history, editable, with the original geometry kept beside it. If `solidcraft-cli` is
  installed, Dutchman replays the history in it and compares area and volume with the
  original before keeping it. A face-count difference (SolidCraft merges faces that share a
  surface) or an open-shell warning alone does not fail that comparison. What maps, most of it from Fusion:
  - sketches: lines, arcs, circles, conics, splines, and many constraints and dimensions;
  - extrudes and revolves, with their end forms;
  - holes: counterbores, countersinks, chamfered entries, and thread labels on tapped holes;
    drill points on plain drilled holes only (SolidCraft ignores them on entry recesses);
  - sweeps, lofts, and coils;
  - linear, circular, and path patterns of features or bodies, body mirrors, and body
    booleans;
  - sheet-metal base flanges;
  - constant-radius fillets and equal-setback chamfers, on edges proven on SolidCraft's own
    replay of the history before them (so they need `solidcraft-cli`); a Fusion fillet with a
    tangency weight of exactly 1 maps as a circular fillet.

  [Feature history](docs/guide.md#feature-history-and-the-solidcraft-check) in the guide has
  the detail, and what stops a history.
- A blocker keeps the editable prefix and resolved sketches hidden beside the visible
  original shape. Without a body-building prefix, the design keeps imported geometry and
  hidden reference sketches.
- Assemblies keep their components and placements. Fusion and FreeCAD assemblies keep their
  joints too (revolute, slider, and rigid), with their grounded parts held where they are.
  Fusion components keep their current poses, including child placements used to resolve an
  inserted design's pose. A joint that conflicts with the placements
  is kept suppressed, so no part moves when SolidCraft opens the design. Inventor assemblies
  place their parts, without joints. Sketches in the assembly's own file are kept, hidden, for
  reference.
- FreeCAD output is experimental: `--to freecad` writes a self-contained rebuild macro,
  `--to fcstd` runs FreeCAD headlessly to save a native `.FCStd` document and checks its
  solids against the original, and `--to freecad --verify` runs the same check on a macro.
- Every conversion lists what it could not carry across, as Info, Degraded or Dropped.

By default, the output is a SolidCraft design. `--to freecad` writes a FreeCAD rebuild macro (`.FCMacro`)
instead. It rebuilds the mapped history in FreeCAD, keeps the original geometry, and rebuilds
an assembly's joints (fixed, revolute, slider, cylindrical, ball, and more, through
sub-assemblies too) in the Assembly workbench, with its grounded parts. When a later feature
can't be rebuilt, the mapped features before it and the mapped sketches are kept, hidden, in
a "Rebuilt history (partial)" group beside the original geometry (experimental; see
[FreeCAD output](docs/guide.md#freecad-output-experimental)).

`dutchman formats` lists what converts from each format; `dutchman convert --help` lists the
options.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

## Pilot

`dutchman pilot` runs [Pilot](https://github.com/dutchman-cad/pilot), a local helper for
driving SolidCraft from plain-language requests through its public interfaces.

```sh
dutchman pilot --help
dutchman pilot how "round all edges 3 mm"
dutchman pilot model --upstream http://localhost:11434/v1
```

Install SolidCraft separately. The `model` command connects to a chat-completions endpoint
such as Ollama or LM Studio. Pilot is included with the default `pilot` Cargo feature.

`dutchman` also runs the other Dutchman CAD tools: `dutchman pilot` and `dutchman fixtures`
run installed `dutchman-pilot` and `dutchman-fixtures` executables, found next to `dutchman`
first, then on `PATH`. `dutchman --help` lists the installed tools and their versions. An
installed Pilot takes precedence over the built-in one.

## Desktop app

`dutchman-gui` converts without a terminal. Drop design files or folders on the window (or use
**Add files…** and **Add folder…**), choose an output folder, and press **Convert**. Each file
shows whether it converted, what was written (editable history or geometry), and its losses as
Info, Degraded or Dropped. A converted design opens in SolidCraft from its card: the app that
`SOLIDCRAFT_APP` names, else `solidcraft` on `PATH`, else the system's app for `.solidcraft`
files. When an Inventor assembly's part files are not found, its card names them: **Search a
folder…** or **Find a file…** converts it again, searching there too. The app follows the
system's light or dark setting, or can be set to Day or Night. Its **Output format** choice
offers a FreeCAD macro, and the window title shows its version. A **Report a problem** link
opens the **A file didn't convert** form; a failed file's **Report this on GitHub** opens it
filled in with the version, system, error, and an anonymous report. `dutchman-gui --version`
and `--help` print without opening a window.

Choose **Fast** for shape only and the least memory, **Standard** (the default) for history,
sketches, and joints within the assembly history memory budget, or **Full** for everything.
**Advanced** lets you leave feature history, sketches, or joints out; the selection then reads
**Custom** and keeps its base preset. Choices are remembered between launches. Queued files
show approximate memory and time when an estimate is available, with free memory shown once.
If a file is expected to exceed free memory, choose the offered lighter preset,
**Convert anyway** for that run, or **Cancel**. Assembly history left out by Standard is
explained on the result card.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/dutchman-gui-night.png">
  <img alt="The Dutchman Convert app after converting three files: two converted, one failed with its reason" src="docs/images/dutchman-gui-day.png" width="480">
</picture>

On macOS it comes as **Dutchman Convert.app** in a disk image: open it and drag it onto Applications in the window that appears (see
[opening it the first time](docs/guide.md#opening-the-app-the-first-time): approve it once
and the commands inside it run too). It ships in the
release archives beside `dutchman`, or build it from source with Rust 1.88 or newer:

```sh
cargo install --git https://github.com/dutchman-cad/dutchman dutchman-gui
```

On macOS, let SolidCraft itself run once before the first check: open SolidCraft from
Applications, click **Done** when macOS stops it, then click **Open Anyway** in **System
Settings > Privacy & Security**, the same steps as for Dutchman Convert. Dutchman reads the
app's quarantine attribute and its user-approved flag without changing them. The desktop app
retries a blocked check when its window regains focus or you start a conversion.

On Linux it needs a desktop session (X11 or Wayland with OpenGL); the file dialogs use the
desktop portal, and dropping files works without it.

## Docs

- [Getting started](docs/guide.md): install, convert, read the loss report, open the result
  in SolidCraft, troubleshooting.
- [The MCP server](docs/mcp.md): tools, client setup for Claude Code, Codex and others,
  example calls, safety.
- [Docker](docs/docker.md): pull or build the image, convert in a container, run the MCP server
  from it.
- [Dutchman Web](docs/web.md): convert from a browser, on a server you host.
- [Dutchman Print](https://github.com/dutchman-cad/print): `dutchman printers` and `dutchman print`, a
  separate app that prepares and sends print jobs from SolidCraft.
- [Agent skill](skills/README.md): teaches an AI assistant to convert files with Dutchman and
  explain the results.
- [JSON reports](docs/json.md): every field of the `--json` reports, and
  [the anonymous batch report](docs/report-anon.md) that is safe to share.
- [Testing Dutchman on your library](docs/testing-your-library.md): convert your own files
  and report what broke, without sharing a file.
- Formats: [Fusion](docs/fusion.md), [Inventor](docs/inventor.md),
  [SolidWorks](docs/sldprt.md), [FreeCAD](docs/freecad.md).
- Reporting a problem: a file that doesn't convert goes on the
  [**A file didn't convert**](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
  form, with no design file; see [Troubleshooting](docs/guide.md#troubleshooting).
- Questions and help: the [Dutchman CAD Discord](https://discord.gg/yTcseBEdeu) has a forum for Dutchman Convert.
- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md), and [AGENTS.md](AGENTS.md) for AI
  contributors.

## Use from scripts and AI agents

Download a prebuilt archive from the
[latest release](https://github.com/dutchman-cad/dutchman/releases/latest) (see
[Getting started](docs/guide.md#install)), or install from source (Rust 1.88 or newer):

```sh
cargo install --git https://github.com/dutchman-cad/dutchman dutchman
```

See what would carry across, without writing anything:

```sh
dutchman inspect --json part.ipt
```

Convert, with one JSON report on stdout:

```sh
dutchman convert --json part.f3d part.solidcraft
```

Convert a whole project folder, four files at a time:

```sh
dutchman convert-all --json --jobs 4 --out-dir converted/ project/
dutchman watch inbox/ --out-dir converted/  # Poll for settled uploads; keep reports beside results
```

Exit codes: `0` converted, `1` could not convert (the JSON report has `"ok": false` and
`error.message`), `2` usage error. A written history is checked in SolidCraft when
`solidcraft-cli` is found: set `SOLIDCRAFT_CLI` to its path, or pass `--solidcraft PATH`.
[docs/json.md](docs/json.md) describes every field; field names are stable.

### As an MCP server

`cargo install` also installs `dutchman-mcp`, a [Model Context Protocol](https://modelcontextprotocol.io)
server on stdio. It gives an agent four tools, each returning the same report as the matching
`--json` command:

| Tool | Arguments | Returns |
|---|---|---|
| `inspect` | `input` | `dutchman.inspect/1` |
| `convert` | `input`, `output` (optional for `dry_run`), and optionally `preset`, `history`, `sketches`, `joints`, `no_memory_check`, `dry_run`, `import_only`, `verify`, `solidcraft`, `solidcraft_version`, `verify_timeout`, `overwrite` | `dutchman.convert/1` |
| `convert_all` | `inputs` (files and folders), `out_dir`, and optionally `jobs` and the convert options | `dutchman.convert-all/1` |
| `formats` | none | `dutchman.formats/1` |

A conversion that fails comes back as a tool error carrying its failure report. Paths are on
the machine running the server, so give absolute ones. `convert` writes only its `output`
file. `convert_all` writes converted designs and its default log inside `out_dir`. If supplied,
`journal`, `report_anon`, and `report_key` are also written at their specified paths.

For Claude Code:

```sh
claude mcp add dutchman -- dutchman-mcp
```

For other clients, the usual configuration entry is:

```json
{
  "mcpServers": {
    "dutchman": {
      "command": "dutchman-mcp"
    }
  }
}
```

Dutchman finds `solidcraft-cli` on the server's `PATH` or inside an installed SolidCraft app.
Otherwise, a written history is reported unverified. To set an optional `SOLIDCRAFT_CLI`
path, see [client setup](docs/mcp.md#set-up-a-client); replace the placeholder with your
installed executable's absolute path.

## Building

```sh
cargo build --release
cargo test --workspace
```

For packagers: the std-only `dutchman-launcher` crate provides a tiny front command for installations that
ship with Pilot alone. Packagers install its binary as `dutchman`; `dutchman --version`
identifies it as `dutchman launcher <version>`. Install Dutchman Convert for conversion commands.

## Licence

Licensed under the [Apache License, Version 2.0](LICENSE).

### FreeCAD TechDraw drawing output

`dutchman convert part.FCStd --to dxf` writes `part-PageName.dxf` for each
TechDraw page beside the input. `--page NAME` selects a page by its FreeCAD object
name. Use `-o existing-directory` for per-page files there, or `-o drawing.dxf`
with `--page` for an exact filename; without `--page`, that path is the filename
base (`drawing-PageName.dxf`). Default files require `--force` to replace;
explicit outputs replace unless `--no-clobber` is given.

DXF output keeps neutral view curves, positioned annotation text, dimension labels,
template frames, and resolved title-block text in millimetres at 1:1 page scale.
Dimensions keep verified measurements or replacement text. Neutral linear and
radial anchor pairs generate non-associative lines and open arrows; dimensions
without usable anchors keep labels only. Missing curves, unresolved dimensions, and unsupported presentation are
reported per page. `--json` lists entity counts and losses.
See [FreeCAD drawing coverage](docs/freecad.md#techdraw-to-dxf). DXF is available
through `convert`; batch, desktop, web and MCP drawing conversion are not supported yet.
