### cadmpeg

- The cadmpeg pin moves from `66550bb` to `c3a1b1b`: Fusion keeps nested local component
  placements, more resolved face selections, and compact coil handedness, and reads designs
  whose exact edge curves end slightly away from their vertices (as in some large multi-part
  assemblies) instead of refusing them; Inventor reads more extrusion and hole extents, full
  revolves about a work axis, and more extrusion profiles and fillet and chamfer edge
  selections; SolidWorks corrects compact-curve edge orientation; Creo preserves plane axes
  and boundary orientation, avoids ambiguous operation states, reads circle radii from type-4
  dimensions, and binds two-cap circular sweeps; FreeCAD reads point symmetry and improves
  mesh placement, spline domains, joint angles, Body visibility, and document restoration; and
  STEP retains unclaimed bodies and corrects replica transformations.
