### Formats

- Read Onshape bundles through cadmpeg's Onshape codec, the same decode path as every other
  format. Sweeps, offset planes and the sketches on them, sketches on model faces, shells, up to
  next extrudes, and subtractions after a Derived insert now keep their editable history. On 23
  Onshape-built test bundles every bundle converts with its full history verified (was 0 of 23),
  and 63 of 63 features map (was 23 of 59).
- A union or intersection Body Boolean after a Derived insert now stays a native feature, and
  history stops there with a named reason. Onshape stores no target body for these two, so the
  join or intersect target the earlier reader chose was not the source's. Subtraction keeps its
  editable history.
- Keep an Onshape bundle's stored history when its STEP export cannot be read. The report then
  says that the history is kept without a check against the final Onshape shape, and why.
