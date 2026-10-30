# Format details

The [generated table](../README.md#what-converts) shows this build's readers and cadmpeg's
published levels. Run `dutchman formats` for the live list. A reader's level describes its
capabilities; the conversion report says what a particular file kept.

## Reading geometry

- Fusion, Inventor, SolidWorks parts, FreeCAD, CATIA, Creo View, Rhino, ACIS, and STEP can
  supply geometry. Conversion restates it as STEP inside the SolidCraft design.
- Creo and NX share `.prt`; contents identify the format, including numbered `.prt.N`
  saves. These readers currently open files without supplying geometry. An inconclusive
  file stays recognised without choosing a reader.
- SolidWorks assemblies are recognised without a reader.
- IGES is refused before decoding. Open it in SolidCraft directly (File > Import).
- FreeCAD input needs the default `freecad` feature. Experimental FreeCAD macro output
  works independently of that input feature.

## What the result keeps

| Source or outcome | Result |
|---|---|
| Every active feature maps and replay matches | Editable history, with the original shape kept as a suppressed import |
| A later feature blocks history | An editable prefix and resolved sketches hidden beside the visible original shape |
| No body-building prefix maps | Original geometry and resolved reference sketches |
| Stored solid with no recipe | A base-body Import feature; later features can remain editable |
| Assembly | Components and placements; supported joints when their operands and frames resolve |

Show **Editable history (partial)** to edit a retained prefix. Hide the visible imported
final shape while working on it. SolidCraft's numeric evaluation includes hidden bodies,
so evaluating the entire saved design is not a comparison of visible bodies alone.

Fusion archives can keep components and mapped joints. Multi-document archives merge
referenced designs through cadmpeg. Inventor assemblies read placed parts and sub-assemblies
from their project files; Inventor joints are not decoded yet. SolidWorks parts retain
geometry while sketch bindings and extrusion profiles still block feature history.

Fusion consumed tools and removed bodies are omitted only when cadmpeg marks them hidden
and no component owns them. User-hidden component members remain available; mixed shown
and hidden parts may be shown whole. See [Fusion visibility and colours](fusion.md).

## Reports and checks

Every conversion lists what it could not carry across:

| Level | Meaning |
|---|---|
| Info | A conversion decision or content that needs no target feature |
| Degraded | Content changed or a check could not establish full fidelity |
| Dropped | Content could not be carried across |

A SolidCraft CLI can replay editable history and compare body count, area, and volume.
A face-count difference alone does not fail the check. Without a usable CLI, retained
history is reported as unverified. See [history coverage](guide.md#feature-history-and-the-solidcraft-check)
and [SolidCraft compatibility](compatibility.md).

Experimental [FreeCAD output](guide.md#freecad-output-experimental) writes a rebuild macro
or runs FreeCAD to save a native document. It keeps original geometry for fallback.
[TechDraw pages](drawings.md) convert separately to DXF.

For upstream limits, see [Fusion](fusion.md), [Inventor](inventor.md),
[SolidWorks](sldprt.md), and [FreeCAD](freecad.md). After a refused conversion (except IGES)
or partial history, the CLI offers [fixture recipes](https://dutchman-cad.github.io/fixtures/)
and private help in [Discord](https://discord.gg/yTcseBEdeu); `--quiet` hides the invitation.
