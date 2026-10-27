### Formats

- Add partial FreeCAD TechDraw page output with `convert --to dxf` and `--page`:
  one millimetre DXF per page, positioned annotation text, separate view and
  annotation layers, and per-page JSON entity counts and losses. Missing cached
  view edges, dimension anchors/measurements, and typed template/frame geometry
  are reported as Dropped at the current cadmpeg pin. Include an ASCII R2000
  encoder for lines, arcs, circles, polylines, splines and text, plus synthetic
  FreeCAD fixtures generated with an authored script.
