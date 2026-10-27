### cadmpeg

- The cadmpeg pin moves from `fb1cd0a` to `PINSHA`. SolidWorks parts that 0.1.5 read and
  0.2.0 to 0.2.2 read empty, with fewer bodies, or failing cadmpeg's edge checks read again:
  bytes inside other records no longer hide or refuse a part's bodies, and arcs stored past
  the seam keep their range. CADMPEG_SUMMARY
