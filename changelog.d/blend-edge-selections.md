### History

- A fillet or chamfer left unmapped for its edges says which case it is: a selection cadmpeg
  does not resolve to edges yet, edges only partly resolved in its input topology, or edges of
  the final body.
- A fillet or chamfer on edges an extrusion generated maps when cadmpeg names them by the
  sketch entity that made them, as it does for Inventor: a sketch point's side edge, or a sketch
  line's edge where the extrusion starts or ends. Each edge is proven on SolidCraft's rebuilt
  body before the blend is written.
