---
name: dutchman-converting
description: Use when someone wants a CAD file (Fusion, Inventor, SolidWorks, FreeCAD, CATIA, Creo View, Rhino, ACIS or STEP) turned into a SolidCraft design (.solidcraft) with Dutchman, through the dutchman-mcp tools or the dutchman command. Covers inspecting first, reader status and refusals (including IGES, Creo and NX), converting one file or a folder, checking the result in SolidCraft, and explaining losses in plain words.
---

# Converting CAD files with Dutchman

The `dutchman` command also dispatches installed Dutchman CAD tools such as `pilot` and
`fixtures` through `dutchman-<tool>` beside the command or on `PATH`. `dutchman --help`
lists them. These external commands provide their own interfaces; the conversion MCP tools
and JSON reports described here retain their existing schemas.

The full CLI also accepts `dutchman part.f3d` (or `dutchman convert part.f3d`) and derives
the output beside the input from `--to`. An existing derived output is kept with exit 2;
choose `-o PATH` or use `--force` to replace it. `--force` conflicts with `--no-clobber`.
Explicit outputs retain their replacement behaviour. MCP output paths remain required.

Dutchman converts native CAD files into SolidCraft designs. Use the `dutchman-mcp` tools when
they are connected (`inspect`, `convert`, `convert_all`, `formats`); otherwise run the
`dutchman` command with `--json`. Both return the same reports, described in Dutchman's
`docs/json.md`.

## Ground rules

- **The user's designs stay on the user's machine.** Never upload, paste, attach or send a
  design file, or its contents, anywhere: not to a web service, an issue tracker, a chat or
  another tool. Work with local paths only. If a bug report needs a file, say so and let the
  user decide what to share; suggest a small design they made themselves for the purpose.
- **Use absolute paths.** Paths are on the machine running the server; a relative one is read
  against the server's working directory.
- **Don't overwrite without asking.** `convert` replaces an existing output file and
  `convert_all` replaces same-named designs in `out_dir`. Write to a new file or folder
  (for example `converted/` next to the originals) unless the user said otherwise.
- **Say "convert", not "import".** The result is a SolidCraft design; the user's file is not
  changed.

## 1. Check the format

Call `formats` for the installed build's formats, status `words` and `converts`.
Fusion, Inventor, SolidWorks parts, FreeCAD, CATIA, Creo View, Rhino, ACIS and STEP
can convert geometry. Creo and NX parts are identified by contents
(they share `.prt`) and currently open without geometry, so conversion is refused. SolidWorks
assemblies are recognised without a reader. `input.words` appears on
conversion successes and failures. Reader status describes capability; the actual
history and verification report says what the file kept. CLI `--quiet` hides the volunteer
invitation; JSON and MCP reports never include it.

Directory conversion keeps the highest numbered `.prt.N` revision per part, reports older
saves as skipped, and uses the part's base output name without replacing an existing file.
After a refused conversion (except IGES) or a history that did not fully map, the invitation
links to test-file recipes and offers private help: send the file and its STEP export to
Dutchy in Discord.

What the installed build converts is what its `formats` says; check it rather than this
list when unsure.

For anything else, don't try workarounds such as renaming the file. Tell the user plainly:

- **STEP (`.step`, `.stp`)**: converts through the geometry path; SolidCraft also opens STEP directly.
- **IGES (`.igs`, `.iges`)**: recognised with `input.words: "open it in SolidCraft directly"`
  and refused before decoding: "Dutchman doesn't convert IGES yet: open it in SolidCraft
  directly (File > Import)." Use SolidCraft's own importer. Batch results retain the same
  refusal; do not invite the user to make IGES test files.
- **SolidWorks assemblies (`.sldasm`)**, Creo and NX: reading geometry is not supported yet.
  Suggest exporting STEP from the program that made the design and converting that.
- **Inventor assemblies (`.iam`)** convert, but need their part files next to the assembly
  (or at the paths it stores). If parts are missing, the report lists them as `Dropped`.

## 2. Inspect first

Call `inspect` with `input`. It writes nothing. Read:

- `conversion.possible` and `conversion.reason`: whether conversion can succeed and why it
  cannot. `ok: true` means inspection succeeded, even when conversion is impossible.
- `geometry.bodies`, `geometry.faces`: what was read.
- `mode`: `history` (editable feature history) or `import` (solid geometry only).
- `verification.reason`: why there is no history, when there is none.
- `history.features`: each feature's `status`; a `blocked` one has a `reason`.
- `losses`: everything that will not carry across intact.

Sweep histories support twist, end scale, supported orientations, and one guide rail, with
single-curve whole sketches accepted as paths or rails. A blocked sweep's reason names
unsupported corner transitions, section interpolation, tangent-path extension or
simplification. Explain that the converted design keeps its original geometry.

Holes keep chamfered entries and tapped holes (the bore plus an ISO metric thread label; a
thread's hand, class, and length are listed as losses). Taper, exit treatments, modeled
threads, and entry recesses with drill points block history: explain the reported reason.
"SolidCraft holes take a drill point only on a plain drilled hole" means SolidCraft 0.21.0
ignores drill points on counterbores and countersinks; conversion keeps the original geometry.

Tell the user what to expect in one or two sentences before converting, for example: "This
part has one body. Its history won't carry across (the fillet's input edges can't be proven),
so you'll get the solid shape without the feature history."

Sheet-metal histories map base flanges and full-edge flanges on unchanged rectangular base
sheets when their own input-edge midpoint, tangent, and vertices prove the pick. Fixed
outer-face heights, inside/outside placement, angles, and bend radii map. Other dimensions
or unproven selections keep geometry and explain the blocked feature. CADIR's hem developed
length has no exact mapping to SolidCraft's outer-face length. Do not claim
that K-factor or relief settings were preserved: CADIR does not supply them, so sheet rules
use SolidCraft defaults. Contour flange, bend, unfold/refold and corner-relief operations
are not present in the pinned CADIR version.

## 3. Convert

- One file: `convert` with `input` and `output` (ending in `.solidcraft`).
- Many files or a folder: `convert_all` with `inputs` and `out_dir`. One failure does not stop
  the rest; the call is an error when any file failed, and each file has its own report in
  `results`.
  Designs and the default log go inside `out_dir`; optional `journal`, `report_anon`, and
  `report_key` files go to their specified paths, which may be outside it.
- `import_only: true` when the user wants only the shape. Leave `verify` at its default.

A failed conversion comes back with `ok: false` and `error.message`. The message already says
what to do next; pass it on in plain words. A failed file creates no design. A mixed batch
may still write successful designs, logs, and requested reports.

For unreadable archives, suggest downloading or copying again and checking the extension.

### Reporting a problem

When a file fails, loses something it should not, or looks wrong in SolidCraft, and the
original opens in its CAD program, point the user to Dutchman's
[**A file didn't convert**](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form (for a whole library, the **Library test report** form and
`docs/testing-your-library.md`). It asks for the format, the versions, the operating system,
what was expected and what happened, and an anonymous report for that one file:
`convert_all` with only that file in `inputs` and a `report_anon` path (or `dutchman
convert-all --out-dir converted --report-anon report-anon.json /abs/part.f3d`). The user
reads that report before pasting it; ordinary JSON, logs, the journal, the key, and the salt
stay private. Do not attach designs publicly; a user who wants to share a reproducer can
offer it privately in the issue. The issue goes on Dutchman even when the report says
`upstream: cadmpeg` or `upstream: solidcraft`. Draft the issue for the user to post; don't
post it yourself.

## 4. Verify in SolidCraft

Never report success on the reply alone.

If macOS blocks SolidCraft, follow the report's first-run guidance: open SolidCraft once
from Applications, then click Open Anyway in System Settings > Privacy & Security, and
try again. Dutchman checks whether the quarantine attribute has the user-approved flag.
Never remove or change the quarantine attribute.

- `verification.status` `verified`: SolidCraft replayed the history and it matches the
  original. `unverified`: written without that check (the reason says why, usually no
  `solidcraft-cli` was found); mention that the history has not been checked. `flattened`:
  the history did not map or did not match, so the design keeps the imported shape and any
  mapped sketches with resolved planes, hidden for reference with no features built on them.
  An Info `sketches` line counts sketches kept and, when applicable, left out.
- Open the design and look at it: `solidcraft-cli eval /abs/path/design.solidcraft`, or with
  SolidCraft's MCP server, start it on the design (`solidcraft-cli mcp --in
  /abs/path/design.solidcraft`) and call `measure`.
- Check `body_count` against `geometry.bodies` from `inspect`, that no timeline entry has an
  `error` or `warning`, and that volumes and bounding boxes are plausible for the part (ask
  the user for a known dimension if you have nothing to compare with).
- If something is off, say exactly what ("SolidCraft shows 1 body, but 3 of its 6 faces
  could not be shown"), and don't claim the conversion is fine.

## 5. Explain the loss report

The raw messages are technical. Group them by level and translate:

- **Info**: no effect on the shape. Usually say nothing, or "a few internal records that
  SolidCraft doesn't use were left out."
- **Degraded**: carried across in a weaker form. Name the thing affected:
  `f3d/material.not-transferred` means "the shape converted, but its materials did not
  (check the colours too); you can set them again in SolidCraft." A sketch dimension "retaining native
  operands" means "the sketch is the right size, but that dimension may not drive it."
- **Dropped**: left out. Always mention these, with the part or feature named.
- History: say which feature stopped it, from `verification.reason` or the first `blocked`
  feature, and that the shape itself is complete.

Keep it short, lead with what the user got ("You have an editable design with 7 features"
or "You have the solid shape, without its feature history"), then what is missing. Don't
call the user's file corrupt or invalid; when Dutchman cannot read something, it is not
supported yet.

## Command line equivalents

```sh
dutchman formats --json
dutchman inspect --json /abs/part.f3d
dutchman convert --json /abs/part.f3d /abs/converted/part.solidcraft
dutchman convert-all --json --out-dir /abs/converted /abs/project
```

Exit codes: 0 converted, 1 could not convert (report has `ok: false`), 2 usage error.

When a sketch reports `constraints not carried`, translate the count and reason as lost
editability: the solved geometry remains exact. Coordinate, intersection, coradial,
endpoint tangency, curvature, point symmetry, regular polygon, and repeated dimensions can
now carry across. Static patterns keep point and radius associations, but driving count
or span parameters still need a loss. Bounded parabolas and hyperbolas, elliptical arcs
up to one turn, rational quadratic Béziers, and equal-weight rational splines map; selected
multi-piece elliptical arcs, unbounded conics, other weighted/nonuniform splines, and text without
solved outlines still prevent the sketch's history from mapping.

Constant-radius fillets and equal-setback chamfers map when the feature's own input edge
geometry proves which edge each one acts on (currently after one extrusion of a convex
polygon); otherwise the report names why the blend was refused.

Body mirror patterns and join/cut/intersect body combines (optionally keeping the tool
bodies) can keep editable history; their bodies must be named by earlier mapped features.
Shells and drafts are refused, since their faces are known only on the final body, where
the feature has already removed or tilted them; they wait on input face geometry.
Replacing a source shape with its mirror is not supported yet: SolidCraft's mirror keeps
the original. Cosmetic threads lack a resolved standard and pitch in CADIR, so assigning
SolidCraft's ISO metric designation would invent thread data; these remain flattened.

### FreeCAD output (experimental)

When FreeCAD is the requested destination, pass `to: "freecad"` to `convert` or
`convert_all`; use an output path ending in `.FCMacro` for a single conversion. The CLI
form is `dutchman convert INPUT --to freecad -o OUTPUT.FCMacro`. This does not require the
FreeCAD input feature. Tell the user to run the result in FreeCAD with **Macro > Macros… >
Execute**, or `freecadcmd OUTPUT.FCMacro`, then save the document as `.FCStd` if needed.
The macro embeds original STEP geometry; parts whose features cannot be rebuilt remain
imported geometry, with the mapped prefix and mapped sketches hidden in **Rebuilt history
(partial)** when available. Explain `rebuilt N of M features; kept as reference`,
`history.features` and their reasons, and that the history is unverified until the macro
runs. The original remains available for comparison; a runtime
failure restores it and warns in FreeCAD. Parameter values are kept; source expressions
remain inert text. See `docs/guide.md#freecad-output-experimental` for the supported cases and limits.
Assembly macros also write supported joints (through sub-assemblies too) with frames,
limits, and grounding. Explain `joints` losses: unsupported operands/options retain stored
placements, and an ungrounded joint chain gets an inferred grounded part (`Info`). A failed
Assembly solve removes the joints, restores stored placements, and warns in FreeCAD.
`import_only` leaves joints out.

For CLI conversions, `--to fcstd -o OUTPUT.FCStd` runs FreeCAD headlessly, saves the native
document and verifies every part against embedded STEP. `--to freecad --verify` checks a
macro without saving a native document. Set `FREECAD_CMD` to one headless executable path (or the Flatpak form)
when discovery cannot find it; `--freecad-timeout SECONDS` defaults to 300 and kills a slow
process. Explain `differs` using the named part and reason; a runtime fallback restores
STEP and adds a specific loss. A macro may be written with `not checked: FreeCAD not found`;
native output requires working FreeCAD. These extra flags are CLI-only and experimental.
