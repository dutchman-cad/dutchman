# Inventor: what converts today, and what stands in the way

Dutchman reads Inventor through cadmpeg's published Inventor codec (cadmpeg-registry's
`inventor` feature, 0.6). Improvements to Inventor decoding land in cadmpeg and reach Dutchman
when the pin is bumped. This page says what Dutchman makes of what that codec gives, and which
upstream gaps keep Inventor history from replaying. It drives what is asked of cadmpeg next.

State as of cadmpeg 0.6.2 (docs/format-support.md upstream rates Inventor L1 with extras).

## Parts (`.ipt`)

| What the codec gives | What Dutchman does |
|---|---|
| The part B-rep, from one typed ACIS/ASM kernel carrier (verified for ACIS majors 217 and 218). | Converts it: restated as STEP, written as SolidCraft's import container, like Fusion. |
| No geometry (empty or invalid carrier: blocking `geometry_not_transferred`). | Stops: "no geometry could be read". |
| Parameters (closed scalar graphs). | Mapped to SolidCraft parameters. |
| Planar sketches: points, lines, circles, ellipses, with typed and native constraints. | Mapped, ellipses as whole ellipses; typed constraints mapped, native ones reported as not carried. |
| Extrude, hole, constant-radius fillet, equal-distance chamfer, when every property, dimension and selection resolves. | Extrude: blocked by its profile reference (below). Hole, fillet, chamfer: not mapped yet. |
| Suppression and dependency state: unresolved (`suppressed` absent). | Features are treated as active; a replay that differs is caught by verification and flattened. |
| Product name: none for the root product definition. | cadmpeg's STEP writer then names the product by its id (`inventor:document:product#root`), which SolidCraft shows as the body name. |

## Upstream gaps, in the order they block history

1. **Extrude profiles are native selections.** The codec gives an extrude's profile as
   `sketch_selection`: the sketch plus Inventor profile-selection property ids. Nothing in
   CADIR ties those ids to the sketch's loops, so Dutchman cannot say which profiles they are,
   and every Inventor extrude stops the history ("sketch_selection profiles are not mapped
   yet"). Needed: the selections resolved into `sketch_profiles` (loops of the sketch) or
   `sketch_regions`.
2. **Edge and face selections are native.** Fillet, chamfer and hole carry `Native` edge or
   face selections. SolidCraft finds edges by points on them and faces by persistent names, so
   these need typed historical or generated references (cadmpeg's `Historical`/`Generated`
   selections) before Dutchman can place them. The same gap holds for Fusion.
3. **No revolve.** The codec has no revolve projection yet.
4. **Suppression and dependencies unresolved.** Suppressed Inventor features currently look
   active.
5. **Through-next ends.** An extrude ending at the next face is not mapped by Dutchman yet; it
   needs the face it stops at, which also depends on (2).
6. **STEP product name.** cadmpeg's STEP writer names a product with no name by its
   product-definition id.

## Assemblies (`.iam`)

Assemblies convert as placed parts, each with its own feature history when that history
maps (see the parts table above for what blocks Inventor history today):

- cadmpeg gives each occurrence its stored part path (often absolute on the machine that
  saved it), its rigid placement in millimetres, and its name. Dutchman looks for the part at
  the stored path, then by file name next to the assembly (ignoring case), reads each part
  file once, and writes one SolidCraft component per part, holding the part's geometry as an
  import, with one occurrence per placement.
- Sub-assemblies are read from their own `.iam` files the same way, recursively (up to 32
  deep; an assembly that places itself, directly or not, is refused). Each becomes a component
  whose parts are components beneath it, placed by occurrences inside it, so SolidCraft keeps
  the assembly's structure. The occurrences a parent file records inside its sub-assemblies
  are taken from the sub-assembly files instead.
- Left out and reported as Dropped: parts or sub-assemblies that cannot be found or read,
  scaled placements, and occurrences whose part is not a separate file.
- Upstream: cadmpeg leaves local prototypes untransferred, and joints arrive with CADIR
  version 7 (cadmpeg PR #254).

## Testing without real Inventor files

cadmpeg publishes no Inventor part with geometry or history, and Dutchman never uses customer
files. `crates/dutchman-core/tests/fixtures/synthetic_cube.ipt` is generated from cadmpeg's own
synthetic sources (its README has the recipe). History behaviour is tested on CADIR built in
the tests in the shape the Inventor codec emits, until cadmpeg publishes generated Inventor
goldens with a design segment (sketch and extrude).
