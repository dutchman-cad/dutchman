# Fusion: what converts today, and what stands in the way

Dutchman reads Fusion archives (`.f3d`) through cadmpeg's Fusion codec,
cadmpeg's most complete native reader. This page says what Dutchman makes of what that codec
gives and which upstream gaps remain. Improvements land in cadmpeg and reach Dutchman when the
pin is bumped.

State at the cadmpeg commit pinned in `Cargo.toml` (`f1338aa2`, after cadmpeg 0.6.12).
More thread and hole forms, healed Delete Face operations, shell wall selections, loft
sections and rails, circular pipes, and pattern placements decode for history mapping.
More extrude profiles and face ends, fillet edge selections, body moves, and component
placements resolve. Mapping still depends on the resolved operands each feature needs.
Sketch names follow their timeline sketch features.

Revolve profiles and axes now follow their stored curve and region selections, including
resolved unions of subdivided regions. Unresolved stored selections remain native and
reported; a known sketch carrier can still map when Dutchman's source body check confirms
the inferred region.
An unreadable sketch point no longer discards otherwise readable design history. More
component occurrence frames and inserted-component placements resolve, and construction-plane
mirrors read their selected work plane. Known simple and clearance hole forms no longer
report an assumed diameter; tapped holes still report how their stored bore is interpreted.

Bodies now follow the component that stored them, and component placements read from saved
local occurrence paths, so more designs open as placed parts with their own history. A local
occurrence whose placement no stored path proves stays unplaced and is reported.
The reader also retains Combine operations, partial Sweeps, path patterns, face-group edits,
spheres, Emboss, and mesh conversions in neutral history. Features that still need an operand
or form Dutchman does not map stay blocked, with the final shape preserved.
Fusion sheet-metal bends drawn on sketch lines, contour flanges, unfold and refold, mixed and
partial chamfers, mesh removals, combinations and plane cuts, surface untrim settings, and
timelines with empty slots also come through as neutral history. A Combine whose tools are
generated in another component binds them to the feature that made them. Bodies a definition
holds without being placed (stored frames) are placed with the occurrence that inserts them,
and an archive whose design metadata joins an asset identity to its version now reads.

The reader also retains more box, Pipe, unequal-wall Shell, and UnStitch definitions in
neutral history. These still need the operands and feature forms Dutchman supports;
UnStitch remains a blocker, with the final shape preserved. A box maps when its frame lies along the model axes (cadmpeg places a box by its minimum corner); one turned off them stays a blocker, since SolidCraft's box is axis-aligned.

Historical body centres use bounded samples along curved edges as well as vertices and
proven circular extrema. cadmpeg identifies incomplete edge or topology evidence separately;
only bodies certified as exact have exact bounding-box centres.

For SolidCraft 0.44 and newer, an extrude join, cut or intersection can select the
bodies it modifies. Dutchman tries the reader's modified-body outputs as targets
when earlier features have named every one, and keeps that scope only at the final
body-building feature when replay matches the final source geometry. Intermediate
body centres do not establish all participants, even when exact; intermediate
features keep the previous mapping. Unknown or conflicting evidence also retains
the previous mapping. Native participant expressions are not interpreted. A join
with several output bodies still uses the previous mapping because its surviving
body identities are not established by those outputs.

On SolidCraft 0.44 and newer, a consumed boolean tool with no final body row
keeps a name only when its identity is proved: the source retains exactly one
history body with `produced_by` matching its single-output producer, or the
complete replay matches the final source geometry. Replay candidates keep
their names only after that check passes; intermediate body centres cannot
confirm them. An output ID alone is not proof. Without proof, selecting the
tool stops editable history with "it selects a body that an earlier feature
has not named". Older targets keep the previous mapping.
A mirror of one body names its single new output for later combines.
A generated selection can resolve through its earlier producer when that
producer has one declared result body and one named output. Ambiguous generated
results, historical selections and native body references remain blockers.
Targets before 0.44 keep the previous output.

## Public Fusion shares

The desktop app accepts pasted `a360.co` and `*.autodesk360.com` links and dropped
`.url`/`.webloc` shortcuts (or URL text where the desktop delivers it);
`dutchman convert <url>` accepts them too. The share page must expose a public file
download URL or download link reachable without login. Dutchman downloads that native
file and runs the existing local conversion; conversion coverage is unchanged.

Public share downloads screen resolved addresses on every hop and connect only to the
screened public destinations. DNS, connection, TLS and body streaming share the two-minute
deadline. CLI cancellation and intentional exits remove the temporary source.

When a share needs login, has expired, disables downloads, or exposes no public download,
Dutchman asks you to download the design in Fusion and drop the file into the app.
Downloads stay on allowlisted Autodesk HTTPS hosts, with a 200 MB cap and two-minute
timeout. See [Public share links](guide.md#public-share-links) for limits and cleanup.

## Mesh bodies

When cadmpeg reports mesh bodies for a Fusion design with no decoded B-rep bodies or
placed components, Dutchman says "it has mesh bodies (N), which Dutchman
doesn't convert yet, and no solid bodies could be read" and asks you to convert the meshes to
solids in Fusion, then save a new copy. The count comes from typed mesh selections and from
decoded meshes, also those of a design without a mesh-import feature, such as a mesh-workspace
design; a mesh counts once. An undecoded mesh container or an unresolved source alone cannot
supply a count, so the existing read error remains.
Designs with both B-rep and mesh bodies still convert their B-rep bodies and report reader
losses for meshes it could not decode.

Cadmpeg projects decoded mesh containers as CADIR tessellations in model millimetres,
with placement applied, and links them from mesh-import features. When a container cannot
be decoded, a mesh-import feature may instead retain native body members with the loss
`f3d/feature.mesh-import-tessellations-unresolved`. Those references carry no triangles.
Dutchman does not reconstruct mesh geometry from native records.

When cadmpeg reports `f3d/design.mesh-only-untransferred` for a Fusion design with no decoded
B-rep bodies or placed components (its only bodies are meshes, and none of them could be
read), Dutchman says "it holds only mesh bodies, which Dutchman doesn't convert yet, and none
of them could be read" with the same advice, at `error.stage: "decode"`. That loss carries no
mesh count, so the message names none.

## Parts and history

A face extrusion on an imported base needs a matching source result as well as a
proved input face. Matching a boundary and its outward side cannot establish that
the historical base and the final body use the same frame. The writer checks a
single-body result at the final body-building feature, including surface distance;
a missing or differing result refuses the extrusion and retains the imported final
shape. Intermediate face extrusions on imported bases are not supported yet when
that result cannot be proved.

As-built joints build no geometry and do not stop the part's feature history. Typed
assembly-joint timeline rows also need no part feature, and neither do rows that copy a
component or move it into another component (Copy/Paste and Cut/Paste of components): the
placed occurrences carry them. Unresolved connector context and
assumed motion bindings are reported separately.

Bodies a Combine used up as a tool, or that a Remove step took out of the design, are left out when cadmpeg marks them hidden and no component owns them; an Info `bodies` note gives the count. A body an active Remove deletes is left out too, even when cadmpeg lists it as shown, as long as cadmpeg resolves which body the Remove names; an Info note on the Remove gives the count. Bodies you hid yourself stay available and are written marked hidden: SolidCraft 0.42 and newer open them hidden, while older releases show a part with both shown and hidden bodies whole. An Info note gives the count. A hidden body the editable history rebuilds has no STEP of its own to carry that mark, so it is listed in the design's hidden bodies instead, at every placement of its part, and opens hidden too.

Fusion's own STEP export leaves hidden bodies out, so a STEP you export from Fusion can have fewer bodies than the design Dutchman writes, and a design whose bodies are all hidden exports as an empty STEP. When the counts differ, check the extra bodies in SolidCraft's browser: if they're hidden there, they're the bodies you hid in Fusion, not extra geometry.

| What the codec gives | What Dutchman does |
|---|---|
| The design's B-rep, with body transforms applied. | Converts it: restated as STEP, written as SolidCraft's import. |
| A design saved while Fusion was offline, which records no cloud lineage. | Reads it like any other design. |
| A Base Feature that holds a stored solid (no recipe), typed as a base body. | Writes it as a SolidCraft Import at its place in the timeline; later features keep their history. When cadmpeg keeps the Base Feature's own body (a history body, as it stood when the Base Feature ended, kept when a later feature changes it), that body is imported and the later features rebuild on it; the replay right after it is compared with the bodies the source stores there. Otherwise cadmpeg gives a base body only as the design ends: later joins and cuts on it re-apply without changing the shape, so they are kept but counted as re-applied, not rebuilt; a Base Feature whose body any other later feature changes (a Move, a blend, a deleted face) is refused, naming that feature. A Base Feature proven to hold no bodies is not needed. |
| A Copy/Paste Bodies that pastes copies into another component (`insert_bodies`). | Writes the copies as a SolidCraft Import at the paste's place in the timeline, using their final bodies. Later joins and cuts re-apply without changing the shape and are counted as re-applied; other later changes are refused, naming the feature. |
| A Canvas: a reference image placed in the design. | Needs no feature: it builds no geometry, so it is counted as not needed and doesn't stop the history. |
| Stored geometry from any format (`stored_geometry`) with output bodies, solids and sheets. | The same Import, holding exactly the bodies it outputs. |
| A mesh body whose container cadmpeg cannot decode. | Leaves it out and reports it as an undecoded mesh body. |
| Parameters, planar sketches with typed and native constraints. | Mapped, including point-on-curve coincidences, point-to-line distances, resolved finite-endpoint distances, and parallel-line spacing. Dimensions keep their parameter expressions, so user parameters drive the editable geometry. Native constraints without neutral operands are reported as not carried. |
| Spatial (3D) sketches. | Mapped as SolidCraft 3D sketches at their solved positions (lines, arcs, circles, splines, points). On targets 0.44 and newer, neutral point joins, point distances, line lengths (including repeated lengths), principal-axis directions and line-to-line and line-to-arc endpoint tangencies carry across; dimensions keep their parameters. Meeting endpoints alone do not create relations. Sweep paths and rails follow their selected curves. Other constraints and links to model edges are reported as not carried. |
| Cosmetic thread annotations with diameter, pitch, extent, and a resolved cylindrical face. | Writes a cosmetic ISO metric thread, including a blind length or the whole face. The designation supplies the nominal size; secondary diameter and pitch may agree at their stated decimal precision. An absent designation is derived exactly from diameter and pitch. Contradictions name both sizes; UNC/UNF cosmetic Thread output, modeled threads, and unresolved faces remain reported losses. Unspecified construction keeps the annotation with a Degraded note. Historical face IDs without input surface geometry remain reported losses. |
| Body Mirror and Remove steps. | A Mirror of bodies maps when its bodies are named by earlier features, are a body the final model leaves out that cadmpeg names by the extrude that made it, or are the only body in the Mirror's own input; it joins its copy to the body when Fusion did. A Remove of such bodies maps as a SolidCraft Remove on SolidCraft 0.40 and newer. |
| Typed extrudes, revolves, fillets, chamfers and more, with selections often native. | Extrudes, revolves, lofts, sweeps, spheres, tori, coils and whole cylinders map, and whole cones on SolidCraft 0.24.1 and newer (see ARCHITECTURE.md for the cases); boxes map when their frame lies along the model axes; simple, counterbore and countersink holes drilled at a sketch's points map, on a face of the extrusion or box made just before them; constant-radius fillets and equal-setback chamfers map from proven feature input edge geometry, including on full circles such as a boss's top edge or a hole's rim, and unequal chamfers on straight edges and full circles with the face their distance runs along proven on the source's final body, matched on SolidCraft's own replay of the history before them; selections on several bodies become one independent blend per body, with one mapped source outcome (a fillet with a tangency weight of exactly 1 maps as a circular fillet, with an Info note; any other weight is refused); other blends and holes placed on unresolved faces wait on input geometry (see guide.md). |

Fusion stores consumed Combine tools (with keep-tools off) and bodies removed by the
timeline alongside its live bodies. For `.f3d`, designs inside `.f3z`, and component parts,
Dutchman leaves out bodies cadmpeg marks hidden only when no CADIR product definition lists
them in its `bodies`. Their exclusive geometry is left out too, with one Info note giving
the count and reason. History remains available; a reference that can no longer map is
reported as a history blocker.

User-hidden bodies that still belong to a component are kept, with an Info note. Hidden
occurrences remain hidden; the current writer shows a part with both shown and hidden bodies
whole, so its hidden members can appear visible in SolidCraft.

Through-all holes omit the drill-point angle because no bottom forms, for every entry form and
every target down to SolidCraft 0.18.2. This preserves the geometry without a loss note. Blind
and to-face counterbores, countersinks, and chamfered entries retain their point on SolidCraft
0.22.1 and newer; older targets retain the existing refusal and original geometry.

Holes ending at a proven earlier planar extrusion or box cap retain their drill point,
including countersunk entries. Fusion measures Extents To to the cylindrical shoulder;
the conical point extends beyond the selected face. Dutchman resolves that distance
along the drilling direction and keeps the countersink at the entry. Unresolved,
offset, tilted, or unproven terminating faces retain a specific history blocker.
Later features rebuild when the hole maps. A refused hole still stops body-building
history; independently resolved reference sketches remain available.

Closed-edge blends need an exact historical curve carrier in addition to their
sample positions and adjacent face identities. When that carrier is missing, the
conversion reports `its closed input edge has no resolved historical curve carrier, which is not supported yet`.
The samples alone cannot distinguish a circle from an ellipse or another closed
curve. The original final geometry and the editable prefix are retained.

Holes on planar historical entry faces can keep their editable history when the boundary
includes circular arcs or whole circles, including a rounded corner or an earlier through
hole. The source's own input-edge ends, midpoints, and tangents establish the circular
centres, radii, and common plane. SolidCraft must then rebuild exactly that bounded face
before the hole; matching the plane alone is insufficient. Placements must lie inside the
filled face, clear of its curved boundary and existing holes, and drill inward. An unsigned
axis takes its drilling side from the proven face. Non-planar faces other than the existing
whole-cylinder radial-hole case remain refused with a specific reason. Without replay,
conversion keeps the original geometry and reports why the hole history cannot map.
Previously mapped straight-cap holes keep their existing output.
This extension requires a target of SolidCraft 0.40.0 or newer; older targets retain the
original geometry with a named reason.

When every active feature using a sketch selects explicit whole-loop regions, unused lines
crossing selected whole circles are kept as construction geometry. This preserves
layout lines through hole centres without splitting the circular holes during replay. If
another active consumer needs the whole sketch or a different selection form, those lines
stay solid.

When a complete-loop extrusion needs a region that solid curves it does not use subdivide
(those lines, or a notch drawn from the loop's edge that another feature's region keeps
solid), it uses an editable profile copy of the sketch. The copy retains its points, curves,
constraints, and parameter expressions, with every unused curve that touches, enters or
crosses a selected loop as construction geometry. Trimmed selections
continue to use the shared sketch. Each selected boundary must still match exactly one
SolidCraft region; unavailable or ambiguous boundaries remain history blockers.

Resolved `to_plane` extrude ends map before any body-building feature when the terminating
plane is parallel to the sketch. The plane origin becomes a fixed SolidCraft To Object point;
a construction plane or body face need not exist there. After earlier geometry, `to_plane`
ends are not supported yet: SolidCraft can reinterpret a point on an existing face as a
different end, and CADIR supplies no input face geometry to prove which face exists then.
Final-body faces cannot establish that timeline state. Tilted plane ends and native,
generated, or historical vertex selections are not supported yet. Symmetric To Object ends
and non-blind ends against the sketch normal on two-sided extrudes remain unsupported.

Planar historical face selections also search intervals between trim-loop crossings when
corner-triangle picks all land in holes. Each new point must lie in the material region,
clear of every loop and the error from sampling circular boundaries. This retains offsets
on perforated frames without selecting a hole. SolidCraft 0.43 supports normal offsets,
but has no face-translation or face-rotation feature; affine MoveFace motions stay refused.

An extrude whose profile is a face of the model as it was earlier in the timeline
(`historical_faces`, "extrude this face") is rebuilt from that face's boundary. CADIR carries
no surface for such a face, but it keeps the ends, midpoint and tangent of every edge around a
selected face in the feature's own input topology. Dutchman reads those edges as straight lines,
circular arcs and full circles in one plane, chains them into closed loops (a loop inside a loop
is a hole) and writes a sketch of them on that plane, ahead of the extrude, taking every
profile. The extrude runs along the face's outward normal, which only the body knows: after
SolidCraft has replayed the history before the extrude, Dutchman finds the face among the
rebuilt faces (same plane, same area, every boundary edge on it and named for it; SolidCraft reports
a curved edge from its mesh, so an arc counts as found when the pieces it reports, a single chord
for a small rounded corner, span the arc's whole turn about its centre) and mirrors the
sketch if it was written on the inner side; fillets and chamfers after a mirrored sketch are then
proven on a fresh replay that has the extrude on its outward side. A face it cannot find exactly once, an edge that is
not a line or circular arc, a boundary that does not close or is not flat, an input topology
without edge geometry for the face, a direction of its own, ends to or from an object, and
an extrude that re-applies to a base body cadmpeg gives only as the design ends (where the
face's outward side is not the one it had) leave the extrude unmapped with the reason. Faces that share an edge are refused. Without a
SolidCraft CLI to replay the history, the extrude waits for one like a blend does.

### Split Body history

Fusion Split Body keeps editable history when its tool is a resolved datum plane or
input faces whose boundaries prove they lie on one plane. The faces' plane extends through
the target body. Curved or noncoplanar tools, incomplete input selections, and final-body
face selections receive specific refusal reasons; conversion keeps the original geometry.
A body kept only earlier in the timeline is imported from the source's history body before
the Split. When a Base Feature already imported that same body, the Split reuses it.
The new Split history body imports and input-face tools need SolidCraft 0.40 or newer.
Existing Base Feature imports retain their version support.
Older targets keep the original geometry with a named version reason; existing datum-plane splits remain available. Targeting an installed 0.25–0.39 release also shows the usual older-version notice.

The mapper uses only cadmpeg's resolved plane, input face boundaries and nested
`history_bodies` (`produced_by` / `consumed_by`). It does not infer a splitting plane
from final result bodies when the tool is native. Component slices carry history bodies
with their producing or consuming feature, without mixing their topology into final geometry.
The current Fusion reader pin emits produced snapshots but does not populate `consumed_by`.
A native target without a consumed body association remains a reported loss; consumed-body
preservation and mapping are covered by synthetic tests against the current neutral model API.


## Appearances

Body and face appearance bindings follow the component definition's own BREP ids into its
written geometry, including imported and stored solids. Every placement inherits that
geometry's looks. Complete xref and instance identities are kept distinct through the shared
component index. Saved wire edges beside a solid do not consume its imported body name or
discard its appearance. When STEP omits an unwritable sibling body, Dutchman checks each
body separately, for groups of up to 32 bodies, before keeping the remaining body names
and colours. These solids keep their existing STEP import names, including numbered
siblings; matching their colours does not rename them or change other components' names.
A body binding supplies
the body colour, and face bindings override it per face.
When a body has no body binding and all its faces have the same resolved appearance,
Dutchman writes one body colour, which SolidCraft shows in its body appearance controls.
Mixed appearances remain face looks, selected by interior points in the component's
coordinates. Resolved analytic and NURBS faces use bounded trim-curve sampling in surface
coordinates, with spatial edge evaluation when parameter-space curves are unavailable.
Planes, cylinders and cones also have analytic picks when a trim chart is unavailable.
A projected trim must stay within
0.002 mm of its support, and its pick must clear the sampled boundary by eight times that
deviation. Cylindrical and conical trim charts account for periodic seams and holes;
narrow torus bands also check their trim orientation and retry across the minor-angle seam.
Sampling preserves exact knot endpoints. Spherical corners bounded by three minor
great-circle arcs use an oriented interior pick across the pole. Disconnected charts and
carriers that cannot be evaluated retain a Dropped note.
Resolved trim charts take precedence over vertex-only picks, so a cylindrical face keeps
its own long arc and a planar appearance cannot pick through a hole in its outer boundary.
Imported body names are made unique across the design so an unpainted namesake in another
component cannot receive a painted body's appearance.
When a partial or unverified editable history is hidden beside the final shape, the
visible final-shape Import receives its own names and colours. Names remain distinct
from those of the editable copy.
When a partial history is kept hidden, appearance losses refer to the visible final
shape rather than to final bodies absent from that partial copy.
The look keeps the resolved colour, opacity, and appearance name; it does not infer a
physical material from a visual preset.

A binding whose target or appearance cannot be resolved is reported as Dropped under
`appearance`. An unplaced binding identifies its body or face target and distinguishes a
target absent from the decoded model from existing geometry that is not placed. Missing
decoded targets need a cadmpeg decoding fix; Dutchman does not guess another instance by
matching an id suffix. Bindings to consumed or removed Fusion geometry are reported
separately when that geometry is filtered out. Faces without a resolved interior point
cannot receive SolidCraft's point-selected face looks yet; their bindings are also reported
as Dropped. A decoded body without geometry that STEP can write retains a separate note;
its writable siblings can still receive their colours.
Uniformly painted curved bodies can still use a body colour. Conflicting assignments to
one target are left out and reported. Textures and shader properties are not mapped by this
look mapping.

The pinned cadmpeg CADIR supports body and face appearance targets, but has no component or
occurrence appearance target kind. cadmpeg must project those assignments onto body/face
targets, or represent them through an IR extension, before Dutchman can map them. Source
assignments absent from CADIR cannot be reconstructed; remaining grey bodies may therefore
still need additional decoding in cadmpeg.

## Assembly memory

Reference sketches are mapped from the sketch, feature and parameter data alone, built once per
conversion: on the Voron V1.8 that takes the peak from 11.0 GB to 8.7 GB, with the same output.

## Sketches without a timeline row

Fusion keeps some sketches with no Sketch step in the timeline; cadmpeg then lists the sketch
alone. A feature whose profile uses such a sketch gets it written just before itself, so the
feature finds it; any other such sketch is kept for reference after the timeline, as before.

## Trimmed and unresolved profile regions

One stored sketch loop does not prove a native profile selection when other solid
curves remain, including curves needed by a later trimmed cut. Dutchman requires
source-confirmed inference in that case; otherwise it preserves the final shape
and reference sketches instead of rebuilding extra regions. A later fillet may
therefore be reported behind the earlier unresolved profile.


Trimmed regions select a point strictly inside their stored boundary, for revolves and other
profile consumers. Parameter intervals and traversal directions distinguish regions that share
whole curves, including the two halves of a circle split by a diameter. Boundaries must close
unambiguously; a missing interior point blocks the feature with a named reason. Angular carriers
whose stored chart differs from the solved sketch still require consistent directed closure.

Narrow regions use a finer search only when the existing point search cannot select them.
It samples curves more closely, scans between boundary crossings in both directions, and
scales the preferred clearance to the room inside the region, capped at 0.001 mm. The point
must still pass the even-odd boundary test inside that region and outside its holes and
smaller selected regions, with clearance greater than the sampled curves' sagitta and
measured endpoint gaps, plus displacement from merging defining sketch points. Regions
narrower than that combined bound are refused. Successful existing point selections keep
their exact coordinates.
Legacy angular readings that agree only within the existing 0.001 mm tolerance retain
that uncertainty in the clearance proof; narrow regions cannot settle an ambiguous chart.

When a native profile selection cannot resolve to a region, Dutchman tries its selected neutral
curve identities, when available, and then its carrier sketch. Each candidate runs the normal
feature mapping and is kept only when the existing source body comparison confirms its replay.
At the last body-building feature, the complete rebuilt shape must match the original. Earlier
features need usable stored source bodies at that same state. Unknown or failed checks block the
feature. Every accepted inference adds `history/profile-region-inferred`; the original geometry
remains available as before.

When neither candidate is confirmed, measured inference takes over. The regions of the carrier
sketch come from its loops, each loop with the loops directly inside it as holes (a ring and the
disc inside it are two regions); a selection of one native pick tries each region, a selection of
two tries each pair (under even-odd nesting a union is the loops in an odd number of them). When
other curves divide those loops (a line across a circle, a loop cadmpeg does not list), the
regions SolidCraft finds in the written sketch (`sketch.inspect`) have no proven selectors yet.
Their area centroids may lie outside their own regions; an annulus and its inner disc can share
one. Dutchman refuses this fallback rather than omit a candidate and claim a unique match.
Source-loop candidates retain their explicit boundaries, and deduplication uses those loop
identities. At most 24 candidates and one 120 s budget per feature, covering region discovery,
including `sketch.inspect`, mapping, source, replay and surface evaluations; late results are
refused.
Each candidate is mapped normally and replayed in
SolidCraft against the original final shape: body count, box, volume and area within the check's
tolerances, both face counts, and the largest distance between the two surfaces from
`inspect.compare` with the source loaded beside the replay, within the box allowance of 0.01 mm
or 2e-4 of the body's diagonal. New measured regions are tried only at the last body-building
feature; stored body centres alone cannot prove an earlier region's shape. Exactly one match is kept, with
`history/profile-region-measured`. Of several, the one alone with the source solid's face count is
kept; otherwise it is a tie and the feature stays blocked, as it does when none matches or any
candidate cannot be measured. New final-shape inferences require a one-body part and a successful
surface comparison with both face counts. Every source and replayed body must also have
finite volume, area, complete bounds and derived placement; unavailable comparisons or missing
measurements keep the refusal. The earlier two-reading comparison stays permissive. A final shape that later differs from the source cuts the timeline
before the first measured feature, so it never keeps fewer features than without measuring.

The same applies to the last fillet or chamfer of a history when its input edges cannot be proven
on the rebuilt body (a midpoint off every edge, or an edge with other ends or length). For each
source edge the candidates are the up to three nearest rebuilt edges of its kind (straight or
curved) within 2 mm of its midpoint; every combination using distinct edges, up to 24, is written
and the complete replay compared with the original final shape. Blends before the last stay
refused: the bodies the source stores after them barely move with a fillet, so they cannot tell
candidate edges apart.

## Coordinate orientation

Fusion 360 models in a right-handed coordinate system that defaults to Y-up in older versions
(Autodesk made Z-up default for new installations around 2019). SolidCraft uses Z-up.

By default, Dutchman preserves the source coordinates unchanged (`--no-stand-up`).
For models created with Y-up orientation, `--stand-up` rotates the model +90° about +X
(mapping source +Y to SolidCraft +Z and +Z to -Y). The rotation applies to imported geometry,
assemblies, and rebuilt history.

## Assembly placements

**Part positions** defaults to **Smart**. For each occurrence, Smart prefers a displayed
pose bound to it by the decoder's `native.f3d.display_poses` arena. Only rows with
`binding.state: "bound"` select a pose; ambiguous and unbound rows are skipped. These
root-design world matrices are already in millimetres and bypass the current-pose guards.
Missing holder worlds are derived bottom-up as child displayed world × inverse of child
local placement, only when all available children agree within 0.001 mm and 1e-6 rotation
entries and reproduce each of them. A holder whose children conflict keeps its first
placement, even inside a holder that moves, and is reported. Admitted float32 scene rotations
are normalized before inversion; translations are unchanged.
Smart also checks every displayed holder (bound or derived): when a part below it that follows
its pose, or a part with a displayed pose that is known to lie outside, would leave the
first-placement assembly bounds grown by 10%, the holder and everything inside it keep their
first placement. A local placement that matches the stored one within 0.001 mm and 1e-6
rotation entries is written unchanged, so components whose copies keep the same arrangement
stay shared.
An absent arena supplies no displayed poses; unbound scene matrices do not choose a
placement. Otherwise it accepts a native
current pose only when touching joints close (or there are none), the placed geometry fits
inside the assembly's first-placement bounds grown by 10%, and copies of the same inserted
design keep their relative placement. Other occurrences retain their first placement.

Bounds checks use conservative geometry envelopes. Unsupported or missing bounds keep the
first placement, as do unsupported touching joints and ambiguous pose carriers. Rejections
are deterministic. Joints then work from the selected placements as before.

The report names the parts whose position nothing confirms. A part whose placement could not be
read sits at its design's origin, and the sentence always shown on the result card says so,
naming the first few, with all of them in a warning below. A part with only a
stored pose and no joint, or with a later pose that was not used, is named in a note, because
those positions are usually right: when one looks misplaced, it's the place to look. The full list is
`part_positions.assumed` in the [JSON report](json.md).

**As Fusion last showed them** prefers the displayed-pose source, then uses current poses
where available without Smart's checks, and first placements otherwise. Until displayed
poses are bound by the decoder, this option relies on current poses, which can be stale.
**As first placed** ignores later poses throughout. Native current-pose matrices use centimetres and are
converted to millimetres. A pose in an inserted design is composed with that document's
insertion. Definition-local operation poses are not used as world poses. First placements retain
the decoder's existing path and component-operation placements; native current-pose records
remain a separate selectable source.

Select with CLI `--part-positions smart|last-shown|first-placed`, desktop **Advanced →
Part positions**, or MCP `part_positions`. The desktop remembers the choice. To restore
the old placement choice, use `--part-positions first-placed`; `--stored-placements` remains
an alias. Reports include an Info summary of the rule counts, `choices.part_positions`,
and `part_positions` counts and fallback reasons. Counts describe source occurrences,
including assembly holders, rather than additional history representations in the output.

## Components

Assemblies with multiple top-level designs group each design's features into a uniquely
named timeline folder, including the root design's own features when present, while keeping
feature order and leaving joints outside folders; a single part or one top-level design
gets no folders.
Features of one design separated by another design's features use numbered folders to
preserve timeline order.

A Fusion design can define components and place them, several times over (a copy and paste
of a component places it twice). cadmpeg gives each placement as a local occurrence with its
transform, and each component as a product definition.

- **When a component lists its bodies,** Dutchman writes the design as SolidCraft components:
  one per component, holding only that component's bodies as an import, one occurrence per
  placement, and the design's own loose bodies as one more component at the origin. This works
  for any format whose CADIR says which bodies a component owns.
- **Each component keeps its own history:** all features and parameters cadmpeg identifies as
  belonging to its definition, including features with no output-body links, plus the features
  that make its bodies and the sketches it owns with their building features. Referenced
  features come along, but timeline predecessors alone do not. A feature explicitly owned by
  another component stays out; without ownership, the existing body and sketch links still
  select history. A sketch another component builds that a feature of this one uses (designed
  in the context of the assembly) is the exception: when each of the two components has a single
  instance, its sketch feature comes in as this component's, with the sketch's plane moved into
  this component's coordinates. A component placed more than once also takes the sketch when
  exactly one of its instances puts the selected profile on the part. If none or several do,
  the history stops at the feature that uses it.
  Root-owned history goes to the design's loose-body component when it exists.
  Each sketch is written under the part that owns it, once per part however often it is
  placed, and a sketch owned by a component that holds no bodies goes under that component;
  only sketches with no owner, or whose owner component is not written, stay in the assembly
  root (counted in the report).
  A sketch the part's history builds nothing on, or that the source hides, starts hidden.
  The component's history is then mapped and checked like a single part's; when it does not
  map, the component keeps its bodies as an import and the report names its first blocker.
- **When it does not,** the design converts as stored and the report says so (Degraded,
  `components`): the geometry is written once, so a placement whose geometry the file does
  not store separately is not shown.
- **Instances that hold different layouts** are written as one definition per layout. When
  two instances of a component hold different components, or the same ones at different local
  placements (compared deepest first, within 1e-7 of each cell, so a difference anywhere below
  makes every component above it differ), the component becomes `<component>` for the first
  layout met and `<component> (2)`, `<component> (3)` for the others, in order of first
  appearance; every instance sits at its resolved pose, and instances with the same layout
  share one definition. The report notes how many components were written as how many
  definitions. A component's own bodies are the same in every layout, but each definition
  carries its own copy of them (the geometry and its STEP data are written once per
  definition). Mirrored placements combine with this: a mirrored instance of a layout uses
  that layout's `(mirrored)` copy.
- **Mirrored placements** use a shared component named `<component> (mirrored)` with
  reflected geometry and outward-facing solids. Ordinary copies retain the original
  component and its history. Nested reflections compose, including pairs that cancel.
  Mirrored definitions keep imported geometry only; the report notes any history left out.
  Joints touching a mirrored component are left out with a note and the placed pose retained.
  Tiny numeric drift in placements is corrected. Non-finite, scaled, or sheared placements
  beyond tolerance are left out with a Dropped note naming the occurrence index and kind.
- **Components inside components** become sub-assemblies: a component that holds others
  becomes a SolidCraft component with them beneath it, placed by occurrences inside it, and its
  own bodies as one more component at its origin. cadmpeg places the components a design's root
  component contains inside one occurrence of that root component, so such a design converts
  as the root component with its components beneath it. A component placed several times that
  holds different components in different instances is written once per layout (above); when
  a placement is inside an occurrence the design does not place, the design converts as stored
  and the report says why.
- **Hidden components and bodies** stay hidden: a placement whose occurrence the design hides,
  or whose part holds only bodies the design hides (a design's own body hidden beside its
  components, say), is written as a SolidCraft occurrence hidden in every placement of what
  holds it. Its geometry is still converted and can be shown again in SolidCraft. In a part with
  shown and hidden bodies, the hidden ones are written marked hidden: SolidCraft 0.42 and newer
  open them hidden, older releases show the part whole.
- **Joints** between components become SolidCraft joints: fixed ones rigid (their offset kept),
  revolute ones turning about the connector's z axis with their limits, slider ones along it.
  Each connector frame is in its own part. Offsets apply to the second connector:
  `C0 = C1 * T(offset) * Rz(angle) * F`, where `C0` and `C1` are the stored connector
  world frames. `F` is a half turn about x for `opposed` frame sense (also the default),
  or identity for `aligned`. Fixed offsets translate in the second connector's axes before
  turning about its z axis; a revolute's angular offset turns that same connector.
  Dutchman folds these offsets into SolidCraft's second snap, so placement and the written
  joint use the same relation. Slider translation offsets also apply to the second snap;
  their distance is expressed along the first snap's z, with its sign reversed for aligned
  frames. With no offsets, opposed frames meet face to face: the same origin and opposite
  z axes; aligned frames meet with their axes pointing the same way. Fusion can store a joined
  part where it was before its joints were solved, so Dutchman places joined parts where their
  joints put them: from the design's own root, each joint places the part at its other end; a
  joint neither of whose parts is placed yet normally moves its first part to its second,
  as Fusion moves the first part picked for a joint. When starting a group of joints that
  reaches nothing placed, a non-identity stored world pose takes precedence over an identity
  pose on the other side; otherwise the outermost occurrence stays put. Joints that already
  close at the stored poses hold their parts in their common component before any open joint
  moves a part. That component can still travel with an outer joint.
  A joint to a part inside an identity-placed component carries the component when child
  poses describe its relative layout, or all its children belong to the joined group, and
  the joints reaching into it agree on its place. An unrelated internal joint is insufficient. If that outer component is only an organiser,
  Dutchman tries the deepest smaller holder whose children are connected, then the named part.
  A body's identity placement at a holder's origin can also identify the holder as the moving
  assembly, including a holder with a non-identity placement, provided every outside joint
  reaches it through that same body. A joint that names the holder itself prevents this
  promotion. Children independently reached by other joints stay with their own placements.
  Internal joints solve first, so the outer joint uses the inner part's
  solved connector position. A file pose, grounding joint, or earlier joint holds that component in
  place instead; a conflicting outer joint is suppressed. Of a joint between a component and a
  part inside it, as a rigid group holds its members to its first, the inner part moves and the
  component keeps its own placement; a component holding a part already placed by an outer
  joint is never moved to meet that part, as that would move the part too. A component a joint
  moves leaves the parts inside it that outer joints placed already where they are, and carries the
  parts the file poses inside it, whose poses are relative to it. Internal
  joints alone do not fix their holder in the assembly. A component the file stores at the pose it shows
  it in starts placed: its joints are checked there instead of moving it back to where it was
  inserted. Within an already posed component, an isolated leaf pose marker yields to a
  partner held by joints that already close; the design root alone does not override that
  child pose. Explicit holder poses and child poses already
  confirmed by a solved joint stay protected. This also preserves a child component named by
  a contained pose inside an inserted design when that design has no current pose of its own. An inserted design's own current
  pose holds the design; its children still use their own poses or joints. A contained pose is
  matched within that design instance and its stored owner chain, so an
  unrelated component or mirrored sibling does not inherit the pose. A part two joints would
  place differently is reported, and the joint that cannot hold it is written suppressed:
  SolidCraft solves the joints when it opens a design, and the part stays where the file places
  it. Earlier internal joints are checked again after outer joints place their holders,
  so an active joint cannot move those parts again on open. A joint to the design's root component holds its occurrence to the design. Fusion joint motion converts from Dutchman 0.1.5 on (before it, every Fusion joint was
  fixed). A revolute limit on one side only is left out, and the report says so. A component
  placed more than once is held once in the SolidCraft design, so a joint between two parts
  inside one copy of it is that component's own: it is written once, through the first copy, as
  SolidCraft solves joints between repeated parts in the first placement's frame, and every copy
  follows; so is a joint between a copy and a part inside that copy. A joint from a part inside
  one copy to something outside that copy cannot hold the part alone, so it holds the whole copy
  where the part is, and the report says so; when another joint already holds that copy itself
  to the same occurrence, as a rigid group holding both does, the part's joint is left out
  instead. A part inside a design inserted into a component, which Fusion names by its key
  alone, is found inside that component. A component empty in its source becomes an empty
  SolidCraft component, so the joints to it are kept. A joint is left out, and reported as
  Dropped (`joints`), when it is of another kind, joins a component to itself or to one in
  another design, or grounds a part inside a component placed more than once (grounding it would
  ground every copy).

### Upstream gaps

1. **Component bodies.** cadmpeg says which bodies a component owns when the file binds the
   component to its entities (its naming space): the bodies whose body-map entries lie in the
   component's entity range. A component placed by an operation the file does not bind (as in
   some copy-and-paste designs) has no bodies, so that design converts as stored.
2. **Joint motion.** cadmpeg reads revolute and slider joints, and revolute limits; other
   motion types, slider limits and one-sided limits stay fixed with its
   `assembly.joint-motion-undecoded` loss, which the report carries.
3. **Joints between referenced designs.** In an `.f3z`, a joint names each part by its design's
   Fusion document id and occurrence id, which cadmpeg does not yet tie to the occurrences it
   merges, and the merged occurrences do not say which bodies they hold; such a design converts
   as stored.

`.f3z` multi-document archives (a design with the designs it references) convert like `.f3d`:
cadmpeg merges the referenced designs into the root design with their placements, as
components.

While reading an `.f3z`, Dutchman shows the current stage and elapsed time with a five-second
still-working heartbeat. cadmpeg owns member discovery and composition; decode attempts
can repeat, so the progress display does not infer unique design counts or a determinate
reading bar for `.f3z`.
A single `.f3d` reports one design when its read succeeds.

Optional [diagnostic logs](troubleshooting.md#diagnostic-logs-for-unfinished-runs) record salted `.f3z`
member ids, sizes, actual decode starts/finishes/failures, and re-decode counts. Per-member
records appear when the decoder reports them; they do not establish a unique design total
for the progress bar. The pinned decoder's observer is installed on the decoding thread.

### Spatial sketch profiles on SolidCraft 0.44

An explicitly selected closed planar loop in a 3D sketch can supply an extrude.
Lines, circles, arcs and NURBS with an exact planar representation are supported.
Dutchman keeps the 3D sketch and writes a separate planar profile sketch in CADIR's
resolved profile frame. Its source body check must confirm the extrusion before it
is retained. Skew loops, curves with no exact planar representation and native selections without
neutral boundaries remain blockers. The profile copy retains its initial geometry;
line lengths and in-plane axis directions also carry into that copy. Other relations
do not drive subsequent edits to the profile copy. The copy has a total budget of
400,000 constraints, counting every repeated line occurrence across all rows.
Exceeding that budget refuses the profile with a constraint copy limit reason;
reduce repeated relations and convert again. The original shape is retained.

The pinned decoder does not expose neutral fixed, parallel, perpendicular, collinear
or equal spatial-line constraints, or spatial radius/angle dimensions. Those native
relations remain reported losses. One sketch carries at most 400,000 wire constraints;
additional relations are reported and their solved geometry is retained. An unresolved sketch binding cannot be replaced
by guessed geometry. A nonplanar historical face boundary remains a separate face
selection blocker. Targets before 0.44 keep their existing output and refusals.

## Stored body fallback

When a new-body extrude, revolve, sweep or loft requires a profile or path absent
from the source, Dutchman can keep its stored result as an Import named after the
feature and continue the later history. Ordinary mapping and measured inference
must fail first. Unresolved selections within existing sketches, unsupported options
and failed measurement checks keep their usual refusal. A missing input is proved
by its CADIR reference or a missing-profile loss attributed to that feature; an
unresolved carrier alone does not establish that it belongs to an external design.
It uses the history body produced by that feature first. A final-model body is used
only when no later active feature changes it. Joins, cuts and intersections never use
this fallback: replacing them would discard earlier body history. No geometry is
reconstructed from an unavailable sketch. The feature is `mapped`, written as an Import, and
`history/body-fallback-imported` reports the lost recipe as Degraded. Later features
and verification use the imported body as usual.

Fusion thread construction records carry a designation, nominal-size text, profile/type,
physical pitch, and, in extended records, class and hand controls. The pinned codec projects
the designation, numeric nominal size, profile as `standard`, and pitch into CADIR; it does
not expose nominal-size text precision or class on the neutral cosmetic-thread operation.
Dutchman uses only that neutral operation. The table designation supplies the nominal size;
secondary numeric values are compared at their serialized decimal precision. Unsupported
class or hand suffixes in a designation are still reported. See [cosmetic threads](history.md)
for rounding, inch units, and target limits.

Unreadable local and xref positions, including local stored-frame fallbacks and
positions assumed to be the identity, count
as unplaced in `completeness.unplaced`, together with children whose world position
depends on them. They produce an incomplete result with partial verification.
The release gate rejects these positions even when the written design matches the
imported source. Exceptions must name each part and document the reason and tracking
issue in the gate matrix's `unplaced_allowances` map; unmatched legacy loss
explanations also need an explicit allowance key. There are no default exceptions.
