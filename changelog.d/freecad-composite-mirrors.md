### History

- FreeCAD MultiTransforms containing mirrors keep editable history as ordered mirror and
  pattern features when replay matches the original, targeting SolidCraft 0.25.1 or newer.
  Older targets keep the original geometry with `needs SolidCraft 0.25.1 or newer`
  for chained stages; single-stage mirrors and patterns remain available.
  Full-turn circular patterns omit a
  repeated seed endpoint while retaining every distinct instance.
  An unfinished replay check keeps the original geometry for these patterns.
- Whole-sketch hole placements exclude construction geometry, so reference origin points
  do not create extra holes. Explicit entity selections retain construction locations.
