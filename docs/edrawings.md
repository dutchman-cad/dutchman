# eDrawings: what converts today, and what stands in the way
Dutchman reads eDrawings parts (`.eprt`) and assemblies (`.easm`) through cadmpeg's
`edrawings` codec (cadmpeg-registry's `edrawings` feature). Improvements to eDrawings decoding
land in cadmpeg and reach Dutchman when the pin is bumped. This page says what Dutchman makes
of what that codec gives, and what the files cannot give at all.

Two kinds of file convert: files that SolidWorks wrote directly, and files that the eDrawings
viewer saved again. Both give the same result. The codec reads the model
stream versions 18.00, 19.00 and 19.10.

State as of the cadmpeg commit Dutchman pins (see `Cargo.toml`).

## What an eDrawings file holds
An eDrawings file is a package made for viewing and sharing a design, not for editing it. It
carries a display mesh (triangles), not the exact surfaces the design was drawn with. The codec
rebuilds a B-rep from that mesh, so what reaches SolidCraft is a faceted copy of the shape. No
exact geometry exists in the file for Dutchman or cadmpeg to recover.

## Parts (`.eprt`)
| What the codec gives | What Dutchman does |
| --- | --- |
| One body rebuilt from the display mesh: coplanar triangles merged into planar faces, straight edges. | Converts it: restated as STEP, written as SolidCraft's import container, like Fusion. |
| A mesh that closes into a watertight shell. | A solid body. |
| A mesh that does not close. | A sheet body, with a loss note saying so. |
| Curved surfaces (cylinders, spheres, fillets). | Many small flat faces. The shape is close, but faces are not exact curved surfaces, so SolidCraft cannot select a cylinder as one face or edit its radius. |
| A mesh with small gaps or cracks, or with a point that sits on the edge of a neighbouring patch. | Closed before the body is built, so the body can still be a solid. |
| Colours, per body. | Carried as a plain body colour in the STEP. No appearance is written: no name, material or opacity. |
| Several colours on one body. | Reduced to the body colour. The report counts the faces affected. |
| Feature history, sketches, parameters. | None exist in the file. The part converts without history. |

## Assemblies (`.easm`)
An `.easm` is self-contained: every part is inside the one file. Dutchman does not look for
part files beside it, unlike Inventor and SolidWorks assemblies. The file is converted like a
single-file design, and the components it places are built from that one decode.

| What the codec gives | What Dutchman does |
| --- | --- |
| One part definition per body. | One SolidCraft component per definition, built like a part above. |
| A part that holds several bodies. | One component with all its bodies. |
| Parts that re-use the same set of bodies. | They share one definition. |
| Occurrences with transforms and names. | Placed as SolidCraft occurrences with those names and transforms. |
| Nested sub-assemblies, to any depth. | Kept as nested components. |
| Hidden parts and sub-assemblies. | Stay hidden. |
| Joints, mates. | None exist in the file. The assembly converts without joints. |

## Not supported yet, and upstream gaps
1. **No exact geometry in the files.** Everything is faceted. Converting to STEP and
   re-importing does not make curved faces exact again. If you need exact surfaces, export a
   STEP file from the program that made the design.
2. **Faceting.** Curved surfaces become many small faces, so face counts are much higher than
   in the original design, and finely tessellated models make large designs.
3. **Thread textures dropped.** Thread surfaces stored in the mesh convert as faceted
   geometry, but the texture images drawn on them are not geometry and are dropped.
4. **Display states.** Parts hidden in the file's default display state stay hidden after
   conversion, as do parts the file hides by default. The other display states are not
   carried over yet.
5. **Pieces dropped.** Very thin triangles that collapse when the mesh is closed are
   dropped. So are pieces with no volume, cavities too thin to tell inside from outside,
   and tiny loose slivers. Faces with no area are removed and their neighbours joined.
   The report counts them as a loss. Pieces that were inside out are turned the right way
   round, and the report says so. Closed cavities inside a body are kept.
6. **Colours only, no appearances.** Each body keeps one plain colour. No appearance entry
   (name, material, opacity) is written. A colour set on a part or in a display state, rather
   than on the body, is dropped; the body keeps its own colour. The report counts these.
7. **No history.** Features, sketches, dimensions and parameters are not in the file, so
   no feature history is written. See [Feature history](history.md) for what other formats
   can map.
8. **Other content.** Only the model is converted; anything else the file holds for viewing,
   such as hole markers and other 2D annotations, is not supported yet.
9. **Open surfaces stay sheets.** A body that is an open surface in the original design (a
   surface model, a flat pad with no thickness, a body with a missing cap) has no closed mesh,
   so it converts as a sheet body, and the report names it.
