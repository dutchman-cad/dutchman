# Keep a converted design up to date

When the CAD file you converted changes, the converted `.sc` should follow it without you
copying anything by hand. SolidCraft keeps placements and joints when a design it has placed
with **Insert Design** is replaced by a new version of the same design, as long as the
component, body and feature names and ids stay the same. Dutchman gives the same names and ids
every time it converts the same source, so a refresh keeps everything attached.

There are three routes. Today the command line works; the app and Dutchman Sync are coming.

## How it works

Every conversion writes a [link record](command-line.md#link-records), `part.link.json`, beside
the design `part.sc`. It names the source file, its SHA-256, size and modified time, the options
used, and the design's own digest. Keeping up to date reads that record, checks whether the
source still has the recorded bytes, and converts again only when it does not.

1. Convert as usual: `dutchman bracket.f3d out/bracket.sc` writes `out/bracket.sc` and
   `out/bracket.link.json`.
2. In SolidCraft, place `out/bracket.sc` with **Insert Design**.
3. When `bracket.f3d` changes, Dutchman converts it again and replaces `out/bracket.sc`.
4. In SolidCraft, **Update Links** picks up the new `bracket.sc`.

## The command line (works today)

Bring a folder of converted designs up to date once:

```sh
dutchman sync out/
```

It searches `out/` and its subfolders for link records and prints one line per design:

| Line | Meaning |
| --- | --- |
| `unchanged out/bracket.sc` | The source still has the recorded bytes. Nothing was converted. |
| `updated out/bracket.sc` | The source changed. The design and its record were replaced. |
| `failed out/bracket.sc  <reason>` | The conversion failed. The previous design and record are kept. |
| `source missing out/bracket.sc` | The source file is not there. Nothing was changed. |

The source is found from the record: its path is relative to the record's folder, or absolute.
Size and modified time are a quick first check; when they differ the file is read and its
SHA-256 compared, so a file that was only saved again with the same contents is `unchanged`.
A design is converted again with the options recorded in its record (output format, preset,
what to keep, part positions, SolidCraft version, and the project and search folders). The new
design is written to a temporary file in the same folder and renamed over the old one, so a
reader never sees half a design. The exit status is 1 when a design failed.

Keep a folder up to date as you work:

```sh
dutchman sync out/ --watch
```

This keeps running, like [`dutchman watch`](command-line.md#watch-a-folder). Every `--interval`
seconds (default 5) it looks at each record's source; a source that was saved is converted again
once its size and modified time have stayed the same for `--settle` seconds (default 10), so a
save in progress is not converted half written. Stop it with Ctrl+C.

For scripts, `--json` prints one `dutchman.sync-event/1` object per line, as
`dutchman watch --json` does:

```json
{"schema":"dutchman.sync-event/1","event":"updated","path":"bracket.sc","elapsed_ms":8120,"message":"","report":{"ok":true}}
```

`event` is `unchanged`, `updated`, `failed` or `source_missing`; `path` is the design relative to
the folder given; `report` is the conversion's `dutchman.convert/1` report for `updated` and
`failed`. With `--watch`, `unchanged` is written once for each design when the watch starts, then
only changes.

The checks in SolidCraft run as in a normal conversion; pass `--no-verify` to skip them.

## The Dutchman app (coming)

The app will list the designs in a chosen output folder that changed since they were converted,
with **Re-convert changed**, and will have a **Keep this folder up to date** switch for each
output folder.

## Dutchman Sync (coming)

A small background helper with its own icon, in the system tray or menu bar, will run the same
conversions without a terminal, show each design as up to date, source changed, updating, failed
or source missing, and notify you when a design is updated or an update fails.

## The SolidCraft plugin (once SolidCraft linked imports exist)

When SolidCraft can link an imported design to its source file
([tangency-labs/solidcraft#20799](https://github.com/tangency-labs/solidcraft/issues/20799)),
the same record will let a plugin inside SolidCraft refresh the design itself. A plugin has
limits (memory, fuel, no network), which is why the command line route stays.
