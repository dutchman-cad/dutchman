# Dutchman in SolidCraft

The Dutchman plugin lets SolidCraft open Fusion (`.f3d`, `.f3z`), Inventor (`.ipt`, `.iam`),
SolidWorks (`.sldprt`, `.sldasm`), and FreeCAD (`.FCStd`) files directly. File > Open converts
the file with the same code as the `dutchman` command and opens the result as a new, unsaved
design with its editable history. SolidCraft shows what could not be carried over in its
Import report window, one line per kind of loss.

It needs SolidCraft 0.44 or newer, on the desktop. SolidCraft's browser version does not run
plugins yet. Older SolidCraft releases refuse to install it: they cannot run importers or grant
the input folder it reads assembly parts from.

## Install

1. Download `dutchman-plugin-X.Y.Z.zip` from the
   [latest release](https://github.com/dutchman-cad/dutchman/releases/latest). The release also
   lists `dutchman-plugin-X.Y.Z.zip.sha256`; `dutchman-plugin.zip` is the same file under a name
   that does not change.
2. In SolidCraft, choose **Install plugin...**, then pick the zip. No change to **Max plugin
   fuel** is needed.
3. Read the confirmation screen: it shows the plugin's name, version, and what it asks for (see
   below):
   - **2048 MiB of memory**, above SolidCraft's usual 128 MiB plugin limit;
   - **100,000,000,000 fuel**, SolidCraft's default ceiling;
   - **read access to the input folder**, the folder of each file you open, for an assembly's
     parts.

   **Install** approves the package's memory and folder access; **Cancel** installs nothing.

Installing never runs the plugin. After that, **File > Open** offers the Dutchman file types, and
Dutchman is listed in the Plugins menu, where its dialog shows the options before it asks for the
file. To update, install the newer zip over the old one; to remove it, use **Remove** on its card
in **Preferences > Plugins**. Designs you made with it stay as they are.

The package includes a small Dutchman ship icon in SolidCraft's vector icon format,
referenced by the conversion entry's option schema. Its solid and wireframe sails
keep the mark readable at small sizes; SolidCraft's palette supplies its theme and
accent colours. SolidCraft documents schema icons for command buttons, menus and
dialog headers; it does not document a ribbon button or icon display for importers.

SolidCraft checks the converted history against the original geometry. A history that matches
is the visible model; one it cannot check stays hidden beside the original geometry, marked as
not checked.

## First run in one pass

Everything the plugin needs is set once, before the first file. The default **Max plugin fuel**
is enough:

| Step | Where | Value | What it is for |
|---|---|---|---|
| 1 | **Preferences > Plugins > Max plugin memory (MiB)**, only if SolidCraft refuses the memory request | at least 2048 | Room for the decoded model. |
| 2 | **Install plugin...** and approve | memory and "read access to the input folder" | Assembly parts are read from the folder of the file you open. |

The plugin cannot open a dialog or change SolidCraft's preferences, so these are SolidCraft's
settings, not something Dutchman can do for you. What Dutchman does itself: when you open a
`.iam` or `.sldasm` and the folder was not granted, it stops before reading anything and says
which grant is missing and what it is for, instead of failing on the first missing part. A
design above the plugin's estimate is sent to the Dutchman app before any work is done.
A design that needs more work than the plugin's 100,000,000,000 fuel is not attempted; open it
in the Dutchman app instead.

## What it asks for

| Asks for | Why |
|---|---|
| Read access to the folder of the file you open | An assembly names its part files; the plugin reads them from that folder. Without it, only the file you open is read. |
| 2048 MiB of memory | Real CAD files need it; see below. |
| 100,000,000,000 fuel | The work budget; SolidCraft's default ceiling. Larger designs go to the Dutchman app. |

It has no network access, no clock or settings access, and no permission to write files. It
cannot change the design you have open: it only produces a new one.

## The memory ceiling

SolidCraft limits a plugin to 128 MiB unless its manifest asks for more and you approve that at
install. Dutchman asks for 2048 MiB, because converting a Fusion, Inventor, SolidWorks,
or FreeCAD file keeps the whole decoded model in memory, not only the file's bytes. The install
screen says how much it asks for and what your ceiling is; **Cancel** installs nothing. The figure
is a limit for one conversion, not memory the plugin holds on to between files.

Measured: the public test files (5 to 15 KB each) need 3 to 15 MiB inside the plugin. Real
designs are far larger, and memory grows with the design's size and its number of features, not
the file size alone. A 68 MB Fusion archive with a long history needed about 3.8 GiB on the command
line, which is more than the plugin can have. File size alone cannot predict whether a
design fits: the plugin checks expanded archive sizes and estimated work before decoding.
Designs above its conservative conversion budget are directed to the Dutchman app, which
can save a `.sc` to open in SolidCraft. See [the preflight limits](plugin.md) for the formula
and its calibration.

## When a file does not convert

The message SolidCraft shows says why, in the app's plain words, and then carries the full
failure report: the `dutchman.convert/1` report the Dutchman app writes to `<name>.failed.json`,
with the error's kind, stage, and reason codes, as one line of JSON under "Full report". The
Dutchman app writes the same file when you open the design in it, which also works when the
message is cut short in SolidCraft's status area.

SolidCraft gives a plugin no clipboard and no file to write (its importer interface can only
read the file you open and the folder beside it, and answer with a design or an error), so the
message text is the only place the plugin can put the report. SolidCraft's refusal dialog for
importers is not in a release yet; the report is in the text it will show. A design that is only
too large for the plugin gets one plain line instead, with no report: it says which limit,
what the design needs, what the plugin has, and sends you to the Dutchman app.

## Options

The dialog shows three options:

- **Keep editable history** (on): keep the features Dutchman could map next to the original
  geometry. Turn it off for geometry only.
- **Up axis**: **As in the file** keeps the file's coordinates. **Stand up (Y-up to Z-up)**
  turns a Y-up design to Z-up.
- **Include hidden bodies** (off): also bring in hidden or consumed bodies.

## Limits

- SolidCraft 0.44 or newer, desktop only for now.
- SolidCraft defaults to a ceiling of 100,000,000,000 fuel and the plugin asks for exactly that.
  A package runs with the fuel its manifest declares; raising **Max plugin fuel** does not give
  it more.
- One file per conversion, at most 1 GiB read and 256 MiB written, as SolidCraft sets.
- A file that SolidCraft's own reader handles (for example FreeCAD) still opens with SolidCraft's
  reader on File > Open when both are installed; pick Dutchman's entry from the Plugins menu to
  use the plugin.

For how the plugin is built and tested, see [the plugin notes](plugin.md).
