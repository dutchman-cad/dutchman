### cadmpeg

- The cadmpeg pin moves from `65d6e87` to `5c1a97e`, now taken from cadmpeg's own repository
  rather than a fork of it. For Fusion files, the decoder reads text parameters' text envelope,
  sketch scopes that list deleted sketch members, the 466-byte coil form, dimension annotation
  frames without a metadata block, and the direction of diagonal two-point linear dimensions.
  It restores work plane frames, resolves trimmed extrude regions, binds an incomplete fillet
  group to the edges its blends replace, maps a replaced fillet or chamfer support face to the
  face it lost, and resolves Move Face and Offset Faces members by their bounded-face
  recipes. SolidWorks parts read Deltas carriers and patched point values from the node
  sequence, and NX parts decode native Deltas instance fields.
