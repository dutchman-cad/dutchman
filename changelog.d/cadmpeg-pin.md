### cadmpeg

- The cadmpeg pin moves from `65d6e87` to `478f1df`, now taken from cadmpeg's own repository
  rather than a fork of it.
- Fusion: extrude profiles are proven from their end caps and recipe operands are bound per
  counted operand, so extrudes no longer sweep the wrong sketch region where cadmpeg picked one
  by heuristic. Fillet, chamfer and face selections match their recipe faces exactly, and
  chamfer edges between faces of other features resolve.
- Fusion also reads text parameters, Design parameters in their comment form, sketch scopes
  that list deleted members, the 466-byte coil form, pipe groups, diagonal and frameless
  dimension annotations, more selection and scope forms, lofts to a sketch point, and work
  plane frames placed by their U direction. It resolves trimmed extrude regions, incomplete
  fillet groups, replaced fillet and chamfer support faces, Move Face and Offset Faces members,
  and a cylinder blend's edge by geometry. An unprofiled sketch curve that bounds a selected
  extrude region is reported at Info level, since nothing is lost.
- Fusion assemblies read occurrences that reference a source design, leading-flag local
  occurrences, version-4 and localized joint scopes, and joints of a standalone design bound by
  their relations only.
- Inventor: polygon sketches close, parameters keep their values when the unit type is
  unrecognised, bodies are named from the feature that owns them, fillet and chamfer input
  edges resolve on elliptical carriers and with secondary names, and attached plates in a
  rotated frame are read.
- SolidWorks and NX parts read more of their node sequences (Deltas carriers and blends,
  patched points, terminated-stream nodes, attributes and NURBS lanes), and analytic and
  B-spline carriers are decoded once for both.
