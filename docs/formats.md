# Formats

`dutchman formats` lists what this build reads and converts. **Reads** says whether a
format's geometry can be read today; **Level** is cadmpeg's published headline level on its
format-support ladder (`L0`–`L9`); each conversion's own report says what that file kept.
The desktop app's **Formats** list shows the same and explains the level codes.
The **Designs** input row shows only the plain reader status.

<!-- formats: generated from dutchman formats --markdown -->
| Format | Files | Reads | Level | Converts |
|---|---|---|---|---|
| Fusion archive | `.f3d` | Reads geometry | L5 | Geometry; feature history when every feature maps; components and mapped joints |
| Fusion multi-document archive | `.f3z` | Reads geometry | L5 | As a Fusion archive, with referenced designs merged in by cadmpeg |
| Inventor part | `.ipt` | Reads geometry | L3 | Geometry; feature history when every feature maps |
| Inventor assembly | `.iam` | Reads geometry | L3 | Placed parts and sub-assemblies, read from their files in the project |
| SolidWorks part | `.sldprt` | Reads geometry | L3 | Geometry; feature history waits on cadmpeg binding sketches and extrude profiles |
| SolidWorks assembly | `.sldasm` | Reads geometry | L3 | Placed parts and sub-assemblies, read from their files in the project |
| FreeCAD document | `.FCStd` | Reads geometry | L5 | Geometry and components; feature history and joints when they map |
| CATIA part | `.CATPart` | Reads geometry | L3 | Geometry through STEP; solids and surfaces SolidCraft can import |
| Creo part | `.prt`, `.prt.N` | Reads geometry | L1 | Geometry; solids and surfaces SolidCraft can import |
| NX part | `.prt` | Reads geometry | L1 | Geometry through STEP; solids and surfaces SolidCraft can import |
| Creo View | `.pvz` | Reads geometry | L3 | Geometry through STEP; solids and surfaces SolidCraft can import |
| Rhino | `.3dm` | Reads geometry | L2 | Geometry through STEP; solids and surfaces SolidCraft can import |
| ACIS | `.sat`, `.sab`, `.smt`, `.smb` | Reads geometry | L3 | Geometry through STEP; solids and surfaces SolidCraft can import |
| STEP | `.step`, `.stp` | Reads geometry | L8 | Geometry through STEP; solids and surfaces SolidCraft can import |
| Onshape bundle | `.onshape-bundle` | Reads geometry | – | One offline Part Studio; geometry and editable history where features map |
| IGES | `.igs`, `.iges` | Recognized, not converted | – | Import it in SolidCraft directly (File > Import) |
| Circuit board | `.PcbDoc` | Reads geometry with the electronics module | – | Placed parts and flat board layers, through the electronics module (`dutchman-electronics`, installed separately) |
| Unidentified part (Creo or NX) | `.prt` | Recognized, not read yet | – | Not yet |
| Inventor drawing | `.idw` | Recognized, but reading its sheets isn't supported yet. The report lists what was found. | – | A report of what was found; reading the sheets and views isn't supported yet, so no drawing is written |
| SolidWorks drawing | `.slddrw` | Recognized, but reading its sheets isn't supported yet. The report lists what was found. | – | A report of what was found; reading the sheets and views isn't supported yet, so no drawing is written |
| Fusion drawing | `.f2d` | Recognized, but reading its sheets isn't supported yet. The report lists what was found. | – | A report of what was found; reading the sheets and views isn't supported yet, so no drawing is written |
| DWG drawing | `.dwg` | Recognized, but reading its sheets isn't supported yet. The report lists what was found. | – | A report of what was found; reading the sheets and views isn't supported yet, so no drawing is written |
| DXF drawing | `.dxf` | Recognized, but reading its sheets isn't supported yet. The report lists what was found. | – | A report of what was found; reading the sheets and views isn't supported yet, so no drawing is written |
| Solid Edge drawing | `.dft` | Recognized, but reading its sheets isn't supported yet. The report lists what was found. | – | A report of what was found; reading the sheets and views isn't supported yet, so no drawing is written |

FreeCAD TechDraw pages are written into the converted SolidCraft design as a drawing (sheets and views), and also convert to DXF (`--to dxf`): page curves, dimension labels, annotations, and templates, with unsupported content reported as Dropped.

Fusion tool libraries (`.tools`, `.json`) also convert to CamCraft's `.camtools` (`dutchman library.tools`): tool geometry, holder and cutting data, with every tool and field CamCraft has no place for named in the report. See docs/camcraft-tools.md.

Output is a SolidCraft design (`.sc`), or, experimental, a FreeCAD rebuild macro (`.FCMacro`, `--to freecad`).
<!-- /formats -->

## How formats are recognized

- Formats with geometry readers convert through STEP; Fusion, Inventor, and FreeCAD also
  keep feature history where it maps (see [Feature history](history.md)).
- Creo and NX share `.prt` and are told apart by their contents; numbered saves (`.prt.N`)
  are recognized too. A folder conversion keeps only the highest `.prt.N` of each part and
  reports the older ones as skipped; `bracket.prt.7` writes `bracket.sc`, and an existing
  output for a numbered save is kept unless you choose another path (or `--force` for one
  file).
- Inputs without a geometry reader are refused with exit 1. IGES (`.igs`, `.iges`) is
  refused before decoding, in single files and folders alike: "Dutchman doesn't convert
  IGES yet: open it in SolidCraft directly (File > Import)."
- Circuit boards are read by the optional electronics module, a separate program,
  `dutchman-electronics`. The module lists the extensions it reads (`.PcbDoc` today); Dutchman
  recognizes `.PcbDoc` even without it and then says to install the module. See
  [Optional modules](modules.md#how-dutchman-finds-and-runs-a-module).
- A recognized format whose geometry has no solids or surfaces SolidCraft can import says so.

## Drawings

Drawing files are accepted everywhere models are: the desktop app (picker and drop),
`convert`, `convert-all`, watch mode and the batch lists. Each one gets a result and a
report with a `drawing` section ([JSON reports](json.md#drawings-additive-to-dutchmanconvert1)),
and none is skipped without one. The format is checked against the file's contents, not
only its suffix.

| Drawing | Files | Level today |
|---|---|---|
| FreeCAD TechDraw | pages inside `.FCStd` | Sheets (paper size) and views (direction, scale, place) are written as a SolidCraft drawing inside the converted `.sc`, drawn from that model. Dimensions, notes, balloons, section and detail views are reported as not written. |
| Inventor | `.idw` | Accepted and identified; sheets and views are not decoded yet (`drawing.format.not_decoded`). |
| SolidWorks | `.slddrw` | Accepted and identified; not decoded yet. |
| Fusion | `.f2d`, and drawings inside `.f3z` | Accepted and identified; not decoded yet. |
| Solid Edge | `.dft` | Accepted and identified (compound file with the draft stream); not decoded yet. |
| AutoCAD, Inventor | `.dwg` | Accepted; the version is named from the file's magic; not decoded yet. |
| Onshape drawing export | `.dwg`, `.dxf` | Accepted as DWG or DXF; not decoded yet. A PDF export carries no geometry Dutchman can use and is not accepted. |

A drawing that cannot be read yet is reported as a failed conversion with the section's
`status: "not_decoded"`; the report says what was identified so the reader can be built
from it. The plugin converts models only and does not list drawings.

## Onshape bundles

`.onshape-bundle` converts one offline Part Studio, with editable sketches, extrudes,
revolves, constant fillets, equal chamfers and simple holes where their selections
resolve. ZIP magic and the format 1 manifest are checked. Unsupported history uses
the shared STEP fallback, with a named loss. See [Onshape reader](onshape.md) for
the mapping and bundle requirements.

## When a file doesn't convert

After the results, a refused conversion (except IGES) or a history that didn't fully map
prints one invitation to make [test files](https://dutchmancad.xyz/fixtures/) or type
`/volunteer` in [Discord](https://discord.gg/yTcseBEdeu); `--quiet` hides it, and with
`--json` stdout holds only the report. The desktop app shows the same under its results.
Got a file that converts wrong? Please send it, with a STEP export of the exact same design, to Dutchy in a Discord DM.

## Per program

What converts today, and what still stands in the way upstream:
[Fusion](fusion.md), [Inventor](inventor.md), [SolidWorks](sldprt.md), and
[FreeCAD](freecad.md). [cadmpeg](https://github.com/cadmpeg/cadmpeg) reads the native files,
so a format improves in Dutchman when it improves there.
