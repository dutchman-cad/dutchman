# Fusion: what converts today, and what stands in the way

Dutchman reads Fusion archives (`.f3d`) through cadmpeg's Fusion codec,
cadmpeg's most complete native reader. This page says what Dutchman makes of what that codec
gives and which upstream gaps remain. Improvements land in cadmpeg and reach Dutchman when the
pin is bumped.

## Parts and history

| What the codec gives | What Dutchman does |
|---|---|
| The design's B-rep, with body transforms applied. | Converts it: restated as STEP, written as SolidCraft's import. |
| Parameters, planar sketches with typed and native constraints. | Mapped; native constraints are reported as not carried. |
| Typed extrudes, revolves, fillets, chamfers and more, with selections often native. | Extrudes, revolves, lofts, sweeps, spheres, tori and coils map (see ARCHITECTURE.md for the cases); boxes and cylinder primitives wait on cadmpeg placing them; simple, counterbore and countersink holes drilled at a sketch's points map; constant-radius fillets and equal-setback chamfers map from proven feature input edge geometry, matched on SolidCraft's own replay of the history before them (a fillet with a tangency weight of exactly 1 maps as a circular fillet, with an Info note; any other weight is refused); other blends and holes placed on unresolved faces wait on input geometry (see guide.md). |

## Components

A Fusion design can define components and place them, several times over (a copy and paste
of a component places it twice). cadmpeg gives each placement as a local occurrence with its
transform, and each component as a product definition.

- **When a component lists its bodies,** Dutchman writes the design as SolidCraft components:
  one per component, holding only that component's bodies as an import, one occurrence per
  placement, and the design's own loose bodies as one more component at the origin. This works
  for any format whose CADIR says which bodies a component owns.
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
- **Joints** between components become SolidCraft joints: fixed ones rigid (their offset kept),
  revolute ones turning about the connector's z axis with their limits, slider ones along it.
  Each connector frame is in its own part, as cadmpeg gives it, which is how SolidCraft takes
  it. The two frames meet face to face, as the faces they were snapped to do: same origin,
  opposite z axes, the second half a turn about the first's x axis. Fusion can store a joined
  part where it was before its joints were solved, so Dutchman places joined parts where their
  joints put them: from the design's own root, each joint places the part at its other end; a
  joint neither of whose parts is placed yet moves its first part to its second, as Fusion
  moves the first part picked for a joint. A part two joints would place differently is reported,
  and SolidCraft reports the conflict when it solves. A joint to the design's root component
  holds its occurrence to the design. Fusion joint motion converts from Dutchman 0.1.5 on
  (before it, every Fusion joint was fixed). A revolute limit on one side only is left out,
  and the report says so. A joint is left out, and reported as Dropped (`joints`), when it is
  of another kind, joins a component to itself or to one in another design, or joins a component
  inside one placed more than once (the SolidCraft design holds that component once, so it would
  not say which copy).

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
components. No public `.f3z` with references exists to test against yet; a single design
saved as `.f3z` is tested.
