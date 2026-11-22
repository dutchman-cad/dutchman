# The FreeCAD add-on

[Dutchman for FreeCAD](https://github.com/dutchman-cad/freecad-addon) is a FreeCAD add-on that opens the files Dutchman reads (Fusion, Inventor, SolidWorks, CATIA, NX, Rhino, Creo View, ACIS, STEP, IGES and more) directly in FreeCAD. Dutchman converts the file in the background and the add-on loads the result into your document.

## Install

1. In FreeCAD, open **Tools > Addon Manager**, then the Addon Manager's **Preferences** (Addon Manager Options).
2. Add a custom repository: `https://github.com/dutchman-cad/freecad-addon`, branch `main`.
3. Reopen the Addon Manager. The add-on is listed as **freecad-addon** (it is called **Dutchman** once installed). Select it, click **Install**, then restart FreeCAD.

The add-on is not yet in FreeCAD's own add-on index, so the custom repository step is needed for now.

## Use

- **File > Open**, choose a `Dutchman: …` file type, or **File > Import** to add a file to the current document.
- If Dutchman is not installed, the first import offers to download it (checksum-verified). If you already have it, cancel and set its path in **Edit > Preferences > Dutchman**.
- The **Dutchman import report** panel opens on the right: what was imported, what was lost, and where to get help.

Problems with the add-on itself go to [its issues](https://github.com/dutchman-cad/freecad-addon/issues); a file that converts wrong is a Dutchman problem ([report a file](../README.md#report-a-file-that-fails-or-converts-wrong)).
