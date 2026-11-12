# Onshape bundles

Dutchman reads a local `.onshape-bundle` containing one Part Studio at one pinned
microversion. Conversion is offline. No account, API keys, network connection, or
Onshape installation is used by this reader.

```sh
dutchman convert bracket.onshape-bundle bracket.sc
```

The ZIP follows `dutchman.onshape-bundle/1`: `manifest.json`,
`partstudios/<eid>/features.json`, `bodydetails/index.json` and the required
`bodydetails/<i>.json` responses, plus `export.step`. Features and bodydetails
are raw REST responses. The reader accepts stored and deflated members, without
extracting them to disk. Assemblies and bundles with multiple elements are not
supported yet; fetch one Part Studio instead.

Amended v1 manifests may record each file's request path kind (`m`, `v`, or `w`)
and microversion, with a bundle-level `consistent` flag. A bundle marked
`consistent: false` is refused: fetch it again, or use an immutable version link.
Older v1 bundles without these fields still convert; response microversion checks
continue to apply. A present consistency flag must be a JSON boolean.

## Editable history

| Source construction | Reader mapping | Limits and named losses |
|---|---|---|
| Planar sketch | CADIR sketch and solved curves | Built-in Top, Front and Right planes; other attachments need the source's full in-plane frame. |
| Lines, circular arcs, circles, points | Exact sketch entities | Text, images, projected references without solved curves, and other curves are not supported yet. |
| Simple spline | Polynomial planar CADIR NURBS | Non-periodic, non-rational curves with complete knots and control points; the writer retains its own knot and degree limits. |
| Constraints | Horizontal, vertical, fixed, parallel, perpendicular, tangent, equal, concentric, coincident loci | Unsupported relations are dropped with `onshape/sketch.constraint-not-supported`; solved geometry remains. |
| Dimensions | Length, distance between loci, radius, diameter, angle and CADIR parameters | Literal units or an evaluated quantity are required. Driven dimensions are reported; expressions needing FeatureScript evaluation are not evaluated locally. |
| Solid extrude | Blind, through all, up to a parallel planar face; NEW/ADD/REMOVE/INTERSECT | Whole resolved sketch selection, positive or reversed normal. Multiple-body Boolean scopes, symmetric/second direction, draft, offset starts, thin/surface bodies, and other bounds are not supported yet. |
| Solid revolve | Full or one-direction angle about a resolved straight input edge | Other bounds and body types are not supported yet. One-direction geometry still passes the writer's side proof. |
| Fillet | Circular, constant radius, explicit input edges | Variable, partial, conic, curvature, overflow, and tangent-chain expansion are not supported yet. The writer must prove every edge on replay. |
| Chamfer | Equal offsets, explicit input edges | Unequal and angle forms and tangent-chain expansion are not supported yet. Replay proof still applies. |
| Simple hole | Sketch points; through or flat blind | Recesses, tapping, reversed drilling, blind drill points, mate connectors and other ends are not supported yet. |
| Other features, custom FeatureScript | Named `onshape/history.not-supported` loss | The shared partial-history path keeps the final STEP shape visible and retains the editable prefix where possible. |

Face and edge queries use `deterministicIds`, looked up only in bodydetails at
the feature's rollback index (before that feature). The reader rejects missing or
duplicate IDs, a mismatched microversion, and unsupported analytic geometry.
An explicit Boolean scope must select the sole input body at that rollback index.
Face loops supply their boundary edges; historical selections belong to exactly
one CADIR feature. Final STEP faces never stand in for historical input faces.
Queries containing only arbitrary FeatureScript query strings cannot be evaluated
offline. The built-in plane query forms are recognized explicitly. Deterministic
sketch-region subsets are not supported yet and never expand to the whole sketch.

Lengths from REST geometry are converted from metres to millimetres; angles use
radians. The source axes, origin and scale are preserved. Unsupported constructions
stop body-building history rather than guessing a selection.

## Checking the result

The STEP export supplies the exact final source bodies and the fallback shape.
It does not supply construction history or historical selections. The same
SolidCraft writer and [history checks](history.md#how-a-history-is-checked) used
for the other formats replay the CADIR history, pair bodies by position and box,
and compare boxes, area and volume against `export.step`. Existing tolerances and
SolidCraft version gates apply unchanged. Fillets and chamfers need a usable
SolidCraft CLI (or host evaluator) for their input-edge proof. An absent evaluator,
failed proof or differing replay is reported and preserves the final geometry.

These mappings have synthetic offline coverage. No real Onshape document has
been tested here; feature-library parameter drift and the API's actual rollback
and tangent propagation semantics still require validation on owner-created data.

The reader has defensive limits: 4,096 ZIP members, 16 MiB directory/ZIP64
metadata, 32 MiB per JSON member, 256 MiB total expanded data, and 10,000 rows per
input collection or topology arena. These are admission limits, not memory/time
estimates. Bodydetails indices may be a JSON array of rollback integers (or
`"i.json"` names), or an object with that array under `indices`.
