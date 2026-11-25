### Formats

- Drawing files are accepted everywhere models are (`.idw`, `.slddrw`, `.f2d` and drawings inside `.f3z`, `.dft`, `.dwg`, `.dxf`), found by their contents, and every one gets a report with a `drawing` section, never a silent skip. Formats whose sheets are not decoded yet say so with the reason code `drawing.format.not_decoded`.
- FreeCAD TechDraw pages are written into the converted SolidCraft design as a drawing: sheets with their paper size, and views with direction, scale and place. Dimensions, notes, balloons, and section and detail views are named in the report as not written yet.
