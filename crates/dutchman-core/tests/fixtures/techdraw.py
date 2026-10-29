# Synthetic fixture, authored for Dutchman using FreeCAD's public scripting API.
# Run: flatpak run --command=FreeCADCmd org.freecad.FreeCAD techdraw.py
# Optional output folder: DUTCHMAN_FIXTURE_OUT. No installed template is copied.
import os
import FreeCAD as App
import Part
import TechDraw

out = os.environ.get("DUTCHMAN_FIXTURE_OUT", os.path.dirname(os.path.abspath(__file__)))
os.makedirs(out, exist_ok=True)
svg = os.path.join(out, "techdraw-template.svg")
with open(svg, "w") as f:
    f.write('''<svg xmlns="http://www.w3.org/2000/svg" xmlns:freecad="http://www.freecadweb.org/wiki/index.php?title=Svg_Namespace" width="210mm" height="297mm" viewBox="0 0 210 297">
<rect x="10" y="10" width="190" height="277" fill="none" stroke="black" stroke-width="0.5"/>
<path d="M110 267 H200 M110 267 V287" fill="none" stroke="black"/>
<text id="TitleText" x="115" y="278" font-size="4" freecad:editable="Title"><tspan id="TitleValue">Synthetic box</tspan></text></svg>''')
doc = App.newDocument("TechDrawFixture")
box = doc.addObject("Part::Box", "Box")
box.Length, box.Width, box.Height = 30, 20, 10
page = doc.addObject("TechDraw::DrawPage", "Page")
template = doc.addObject("TechDraw::DrawSVGTemplate", "Template")
template.Template = svg
page.Template = template
for name, direction, x, y, scale in [("Front", (0,-1,0), 65,180,2), ("Top", (0,0,1), 65,230,1)]:
    view = doc.addObject("TechDraw::DrawViewPart", name)
    view.Source = [box]
    view.Direction = App.Vector(*direction)
    page.addView(view)
    view.ScaleType = "Custom"
    view.Scale = scale
    view.X, view.Y = x,y
doc.recompute()
dimension = doc.addObject("TechDraw::DrawViewDimension", "Length")
dimension.Type = "DistanceX"
dimension.References2D = [(doc.getObject("Front"), ("Edge3",))]
page.addView(dimension)
dimension.X, dimension.Y = 65,160
note = doc.addObject("TechDraw::DrawViewAnnotation", "Note")
note.Text = ["Synthetic box", "30 x 20 x 10 mm"]
page.addView(note)
note.X, note.Y = 120,100
doc.recompute()
if abs(dimension.getRawValue() - 30.0) > 1e-7:
    raise RuntimeError("Expected a 30 mm horizontal dimension; check Front edge numbering")
if template.EditableTexts.get("Title") != "Synthetic box":
    raise RuntimeError("The authored template title was not resolved")
doc.saveAs(os.path.join(out, "techdraw.FCStd"))
App.closeDocument(doc.Name)
# Also exercise multiple pages and files containing pages without any 3D geometry.
doc = App.newDocument("DrawingOnly")
for name, x in [("First",20), ("Second",40)]:
    page = doc.addObject("TechDraw::DrawPage", name)
    template = doc.addObject("TechDraw::DrawSVGTemplate", name + "Template")
    template.Template = svg
    page.Template = template
    note = doc.addObject("TechDraw::DrawViewAnnotation", name + "Note")
    note.Text = [name]
    page.addView(note)
    note.X, note.Y = x,30
doc.recompute()
doc.saveAs(os.path.join(out, "techdraw-pages.FCStd"))
App.closeDocument(doc.Name)
