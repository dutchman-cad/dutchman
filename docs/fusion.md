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
| Typed extrudes, revolves, fillets, chamfers and more, with selections often native. | Extrudes and revolves map (see ARCHITECTURE.md for the cases); fillet, chamfer and hole wait on typed edge and face references. |

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
- **Components inside components** are not converted yet; the design converts as stored and
  the report says so.

### Upstream gaps

1. **Component bodies.** cadmpeg 0.6 leaves `product_definitions[].bodies` empty for Fusion
   components, so nothing says which bodies a component owns. This is the blocker for Fusion
   components: once cadmpeg fills it, Dutchman writes components with no change of its own.
2. **Nested components.** Occurrences placed inside other occurrences.
3. **Joints and constraints between components** (cadmpeg lists intra-document
   multi-component structure and constraints as open).
4. **Shared:** cadmpeg/cadmpeg#255 (STEP face loops written reversed).

`.f3z` multi-document archives (a design with the designs it references) convert like `.f3d`:
cadmpeg merges the referenced designs into the root design with their placements, as
components. No public `.f3z` with references exists to test against yet; a single design
saved as `.f3z` is tested.
