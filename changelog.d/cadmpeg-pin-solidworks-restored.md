### cadmpeg

- The cadmpeg pin moves from `fb1cd0a` to `8b7deb4`. SolidWorks parts that 0.1.5 read and
  0.2.0 to 0.2.2 read empty, with fewer bodies, or failing cadmpeg's edge checks read again:
  bytes inside other records no longer hide or refuse a part's bodies, and arcs stored past
  the seam keep their range.
  Fusion reads rigid groups and their joints in full and binds joint operands in other
  designs to the exact occurrence; reads typed local component occurrences and qualified
  external placement frames; shares one definition for each merged external design; and reads
  fillet face groups, sketches placed on entities, parameters with detached owners, and pipe
  scopes. Inventor projects attached plates with their bends and more revolves. NX reads change
  streams against their embedded schema. FreeCAD applies part and link placements once.
