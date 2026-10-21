# SolidWorks: what converts today, and what stands in the way

Dutchman reads SolidWorks parts through cadmpeg's published SolidWorks codec (cadmpeg-registry's
`sldprt` feature, 0.6, at the same pin as f3d and Inventor). Improvements to SolidWorks
decoding land in cadmpeg and reach Dutchman when the pin is bumped. This page says what
Dutchman makes of what that codec gives, and which upstream gaps keep SolidWorks history from
replaying. It drives what is asked of cadmpeg next.

State as of cadmpeg 0.6.2 (docs/format-support.md upstream rates SolidWorks parts L1).

## Parts (`.sldprt`)

| What the codec gives | What Dutchman does |
|---|---|
| The part B-rep, from the file's Parasolid body streams: analytic and NURBS carriers, offset and blend surfaces where they resolve, the active configuration's bodies. | Converts it: restated as STEP, written as SolidCraft's import container, like Fusion and Inventor. The body is named after the file. |
| No geometry (no Parasolid stream yields a body). | Stops: "no geometry could be read". |
| Bodies that do not form a solid or a surface (for example a lone triangle face). | Stops: "the model has no solids or surfaces SolidCraft can import". |
| Losses: an unverified SolidWorks version, configurations inferred from geometry, opaque carriers, derived body hierarchy. | Printed as Degraded, with cadmpeg's loss code. |
| Parameters, with their expressions (`Depth = 12.5mm`). | Mapped to SolidCraft parameters when the history maps. |
| Feature history from the Keywords XML: extrusions with their operation, extent and depth; sketches; suppression state; many features only as `native` records. | Mapped where every feature maps, else flattened with the first blocker named (below). |
| Sketch geometry from the ResolvedFeatures records: lines, arcs, circles, ellipses, splines, coincidences, with a resolved plane. | Read only through a sketch feature that binds it (below). |

## Upstream gaps, in the order they block history

Found by running Dutchman's history mapper on what the codec decodes from cadmpeg's published
SolidWorks fixtures (`dutchman inspect --json`).

1. **Sketch features do not bind their sketch.** A `Sketch` feature in the history arrives
   without the sketch it owns ("the sketch it binds is not resolved"), and the sketch geometry
   the codec does recover arrives as a separate feature-input sketch that no feature points
   at. Dutchman cannot place the sketch in the timeline, so the first sketch stops the history.
   Needed: the sketch feature's binding set to the CADIR sketch it owns.
2. **Extrusion profiles are unresolved.** An extrusion's profile arrives as `unresolved` (the
   extrusion's own native id), or, when the history places a sketch next to it, as `feature`
   (the sketch feature). Dutchman maps profiles given as a sketch, sketch profiles, sketch
   regions or sketch curves, so every SolidWorks extrusion stops the history ("unresolved
   profiles are not mapped yet"). Needed: the profile as `sketch` or `sketch_profiles` of the
   CADIR sketch. Once (1) lands, Dutchman can also read a `feature` profile as the whole sketch
   that feature binds.
3. **Many features are native.** Features the codec does not type (for example
   `EquationDrivenCurve`) arrive as `native` and stop the history where they come.
4. **Edge and face selections are native.** Fillet, chamfer and hole carry mostly native edge
   or face selections. SolidCraft finds edges by points on them and faces by persistent names,
   so these need typed historical or generated references first. The same gap holds for
   Fusion and Inventor.
5. **Shared upstream issues.** cadmpeg/cadmpeg#255 (STEP face loops written reversed, so
   SolidCraft can read solids inside out).

A design whose sketches carry no sketch feature at all (only feature-input sketches) has, to
Dutchman, no feature history: it converts as geometry, and its sketches are not carried.

## Assemblies (`.sldasm`)

Not supported yet: cadmpeg has no SolidWorks assembly codec. `dutchman convert` says so and
suggests exporting a STEP file.

## Testing without real SolidWorks files

Dutchman never uses customer files. The fixtures (provenance in
`crates/dutchman-core/tests/fixtures/README.md`):

- `body_history.sldprt` and `container_metadata.sldprt`, copied unchanged from cadmpeg's
  SolidWorks golden fixtures: a history with an extrusion (blocked by gap 2), and a file
  with no geometry.
- `solidworks_cube.sldprt`, generated with cadmpeg's own SolidWorks encoder from its 10 mm
  example cube, since cadmpeg's published SolidWorks fixtures hold one-triangle bodies that do
  not restate as solids. It converts end to end against a golden document and opens in
  SolidCraft.
