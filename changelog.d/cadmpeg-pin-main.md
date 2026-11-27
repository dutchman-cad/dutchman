### cadmpeg

- Move the bundled cadmpeg to its upstream main branch, which adds the Alibre reader, deeper Onshape reading, more Inventor support and a wasm-safe threading shim.
- Keep the document root of a Fusion archive member inside its component, so placements stay where the file puts them.
- Read a derived Onshape studio whose reference records are refused as stored geometry with a combine, instead of as a native feature.
