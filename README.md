<h1>
  <img alt="Dutchman Convert" src="assets/logo/dutchman-lockup.png#gh-light-mode-only" width="252">
  <img alt="Dutchman Convert" src="assets/logo/dutchman-lockup-night.png#gh-dark-mode-only" width="252">
</h1>

Converts Fusion, Inventor, SolidWorks parts, FreeCAD, CATIA, Rhino, Creo View, ACIS, and STEP designs into editable SolidCraft designs.

<a href="https://dutchman-cad.github.io/view.html?src=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/docs/images/readme-result-day.png&amp;alt=Dutchman%20Convert%20in%20Day%20theme%3A%20CC0%20FreeCAD%20bracket%20converted%2C%20with%20its%20mapped%20feature%20history%20and%20Open%20in%20SolidCraft%20result%20card"><img alt="Dutchman Convert in Day theme: CC0 FreeCAD bracket converted, with its mapped feature history and Open in SolidCraft result card" src="docs/images/readme-result-day.png#gh-light-mode-only" width="100%"></a>
<a href="https://dutchman-cad.github.io/view.html?src=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/docs/images/readme-result-night.png&amp;alt=Dutchman%20Convert%20in%20Night%20theme%3A%20the%20same%20CC0%20bracket%20conversion%20and%20feature%20history%20result%20card"><img alt="Dutchman Convert in Night theme: the same CC0 bracket conversion and feature history result card" src="docs/images/readme-result-night.png#gh-dark-mode-only" width="100%"></a>

## Download

- **Mac:** `brew install --cask dutchman-cad/tap/dutchman-convert` · [download the app](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.dmg).
- **Windows:** [download the 64-bit ZIP](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-pc-windows-msvc.zip), extract it, and open `dutchman-gui`.
- **Linux:** download for [x86_64](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-unknown-linux-gnu.tar.gz) or [ARM64](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-aarch64-unknown-linux-gnu.tar.gz), extract it, and open `dutchman-gui`.

- **Homebrew:** `brew install dutchman-cad/tap/dutchman` (commands and desktop app).
- **Scoop:** `scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket`,
  then `scoop install dutchman-cad/dutchman`.

<a id="install"></a><a id="direct-downloads"></a>[Checksums, archives, Docker, and command-line installs](docs/install.md)

## How to use it

1. **Drop files or folders in.** Or choose **Add files…**; then choose an output folder.

   <a href="https://dutchman-cad.github.io/view.html?src=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/docs/images/readme-drop-day.png&amp;alt=Day%20theme%20start%20screen%20with%20the%20drop%20area%20and%20Add%20files%20button"><img alt="Day theme start screen with the drop area and Add files button" src="docs/images/readme-drop-day.png" width="100%"></a>

2. **Pick Fast, Standard, or Full.** Fast keeps the shape; Standard keeps history, sketches, and joints within available memory; Full keeps everything that maps. The live estimate shows memory and time before you start.

   <a href="https://dutchman-cad.github.io/view.html?src=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/docs/images/readme-preset-day.png&amp;alt=CC0%20bracket%20queued%20in%20Day%20theme%20with%20Standard%20selected%20and%20a%20live%20memory%20and%20time%20estimate"><img alt="CC0 bracket queued in Day theme with Standard selected and a live memory and time estimate" src="docs/images/readme-preset-day.png" width="100%"></a>

3. **Convert, then Open in SolidCraft.** The result card says what came across and what could not. Your converted design is a `.sc` file (SolidCraft targets older than 0.25.1 get `.solidcraft`).

   <a href="https://dutchman-cad.github.io/view.html?src=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/docs/images/readme-result-day.png&amp;alt=CC0%20bracket%20result%20in%20Day%20theme%20showing%20mapped%20feature%20history%20and%20Open%20in%20SolidCraft"><img alt="CC0 bracket result in Day theme showing mapped feature history and Open in SolidCraft" src="docs/images/readme-result-day.png" width="100%"></a>

## What's new in 0.2.5

- **Editable history:** Fusion features, sketches, and parameters stay editable where they map.
- **Placed assemblies:** parts keep their poses; conflicting joints stay suppressed.
- **Faster conversions:** parallel part work, presets, and separate memory and time estimates.
- **Desktop controls:** update and SolidCraft version pills, plus diagnostic logs.

Full notes: [release page](https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.5)

## Command line

```sh
dutchman part.f3d                 # writes part.sc
dutchman part.f3d out.sc
dutchman convert part.f3d out.sc
dutchman convert part.f3d --preset fast
```

[Getting started](docs/guide.md) covers options, presets, version compatibility, partial history, and parallel conversion.

## What converts

Reader status says whether geometry can be read; the conversion report says what this
file kept. Run `dutchman formats` for the live list ([details](docs/guide.md#formats)).

<!-- formats: generated from dutchman formats -->

| Format | Files | Reads geometry | Level | Converts |
|---|---|---|---|---|
| Fusion archive | `.f3d` | Yes | L5 | Geometry; feature history when every feature maps |
| Fusion multi-document archive | `.f3z` | Yes | L5 | As Fusion; referenced designs merged |
| Inventor part | `.ipt` | Yes | L3 | Geometry; feature history when every feature maps |
| Inventor assembly | `.iam` | Yes | L3 | Placed parts and sub-assemblies |
| SolidWorks part | `.sldprt` | Yes | L3 | Geometry through STEP |
| SolidWorks assembly | `.sldasm` | No | — | Not supported yet |
| FreeCAD document | `.FCStd` | Yes | L5 | Geometry; history and joints when they map |
| CATIA part | `.CATPart` | Yes | L3 | Geometry through STEP |
| Creo part | `.prt`, `.prt.N` | No | L1 | Not supported yet |
| NX part | `.prt` | No | L1 | Not supported yet |
| Creo View | `.pvz` | Yes | L3 | Geometry through STEP |
| Rhino | `.3dm` | Yes | L2 | Geometry through STEP |
| ACIS | `.sat`, `.sab`, `.smt`, `.smb` | Yes | L3 | Geometry through STEP |
| STEP | `.step`, `.stp` | Yes | L8 | Geometry through STEP |
| IGES | `.igs`, `.iges` | No | L9 | Import it in SolidCraft directly |
| Creo or NX part | `.prt` | No | — | Not supported yet |

<!-- /formats -->

FreeCAD TechDraw pages convert to DXF.
Output is a SolidCraft design (`.sc`) or an experimental FreeCAD rebuild macro
(`.FCMacro`, `--to freecad`).

## Send us a file that won't convert

Make a small public test file from a [fixtures recipe](https://dutchman-cad.github.io/fixtures/),
or send a file that converts wrong, with its STEP export, to Dutchy in
[Discord](https://discord.gg/yTcseBEdeu). Share only designs you own and can share.

## Use from scripts and AI agents

`dutchman inspect --json part.ipt` previews conversion; `dutchman convert --json part.f3d part.sc`
writes a design and a JSON report. `dutchman-mcp` exposes the same reports over stdio
through the Model Context Protocol. Add it to your client's MCP configuration:

<a id="as-an-mcp-server"></a>

```json
{
  "mcpServers": {
    "dutchman": { "command": "dutchman-mcp" }
  }
}
```

| Tool | Purpose |
|---|---|
| `inspect` | Preview what a file can keep |
| `convert` | Convert one file |
| `convert_all` | Convert files and folders |
| `formats` | List format capabilities |

See [MCP setup and tool options](docs/mcp.md) and [JSON reports](docs/json.md).

<a id="learn-more"></a>

## Docs

- [Install options](docs/install.md): checksums, archives, package managers, and Docker.
- [Format details](docs/formats.md): geometry, history, assemblies, and losses.
- [Getting started](docs/guide.md): commands, presets, reports, and troubleshooting.
- <a id="desktop-app"></a>[Desktop app](docs/desktop.md): presets, Advanced, estimates, diagnostics, and updates.
- <a id="pilot"></a>[Pilot](docs/pilot.md): drive SolidCraft with plain-language requests.
- [Browser conversion](docs/web.md): run Dutchman Web on your own server.
- [MCP tools](docs/mcp.md): client setup, tools, and examples.
- [JSON reports](docs/json.md): schemas, fields, and exit codes.
- [SolidCraft compatibility](docs/compatibility.md): versions and history checks.
- [Docker](docs/docker.md): containers, Compose, and the MCP server.
- <a id="building"></a>[Building](docs/building.md): Rust builds and packaging.
- <a id="freecad-techdraw-drawing-output"></a>[FreeCAD TechDraw output](docs/drawings.md): drawing pages to DXF.
- [Fusion](docs/fusion.md): history, components, visibility, and colours.
- [Inventor](docs/inventor.md): parts, assemblies, and finding references.
- [SolidWorks](docs/sldprt.md): parts and gaps in assembly support.
- [FreeCAD](docs/freecad.md): history, expressions, assemblies, and macros.
- [Anonymous reports](docs/report-anon.md): what is safe to share.
- [Testing your library](docs/testing-your-library.md): convert your files and report problems.
- [Triage](docs/triage.md): report causes and upstream fixes.
- [Architecture](docs/ARCHITECTURE.md): pipeline and crate boundaries.
- [Releasing](docs/releasing.md): release checks and packaging.
- [Agent skill](skills/README.md): conversion workflow for AI agents.
- [Contributing](CONTRIBUTING.md): checks, fixtures, and clean-room rules.

## Licence

Licensed under the [Apache License, Version 2.0](LICENSE).
