# FreeCAD documents (`.FCStd`)

FreeCAD documents convert in the [nightly builds](https://github.com/dutchman-cad/dutchman/releases/tag/nightly)
for now, through cadmpeg's `fcstd` codec. Stable releases list FreeCAD as not converting yet
and answer a `.FCStd` file with "FreeCAD files convert in nightly builds for now".

## Why only nightly

cadmpeg 0.6.3 reads FreeCAD documents, but not yet their solids as FreeCAD shows them: some
faces are stored as replicas of curves, which SolidCraft leaves out, the intermediate shape
of each feature comes through as a body of its own, and revolved faces can face inwards.
Checked in SolidCraft, none of 18 single-part test documents came out as one solid of the
right volume.

cadmpeg's `main` takes only each Body's shape and fixes those faces: built against it, all 19
of the test documents (pads, pockets, revolves, holes, fillets, chamfers, shells and a
three-part assembly) come out at FreeCAD's volume. Stable builds convert FreeCAD documents
from the cadmpeg release that carries this; until then the nightly builds do.

## What converts

- **Geometry**, written as a SolidCraft import and checked in SolidCraft like any other.
  Only the bodies FreeCAD shows convert: a body cadmpeg marks hidden (`visible: false`, as it
  marks the shape of each step inside a PartDesign Body) is left out, with the geometry only
  it uses, and the report says how many were (`Info`). A document that hides every body
  converts all of them, as stored. cadmpeg 0.6.3 marks none; its `main` does, so this takes
  effect in stable builds with cadmpeg's next release.
- **Components**, when cadmpeg says which bodies each owns, as for a Fusion design.
- **Grounded joints** hold their part fixed to the design where it is stored, as a rigid
  joint to the design; the assembly's revolute and slider joints keep their motion.
- **Feature history**, when every feature maps: sketches, pads and simple holes, replayed
  and checked in SolidCraft like a Fusion history. Fillets, chamfers, thicknesses (shells),
  counterbored holes and revolutions short of a full turn are not mapped yet; a document
  holding one converts as geometry, and the report names the first feature that stopped it.

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
