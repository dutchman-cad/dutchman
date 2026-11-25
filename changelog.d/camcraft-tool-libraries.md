### Formats

- Fusion tool libraries (`.tools`, and `.json` exports) convert to CamCraft's `.camtools`:
  `dutchman library.tools` writes `library.camtools`, and the desktop app takes the same files. Tool
  type, geometry, holder, cutting data per material and the tool number carry across; each tool or
  field CamCraft has no place for is named in the report. See docs/camcraft-tools.md.
- A Fusion form mill that draws rows of equal teeth at one pitch converts to a CamCraft multi-form
  thread mill, a tapered mill with a sharp tip converts to a taper mill, and a dovetail cutter takes
  Fusion's shoulder as its neck. Single-tooth form mills and tapered mills with a flat or ball tip
  are still named in the report.
