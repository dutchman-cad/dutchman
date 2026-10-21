# Fusion: what converts today, and what stands in the way

Dutchman reads Fusion archives (`.f3d`) through cadmpeg's published Fusion codec (0.6),
cadmpeg's most complete native reader. This page says what Dutchman makes of what that codec
gives and which upstream gaps remain. Improvements land in cadmpeg and reach Dutchman when the
pin is bumped.

## Parts and history

| What the codec gives | What Dutchman does |
|---|---|
| The design's B-rep, with body transforms applied. | Converts it: restated as STEP, written as SolidCraft's import. |
| Parameters, planar sketches with typed and native constraints. | Mapped; native constraints are reported as not carried. |
| Typed extrudes, revolves, fillets, chamfers and more, with selections often native. | Extrudes, revolves and lofts map (see ARCHITECTURE.md for the cases); fillet, chamfer and hole wait on typed edge and face references. |

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
- **Joints** between components become SolidCraft rigid joints written as built: each holds its
  two occurrences where they are placed, so moving one in SolidCraft moves the other. A joint to
  the design's root component holds its occurrence fixed. cadmpeg 0.6.3 reads Fusion joints as
  fixed joints only (motion types such as revolute or slider are not decoded yet), so every
  converted joint is rigid. A joint is left out, and reported as Dropped (`joints`), when it is
  of another kind, joins a component to itself or to one in another design, or joins a component
  inside one placed more than once (the SolidCraft design holds that component once, so it would
  not say which copy).

### Upstream gaps

1. **Component bodies.** cadmpeg says which bodies a component owns when the file binds the
   component to its entities (its naming space): the bodies whose body-map entries lie in the
   component's entity range. A component placed by an operation the file does not bind (as in
   some copy-and-paste designs) has no bodies, so that design converts as stored.
2. **Joint motion.** cadmpeg reads Fusion joints as fixed only; revolute, slider and the other
   motion types, and joint limits, wait on it decoding them.

`.f3z` multi-document archives (a design with the designs it references) convert like `.f3d`:
cadmpeg merges the referenced designs into the root design with their placements, as
components. No public `.f3z` with references exists to test against yet; a single design
saved as `.f3z` is tested.
