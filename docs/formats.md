# Formats

`dutchman formats` lists what this build reads and converts. **Reads** says whether a
format's geometry can be read today; **Level** is cadmpeg's published headline level on its
format-support ladder (`L0`–`L9`); each conversion's own report says what that file kept.
The desktop app's **Formats** list shows the same.

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
| Creo part | `.prt`, `.prt.N` | Opens, no geometry yet | L1 | Not yet |
| NX part | `.prt` | Opens, no geometry yet | L1 | Not yet |
| Creo View | `.pvz` | Reads geometry | L3 | Geometry through STEP; solids and surfaces SolidCraft can import |
| Rhino | `.3dm` | Reads geometry | L2 | Geometry through STEP; solids and surfaces SolidCraft can import |
| ACIS | `.sat`, `.sab`, `.smt`, `.smb` | Reads geometry | L3 | Geometry through STEP; solids and surfaces SolidCraft can import |
| STEP | `.step`, `.stp` | Reads geometry | L8 | Geometry through STEP; solids and surfaces SolidCraft can import |
| IGES | `.igs`, `.iges` | Recognised, not converted | – | Import it in SolidCraft directly (File > Import) |
| Unidentified part (Creo or NX) | `.prt` | Recognised, not read yet | – | Not yet |

FreeCAD TechDraw drawings also convert to DXF (`--to dxf`): page curves, dimension labels, annotations, and templates, with unsupported content reported as Dropped.

Output is a SolidCraft design (`.sc`), or, experimental, a FreeCAD rebuild macro (`.FCMacro`, `--to freecad`).
<!-- /formats -->

## How formats are recognised

- Formats with geometry readers convert through STEP; Fusion, Inventor, and FreeCAD also
  keep feature history where it maps (see [Feature history](history.md)).
- Creo and NX share `.prt` and are told apart by their contents; numbered saves (`.prt.N`)
  are recognised too. A folder conversion keeps only the highest `.prt.N` of each part and
  reports the older ones as skipped; `bracket.prt.7` writes `bracket.sc`, and an existing
  output for a numbered save is kept unless you choose another path (or `--force` for one
  file).
- Inputs without a geometry reader are refused with exit 1. IGES (`.igs`, `.iges`) is
  refused before decoding, in single files and folders alike: "Dutchman doesn't convert
  IGES yet: open it in SolidCraft directly (File > Import)."
- A recognised format whose geometry has no solids or surfaces SolidCraft can import says so.

## When a file doesn't convert

After the results, a refused conversion (except IGES) or a history that didn't fully map
prints one invitation to make [test files](https://dutchman-cad.github.io/fixtures/) or type
`/volunteer` in [Discord](https://discord.gg/yTcseBEdeu); `--quiet` hides it, and with
`--json` stdout holds only the report. The desktop app shows the same under its results.
Got a file that converts wrong? Send it, with its STEP export, to Dutchy in a Discord DM.

## Per program

What converts today, and what still stands in the way upstream:
[Fusion](fusion.md), [Inventor](inventor.md), [SolidWorks](sldprt.md), and
[FreeCAD](freecad.md). [cadmpeg](https://github.com/cadmpeg/cadmpeg) reads the native files,
so a format improves in Dutchman when it improves there.
