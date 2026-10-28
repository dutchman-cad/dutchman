### cadmpeg

- The cadmpeg pin moves from `fb1cd0a` to `1337af7`. SolidWorks parts that 0.1.5 read and
  0.2.0 to 0.2.2 read empty, with fewer bodies, or failing cadmpeg's edge checks read again:
  bytes inside other records no longer hide or refuse a part's bodies, arcs stored past the
  seam keep their range, and solid snapshots stored with features are no longer read as
  sketches. Fusion reads rigid groups and their joints in full, with members mated aligned,
  and binds joint operands in other designs, in nested components and in merged member
  designs to the exact occurrence; reads construction planes from their solved placement,
  typed local component occurrences, qualified and legacy external placement frames, edge
  flanges that meet at a corner, localized hole scopes, and through-all hole extents with
  drill tips; shares one definition for each merged external design; and reads fillet face
  groups, sketches placed on entities, parameters with detached owners, and pipe scopes.
  Inventor projects attached plates with their bends, sheet-metal corners that change
  nothing, mirrors about a work plane, and more revolves. NX reads change streams against
  their embedded schema. FreeCAD applies part and link placements once and counts a shell as
  closed when each edge is used as often each way. STEP output recovers edge curves a model
  leaves out, keeps the faces it can bound as surfaces when a body cannot be written whole,
  writes solids as advanced B-rep representations, and leaves a collapsed edge without a
  curve out of its loop.
