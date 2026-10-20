# Test fixtures

Every file here is a synthetic test input from cadmpeg (<https://github.com/cadmpeg/cadmpeg>,
Apache-2.0). None is a real design.

| File | What it is | Where it came from |
|---|---|---|
| `generated_unit_cube.f3d` | A generated Fusion archive holding one solid unit cube (one body, six faces). | Copied unchanged from cadmpeg `main` at `5f01055eb1fd`, `crates/cadmpeg-codec-f3d/tests/golden/fixtures/`. |
| `container_metadata_only.f3d` | A Fusion archive with container metadata and no geometry. | As above. |
| `design_sketch_constraints.f3d` | A Fusion design whose history is one sketch on XY: an arc, points, dimensions and two parameters. | Copied unchanged from cadmpeg `main` at `5d345a29ccd6`, same folder. |
| `synthetic_cube.ipt` | An Inventor part envelope holding one solid 10 mm cube (one body, six faces) as its ASM kernel stream. | Generated (see below). |

cadmpeg publishes no Inventor part with geometry, so `synthetic_cube.ipt` was generated from
cadmpeg's own synthetic sources, at cadmpeg `main` `5d345a29ccd6` / crates.io 0.6.2:

1. `cadmpeg_ir::examples::unit_cube()` (the 10 mm example cube), written as a Fusion archive by
   cadmpeg's f3d encoder; its `Breps.BlobParts/BREP.generated.smbh` entry is the cube as an
   `ASM BinaryFile8` kernel stream.
2. That stream wrapped in an Inventor part envelope by cadmpeg-codec-inventor's test fixture
   builder, `primary_envelope_fixture_with_kernel(EnvelopeDeclarations::default(), &stream)`
   (`src/test_support/test_fixtures.rs`).
