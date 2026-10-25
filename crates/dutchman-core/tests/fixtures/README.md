# Test fixtures

Every file here is a synthetic test input from cadmpeg (<https://github.com/cadmpeg/cadmpeg>,
Apache-2.0; the FreeCAD documents, from its donated corpus, are CC0-1.0). None is a real design.

| File | What it is | Where it came from |
|---|---|---|
| `generated_unit_cube.f3d` | A generated Fusion archive holding one solid 10 mm cube (one body, six faces). | Copied unchanged from cadmpeg `main` at `f816fb00fb54` (cadmpeg PR #257), `crates/cadmpeg-codec-f3d/tests/golden/fixtures/`. |
| `container_metadata_only.f3d` | A Fusion archive with container metadata and no geometry. | Copied unchanged from cadmpeg `main` at `5f01055eb1fd`, same folder. |
| `design_sketch_constraints.f3d` | A Fusion design whose history is one sketch on XY: an arc, points, dimensions and two parameters. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder. |
| `design_copy_paste.f3d` | A Fusion design placing one component twice (a copy and paste), whose component does not list its bodies. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder as the other Fusion files. |
| `synthetic_cube.ipt` | An Inventor part envelope holding one solid 10 mm cube (one body, six faces) as its ASM kernel stream. | Generated (see below). |
| `solidworks_cube.sldprt` | A SolidWorks part holding one solid 10 mm cube (one body, six faces) as a Parasolid partition in an authored native envelope, with no feature history. | Generated at cadmpeg `892efade6b496a0af7ac409568d4b165644b88d4` (see below). |
| `body_history.sldprt` | A SolidWorks part with a one-triangle body and a Keywords history: a 12.5 mm `Boss` extrusion and its `EquationDrivenCurve` profile. | Copied unchanged from cadmpeg `892efade6b496a0af7ac409568d4b165644b88d4`, `crates/cadmpeg-codec-sldprt/tests/golden/fixtures/authored_complete_history.sldprt` (introduced in PR #286, commit `3134762abc`). |
| `container_metadata.sldprt` | An authored SolidWorks native envelope with a synthetic metadata token and no geometry. | Generated with cadmpeg's test helpers at the same commit (see below). |
| `core_design_product.FCStd` | A FreeCAD document: exact geometry (five bodies, 22 faces), persistent names, sketch constraints, design properties, a product link and a spreadsheet. | Copied unchanged from cadmpeg's corpus, `corpus/freecad_fcstd/fixtures/` at `main` `377eb82352dd`: authored in FreeCAD 1.1.1 by the cadmpeg project and dedicated CC0-1.0 (`corpus/manifest.toml`, SHA-256 `69c6aa00…989ba`). |
| `design_history.FCStd` | A FreeCAD PartDesign Body with constrained sketches, an additive Pad, a subtractive Pocket and an edge Fillet. | As above (SHA-256 `c71da85e…ea0d7`). |
| `product_assembly.FCStd` | A FreeCAD Assembly with groups, nested, array and external occurrences, grounding, and a limited revolute joint. | As above (SHA-256 `b8e0e92c…d8340`). |

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

The original SolidWorks cube was regenerated at `f816fb00fb54` in Dutchman PR #42
(`e73c5f0`). cadmpeg PR #286 replaced the old synthetic envelopes with conforming authored
fixtures. The SolidWorks cube is now generated with cadmpeg's own encoder and test helpers
at `892efade6b496a0af7ac409568d4b165644b88d4`:

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
to cadmpeg's detector; all other suffixes identify the format before its decoder checks
the contents.

The STEP, IGES and Rhino conversion tests generate inputs in temporary folders with the
pinned cadmpeg encoders from `cadmpeg_ir::examples::unit_cube()`. Face names are cleared,
as in cadmpeg's Rhino measured-geometry tests, because that writer does not carry face
names. No generated interchange files are stored here.
