# Test fixtures

Every file here is a synthetic test input from cadmpeg (<https://github.com/cadmpeg/cadmpeg>,
Apache-2.0; its donated FreeCAD corpus is CC0-1.0), or a CC0 CAD Fixture Recipes model.
None is a real design.

| File | What it is | Where it came from |
|---|---|---|
| `generated_unit_cube.f3d` | A generated Fusion archive holding one solid 10 mm cube (one body, six faces). | Copied unchanged from cadmpeg `main` at `f816fb00fb54` (cadmpeg PR #257), `crates/cadmpeg-codec-f3d/tests/golden/fixtures/`. |
| `container_metadata_only.f3d` | A Fusion archive with container metadata and no geometry. | Copied unchanged from cadmpeg `main` at `5f01055eb1fd`, same folder. |
| `design_sketch_constraints.f3d` | A Fusion design whose history is one sketch on XY: an arc, points, dimensions and two parameters. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder. |
| `design_copy_paste.f3d` | A Fusion design placing one component twice (a copy and paste), whose component does not list its bodies. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder as the other Fusion files. |
| `synthetic_cube.ipt` | An Inventor part envelope holding one solid 10 mm cube (one body, six faces) as its ASM kernel stream. | Generated (see below). |
| `solidworks_cube.sldprt` | A SolidWorks part holding one solid 10 mm cube (one body, six faces) as a Parasolid partition in an authored native envelope, with no feature history. | Generated at cadmpeg `f4f0e51c299a556c3359be91e9e325100c25b0eb` (see below). |
| `body_history.sldprt` | A SolidWorks part with a one-triangle body and a Keywords history: a 12.5 mm `Boss` extrusion and its `EquationDrivenCurve` profile. | Copied unchanged from cadmpeg `892efade6b496a0af7ac409568d4b165644b88d4`, `crates/cadmpeg-codec-sldprt/tests/golden/fixtures/authored_complete_history.sldprt` (introduced in PR #286, commit `3134762abc`). |
| `container_metadata.sldprt` | An authored SolidWorks native envelope with a synthetic metadata token and no geometry. | Generated with cadmpeg's test helpers at the same commit (see below). |
| `core_design_product.FCStd` | A FreeCAD document: exact geometry (five bodies, 22 faces), persistent names, sketch constraints, design properties, a product link and a spreadsheet. | Copied unchanged from cadmpeg's corpus, `corpus/freecad_fcstd/fixtures/` at `main` `377eb82352dd`: authored in FreeCAD 1.1.1 by the cadmpeg project and dedicated CC0-1.0 (`corpus/manifest.toml`, SHA-256 `69c6aa00…989ba`). |
| `design_history.FCStd` | A FreeCAD PartDesign Body with constrained sketches, an additive Pad, a subtractive Pocket and an edge Fillet. | As above (SHA-256 `c71da85e…ea0d7`). |
| `product_assembly.FCStd` | A FreeCAD Assembly with groups, nested, array and external occurrences, grounding, and a limited revolute joint. | As above (SHA-256 `b8e0e92c…d8340`). |

`freecad-1.1.4-A01/` contains unchanged CC0-1.0 CAD Fixture Recipes booklet 1.30 files,
authored in FreeCAD 1.1.4 (`generated/freecad-1.1.4/manifest.json`):

| File | Recipe and purpose | SHA-256 |
|---|---|---|
| `A01a.FCStd` | A01a: Assembly workbench links to two sibling part documents, with P01a grounded and P02a translated 70 mm along X. | `2538b7c90010aafaecb78df2f948b9994871f76d6f6d21e457ae5527e473b153` |
| `P01a.FCStd` | P01a: box, 6000 mm³; bounds (-10, -15, 0) to (10, 15, 10) mm. | `a306278dd58bb7abb306463887db3df3c032278109358718c5ad3c8b5c103f64` |
| `P02a.FCStd` | P02a: turned part, 62831.8530718 mm³; bounds after A01a placement (40, -30, 0) to (100, 30, 40) mm. | `1a3fbd0781d5468290fcbbba4509c0cd7009286e40b0c257f06d935e0ca0bd82` |

`freecad-1.1.4-N04/` contains unchanged CC0-1.0 CAD Fixture Recipes booklet 1.30
files from the public release
[freecad-1.1.4-recipes-v1.30](https://github.com/dutchman-cad/fixtures/releases/tag/freecad-1.1.4-recipes-v1.30),
asset `freecad-1.1.4-recipes-v1.30.zip`, authored in FreeCAD 1.1.4. The files were copied
from a locally cached copy of that release asset.

| File | Recipe and purpose | SHA-256 |
|---|---|---|
| `N04a.FCStd` | N04a: uniform quadratic periodic sketch B-spline with equal weights and a pad. | `c55205033fe8d9931abcfacf4a827a4feb94b68db1815ff0047fb410d43befdd` |
| `N04b.FCStd` | N04b: a second uniform quadratic periodic sketch B-spline case with equal weights and a pad. | `ff9e07c4aa712b1dbb7686b3d4ee1af7a77942d70ea2de7c63ad12f0a785e12a` |

At the pinned cadmpeg revision, both stop in native sketch decoding because the compact
periodic knot vector is not expanded to CADIR's full vector and corresponding poles.
They record that end-to-end reader gap; periodic mapping tests use synthetic full CADIR.

The three cubes hold `cadmpeg_ir::examples::unit_cube()`, the 10 mm example cube, as fixed in
cadmpeg PR #257 (`main` `f816fb00fb54`): before it, the example's face loops ran clockwise
about their outward normals, so the cube read back inside out (cadmpeg/cadmpeg#255). Each reads
in SolidCraft as a 1000 mm³ solid with six faces, which CI's round-trip job checks.

cadmpeg publishes no Inventor part with geometry, so `synthetic_cube.ipt` was generated from
cadmpeg's own synthetic sources, at cadmpeg `main` `f816fb00fb54`:

1. The `FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh` entry of
   `generated_unit_cube.f3d` above: the cube as an `ASM BinaryFile8` kernel stream, written by
   cadmpeg's f3d encoder.
2. That stream wrapped in an Inventor part envelope by cadmpeg-codec-inventor's test fixture
   builder, `primary_envelope_fixture_with_kernel(EnvelopeDeclarations::default(), &stream)`
   (`src/test_support/test_fixtures.rs`).

cadmpeg #589 corrected the SolidWorks coedge layout, so the earlier cube (written at
`892efade6b`) read inside out on later revisions; this one is written with the corrected encoder.

The original SolidWorks cube was regenerated at `f816fb00fb54` in Dutchman PR #42
(`e73c5f0`). cadmpeg PR #286 replaced the old synthetic envelopes with conforming authored
fixtures. The SolidWorks cube is now generated with cadmpeg's own encoder and test helpers
at `f4f0e51c299a556c3359be91e9e325100c25b0eb`:

1. `cadmpeg_ir::examples::unit_cube()` (the 10 mm example cube), prepared as
   cadmpeg-codec-sldprt's test builder `source_less_cube()` does
   (`src/test_support/ir.rs`): body and face names cleared, edge parameter ranges cleared.
2. Planned with `test_support::plan_fixture_with_native_envelope(EncodeInput::new(&ir, None),
   TargetRequest::Inherit)` and written with `write_to`. This helper supplies the authored
   native envelope now required by `SldprtCodec.plan`. The output is deterministic.

`container_metadata.sldprt` uses `test_support::container::outer_header()`, followed by
`make_block(0, "FixtureEnvelopeTokenSource", b"authored envelope fixture")`, wrapped with
`native_envelope_fixture`. These are the same helpers and metadata token used by the
encoder's fixture envelope context. It exercises a readable envelope without geometry,
rather than a malformed directory.

All three SolidWorks fixtures use cadmpeg's public synthetic sources under Apache-2.0.

Format detection tests also construct short byte signatures in Rust: ZIP/OLE markers,
Creo `#UGC:1`/`#UGC:2`, NX `SPLMSSTR`, CATIA `V5_CFV2`, Rhino's archive header,
ACIS text/binary markers, STEP Part 21 and an IGES start record. They contain no design
data and are generated only in temporary test folders. Creo and NX signatures are passed
to cadmpeg's detector; all other suffixes identify the format without reading the contents.
IGES refusal tests use synthetic text and missing files to check that no decoding occurs.

The STEP and Rhino conversion tests generate inputs in temporary folders with the
pinned cadmpeg encoders from `cadmpeg_ir::examples::unit_cube()`. Face names are cleared,
as in cadmpeg's Rhino measured-geometry tests, because that writer does not carry face
names. No generated interchange files are stored here.

### FreeCAD TechDraw pages

`techdraw.FCStd` (4.9 KB) and `techdraw-pages.FCStd` (2.8 KB) were made by running FreeCAD
1.1.4 as an external program on `techdraw.py`, which uses only FreeCAD's public Python
scripting API. They contain only authored synthetic objects: a 30 × 20 × 10 mm box, front
(scale 2) and top views, a length dimension, a two-line annotation, and an authored SVG
template; the second file has two pages with text and templates and no model body. No
installed template or third-party CAD file was copied. The script writes the template itself
and checks the dimension's 30 mm value and the template's title field before saving.

Regenerate with `DUTCHMAN_FIXTURE_OUT=/path/to/output flatpak run
--command=FreeCADCmd org.freecad.FreeCAD techdraw.py`.

### Neutral TechDraw content fixtures

`techdraw.FCStd`, `techdraw-pages.FCStd`,
`cadmpeg-techdraw-views.FCStd`, `cadmpeg-techdraw-dimensions.FCStd`, and
`cadmpeg-techdraw-variants.FCStd` are copied unchanged from cadmpeg
`f63ba7ea67b6cbcf185534d874a0cd7f4ba1f6d6`,
`corpus/freecad_fcstd/fixtures/techdraw*.FCStd` (excluding `techdraw_annotations`).
They are CC0 fixtures authored by `corpus/freecad_fcstd/author_techdraw_fixture.py`
using FreeCAD 1.1.4 through its public scripting interface (version and CC0
dedication recorded in cadmpeg’s `corpus/manifest.toml`). The first two are byte-identical
to the retained draft fixtures; no duplicate copies are stored. They exercise neutral template primitives,
resolved editable text, projected curves, dimensions, and text height.

### Diagnostic log archive tests

`crates/dutchman/tests/diagnostics.rs` builds temporary `.f3z` archives from the repository's
`generated_unit_cube.f3d`, with a synthetic `Manifest.json` naming its root and an empty
`DesignDescription.json` graph, following cadmpeg's public `test_support::assembly_test`
recipe at `43fbc10c4910f2c549d7ca79001acb11429aea3f`. These temporary files exercise
anonymous member lists and decode events; no third-party models are used or retained.

Minimum-reproduction checks in `crates/dutchman/src/repro/tests.rs` combine independent
copies of `generated_unit_cube.f3d` in memory, rename their identities, and add a periodic
NURBS use-curve whose range crosses its carrier's stored parameter domain. The 88-finding
case groups several cube regions into one body. These models use cadmpeg's public CADIR
types and the existing synthetic cube only; no additional native design file is stored.
The member-session check wraps two unchanged copies of that cube in a temporary F3Z using
cadmpeg's public synthetic manifest recipe.

`placed_cubes.sldasm` is generated by `create_solidworks_assembly.py`, using cadmpeg
`6118386f19e0`'s public synthetic block and native envelope helpers
(`cadmpeg-codec-sldprt/src/test_support/container.rs`) and an authored component tree.
It references the synthetic `solidworks_cube.sldprt` twice through an absolute Windows
path, with one rotated and translated placement. Two more references are suppressed and
mirrored, respectively, to exercise cadmpeg's losses. No real assembly supplied its
names, geometry, structure or placements. Regenerate with `python3
crates/dutchman-core/tests/fixtures/create_solidworks_assembly.py`.
