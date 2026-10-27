### Formats

- FreeCAD parts no longer lose faces bounded by trimmed elliptical (or other trimmed) edges:
  leaving out hidden intermediate shapes kept the edge's curve but dropped the construction
  and full curve it is cut from, so the edge, its loop and its face were left out of the
  geometry.
