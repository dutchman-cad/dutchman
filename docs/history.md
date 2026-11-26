# Feature history

A feature whose source records that it changed no geometry is left out of the rebuilt
history as a no-op and does not stop it.

## What maps

A part that holds an imported solid becomes a base-body Import feature at its place in the
timeline. Later features keep editable history when they map. Stored geometry has no recipe
to lose: the report says, at Info level, “This part holds an imported solid; there is no
feature history to bring over”. A part made only of stored geometry is geometry without
history, with no flattened-history warning.

- When every feature in a design's history maps onto SolidCraft, the SolidCraft design keeps
  that history, editable, with the original geometry kept beside it. If `solidcraft-cli` is
  installed, Dutchman replays the history in it and compares each body's position, box, area,
  and volume with the original before keeping it. Available merged face counts
  must also match on analytic bodies, so small missing holes cannot hide inside mesh tolerances.
  Bodies with free-form faces compare analytic surface-kind counts instead of total topology. Raw kernel counts may differ at periodic seams; SolidCraft
  merges adjacent pieces on the same surface before this check. Merged edge counts remain
  informational because mirror and full-turn revolve operations can split edges differently.
  An open-shell warning alone does not fail that comparison. What maps, most of it from Fusion:
  - sketches: lines, arcs, circles, conics, splines, and many constraints and dimensions;
    3D sketches of lines, arcs, circles, splines and points;
  - extrudes and revolves, with their end forms;
  - holes: counterbores, countersinks, chamfered entries, and thread labels on tapped holes;
    a hole placed at several sketch points, drilled at each when its placements are exactly
    those points and all drill the same way;
    drill points on plain drilled holes, and on counterbores and countersinks for SolidCraft
    0.22.1 and newer;
  - sweeps, lofts, and coils;
  - linear, circular, and path patterns of features or bodies, body mirrors, and body
    booleans;
  - sheet-metal base flanges, edge flanges and square attached plates with their bends;
  - delete face, for faces cadmpeg resolves in the feature's own input topology: planar faces and
    cylindrical faces, each written as a point inside the face, with the heal setting kept. A
    delete-face selection cadmpeg leaves native is reported as such and stops the history there;
  - body translations and axis-angle rotations on SolidCraft 0.45 or newer, including
    explicit pivots, when selected bodies have proven earlier names. Copies and
    unresolved body selections or motion retain the original final geometry;
  - move face, as SolidCraft's offset face: planar faces cadmpeg resolves in the feature's own
    input topology and moves along their normal, each written as a point inside the face with
    the signed distance. For SolidCraft 0.45 or newer, the selection also names the unique
    earlier body in the part’s history. Ambiguous body ownership is refused. Circular trims
    use interior candidates with clearance from both outer boundaries and holes, accounting
    for the exact curves beyond their sampled chords. Older targets keep the previous
    point-only representation. Translations along a direction, rotations, non-planar faces and native
    selections are reported with their reason and stop the history there;
  - constant-radius fillets and equal-setback chamfers, on edges proven on SolidCraft's own
    replay of the history before them (so they need `solidcraft-cli`); a Fusion fillet with a
    tangency weight of exactly 1 maps as a circular fillet.

  The sections below have the detail, and what stops a history.
- A blocker keeps the editable prefix and resolved sketches live in the part's timeline, then
  a Remove of the bodies they make and one Import of the rest of the design. Without a body-building prefix, the design keeps imported geometry and
  hidden reference sketches.
- Assemblies keep their components and placements. Fusion and FreeCAD assemblies keep their
  joints too (revolute, slider, and rigid), with their grounded parts held where they are.
  Fusion components keep their current poses, including child placements used to resolve an
  inserted design's pose. A joint that conflicts with the placements
  is kept suppressed, so no part moves when SolidCraft opens the design. Inventor assemblies
  place their parts, without joints. Sketches in the assembly's own file are kept, hidden, for
  reference.
- FreeCAD output is experimental: `--to freecad` writes a self-contained rebuild macro,
  `--to fcstd` runs FreeCAD headlessly to save a native `.FCStd` document and checks its
  solids against the original, and `--to freecad --verify` runs the same check on a macro.
- Every conversion lists what it could not carry across, as Info, Degraded, or Dropped.

## How a history is checked

A converted history is checked by replaying it in `solidcraft-cli` and comparing the bodies
and each body's position, box, area, and volume with the original. Position and box checks
allow the larger of 0.01 mm and 0.02% of the larger box diagonal, for the center distance and
each minimum/maximum coordinate. The size term is four times the measuring mesh chord
allowance, with margin above the measured 0.0022 mm difference on a 38 mm curved body.
Volume and area keep their measuring mesh tolerances ([every number](#verification-tolerances)). Bodies are paired by nearest box
center, with volume breaking ties. When the evaluator gives no box for a pair, the position and box checks are unavailable. The result is in the
`history:` line of the report and in `verification.status` in `--json`:

| Status | Meaning |
|---|---|
| `verified` | The history replays in SolidCraft and matches the original. |
| `unverified` | The history was written without the check: no `solidcraft-cli` was found, it timed out, or `--no-verify`. The reason is given. |
| `partial` | An editable prefix and resolved sketches stay live in the part's timeline, then a Remove feature takes away the bodies they make and one Import named "Rest of the design (from <first feature not converted>)" holds the imported final shape. The reason names the blocker, counts kept features, and says whether retained features replayed; this does not verify the full history. |
| `flattened` | No body-building prefix could be retained. The design keeps the imported geometry and mapped sketches with resolved planes, hidden for reference. |
| `not_applicable` | There was no history to write, or `--import-only`. |

When the source is one solid body with several disconnected regions, STEP imports
those regions as separate solids. If history retains them in one body, verification
requires every source region to import with positive volume, area and complete bounds,
then compares their summed volume and area and combined bounding box with the replayed
body. Available centroids are combined by volume; closure requires every region to
be closed. Merged face and edge counts and surface-kind counts are summed when
all regions report the corresponding measurement. Missing or overflowing counts
remain unavailable. The combined face evidence still rejects a small missing hole
even when volume and area fall within their allowances. Missing regions still fail. If replay also splits the regions, the usual
per-piece checks apply. Single-region bodies and designs with multiple source bodies
keep their existing one-to-one comparisons.

### Verification tolerances

Every number below is a constant in `crates/dutchman-solidcraft/src/tolerances.rs`, and the
conversion report states them in `verification_tolerances` (see [json.md](json.md)). Millimeters
are mm; a "share" is a fraction of the size it applies to.

**Replay against the original** (both evaluated in the same SolidCraft, so kernel differences
cancel):

| Compared | Allowed difference |
|---|---|
| Bodies | The same comparison count after the single-body disconnected-region normalization described above. Otherwise, bodies are paired by nearest box center, volume breaking ties. |
| Volume | The larger of 1e-4 (0.01%) of the larger volume and the mesh allowance: each body's area times the mesh chord, `max(5e-5 of the box diagonal, 1e-4 mm)`, summed for both bodies. With no area or box from SolidCraft, only 1e-4. |
| Area | 1e-3 (0.1%) of the larger area, when SolidCraft gives both. |
| Position (box center) and box corners | The larger of 0.01 mm and 2e-4 (0.02%, four mesh chords) of the larger box diagonal; the center distance and each minimum and maximum coordinate. Skipped when SolidCraft gives no box. |
| Kernel face, edge and shell counts | Informational only: `info` when equal, `differs` when unequal. Equivalent split or merged faces may differ; counts never reject a final history. Earlier inference retains its geometry comparison policy. |
| Merged face counts | Final history comparisons require equal available top-level counts per paired body. SolidCraft merges adjacent pieces of one surface and periodic seams. A mismatch retains the imported final shape and reports the differing count. Free-form and mesh surface totals are not comparable and are not checked. Missing counts are not checked. |
| Merged edge counts | Informational only: `info` when equal, `differs` when unequal. Edge splitting after mirrors and full-turn revolves can differ across SolidCraft releases without changing the faces or shape. |
| Analytic surface-kind counts | Available merged counts of planes, cylinders, cones and spheres must match, including in bodies with free-form faces. Mesh bodies are not checked. |
| Center of mass | Final history comparisons use the position allowance above on actual centroids, when both are available. A box center is never substituted. |
| Closed shell | Final history comparisons require source and replay closure to agree; a source sheet may be open. |
| Named parameters | Replayed numeric values must match source values within 5e-12 times max(1, absolute source value), allowing measured reply serialization roundoff. Omitted or renamed parameters are not checked. |
| Manifoldness | Not checked: the evaluator exposes no explicit manifold result. |

Final comparisons record each check as `pass`, `fail` or `not_checked` in
`verification_checks`, including numbers or a reason. Kernel counts instead use informational
`info` or `differs` statuses when available. A missing check never strengthens
verification. Entries describe the attempted history even if it is rejected and original
geometry is retained. See [source comparison evidence](json.md#source-comparison-evidence).

**Bodies the source stores at a point in a partial history** (a retained prefix against the
source's own body at that step): the source's body count must not exceed the replay's, and each
stored body needs a replayed body whose box center is within the larger of 0.5 mm and 1% of the
replayed box diagonal when the stored box is the solid's own, or 1 mm and 2% when it is a box
over sampled points. More replayed bodies than the source lists is not a difference.

**Geometry written as an import:** a solid body with a volume below 1e-6 mm³ counts as having
none. For a single solid body with multiple regions, every imported region must have
volume. There is no independent source measurement to compare with; SolidCraft's warnings
also apply.

**Proofs against the source before a feature maps** (a failed proof leaves the feature out or
refuses the choice rather than failing the replay):

| Proof | Allowed difference |
|---|---|
| Fillet and chamfer input edge | Exactly one rebuilt edge passes through the edge midpoint within 1e-6 mm, with ends and length within 1e-6 (of the larger, at least 1 mm), and direction within 1e-6 of parallel. A refusal says the nearest rebuilt edge is farther than 0.01 mm when the rebuilt body, not the tolerance, differs. |
| Inferred fillet and chamfer edges | When the midpoint proof finds no edge, each nearby straight rebuilt edge is a candidate and is picked at its own midpoint; a pick that also lies on another straight rebuilt edge is dropped, so two close parallel edges are never picked between. |
| Profile face the source names | Positions within 1e-5 mm; directions within 1e-5 of parallel; the face's plane within 1e-4 mm of the sketch plane; area within 1e-4 (2e-2 when an edge is curved, as SolidCraft measures those from a mesh) of the face's, at least 1 mm². Its edges are covered within 1e-4 of a straight edge's length, within 1e-3 of the radius (at least 1 mm) from the circle, and within 2e-3 rad of a curved edge's turn. |
| Extrude end cap | The swept profile's area within the larger of 1 mm² and 3% of the cap's area; a face's plane within 1e-4 mm of the end plane. |

The advisory body comparison behind `dutchman diagnose` uses its own, looser numbers (1% for
volume and area, 0.1 mm to 0.5 mm for position) and does not decide whether a history verifies.

Assembly visibility reports count source-hidden placements separately from retained
editable history. An Info `visibility` note, `N editable-history component placement(s)
are placed hidden`, counts the latter across repeated and nested placements. Both use
the same bounded hidden-occurrence paths; a Degraded note reports paths left shown
past the 4096-path limit.

For a flattened design, one Info line reports `sketches: kept N sketches for reference`,
with `; M left out` when any sketches did not map or their planes did not resolve.
`--import-only` leaves out history while retaining reference sketches when the preset
keeps sketches. Use `--no-sketches` as well, or `--preset fast`, for geometry alone.

An assembly keeps the sketches of its own file the same way, hidden, in the design itself
(SolidCraft's root component), with the parameters their dimensions use; one Info line
reports `sketches: kept N sketches in the design itself, for reference`. cadmpeg does not say
which component a sketch belongs to, so a component's sketch stays where it was drawn and
does not move with the component.

Assemblies with multiple top-level designs put each design's features in a uniquely named
timeline folder, including the root's own features when present. Feature order stays the
same, and joints stay outside folders. Each folder added produces an Info note with subject
`timeline folders`: `"Name": N feature(s) grouped`. Parts and assemblies with one top-level
design get no folders.
Root-owned bodies and adjacent root reference sketches share a folder. If another design's
features separate them, they use numbered folders so timeline order stays unchanged.

When the design is written as geometry, the same `solidcraft-cli` opens it too. Faces it
cannot mesh, bodies inside out, solids without volume, and timeline errors still produce a
`Degraded: geometry: …` loss, such as
`Degraded: geometry: solid 'unit cube': 3 face(s) could not be meshed and are not shown`.
Look over those bodies in SolidCraft before relying on them. SolidCraft can merge faces that
share a surface, so a face-count difference alone produces no loss. The history comparison
uses position, box, area, and volume, so an open-shell warning there produces no loss when
those measurements agree.
Geometry-only output has no source measurements to compare against, so a solid it opens as
an open shell is still reported. Sheets remain exempt from volume and closed-shell checks.
`--no-verify` skips these checks too.

## Feature by feature

Sketch entity distances carry line lengths, point pairs, point-to-line distances, and
spacing between parallel lines (a parallel relation plus a point-to-line dimension).
Point-on-curve coincidences carry across alongside point coincidences. Resolved construction
and projected entities use the same mapping as drawn entities; unresolved external entities
cannot supply operands. Dimensions link to named parameters, whose expressions (for example
`Length` or `Head_Diameter / 2`) remain live. The stored numeric value initializes the solved
sketch; the parameter supplies the driving value on recompute. Driven dimensions remain
reference dimensions. Nonparallel line spacing and curve-only coincidences without identified
point loci remain reported losses. A point beyond a finite line's end uses an endpoint
point-pair dimension when the source value matches that distance instead of the perpendicular
distance to the line. A value matching neither measurement is reported, without reshaping
the sketch to force it. Disjoint parallel segments can likewise retain a uniquely nearest
endpoint pair when their source value identifies it. Infinite reference axes never gain
artificial endpoint dimensions.

A feature keeps the parameter that sets it: a blind extrude distance and a partial revolve
angle take the parameter the source feature owns for that value (as `abs()` of it when the
source signs it) when exactly one matches. A sketch lying parallel to an origin plane at the
height of a point of an earlier sketch, which that sketch's dimensions place, is written as
that origin plane offset by those dimensions, so a cut on the end of a screw follows its
length; the sketch's axes are turned back when the source drew them turned by a half or
quarter turn, and a sketch with other curves than points, lines, circles and arcs keeps a fixed
plane. When a selection holds loops that lie inside one another, each inner loop is a hole of the loop around it (an annulus is its outer and inner circle). A sketch
with a parameter and at most four loose points (nothing in the source fixes them) keeps each
where it was drawn, tied to the origin when it sits there and otherwise at its drawn distance
from it, so one edit cannot move it across the profile.

Cosmetic thread features use the designation's nominal ISO metric size, with a blind length
or the whole cylindrical face. Secondary diameter and pitch numbers may be rounded to their
stated decimal precision; floating-point unit-conversion noise does not create a conflict.
Integer numbers must agree apart from arithmetic roundoff. A designation without a pitch
uses the ISO coarse series, so a conflicting explicit pitch is reported. A missing standard
or designation does not block a known metric size: `3 mm` and `0.5 mm` become `M3x0.5`
without rounding. A genuine contradiction names the designation's dimensions and the
secondary numbers and asks you to check the source thread table.

UNC/UNF designations are checked in millimeters (25.4 mm per inch), with pitch expressed as
millimeters or threads per inch. When those numbers agree, a UNC, UNF or UNEF thread is written as a Thread feature in
SolidCraft's Unified form (`#2-56 UNC`, `1/4-20 UNC`), keeping a class the source names
(`-2A` external, `-2B` internal), on targets with Unified thread support. No SolidCraft release
has it yet, so every target reports "Unified threads need a SolidCraft release with Unified
thread support" rather than mislabeling the thread as ISO metric. Tapped-hole support is separate.

The face must resolve to a complete cylindrical band between two circular rims. Historical
face IDs without input surface geometry remain unresolved; Dutchman does not infer faces
from opaque ID text. Modeled threads, unsupported designation controls, other standards,
and unresolved extents or face geometry remain reported losses. When the source does not
resolve whether the thread is modeled, the annotation is kept with a Degraded note; helical
geometry is not reconstructed. Cosmetic annotations do not change the body's geometry.

Fusion may store a cosmetic thread's face at the thread class's diameter (Ø9.85 for an M10 thread) while the sketch that made the cylinder keeps the nominal size (Ø10). A SolidCraft cosmetic thread cannot make that change, so the history writes it as an offset-face feature that sets the cylinder to the stored size, followed by the cosmetic thread; the sketch and extrude keep their history and the rebuilt body is checked against the stored one. Only a face within 3 % of the nominal radius, along the axis of a sketch circle of the nominal size, counts.

Modeled threads retain their original helical geometry and stop the editable history.
The Degraded loss `history/modeled-thread-retained` keeps the source designation when
available and names the missing construction data. SolidCraft 0.45 can cut a modeled
thread, but its documented design profiles use flat crests and roots and omit tolerance
allowances. CADIR's cosmetic-thread operation supplies face selections, standard,
designation, diameter, pitch, a modeled flag, and blind length or full extent; it supplies
no hand, tolerance class, internal/external control, or exact groove profile. A designation
alone cannot prove that SolidCraft will rebuild the same shape. Equal volumes also cannot distinguish right- and left-hand grooves. A helix and sweep would
need that same missing profile and placement. No editable modeled thread is claimed.

Replacing the thread with a cosmetic feature on the earlier body would lose the groove.
Attaching a cosmetic feature to the imported final body also needs a proven cylindrical
band between circular rims; a helical face does not meet that condition. Dutchman keeps
the designation in the loss report rather than guessing a face for an annotation. Later
features remain blocked, and they stay in the imported final shape.

FreeCAD signed horizontal and vertical sketch distances convert to positive dimension
magnitudes at their solved positions. Their signed parameters and expressions remain
available; a dependent magnitude parameter drives each negative distance in SolidCraft.

Constant-radius fillets and equal-setback chamfers keep editable history when their own
feature input topology supplies edge midpoints, tangents, and endpoints, and each midpoint
can be proven to pick exactly one edge of the pre-feature body, whatever history builds it.
SolidCraft itself rebuilds that body: Dutchman replays the mapped history in `solidcraft-cli`
in a batch (two base runs and an optional circular-carrier pass within the check's time limit), up to the last feature a blend
starts after, and reads the edges each blend starts from. If the picks span several
bodies, each body gets its own blend with the same parameters, in the order its edges
first appear. Names share the source name with numbered suffixes, as for radius groups;
the report still records one mapped source feature. Later feature patterns and mirrors
select every emitted part of the blend. Later blends replay the expanded
history before their edges are proven. Without `solidcraft-cli`, or when the replay fails or runs out
of time, blends are refused, saying why. SolidCraft reports each edge's ends, midpoint and
length but no tangent. For an open line or circular arc, its midpoint must
lie on exactly one rebuilt edge across every body, and that edge must have the same ends (in
either order) and length; a straight one must lie along the input tangent. SolidCraft measures
a curved edge along the polyline it draws, so a rebuilt arc's length must lie between its chord
and the input arc's length. Arcs SolidCraft
splits into pieces and other non-circular curves are refused. Closed input circles map for
targets of SolidCraft 0.44.1 or newer. The selected source edge must reference a
stored circle with exact center, axis, radius and full-turn parameter range; historical
samples cannot establish circularity. Stored topology must identify one earlier named owning
body, and only that body supplies replay candidates. Missing carrier or ownership evidence,
ellipses, and transformed source carriers are refused. Each rebuilt piece must lie on its
plane and radius, with mesh length between the sample chords and the circular arc. An extra
`inspect.measure` pass confirms the circular carrier's center, axis, radius, and analytic
length; it cannot certify a circle from its two seam samples alone. Sorted
angular intervals must cover exactly one turn without gaps or overlaps on exactly one body.
An overlapping candidate without carrier metadata on the selected body also blocks the proof.
Picks use the proven pieces' midpoints, independently of the source seam. Other closed
curves retain a named refusal; older targets keep the original geometry. A refusal says which part of the
proof failed: no rebuilt edge through the midpoint (with how far the nearest rebuilt edge is, which
tells a rebuilt body that differs from the source from a rounding gap), more than one, or one with
other ends, another length, another kind of curve or another direction. Equal two-distance chamfers also map; distance-and-angle chamfers map at
45 degrees on perpendicular faces: at a full circle, a plane square to its axis and a coaxial
cylinder of its radius, as SolidCraft's replay reports them. Unequal distances and other angles
map on straight edges when the source's final body still has the chamfer's face: the face order
that puts it there, between that face's own boundary edges along the input edge, is taken. On a
full circle between such a plane and cylinder, the order is taken that puts both of the
chamfer's edges on circles of the source's final body. A selection on several
bodies becomes one chamfer per body, and each is flipped on its own when SolidCraft's first
face (the one facing up most) would put the cut elsewhere on that body. A chamfer face the final body lost, an edge whose faces SolidCraft
cannot tell apart (or, at a full circle, are not such a plane and cylinder), and SolidCraft
cutting it elsewhere either way round are refused, saying which. Variable fillets lack a proven
chain direction. Their radius positions need a binding to an ordered, directed input chain;
carrier sample order alone does not establish that binding. A fillet with several radius
groups becomes one SolidCraft blend per group,
in the source's order; a later group's straight edge may be shorter than the source edge where
an earlier group rounded its end. Final-body edge selections never stand in for input geometry.

For targets 0.44 and newer, a sweep path can name connected topological edges from its
own input state. Resolved edges are preserved only from one hidden earlier dependency
result before that geometry is removed. A wire owned by an earlier mapped sketch resolves
to that sketch only when every edge matches one entire emitted curve by its endpoints,
midpoint, tangent and length. Other paths use the blend edge proof, check connectivity in
source order, and bind the whole path to one rebuilt body. Closed chains assembled from
open edges remain refused until their start and traversal direction can be proven.
Exact curve measurements are requested only for consumers that need them; a measurement
limit or failure preserves earlier straight-edge blends.
Later edge consumers replay after this binding. Topological guide rails remain unsupported;
SolidCraft's rail representation requires sketch curves. Paths without proven earlier
input geometry and older targets keep specific refusals.

A history using these edge paths stays hidden beside the
original unless the full comparison succeeds. This also applies when checking is off
or the original cannot be measured soundly. The original position and shape remain
visible, and the report explains the unverified check. Other histories keep their
existing verification behavior.

Unused FreeCAD binders and datum helpers are `not_needed`, with the Info note
`helper not converted: nothing uses it`. Used datum points and coordinate systems can be
carried by resolved sketch placements, pattern axes, or another resolved datum. A used helper without the required
resolved placement or binder source geometry remains a blocker, with its reason in the report.
These Info notes remain when another feature or a replay mismatch flattens the history.
A body inside a hidden container is hidden even when its own flag says it is shown.

Fusion Split Body keeps editable history when its tool is a resolved datum plane or
input faces whose boundaries prove they lie on one plane. The faces' plane extends through
the target body. Curved or noncoplanar tools, incomplete input selections, and final-body
face selections receive specific refusal reasons; conversion keeps the original geometry.
A body kept only earlier in the timeline is imported from the source's history body before
the Split. When a Base Feature already imported that same body, the Split reuses it.
The new Split history body imports and input-face tools need SolidCraft 0.40 or newer.
Existing Base Feature imports retain their version support.
Older targets keep the original geometry with a named version reason; existing datum-plane splits remain available. Targeting an installed 0.25–0.39 release also shows the usual older-version notice.

FreeCAD fillets and chamfers can supply input edges from their base feature's stored shape.
Those edges must belong to one hidden result of an earlier dependency, and still pass the
same replay proof. A resolved selection without that proven earlier result is refused.

Hole history includes chamfered entries and tapped holes: the hole keeps its bore and an ISO
metric thread label such as `M6x1`. For SolidCraft 0.40 and newer a left-hand thread keeps
its hand, and a blind hole without an entry recess keeps its thread length; SolidCraft 0.43
and newer also keep an ISO tolerance class such as `6H`. What doesn't carry across (a thread
length in a through or recessed hole, or any of these on an older target) is listed in the
report as Degraded. Taper, exit treatments, and modeled threads are not supported yet,
and the report names the option.
Through-all holes omit the drill-point angle because no bottom forms, for every entry form and
every target down to SolidCraft 0.18.2. This preserves the geometry without a loss note. Blind
and to-face counterbores, countersinks, and chamfered entries retain their point on SolidCraft
0.22.1 and newer; older targets retain the existing refusal and original geometry. The
bounded-hole refusal remains "SolidCraft holes take a drill point only on a plain drilled
hole".

Holes ending at a proven earlier planar extrusion or box cap retain their drill point
and supported entry recess. Fusion Extents To measures to the cylindrical shoulder,
with the point extending beyond the face. Dutchman resolves the distance along the
drilling direction after proving the target cap. A tip-measured CADIR bottom subtracts
the conical height. Unresolved or unproven targets remain specific history blockers.

A hole placed on a face of the model as it stood before the hole maps when that face is a plane
the source's own extrusion just before the hole proves (its cap), or a whole cylinder drilled
along a radius toward its axis (a radial hole in a round part): the hole starts where the source
places it on the cylinder. A hole on part of a cylinder, or on any other surface, is not mapped
yet, and the report says which.


3D sketches (Fusion spatial sketches) become SolidCraft sketches with 3D Sketch on, written at
their solved positions: lines, exact arcs, full circles (as two half arcs), NURBS splines and
points. A spline is written as a dense curve through the source curve with fit points along
it, so it keeps its shape but a moved fit point redraws it as a SolidCraft spline. Constraints,
links to model edges and vertices, construction styling, surfaces and source-native geometry
are not carried; each is reported with the sketch, and a curve that cannot be written leaves
out only that curve. A sweep path or guide rail picked from the sketch's curves resolves to
its wires; one given by native selection records is reported and keeps the original geometry.

Sweeps accept ordered sketch curves (planar, or curves picked in a 3D sketch), or a whole
sketch containing a single non-construction curve, for their path and optional guide rail. Taper, explicit right or round corners,
changing sections, tangent-path extension, and post-sweep simplification keep the original
geometry and report the unsupported option. Twist, end scale, and the supported orientations
remain editable; a guide rail requires the default orientation without twist or scale.

Revolves keep one-sided partial solid sweeps about the oriented source axis when the selected
profile curves can be bounded on one side of it: lines, circles, ellipses, and arcs whose
whole carrier circle lies on that side. Reversing the source axis reverses the sweep.
They also keep symmetric angles, equal angles on two sides, and full turns, including unequal
sides that add up to a full turn. An axis that is a line of the profile's sketch stays that
line; an edge or datum axis becomes a fixed line in the sketch plane. Surface revolves keep
their symmetric history short of a full turn, from closed profiles or from picked sketch curves
that form open chains (a single line, say); a pick mixing open chains and closed loops stops
the history. Unproven one-sided sweep sides, one-sided
partial surface sweeps, unequal partial two-sided angles, ends at a face, vertex, or body,
full-turn surface revolves, and faces built other than
the way FreeCAD's default (bullseye) face maker builds them stop the history, and the report
names which.

Whole-sketch hole placements exclude construction entities; explicit entity selections
can still use construction points or circle centers.

When every feature using a sketch selects regions (whole loops or trimmed) or a list of its
profile loops, curves that bound none of the selected regions or loops are written as
construction, so they do not divide them; the report counts them. That covers open segments and
lines laid across a region, and also closed loops no feature takes: a tab along part of a
selected region's edge, or a ring around a hole the region keeps. A feature that takes the whole
sketch takes every region its curves bound, which the profile table may not list in full, so it
keeps every curve. A trimmed region counts only when its boundary closes into one shape;
otherwise nothing is inferred. A piece of a line in a trimmed boundary is read as cadmpeg gives
it: the fraction of the way from the line's start to its end. A curve a feature names, and any
sketch another kind of feature uses, keep their curves as real geometry.

Selected loops that share part of an edge with another solid line (a tab taken together with
the plate it sits on) are named by a point inside each region, since SolidCraft splits the
shared edge and the loop's own curves no longer bound one of its profiles. So is a region two of
whose loops cross (a nut pocket drawn over a bolt hole, both holes of the region): its hole is
their union, and the point lies outside both.

A feature that takes a whole sketch, or a list of its profile loops, treats loops inside loops
as holes (even-odd nesting), as Fusion does: two concentric circles are a ring, not a disc. The
selection is written as whole boundaries (outer loop and its holes). Loops that touch or cross
keep the previous selection. When a selection names a loop together with loops inside it, as
well as that whole loop on its own, the whole loop is not a region of the sketch (the loops
inside divide it), so it is written as the ring around them plus the disc of each: the same
material, in regions SolidCraft knows.

A cut or intersection that comes before any feature builds a body has nothing to cut, so it is
not mapped: the report says so ("it cuts or intersects a body, and no earlier feature in the
history builds one") instead of SolidCraft refusing the history when it replays it.

A selection that takes several separate regions of a sketch can arrive as one loop that crosses
itself (four openings around two crossing ribs given as one loop that follows both diagonals).
That is not mapped yet, and the report says its profile is several selected regions merged into
one self-crossing loop, to tell it from a profile whose loop is crossed by other curves of the
sketch ("a profile crossed by its own sketch's curves is not mapped yet"). The self-crossing
test covers lines, arcs and circles; a loop with other curve types gets the second message.

A selection cadmpeg keeps native (it names no profile Dutchman can read) maps only when the
region is certain. In a sketch with one profile loop that is the whole sketch, unless other solid
curves of the sketch divide that loop into several regions: the source picked one of them, and
cutting or adding the whole sketch would take them all. That case stops the history there ("a
profile cadmpeg does not resolve, in a sketch whose curves divide its one profile into several
regions, is not mapped yet").

Patterns can repeat earlier features or named bodies in one or two linear directions,
around an axis, or at fixed spacing along curves of an earlier planar sketch. Explicit
linear offsets and circular angles map when their intervals are equal; two linear composite
stages map to a rectangular grid. Composite feature patterns containing resolved mirrors
map to an ordered chain: each stage repeats the preceding stage's entire instance set.
Chained stages require a target of SolidCraft 0.25 patch 1 or newer; older targets report
`needs SolidCraft 0.25 patch 1 or newer` and keep the original geometry. Single-stage mirrors
and patterns still map on every supported target.
Full-turn circular angles that repeat the seed endpoint keep only the distinct locations.
Composite mirror chains allow up to 32 stages; patterns are limited to 10,000 instances,
including the product of the counts in a chain. For mirror chains and repeated full-turn
endpoints, an attempted replay that cannot finish keeps the original geometry and reports
why; turning verification off, or having no usable CLI, still writes unverified history. Nonuniform or
skipped instances, progressive scaling, face seeds, and unresolved body or path references
stop editable body-building history with a specific reason. This CADIR version has no solid-pattern symmetry
or extent fields, so Dutchman cannot recover those settings independently of the decoded
spacing and locations.

Sketches retain bounded parabolas and hyperbolas, elliptical arcs up to one turn,
and rational quadratic Béziers as editable conics. Equal-weight rational splines become
polynomial control splines. Uniform periodic (closed) B-splines keep their exact shape
through endpoint knot insertion, becoming clamped control splines with a shared seam point.
Weighted periodic splines with unequal weights are not supported yet because SolidCraft
control splines have no weights. Clamped splines with nonuniform spacing or repeated interior knots become chains of exact
Bézier spline pieces, up to 200 pieces, sharing their endpoints. Uniform splines stay one
spline. Profiles and sweep paths select the entire chain; constraints that cannot apply to
a chain are left out with an Info message. A loft whose section or guide rail uses such a
chain is not mapped yet: more edges in a section would make a different lofted surface.
A loft section that takes several loops of one sketch (the whole sketch, or a list of its
profiles) is not mapped yet either: the source can loft the filled outer loop where the loops
read as a ring, so whether an inner loop is a hole is not known.
Selected multi-piece elliptical arcs, unbounded conics, other weighted splines,
and text without solved outlines still prevent history replay.
Additional coordinate, intersection, tangency, curvature, symmetry, and repeated-dimension
relations carry across. Static sketch patterns retain point and radius associations;
driving count or span parameters are not supported yet. Constraints that cannot carry
across are counted in a `Degraded` sketch loss with a specific reason; they never by
themselves flatten a sketch whose solved geometry maps.

FreeCAD numeric spreadsheet cells and supported formulas become parameters, with aliases
or names such as `Sheet_A1`. Qualified object properties bind to their resolved parameters;
renaming keeps those links. Text cells are omitted silently, and unused numeric cells get
an `Info` note. Unsupported functions, conditionals, ambiguous references, and cycles retain
available scalar values with a specific reason. See [FreeCAD expressions](freecad.md#spreadsheet-parameters-and-expressions)
for decoder limitations, including named sketch constraint bindings. Verification checks
the initial geometry; check geometry again after changing a parameter.

## Sheet metal

You don't need to mark sheet-metal parts: Dutchman reads from the Inventor file whether a part is sheet metal, and converts it the same way as any other part.

Sheet-metal history includes base flanges (not of symmetric thickness) and full-edge flanges
on an unchanged rectangular base sheet. An edge flange needs its own CADIR input topology
with edge midpoint, tangent, and end vertices; each midpoint must identify exactly one edge
of the rebuilt base sheet. Fixed outer-face heights, angles between zero and 180 degrees,
inside/outside bend placement, and positive bend radii map. Inner-face heights, partial widths,
object-ended heights, adjacent/tangent bend placement, and picks on already modified sheets
keep the geometry and report the reason. Final-body edges alone never select a flange.
An attached plate with its paired bend maps to one SolidCraft edge flange (the bend is reported
as mapped with its plate) when the plate is one rectangle that spans the whole proven input edge,
stands square to the sheet at 90 degrees, is flush with the edge's end face, has the sheet's
thickness and starts where the bend ends or, as Inventor stores a flange measured from where
the outer faces meet, at the sheet's outer face; the flange height is the plate's far end measured
from the sheet's outer face. The unchanged base can be a single simple polygon, including an
L-shaped or notched sheet: attachment uses the actual profile edge, including a concave edge,
rather than the edge of a bounding rectangle. Curved, open, crossing, disconnected, or
collinear base edges and base profiles with holes or partial regions are not supported yet
by this attachment proof. A plate attached along several edges is the same flange on every edge when each further
edge is as long as the first, parallel to it and on the same face of the sheet; the flange lists
all of them. Any other plate, or a bend without its plate, keeps the geometry
and reports the unsupported shape. Hems keep geometry: CADIR specifies developed length while SolidCraft requires length to the
outer face, and rolled, teardrop, and unresolved hem forms are not supported yet. A sheet-metal
corner that cadmpeg proves leaves its body unchanged (a single flange's corner, with nothing to
relieve) needs no SolidCraft feature and is counted as not needed. Contour flanges map when
cadmpeg supplies one open chain of sketch lines and arcs, one blind distance along the sketch
normal, and material on the left or right. Thickness,
inside bend radius and any supplied K-factor become a separate sheet rule. Centered,
symmetric, two-sided, drafted, object-ended, oblique and closed-profile contour flanges keep
the source geometry and report the reason. Unfold/refold history and corner relief that
changes geometry are not supported yet; sheet rules keep SolidCraft's defaults for relief
settings. The report names the first feature that stops the history; the original geometry
is still converted.

## Partial history

Roll the timeline marker back before **Remove** to see and edit the body the converted features build.

When a feature blocks SolidCraft history conversion, earlier editable features and
resolved sketches are kept live in the part's own timeline, followed by a **Remove** feature that takes away the bodies they make and one **Import** named "Rest of the design (from <first feature not converted>)" that holds the source's final shape. The visible result is exactly the original shape, with one body per source body. Roll the timeline marker back before **Remove** to see and edit the body the converted features build; to keep modeling from them, delete or suppress the last two features. SolidCraft needs the CLI to name the bodies the features make; without it the kept features stay in a hidden **Editable history (partial)** component beside the visible Import, as before, with their body-building features suppressed so an export holds each part once. The report names the blocker and counts
retained source features; `mode: partial_history` and verification `partial` distinguish
this from full verified history. The reason says whether retained features replayed or
remain unverified. Later sketches with unresolved planes or external geometry are
reported and left out. A sketch that fails replay is retried without constraints, with
the loss reported. If there is no body-building prefix, resolved sketches remain hidden
beside the imported shape for reference. A measured shape mismatch removes unconfirmed body-building features from the
written design. The imported final shape remains visible, and the report names
the failed check and its measured difference. Earlier source imports and independent
sketches remain available; a successfully replayed body cannot stay merely because
its features run without errors. SolidCraft's numerical eval includes hidden bodies
as well as the visible final shape.

When a partial history is checked against the source's stored bodies, each kept body must
sit where the source has it: within the larger of 1 mm and 2% of its box diagonal when the
source's position is approximate, or 0.5 mm and 1% when it is exact.

Dutchman targets SolidCraft 0.49 by default with the writer capabilities available in 0.48 and
keeps design format 23, as for earlier targets. SolidCraft 0.49 opens every earlier
design format. Saving in SolidCraft 0.49 upgrades a design to format 67 (drawing format 6);
SolidCraft 0.48 warns about a design saved in 0.49, opens what it can read and keeps the rest.
Dutchman does not write drawings or require format 67. Explicit older targets keep their existing output.

Converted assemblies embed their parts and STEP blobs in the design container; they do
not link back to the source part files. SolidCraft's requirement (since 0.45) to use Update Links
for changed files outside the design's folder therefore does not affect converted assemblies.

For targets of SolidCraft 0.30 or newer, partial and unverified editable-history copies
use **Hide its History**: their features stay in the Browser and keep rebuilding, but
are left out of the History bar. Use **Show its History** on the component to restore
them. Older targets keep these features in History. A kept history of base features
alone has nothing to edit, so in an assembly no hidden component is written for it.

A writer that can never run the check (the SolidCraft importer plugin, which runs Dutchman
in a sandbox with no `solidcraft-cli`) keeps a whole unchecked history the same way, in a
hidden **Editable history (unverified)** component beside the visible original, with
`mode: partial_history` and verification `unverified`. In an assembly each part with a mapped
history is written that way, inside the part's own component.

A trimmed profile region, including a revolve's, is selected by a point inside its boundary. When a
native profile selection can't be resolved, Dutchman tries the selected curves and then the whole
sketch, and keeps the feature only when SolidCraft's body check confirms it; the loss
`history/profile-region-inferred` says so. Otherwise the feature stays blocked with its reason.
When neither confirms, Dutchman measures every region of the sketch the selection can name (up to
24) in SolidCraft against the source, and keeps the feature only when exactly one region matches;
the loss `history/profile-region-measured` says so. A last fillet or chamfer whose edges can't be
proven is measured the same way, choosing among the rebuilt edges within 2 mm of each source edge.
Such features are marked `inferred` in the JSON report, never exact. A tie, or no match, keeps
the feature blocked.

New measured inference requires finite volume, area, complete bounds and derived placement
for every source and replayed body, plus both face counts and the surface result. The earlier
two-reading comparison is unchanged. Centroid-only region discovery is refused: centroids do
not prove selections for nonconvex regions or distinguish a concentric annulus and disc. Region
discovery, mapping and measurements share one 120 s feature deadline; late results are refused.

## Stored body fallback

When a new-body extrude, revolve, sweep or loft cannot map, Dutchman can keep its
stored result as an Import named after the feature and continue the later history.
It uses the history body produced by that feature first. A final-model body is used
only when no later active feature changes it. Joins, cuts and intersections never use
this fallback: replacing them would discard earlier body history. No geometry is
reconstructed from an unavailable sketch. The feature is `mapped`, written as an Import, and
`history/body-fallback-imported` reports the lost recipe as Degraded. Later features
and verification use the imported body as usual.

When no final source shape is available, conversion keeps the editable history before the
failed feature, leaves out its dependent suffix, and checks the remaining design again.
The report names the omitted features and the rebuild reason. Conversion stops only if
no usable history remains or the remaining design cannot be made to open.

When SolidCraft reports a feature that fails to rebuild, conversion removes that feature and
its dependent body history and keeps the final source shape with the editable prefix before the
failure. The report names the feature and SolidCraft's reason, even when the source shape could
not be measured. Before omission, refused STEP imports with excess shared edge uses get identical edge
copies for uses beyond the first two and are rechecked. Faces, curves, vertices, names, and
placements are retained; `geometry/sheet-edges-split` records the repair. A final opening
check also covers geometry-only output, hidden history, and
assemblies. If SolidCraft also refuses the available final source shape, omission is the last
resort: `geometry/import-failed` says the body geometry is **MISSING** and names the feature
and reason. A combined import may leave out the whole part. Review these losses before using
the result. If a named failure cannot be removed, conversion stops without writing it. An
evaluator error or nonzero exit without named failed features means verification is
unavailable: the design is still written, marked unverified, with the reason. Verification
turned off remains unchecked.