# Test fixtures

All files here are synthetic test inputs copied unchanged from cadmpeg's public test suite
(<https://github.com/cadmpeg/cadmpeg>, Apache-2.0), at
`crates/cadmpeg-codec-f3d/tests/golden/fixtures/` on `main` as of commit `5f01055eb1fd`.
None is a real design. `design_sketch_constraints.f3d` was copied later, from `main` at `5d345a29ccd6`.

| File | What it is |
|---|---|
| `generated_unit_cube.f3d` | A generated Fusion archive holding one solid unit cube (one body, six faces). |
| `container_metadata_only.f3d` | A Fusion archive with container metadata and no geometry. |
| `design_sketch_constraints.f3d` | A Fusion design whose history is one sketch on XY: an arc, points, dimensions and two parameters. |
