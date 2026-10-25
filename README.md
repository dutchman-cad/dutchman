<h1>
  <img alt="Dutchman Convert" src="assets/logo/dutchman-lockup.png#gh-light-mode-only" width="252">
  <img alt="Dutchman Convert" src="assets/logo/dutchman-lockup-night.png#gh-dark-mode-only" width="252">
</h1>

[![Discord](https://img.shields.io/badge/Discord-Dutchman%20CAD-5865F2?logo=discord&logoColor=white)](https://discord.gg/yTcseBEdeu)

Dutchman Convert, part of Dutchman CAD, ferries CAD designs between formats: it converts
native CAD designs into open, editable models. Reading the native file is
done by [cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman maps cadmpeg's model onto a
target format, starting with [SolidCraft](https://github.com/tangency-labs/solidcraft) designs.

```sh
dutchman part.f3d                 # writes part.solidcraft
dutchman part.f3d out.solidcraft
dutchman convert part.f3d out.solidcraft
```

With no output given, Dutchman writes beside the input using the extension chosen by `--to`
(`.solidcraft`, `.FCMacro`, or `.FCStd`). If that default already exists, choose `-o PATH`
or use `--force` to replace it. An explicit output is replaced unless `--no-clobber`;
`--force` conflicts with `--no-clobber`. The default may never replace the input itself.

Dutchman Convert is the `dutchman` command; converting is its main job. It also runs other
Dutchman CAD tools: `dutchman pilot` and `dutchman fixtures` run installed `dutchman-pilot`
and `dutchman-fixtures` executables, found next to `dutchman` first, then on `PATH`.
`dutchman --help` lists the installed tools and their versions. An installed Pilot takes
precedence over the built-in Pilot; the default `pilot` Cargo feature supplies the fallback.

The std-only `dutchman-launcher` crate provides a tiny front command for installations that
ship with Pilot alone. Packagers install its binary as `dutchman`; `dutchman --version`
identifies it as `dutchman launcher <version>`. Install Dutchman Convert for conversion commands.

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
newest. A Homebrew install on macOS needs no first-run approval, because Homebrew's downloads
are not quarantined. Or download a release below.

## Download

<!-- downloads -->
| Your computer | Download | Checksum |
|---|---|---|
| Mac, the desktop app (Apple silicon or Intel) | [dutchman-0.1.5-macos-universal.dmg](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-macos-universal.dmg) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-macos-universal.dmg.sha256) |
| Mac with Apple silicon (M1 or later) | [dutchman-0.1.5-aarch64-apple-darwin.tar.gz](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-aarch64-apple-darwin.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-aarch64-apple-darwin.tar.gz.sha256) |
| Mac with an Intel processor | [dutchman-0.1.5-x86_64-apple-darwin.tar.gz](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-x86_64-apple-darwin.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-x86_64-apple-darwin.tar.gz.sha256) |
| Windows, 64-bit | [dutchman-0.1.5-x86_64-pc-windows-msvc.zip](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-x86_64-pc-windows-msvc.zip) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-x86_64-pc-windows-msvc.zip.sha256) |
| Linux, x86_64 | [dutchman-0.1.5-x86_64-unknown-linux-gnu.tar.gz](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-x86_64-unknown-linux-gnu.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-x86_64-unknown-linux-gnu.tar.gz.sha256) |
| Linux, ARM 64-bit (aarch64) | [dutchman-0.1.5-aarch64-unknown-linux-gnu.tar.gz](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-aarch64-unknown-linux-gnu.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/download/v0.1.5/dutchman-0.1.5-aarch64-unknown-linux-gnu.tar.gz.sha256) |

Not sure which Mac you have? Apple menu › **About This Mac**: a chip named Apple M… is Apple silicon; a processor named Intel… is Intel.
<!-- /downloads -->

Each archive holds `dutchman`, `dutchman-mcp` and the desktop app, Dutchman Convert
(`dutchman-gui`).
[Getting started](docs/guide.md) shows how to check it, install it and convert a sample in a
minute. Every release is on the [releases page](https://github.com/dutchman-cad/dutchman/releases).

## What converts

Early development. Reader status says whether geometry can be read; the conversion report
says what this file kept. Formats with geometry readers attempt conversion through STEP.
Creo and NX share `.prt` and are identified by contents, including numbered `.prt.N` saves.
SolidWorks assemblies are recognised without a reader. `dutchman formats` lists the current
reader status and what Dutchman converts:

<!-- formats: generated from dutchman formats -->
```text
Fusion archive (.f3d)  reads geometry  converts: geometry; feature history when every feature maps; components and mapped joints
Fusion multi-document archive (.f3z)  reads geometry  converts: as a Fusion archive, with referenced designs merged in by cadmpeg
Inventor part (.ipt)  reads geometry  converts: geometry; feature history when every feature maps
Inventor assembly (.iam)  reads geometry  converts: placed parts and sub-assemblies, read from their files in the project
SolidWorks part (.sldprt)  reads geometry  converts: geometry; feature history waits on cadmpeg binding sketches and extrude profiles
SolidWorks assembly (.sldasm)  recognised, not read yet  not yet: nothing yet; reading geometry is not supported yet
FreeCAD document (.FCStd)  reads geometry  converts: geometry and components; feature history and joints when they map
CATIA part (.CATPart)  reads geometry  converts: geometry through STEP; solids and surfaces SolidCraft can import
Creo part (.prt, .prt.N)  opens, no geometry yet  not yet: nothing yet; reading geometry is not supported yet
NX part (.prt)  opens, no geometry yet  not yet: nothing yet; reading geometry is not supported yet
Creo View (.pvz)  reads geometry  converts: geometry through STEP; solids and surfaces SolidCraft can import
Rhino (.3dm)  reads geometry  converts: geometry through STEP; solids and surfaces SolidCraft can import
ACIS (.sat, .sab, .smt, .smb)  reads geometry  converts: geometry through STEP; solids and surfaces SolidCraft can import
STEP (.step, .stp)  reads geometry  converts: geometry through STEP; solids and surfaces SolidCraft can import
IGES (.igs, .iges)  reads geometry  converts: geometry through STEP; solids and surfaces SolidCraft can import
Creo or NX part (.prt)  recognised, not read yet  not yet: nothing yet; reading geometry is not supported yet

Written as a SolidCraft design (.solidcraft), or, experimental, as a FreeCAD rebuild macro (.FCMacro, --to freecad)
```
<!-- /formats -->

After a refused conversion or a history that did not fully map, Dutchman offers short recipes
for making small test files: [fixtures](https://dutchman-cad.github.io/fixtures/) or
`/volunteer` in [Discord](https://discord.gg/yTcseBEdeu). `--quiet` hides the invitation;
`--json` prints only the report. The desktop app shows the same invitation once under its results.
Got a file that converts wrong? Drop it, with its STEP export, in a DM to Dutchy in our Discord.

- When every feature in a design's history maps onto SolidCraft, the SolidCraft design keeps
  that history, editable, with the original geometry kept beside it. If `solidcraft-cli` is
  installed, Dutchman replays the history in it and checks it against the original before
  keeping it. What maps, most of it from Fusion (body patterns, mirrors, booleans, and
  more of the rest on main, in the next release):
  - sketches: lines, arcs, circles, conics, splines, and many constraints and dimensions;
  - extrudes and revolves, with their end forms;
  - holes: counterbores, countersinks, chamfered entries, and thread labels on tapped holes;
  - sweeps, lofts, and coils;
  - linear, circular, and path patterns of features or bodies, body mirrors, and body
    booleans;
  - sheet-metal base flanges;
  - constant-radius fillets and equal-setback chamfers, on edges proven on SolidCraft's own
    replay of the history before them (so they need `solidcraft-cli`); a Fusion fillet with a
    tangency weight of exactly 1 maps as a circular fillet (on main, in the next release).

  [Feature history](docs/guide.md#feature-history-and-the-solidcraft-check) in the guide has
  the detail, and what stops a history.
- Otherwise the design converts as solid geometry without history. Sketches that map, on
  planes that resolve, are kept with it, hidden, for reference (on main, in the next release).
- Assemblies keep their components and placements. Fusion and FreeCAD assemblies keep their
  joints too (revolute, slider, and rigid), with their grounded parts held where they are;
  Inventor assemblies place their parts, without joints. Sketches in the assembly's own file
  are kept, hidden, for reference (on main, in the next release).
- FreeCAD output is experimental: `--to freecad` writes a self-contained rebuild macro,
  `--to fcstd` runs FreeCAD headlessly to save a native `.FCStd` document and checks its
  solids against the original, and `--to freecad --verify` runs the same check on a macro.
- Every conversion lists what it could not carry across, as Info, Degraded or Dropped.

The output is a SolidCraft design. `--to freecad` writes a FreeCAD rebuild macro (`.FCMacro`)
instead. It rebuilds the mapped history in FreeCAD, keeps the original geometry, and rebuilds
an assembly's joints (fixed, revolute, slider, cylindrical, ball, and more, through
sub-assemblies too) in the Assembly workbench, with its grounded parts. When a later feature
can't be rebuilt, the mapped features before it and the mapped sketches are kept, hidden, in
a "Rebuilt history (partial)" group beside the original geometry (experimental, on main, in
the next release; see
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

## Desktop app

`dutchman-gui` converts without a terminal. Drop design files or folders on the window (or use
**Add files…** and **Add folder…**), choose an output folder, and press **Convert**. Each file
shows whether it converted, what was written (editable history or geometry), and its losses as
Info, Degraded or Dropped. A converted design opens in SolidCraft from its card: the app that
`SOLIDCRAFT_APP` names, else `solidcraft` on `PATH`, else the system's app for `.solidcraft`
files. When an Inventor assembly's part files are not found, its card names them: **Search a
folder…** or **Find a file…** converts it again, searching there too. The app follows the
system's light or dark setting, or can be set to Day or Night. On main, in the next release:
its **Output format** choice offers a FreeCAD macro, the window title shows its version,
`dutchman-gui --version` and `--help` print without opening a window, a **Report a
problem** link opens the **A file didn't convert** form, and a failed file's **Report this
on GitHub** opens that form filled in with the version, system, error and an anonymous
report. On main, in the next release, the app is named **Dutchman Convert**: its window, and
on macOS **Dutchman Convert.app**; the commands keep their names.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/dutchman-gui-night.png">
  <img alt="The Dutchman Convert app after converting three files: two converted, one failed with its reason" src="docs/images/dutchman-gui-day.png" width="480">
</picture>

On macOS it comes as **Dutchman Convert.app** in a disk image (**Dutchman.app** in 0.1.5 and
earlier, from 0.1.3 on): open it and drag it onto Applications in the window that appears (see
[opening it the first time](docs/guide.md#opening-the-app-the-first-time): approve it once
and the commands inside it run too). It ships in the
release archives beside `dutchman` (from 0.1.1 on), or build it from source
with Rust 1.88 or newer:

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
| `convert` | `input`, `output`, and optionally `import_only`, `verify`, `solidcraft`, `verify_timeout`, `overwrite` | `dutchman.convert/1` |
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

## Licence

Licensed under the [Apache License, Version 2.0](LICENSE).
