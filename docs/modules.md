# Optional modules

## How Dutchman finds and runs a module

A module is a separate program named `dutchman-<name>` (the electronics module is
`dutchman-electronics`). Dutchman looks beside its own executable first, then in PATH, the same
way it finds every other companion program. Install it from
https://github.com/dutchman-cad/dutchman-electronics.

Dutchman talks to a module with three commands, each with `--json`:

1. `info --json` (`dutchman.module-info/1`) names the module, its version, its interface
   number and every extension it reads. Dutchman remembers the answer per module file (path, modification time and size), so an
   upgraded module is asked again and a failed `info` is retried. It refuses a module whose
   `interface` is not 1, saying which versions to match.
2. `detect <file> --json` (`dutchman.module-detect/1`) answers `yes`, `maybe_by_name` or `no`
   for that file. Dutchman runs it for a file whose extension `info` lists, and treats `no` as
   "not a board" with a note. Detection needs the extension as well as the contents, so a board
   renamed to another extension is not read.
3. `read <file> --out <folder> --json` writes a placed-parts package (below) into a temporary
   folder and prints the manifest. Dutchman reads the package and deletes the folder.

The conversion then goes on like any other: Results card, report, `--json`.

- **Missing module.** A file with a known board extension (`.PcbDoc`) is recognised and the
  message says to install the electronics module. It is never reported as an unknown file.
- **Module cannot run.** If the module is found but `info` fails, a board file gets "the electronics module is installed but could not be run: <reason>".
- **Failure.** A non-zero exit, or `{"ok": false, "message": ...}`, is an error that shows the
  module's own message, or its stderr; its stderr also goes into the report as a note. A
  module that finishes without writing `manifest.json` is an error too.
- **Limits.** `read` is stopped after 10 minutes; the module may leave at most 4 GiB and
  100,000 files in its folder, and Dutchman keeps at most 16 MiB of its output on stdout and
  64 KiB on stderr. The package itself is limited as described below.

## The placed-parts package

A module that reads a file Dutchman does not (a circuit board, for example) hands Dutchman its result as a
**placed-parts package**: a folder of STEP files and a manifest, `manifest.json`, in that folder. Dutchman reads each STEP file the way it reads any
STEP file, places the parts, and writes the design. Dutchman learns nothing about what the parts are.

The manifest is JSON with `"schema": "dutchman.parts/1"`. Fields Dutchman does not know are ignored.

| Field | Meaning |
|---|---|
| `name` | The design's display name. |
| `units` | `mm` (default), `cm`, `m` or `in`: the unit of translations in `transform` and of every overlay coordinate. The STEP files carry their own units. |
| `models` | Each geometry file once: `{"id", "file", "name"?}`. `file` is a STEP file named relative to the package folder, with `/` separators. |
| `parts` | Placed instances: `{"name", "model", "transform", "notes"}`. `model` is a model `id`. `transform` is 3 rows of 4 numbers, the rotation in the first three columns and the translation in the last: `x' = R x + t`. It defaults to the identity. |
| `overlays` | Flat graphics on a part, see below. |
| `notes` | `{"severity": "Info" or "Degraded" or "Dropped", "message"}`: anything the module read but left out. They go into the conversion report as they are. |

A model used by a hundred parts is one STEP file and one definition in the design. A part's `name` is its stable
identity: converting the same package twice gives the same names, so a part that only moves stays the same part.
Names must be unique; a repeated name gets ` (2)` and a note.

### Overlays

An overlay is a flat layer drawn on one part, such as copper or silkscreen on a board. The module does all the
geometry and hands over finished areas.

```json
{
  "name": "Copper top",
  "kind": "copper",
  "finish": "masked",
  "side": "top",
  "part": "Board",
  "z": 1.601,
  "color": "#1f7a3d",
  "areas": [ { "outer": [[0, 0], [5, 0], [5, 5], [0, 5]], "holes": [[[1, 1], [1, 4], [4, 4], [4, 1]]] } ]
}
```

- `kind` is `copper`, `silkscreen` or `mask`. `finish` (`masked` or `bare`) tells the two kinds of copper apart and
  appears only on copper. `side` (`top` or `bottom`) is information.
- A `mask` overlay is the solder mask on one face: the board outline minus the mask openings (the pads) and minus the
  drills, as holes. The module draws it between the copper and the silkscreen, at `z` = thickness + 0.005 on top and
  -0.005 below.
- `part` is the `name` of the part the overlay is drawn on. The overlay takes that part's transform, so `areas` and
  `z` are in the part's own frame. An overlay naming no part in the package is dropped, with a note.
- `z` is the height of the overlay's face in the part's frame, in the package's unit.
- `thickness` is optional, in mm, with 0 < `thickness` <= 1. With it, the overlay is solid: each area extruded
  from `z` away from the part (up when `side` is `top`, or missing; down when `side` is `bottom`) by `thickness`,
  holes included. Without it, or at 0, the overlay is a zero-thickness sheet at `z`, as before. A value out of range
  refuses that overlay by name, and the rest converts.
- Areas in one overlay never overlap or touch. Each area becomes its own closed prism, a solid with a top face, a
  bottom face, and one planar side face for each edge of its outer loop and of every hole, all with outward normals.
  The prisms of one overlay are one part with one look, and each keeps its own volume. SolidCraft names the first
  `Body (<overlay>)` and the others `Body (2)`, `Body (3)` and so on.
- The module sets the stacking through `z`. Dutchman moves nothing.
- Dutchman caps the faces of all thick overlays in a package at 250,000 (each extruded area costs 2 plus its edge count,
  holes included). An overlay that would pass the cap is drawn as a sheet, with a `Degraded` note naming it.
- `metallic` and `roughness` are optional on every overlay and every part, each a number from 0 to 1. When given, they
  are written into the body's appearance beside the colour (SolidCraft's PBR look: `metallic`, `roughness` clamped to
  at least 0.045, and a fixed dielectric IOR of 1.5; a missing one of the pair is 0 for `metallic` and 0.5 for
  `roughness`). A value out of range refuses that overlay or part by name. When both are missing nothing extra is
  written. They go with a colour: on a part without `color`, and on an overlay with an `opacity` below 1, SolidCraft's
  look cannot hold them and they are left out with a `Degraded` note.
- `color` is optional, `#rrggbb` in sRGB. Without it: masked copper and mask are solder mask green, bare copper gold,
  silkscreen white. A value that is not `#rrggbb` refuses that overlay or part by name, and the rest converts.
- `opacity` is optional and appears on `mask` only: a number from 0 to 1 (1 = opaque). Dutchman passes it through to
  the design's surface style (written as transparency, 1 - `opacity`), it does not ignore it. A value outside 0 to 1
  refuses that overlay by name.
- A part may carry `color` too, `#rrggbb`, used for that part's bodies. The board part's colour is its substrate (the
  edge you see). A part named `Plating`, when present, is the copper or finish inside plated holes: thin tubes, one
  model, placed like any other part.
- Each area is `outer` (counter-clockwise, at least 3 points) and `holes` (clockwise, each inside `outer`). An area
  with fewer than 3 points, a point that is not a finite number, the wrong winding, a hole inside another hole, or
  loops that cross, share part of an edge or touch themselves is left out of the overlay and reported as `Degraded` with the overlay's name; the other areas are kept.

Loops of one area may touch at a single point (a hole touching the outer loop, or two holes touching). Points closer
than 1e-6 mm are one point.

Each overlay becomes one body named after it: a sheet with one planar face per area, or, with a `thickness`, a solid for each area.
Colour and opacity apply to every face of either.

### Limits and safety

The package is treated as untrusted. Dutchman refuses a whole package for a manifest over 16 MiB or not valid, another
schema, more than 1,000 models, 100,000 parts or 16 overlays, or any model `file` that is absolute, uses `..`, or
leaves the folder through a link. A STEP file that is missing, over 1 GiB, or that does not decode is a `Dropped`
note naming the model, and every part that uses it is left out; the rest converts.
