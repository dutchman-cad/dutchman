# Test fixtures

Every file here is a synthetic test input from cadmpeg (<https://github.com/cadmpeg/cadmpeg>,
Apache-2.0). None is a real design.

| File | What it is | Where it came from |
|---|---|---|
| `generated_unit_cube.f3d` | A generated Fusion archive holding one solid 10 mm cube (one body, six faces). | Copied unchanged from cadmpeg `main` at `f816fb00fb54` (cadmpeg PR #257), `crates/cadmpeg-codec-f3d/tests/golden/fixtures/`. |
| `container_metadata_only.f3d` | A Fusion archive with container metadata and no geometry. | Copied unchanged from cadmpeg `main` at `5f01055eb1fd`, same folder. |
| `design_sketch_constraints.f3d` | A Fusion design whose history is one sketch on XY: an arc, points, dimensions and two parameters. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder. |
| `design_copy_paste.f3d` | A Fusion design placing one component twice (a copy and paste), whose component does not list its bodies. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder as the other Fusion files. |
| `synthetic_cube.ipt` | An Inventor part envelope holding one solid 10 mm cube (one body, six faces) as its ASM kernel stream. | Generated (see below). |
| `solidworks_cube.sldprt` | A SolidWorks part holding one solid 10 mm cube (one body, six faces) as a Parasolid partition, with no feature history. | Generated (see below). |
| `body_history.sldprt` | A SolidWorks part with a one-triangle body and a Keywords history: a 12.5 mm `Boss` extrusion and its `EquationDrivenCurve` profile. | Copied unchanged from cadmpeg `main` at `7bcc95276548` (the crates.io 0.6.2 release), `crates/cadmpeg-codec-sldprt/tests/golden/fixtures/`. |
| `container_metadata.sldprt` | A SolidWorks part with container metadata and no geometry. | As above. |

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

cadmpeg's published SolidWorks fixtures hold one-triangle bodies, which do not restate as STEP
solids, so `solidworks_cube.sldprt` was generated with cadmpeg's own encoder, at cadmpeg `main`
`f816fb00fb54`:

1. `cadmpeg_ir::examples::unit_cube()` (the 10 mm example cube), prepared as
   cadmpeg-codec-sldprt's test builder `source_less_cube()` does
   (`src/test_support/ir.rs`): body and face names cleared, edge parameter ranges cleared.
2. Written by `SldprtCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)` and
   `write_to`, the codec's source-less semantic writer. The output is deterministic.
