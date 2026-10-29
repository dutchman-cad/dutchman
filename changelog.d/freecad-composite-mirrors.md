### History

- FreeCAD MultiTransforms containing mirrors keep editable history as ordered mirror and
  pattern features when replay matches the original. Full-turn circular patterns omit a
  repeated seed endpoint while retaining every distinct instance.
  An unfinished replay check keeps the original geometry for these patterns.
- Whole-sketch hole placements exclude construction geometry, so reference origin points
  do not create extra holes. Explicit entity selections retain construction locations.
