# FreeCAD TechDraw drawing output

`dutchman convert part.FCStd --to dxf` writes `part-PageName.dxf` for each
TechDraw page beside the input. `--page NAME` selects a page by its FreeCAD object
name. Use `-o existing-directory` for per-page files there, or `-o drawing.dxf`
with `--page` for an exact filename; without `--page`, that path is the filename
base (`drawing-PageName.dxf`). Default files require `--force` to replace;
explicit outputs replace unless `--no-clobber` is given.

DXF output keeps neutral view curves, positioned annotation text, dimension labels,
template frames, and resolved title-block text in millimetres at 1:1 page scale.
Dimensions keep verified measurements or replacement text. Neutral linear and
radial anchor pairs generate non-associative lines and open arrows; dimensions
without usable anchors keep labels only. Missing curves, unresolved dimensions, and unsupported presentation are
reported per page. `--json` lists entity counts and losses.
See [FreeCAD drawing coverage](freecad.md#techdraw-to-dxf). DXF is available
through `convert`; batch, desktop, web and MCP drawing conversion are not supported yet.
