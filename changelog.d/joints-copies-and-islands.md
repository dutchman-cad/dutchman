### Assemblies

- A Fusion joint between a copy of a component placed more than once and a part inside that
  copy is the component's own joint, written through its first copy like the part. It joined
  the copy to the first copy's part, so every later copy moved onto the first.
- Joints that reach nothing placed, such as a design placed where it is stored that holds its
  parts in a rigid group, are placed from the outermost occurrence they join, which stays
  where the file puts it. They started from whichever joint came first, so a part-to-part
  joint could fix both parts where they were stored and leave the design's own joints
  unmet.
