# FreeCAD documents (`.FCStd`)

Dutchman reads FreeCAD documents through cadmpeg's `fcstd` codec, from cadmpeg 0.6.3 on, and
writes them as SolidCraft designs.

## What converts

- **Geometry.** Every solid and surface in the document, written as a SolidCraft import and,
  with `solidcraft-cli`, checked in SolidCraft like any other import.
- **Components.** A document whose cadmpeg model says which bodies each component owns
  converts as SolidCraft components and occurrences, as a Fusion design does.
- **Feature history** is not mapped yet, so every document converts as geometry without
  history, and the report names the first feature that stopped it. FreeCAD's PartDesign
  Body, its sketches, Pads, Pockets, Revolutions, Holes and Thicknesses are the next to map.

`dutchman inspect file.FCStd` shows what a document would carry across.

## Builds without FreeCAD

FreeCAD support is the `freecad` cargo feature of `dutchman`, `dutchman-gui` and
`dutchman-core`, on by default, so every release archive from 0.1.3 on, and every nightly, has it. A build made
with `--no-default-features` leaves cadmpeg's FreeCAD codec out; it lists FreeCAD as not
converting and answers a `.FCStd` file with "FreeCAD files convert in nightly builds for
now", linking the [nightly builds](https://github.com/dutchman-cad/dutchman/releases/tag/nightly).

## Testing without FreeCAD

The FreeCAD fixtures in `crates/dutchman-core/tests/fixtures/` are cadmpeg's corpus
documents, authored in FreeCAD 1.1.1 by the cadmpeg project and dedicated to the public domain
(CC0-1.0). See that folder's README for where each came from.
