# Test fixtures

Both files are synthetic test inputs copied unchanged from cadmpeg's public test suite
(<https://github.com/cadmpeg/cadmpeg>, Apache-2.0), at
`crates/cadmpeg-codec-f3d/tests/golden/fixtures/` on `main` as of commit `5f01055eb1fd`.
Neither is a real design.

| File | What it is |
|---|---|
| `generated_unit_cube.f3d` | A generated Fusion archive holding one solid unit cube (one body, six faces). |
| `container_metadata_only.f3d` | A Fusion archive with container metadata and no geometry. |
