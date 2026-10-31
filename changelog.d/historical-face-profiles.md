### History

- Extrudes of a face as it was earlier in the timeline ("extrude this face") now keep editable
  history. The face is rebuilt as a sketch of its boundary from the edge geometry the design
  keeps for it, and checked against the face SolidCraft rebuilds before the extrude, which also
  decides which side the extrude runs on. Faces whose boundary is not lines and circular arcs,
  or whose edge geometry is missing, still stop the history and say why.
