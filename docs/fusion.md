# Fusion: what converts today, and what stands in the way

Dutchman reads Fusion archives (`.f3d`) through cadmpeg's Fusion codec,
cadmpeg's most complete native reader. This page says what Dutchman makes of what that codec
gives and which upstream gaps remain. Improvements land in cadmpeg and reach Dutchman when the
pin is bumped.

## Parts and history

| What the codec gives | What Dutchman does |
|---|---|
| The design's B-rep, with body transforms applied. | Converts it: restated as STEP, written as SolidCraft's import. |
| A design saved while Fusion was offline, which records no cloud lineage. | Reads it like any other design. |
| A Base Feature that holds a stored solid (no recipe), typed as a base body. | Writes it as a SolidCraft Import at its place in the timeline; later features keep their history. A Base Feature proven to hold no bodies is not needed. |
| A mesh body whose container cadmpeg cannot decode. | Leaves it out and reports it as an undecoded mesh body. |
| Parameters, planar sketches with typed and native constraints. | Mapped; native constraints are reported as not carried. |
| Typed extrudes, revolves, fillets, chamfers and more, with selections often native. | Extrudes, revolves, lofts, sweeps, spheres, tori and coils map (see ARCHITECTURE.md for the cases); boxes and cylinder primitives wait on cadmpeg placing them; simple, counterbore and countersink holes drilled at a sketch's points map; constant-radius fillets and equal-setback chamfers map from proven feature input edge geometry, matched on SolidCraft's own replay of the history before them (a fillet with a tangency weight of exactly 1 maps as a circular fillet, with an Info note; any other weight is refused); other blends and holes placed on unresolved faces wait on input geometry (see guide.md). |

When every active feature using a sketch selects explicit whole-loop regions, unused lines
crossing selected whole circles are kept as construction geometry. This preserves
layout lines through hole centres without splitting the circular holes during replay. If
another active consumer needs the whole sketch or a different selection form, those lines
stay solid.

Resolved `to_plane` extrude ends map before any body-building feature when the terminating
plane is parallel to the sketch. The plane origin becomes a fixed SolidCraft To Object point;
a construction plane or body face need not exist there. After earlier geometry, `to_plane`
ends are not supported yet: SolidCraft can reinterpret a point on an existing face as a
different end, and CADIR supplies no input face geometry to prove which face exists then.
Final-body faces cannot establish that timeline state. Tilted plane ends and native,
generated, or historical vertex selections are not supported yet. Symmetric To Object ends
and non-blind ends against the sketch normal on two-sided extrudes remain unsupported.

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
- **Each component keeps its own history:** the features that make its bodies, the sketches
  it owns with the features that build them, and the features those depend on. A feature that
  makes only another component's bodies stays with that component. The component's history is
  then mapped and checked like a single part's; when it does not map, the component keeps its
  bodies as an import and the report names its first blocker. When cadmpeg links no feature to a
  component's bodies, it has no history to map.
- **When it does not,** the design converts as stored and the report says so (Degraded,
  `components`): the geometry is written once, so a placement whose geometry the file does
  not store separately is not shown.
- **Components inside components** become sub-assemblies: a component that holds others
  becomes a SolidCraft component with them beneath it, placed by occurrences inside it, and its
  own bodies as one more component at its origin. cadmpeg places the components a design's root
  component contains inside one occurrence of that root component, so such a design converts
  as the root component with its components beneath it. A component placed several times must
  hold the same components in the same places each time; when it does not, or when a placement
  is inside an occurrence the design does not place, the design converts as stored and the
  report says why.
- **Hidden components and bodies** stay hidden: a placement whose occurrence the design hides,
  or whose part holds only bodies the design hides (a design's own body hidden beside its
  components, say), is written as a SolidCraft occurrence hidden in every placement of what
  holds it. Its geometry is still converted and can be shown again in SolidCraft. A part with
  shown and hidden bodies is shown whole.
- **Joints** between components become SolidCraft joints: fixed ones rigid (their offset kept),
  revolute ones turning about the connector's z axis with their limits, slider ones along it.
  Each connector frame is in its own part, as cadmpeg gives it, which is how SolidCraft takes
  it. The two frames meet face to face, as the faces they were snapped to do: same origin,
  opposite z axes, the second half a turn about the first's x axis. Fusion can store a joined
  part where it was before its joints were solved, so Dutchman places joined parts where their
  joints put them: from the design's own root, each joint places the part at its other end; a
  joint neither of whose parts is placed yet moves its first part to its second, as Fusion moves
  the first part picked for a joint; joints that reach nothing placed start from the outermost
  occurrence they join, which stays where the file puts it. Of a joint between a component and a
  part inside it, as a rigid group holds its members to its first, the inner part moves and the
  component keeps its own placement; a component holding a part already placed is never moved,
  as that would move the part, and a component a joint moves leaves the parts inside it that
  other joints placed already where they are. A component the file stores at the pose it shows
  it in starts placed: its joints are checked there instead of moving it back to where it was
  inserted. This also preserves a child component named by a contained pose inside an inserted
  design when that design has no current pose of its own. An inserted design's own current
  pose holds the design; its children still use their own poses or joints. A contained pose is
  matched within that design instance and its stored owner chain, so an
  unrelated component or mirrored sibling does not inherit the pose. A part two joints would
  place differently is reported, and the joint that cannot hold it is written suppressed:
  SolidCraft solves the joints when it opens a design, and the part stays where the file places
  it. A joint to the design's root component holds its occurrence to the design. Fusion joint motion converts from Dutchman 0.1.5 on (before it, every Fusion joint was
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

Optional [diagnostic logs](guide.md#diagnostic-logs-for-unfinished-runs) record salted `.f3z`
member ids, sizes, actual decode starts/finishes/failures, and re-decode counts. Per-member
records appear when the decoder reports them; they do not establish a unique design total
for the progress bar. The pinned decoder's observer is installed on the decoding thread.
