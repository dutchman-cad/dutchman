---
name: dutchman-converting
description: Use when someone wants a native CAD file (Fusion .f3d or .f3z, Inventor .ipt or .iam, SolidWorks .sldprt) turned into a SolidCraft design (.solidcraft) with Dutchman, through the dutchman-mcp tools or the dutchman command. Covers inspecting first, converting one file or a folder, checking the result in SolidCraft, explaining the loss report in plain words, and what to say about formats Dutchman does not convert.
---

# Converting CAD files with Dutchman

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

The extensions that convert are `.f3d`, `.f3z`, `.ipt`, `.iam` and `.sldprt`. Call `formats`
if unsure; its `status` is `converts`, `coming` or `not_supported`.

For anything else, don't try workarounds such as renaming the file. Tell the user plainly:

- **STEP (`.step`, `.stp`)**: SolidCraft opens these itself; Dutchman is not needed.
- **SolidWorks assemblies (`.sldasm`)** and other native formats: not supported yet. Suggest
  exporting STEP from the program that made the design and opening that in SolidCraft.
- **Inventor assemblies (`.iam`)** convert, but need their part files next to the assembly
  (or at the paths it stores). If parts are missing, the report lists them as `Dropped`.

## 2. Inspect first

Call `inspect` with `input`. It writes nothing. Read:

- `geometry.bodies`, `geometry.faces`: what was read.
- `mode`: `history` (editable feature history) or `import` (solid geometry only).
- `verification.reason`: why there is no history, when there is none.
- `history.features`: each feature's `status`; a `blocked` one has a `reason`.
- `losses`: everything that will not carry across intact.

Tell the user what to expect in one or two sentences before converting, for example: "This
part has one body. Its history won't carry across (the fillet isn't supported yet), so you'll
get the solid shape without the feature history."

## 3. Convert

- One file: `convert` with `input` and `output` (ending in `.solidcraft`).
- Many files or a folder: `convert_all` with `inputs` and `out_dir`. One failure does not stop
  the rest; the call is an error when any file failed, and each file has its own report in
  `results`.
- `import_only: true` when the user wants only the shape. Leave `verify` at its default.

A failed conversion comes back with `ok: false` and `error.message`. The message already says
what to do next; pass it on in plain words. Nothing was written.

## 4. Verify in SolidCraft

Never report success on the reply alone.

- `verification.status` `verified`: SolidCraft replayed the history and it matches the
  original. `unverified`: written without that check (the reason says why, usually no
  `solidcraft-cli` was found); mention that the history has not been checked. `flattened`:
  the history did not map or did not match, so the design is the shape alone.
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
