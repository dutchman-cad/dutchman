# FreeCAD documents (`.FCStd`)

FreeCAD documents convert in the [nightly builds](https://github.com/dutchman-cad/dutchman/releases/tag/nightly)
for now, through cadmpeg's `fcstd` codec. Stable releases list FreeCAD as not converting yet
and answer a `.FCStd` file with "FreeCAD files convert in nightly builds for now".

## Why only nightly

cadmpeg 0.6.3 reads FreeCAD documents, but not yet their solids as FreeCAD shows them. Checked
in SolidCraft, none of 18 single-part test documents (pads, pockets, revolves, holes, shells)
came out as one solid of the right volume:

- some of FreeCAD's faces are stored as replicas of curves, which SolidCraft leaves out, so
  solids open as empty shells of zero volume;
- the intermediate shapes FreeCAD keeps for each feature come through as bodies of their own,
  beside the part;
- revolved faces can face inwards, so their volume reads wrong.

A cadmpeg release that takes only each Body's Tip shape, keeps those faces and sets their
sense makes stable builds convert FreeCAD documents too; until then the nightly builds, made
against cadmpeg's `main`, carry the fixes as they land.

## What converts

- **Geometry**, written as a SolidCraft import and checked in SolidCraft like any other.
- **Components**, when cadmpeg says which bodies each owns, as for a Fusion design.
- **Feature history** is not mapped yet, so every document converts as geometry without
  history, and the report names the first feature that stopped it. FreeCAD's PartDesign
  Body, its sketches, Pads, Pockets, Revolutions, Holes and Thicknesses are the next to map.

## Building with FreeCAD

FreeCAD support is the `freecad` cargo feature of `dutchman` and `dutchman-gui`, off by
default:

```sh
cargo build --release -p dutchman -p dutchman-gui --features dutchman/freecad,dutchman-gui/freecad
```

## Testing without FreeCAD

The FreeCAD fixtures in `crates/dutchman-core/tests/fixtures/` are cadmpeg's corpus
documents, authored in FreeCAD 1.1.1 by the cadmpeg project and dedicated to the public domain
(CC0-1.0). See that folder's README for where each came from.
