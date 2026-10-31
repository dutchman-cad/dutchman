# The desktop app

Dutchman Convert (`dutchman-gui`) converts without a terminal. [How to use it](../README.md#how-to-use-it) shows it step by step.

## Converting

Open `dutchman-gui`, add design files or folders, choose an output folder, and press
**Convert**. Convert stays visible while you scroll through settings or results. The
**Preset** selector starts at **Standard**:

- **Fast**: shape only, least memory.
- **Standard**: history, sketches, and joints; leaves history out if it would
  not fit in free memory.
- **Full**: everything, most memory.

Under **Advanced**, **Feature history**, **Sketches**, and **Joints** let you leave content
out of the selected preset. A changed checkbox makes the selector read **Custom (based on
Standard)** or **Custom (based on Full)**. Choosing a preset again resets these switches.
Fast already leaves all three out; choose Standard or Full to enable them. These choices
are remembered between launches. Older `import_only` settings restore with **Feature
history** unchecked and Standard as the base.

Queued files show **about N GB, about M min** when an estimate is available. Files in queued
folders are listed with their estimates too. Estimates below 0.1 GB show **under 0.1 GB**,
and times below 60 seconds show **under a minute**, just as in the CLI. **M GB free** appears
once when the machine's free memory can be read. These are estimates, not limits or promises; an unavailable estimate
leaves no label. Planning and estimation run in the background.

Before a conversion or retry, Dutchman checks each file's estimate against free memory.
If one is too large, a dialog shows only its file name, without the folder path:
“`<file name>` may use about `<N GB>` of memory while converting. Your computer has about
`<M GB>` free, so it may not have enough.” A separate line advises using the lighter preset
when one is available. All dialog buttons appear in one horizontal row.
**Use Fast** or **Use Standard** selects the offered preset and checks the files again.
**Convert anyway** skips the memory check for this run only; **Cancel** starts no conversion.
When Fast is already selected, the dialog offers only Convert anyway and Cancel.
When Standard leaves history out to fit in memory, its result card shows the
history Info message even with the full loss log collapsed. The original geometry is kept.

A converted design opens in SolidCraft from its card: the app that `SOLIDCRAFT_APP` names, else
`solidcraft` on `PATH`, else the system's app for `.sc` files. When an Inventor assembly's part
files are not found, its card names them: **Search a folder…** or **Find a file…** converts it
again, searching there too. The app follows the system's light or dark setting, or can be set to
Day or Night, and its **Output format** choice also offers a FreeCAD macro.

## SolidCraft version

The header’s SolidCraft status pill opens a small panel with **Target SolidCraft** and a
notice explaining when the installed SolidCraft is newer than Dutchman knows, too old
(before minor 0.18), or older and the selected target loses editable history. Only the last case suggests newer SolidCraft for more history;
0.25 patch 1 through 0.28 currently need no such notice. The notice never blocks conversion.
The pill shows the detected release, or **SolidCraft not found** with a download link in its
panel. **Advanced > SolidCraft version…** opens the same panel. Narrow windows show only
the version or **not found**, shorten **Update**, and put theme choices in a menu.
For an installed release newer than known, the panel has an **Update Dutchman** button, which
opens the update window, when the update check already knows a newer Dutchman. Too-old releases have a
[Download SolidCraft](https://github.com/tangency-labs/solidcraft/releases) link.

Choose the target with the **Target SolidCraft** selector: it defaults to the found CLI's version, else the built-in newest (currently 0.28), even when history checking is off. **Automatic** follows the detected installation; **0.25 and newer** keeps supported FreeCAD helices as editable coils.

## Reporting a problem

The window's title shows its version. **Report a problem**, at the top right beside the
**System / Day / Night** theme switch, opens the
[A file didn't convert](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form. `dutchman-gui --version` and `dutchman-gui --help` print and exit without opening a
window (on Windows, read the version with `dutchman --version`).

When a file fails, its card says so and offers **Report this on GitHub**. It opens the same
form in your browser with Dutchman's version, your system and the error's stage, upstream and
code filled in, and copies the file's anonymous report, which names no file and no folder.
Paste it into the form's **Anonymous report for this file** box and read it before you
submit. Never attach the design; a link to a file that is already public is welcome. The
link stays under 2,000 characters so GitHub opens it while you are signed in.

## Updates

Release builds check for updates at startup and every hour while open. A small **Update**
button beside the theme switch opens the new version and plain-text release notes; an available update never
opens a window automatically. **Download** opens the package for this platform (or the release page).
Homebrew and Scoop installations offer **Update with Homebrew** or **Update with Scoop**.
Homebrew runs `brew update` followed by `brew upgrade --cask dutchman-cad/tap/dutchman-convert`
(or `brew upgrade dutchman-cad/tap/dutchman` for the formula), with
`HOMEBREW_NO_AUTO_UPDATE=1` for the upgrade. The window shows progress and the last 12 output
lines. Closing the update window keeps Homebrew running; reopening it shows the same progress.
After success, **Restart Dutchman** opens the installed app and closes this copy. Homebrew can
replace the running app on macOS.

Scoop needs Dutchman to close before Windows can replace its executable. **Update with Scoop**
first explains the hand-off; **Update and restart** confirms it, and **Cancel** leaves the app
open. A detached, hidden PowerShell helper waits up to two minutes for Dutchman to exit, runs
`scoop update` then `scoop update dutchman` for the `dutchman-cad/dutchman` package, and opens
Dutchman from its Scoop shim whether the update succeeds or fails. Settings save on normal
close. Finish active conversions before confirming. Output goes to
`%LOCALAPPDATA%\Dutchman\diagnostics\scoop-update.log`. On the next start, a failed or
incomplete result opens the update window with its error, output tail, and **Copy command**;
the log is consumed once and deleted. Success opens no update window.

No package-manager commands run until confirmed. Missing commands, failed commands, and a
15-minute update timeout retain the copy-command fallback. **Copy command** remains a
secondary action for both package managers; close Dutchman before running it with Scoop.
**Skip this version** hides that release; **Remind me later** hides updates for three days.
These choices persist in the desktop settings. **Settings** offers **Check for updates
automatically** (on by default) and **Check now**, which also shows skipped releases.
`DUTCHMAN_UPDATE_CHECK=0` disables all checks, including **Check now**. Builds from source
never check; only release-workflow builds enable this feature.

## Installing it

On macOS it comes as **Dutchman Convert.app** in a disk image: open it and drag it onto Applications in the window that appears (see
[opening it the first time](install.md#opening-the-app-the-first-time): approve it once
and the commands inside it run too). It ships in the
release archives beside `dutchman`, or build it from source with Rust 1.88 or newer:

```sh
cargo install --git https://github.com/dutchman-cad/dutchman dutchman-gui
```

On macOS, let SolidCraft itself run once before the first check: open SolidCraft from
Applications, click **Done** when macOS stops it, then click **Open Anyway** in **System
Settings > Privacy & Security**, the same steps as for Dutchman Convert. Dutchman reads the
app's quarantine attribute and its user-approved flag without changing them. The desktop app
retries a blocked check when its window regains focus or you start a conversion.

On Linux it needs a desktop session (X11 or Wayland with OpenGL); the file dialogs use the
desktop portal, and dropping files works without it.

## What the update check sends

For direct downloads, each check sends an HTTPS GET to
`https://github.com/dutchman-cad/dutchman/releases.atom` with
`User-Agent: Dutchman-update-check/<version>` and, on repeat checks, `If-None-Match`
containing the feed ETag GitHub returned. An unchanged feed (304) needs no API request.
Only a newer stable `vX.Y.Z` release triggers one GET to
`https://api.github.com/repos/dutchman-cad/dutchman/releases/latest` for package links;
if the feed fails, one API request is tried instead, then the next automatic check is in
an hour.

For package-manager installations, the hourly check first sends an HTTPS GET to exactly one
of these public package files, using the same User-Agent and a conditional `If-None-Match`
header with that file's ETag:

- Homebrew cask: `https://raw.githubusercontent.com/dutchman-cad/homebrew-tap/main/Casks/dutchman-convert.rb`
- Homebrew formula: `https://raw.githubusercontent.com/dutchman-cad/homebrew-tap/main/Formula/dutchman.rb`
- Scoop: `https://raw.githubusercontent.com/dutchman-cad/scoop-bucket/main/bucket/dutchman.json`

An unchanged package file (304) causes no further request. Only a package version newer than
the version already known triggers a releases-feed request for that exact version's notes;
no releases API request is needed. The pill appears only once the package has the newer
stable version. If its feed entry is missing, the window links to the release page.
Package metadata failures retry in an hour without offering a feed-only update.

These update-check requests send no cookies, identifiers, usage data, file names, paths, or
file contents. Clicking an update button also runs the package manager, which contacts its
own configured package sources and download hosts using its own network and privacy settings. GitHub sees the connecting IP address, ordinary HTTPS request metadata,
the app version in the User-Agent, and the public feed or package ETag. Clicking **Download** opens
the release asset or page in the default browser, under that browser's privacy settings.
