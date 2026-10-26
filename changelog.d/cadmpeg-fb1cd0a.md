### cadmpeg

- The cadmpeg pin moves from `66550bb` to `fb1cd0a`. Fusion keeps nested local component
  placements, more resolved face selections, and compact coil handedness; reads designs whose
  exact edge curves end slightly away from their vertices (as in some large multi-part
  assemblies) instead of refusing them; gives each design merged into a multi-document
  archive its own parameter scope; and assigns sketches to their components. Inventor reads
  more extrusion and hole extents, full revolves about a work axis, center-point holes,
  spotfaces and hole end targets, oriented chamfers, circular sweeps, unguided two-section
  lofts, closed sketch splines, stored-body base features, and bend-free sheet-metal plates as
  base flanges, and resolves more extrusion profiles and fillet and chamfer edge selections.
  SolidWorks corrects compact-curve edge orientation. Creo preserves plane axes and boundary
  orientation, avoids ambiguous operation states, reads circle radii from type-4 dimensions,
  and binds two-cap circular sweeps. FreeCAD reads point symmetry and improves mesh placement,
  spline domains, joint angles, Body visibility, and document restoration. STEP keeps
  unclaimed bodies, corrects replica transformations, and writes reflected conic pcurves as
  exact rational splines.
