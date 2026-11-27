# Keep a converted design up to date

When the CAD file you converted changes, the converted `.sc` should follow it without you
copying anything by hand. SolidCraft keeps placements and joints when a design it has placed
with **Insert Design** is replaced by a new version of the same design, as long as the
component, body and feature names and ids stay the same. Dutchman gives the same names and ids
every time it converts the same source, so a refresh keeps everything attached.

There are three routes. Today the command line and Dutchman Sync work; the app's list and switch are coming.

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

## Dutchman Sync

Dutchman Sync is a small background program that runs the same checks and conversions as
`dutchman sync --watch`, without a terminal. It is installed with Dutchman and has its own
launcher entry and icon: **Dutchman Sync** in the Start menu on Windows, `Dutchman Sync.app` in
Applications on macOS (it comes on the same disk image), and **Dutchman Sync** in the app list on
Linux (the Arch package and the `dutchman-sync` program in the archives; from the AppImage, run
`Dutchman.AppImage --run dutchman-sync`, or use the **Dutchman Sync** action of its launcher).

The first time you start it, one small window opens: **Choose a folder of CAD files to keep
converted**. Choose the source folder (where your CAD files are) and the output folder (where
the SolidCraft designs go) and click **Start**. Sync converts the files that have no design yet,
then keeps every design up to date. After that Sync lives in the system tray (the menu bar on
macOS) and the window closes.

The tray menu has:

| Item | What it does |
| --- | --- |
| **Pause** / **Resume** | Stops checking and converting, or starts again. |
| **Add folder** | Opens the window to choose another pair of folders. |
| **Open Dutchman** | Opens the Dutchman app. |
| **Quit Dutchman Sync** | Stops Sync and turns every folder off, so the app's switch shows off. |

Clicking the icon (on Windows and Linux) opens the window. On a Linux desktop that shows no tray
icons (GNOME without an AppIndicator extension), Sync still runs; start it again to open its window.

- **Notifications.** A desktop notification appears when a design is updated, when an update
  fails (the previous design is kept), and, for a folder with auto-update off, when a source
  changed.
- **Auto-update off.** For a folder whose `auto_update` is off, Sync only detects a changed
  source and tells you; it converts nothing until the setting is turned on.
- **Start at login.** The window has a **Start Dutchman Sync when I log in** checkbox, off by
  default. Turning it on adds a value to the per-user Run key (Windows), a LaunchAgent in
  `~/Library/LaunchAgents` (macOS), or a file in `~/.config/autostart` (Linux); turning it off
  removes it. A copy started at login opens no window.
- **One copy.** Starting Dutchman Sync again, from the launcher or the command line, opens the
  running copy's window and exits; it never starts a second copy.

The app, the tray and the launcher agree through one file, `dutchman/sync-state.json` in the
per-user config folder (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
`$XDG_CONFIG_HOME` or `~/.config` on Linux), schema `dutchman.sync-state/1`:

```json
{
  "schema": "dutchman.sync-state/1",
  "folders": [
    {"output": "/home/me/designs", "source": "/home/me/cad", "auto_update": true,
     "enabled": true, "last_checked": "2026-11-22T09:30:00Z"}
  ],
  "paused": false,
  "start_at_login": false
}
```

A missing or unreadable file reads as no folders. Sync reads the file about once a second, so a
folder turned on or off elsewhere takes effect at once. The window that lists every design with
its status (up to date, source changed, updating, failed, source missing) and an **Update**
button is still to come.

## The SolidCraft plugin (once SolidCraft linked imports exist)

When SolidCraft can link an imported design to its source file
([tangency-labs/solidcraft#20799](https://github.com/tangency-labs/solidcraft/issues/20799)),
the same record will let a plugin inside SolidCraft refresh the design itself. A plugin has
limits (memory, fuel, no network), which is why the command line route stays.
