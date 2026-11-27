### Assemblies

- A FreeCAD Assembly that links PartDesign Bodies of its own document now converts each linked Body once, at its link's placement, with the Body's own Pad, Pocket and Revolution history mapped and checked like a standalone Body's.
- The document's original Body is no longer placed a second time when a visible Assembly link places it; a hidden or suppressed link, or a Part container, leaves it as stored.
- A linked Body whose features cannot be told from another Body's (a Boolean or a sketch used by two Bodies) is reported with those features named and stays placed as stored; sketches and datums no feature uses are reported too.
