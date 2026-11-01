# The desktop app

## Project inputs

Files whose names start with `~$` are lock files. They are ignored in project planning
and excluded from job, failure and skipped-file counts.

ZIP directory data and ZIP64 extensible records are limited to 16 MiB each before
the ZIP directory is allocated; entry budgets count every declared entry.

Drop a CAD file, a folder, or a project `.zip`. The card shows assembly, part and STEP
counts before conversion. A new folder beside the input, named `Project (SolidCraft)`,
is selected automatically; **Choose…** can select another output folder. Existing folders
receive a numbered alternative.

ZIP projects unpack into a private temporary folder, limited to 10,000 entries and 4 GiB
of extracted data. Paths outside the project are refused and symbolic links are skipped.
Temporary files are removed when the conversion and its retained retry rows are released.
Inventor assemblies convert from their top-level roots with referenced parts resolved
inside the project; unused parts convert separately. SolidWorks assemblies are not read
yet: the plan says so and their parts convert on their own. A same-name STEP export beside
a native file is kept as a fallback if the native conversion fails.

The Inventor project folder setting appears only for inputs containing Inventor files.
Available memory is labelled as memory and hidden when unknown. Automatic SolidCraft
selection compares the versions found on PATH and in installed applications, choosing
the newest; `SOLIDCRAFT_CLI` remains an explicit override.

Dutchman Convert (`dutchman-gui`) converts without a terminal. [How to use it](../README.md#how-to-use-it) shows it step by step.

## Converting

Open `dutchman-gui`, add design files, folders or project ZIPs, and press
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
and times below 60 seconds show **under a minute**, just as in the CLI. **M GB of memory free** appears
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
0.25 patch 1 through 0.29 currently need no such notice. The notice never blocks conversion.
The pill shows the detected release, or **SolidCraft not found** with a download link in its
panel. **Advanced > SolidCraft version…** opens the same panel. Narrow windows show only
the version or **not found**, shorten **Update**, and put theme choices in a menu.
For an installed release newer than known, the panel has an **Update Dutchman** button, which
opens the update window, when the update check already knows a newer Dutchman. Too-old releases have a
[Download SolidCraft](https://github.com/tangency-labs/solidcraft/releases) link.

Choose the target with the **Target SolidCraft** selector: it defaults to the found CLI's version, else the built-in newest (currently 0.29), even when history checking is off. **Automatic** follows the detected installation; **0.25 and newer** keeps supported FreeCAD helices as editable coils.

## Reporting a problem

The window's title and top row show its version beside **Dutchman Convert**, for example
**v0.2.6**; a nightly shows its nightly version. **Report a problem**, at the top right beside the
**System / Day / Night** theme switch, opens the
[A file didn't convert](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form. `dutchman-gui --version` and `dutchman-gui --help` print and exit without opening a
window (on Windows, read the version with `dutchman --version`).

When a file fails, its card offers two routes. To send the file, follow the steps on the card:
join the Discord server, then **Copy and open DM** copies a one-line message and opens Dutchy's
DM. A file within Discord's upload limit is dragged in, and its folder opens for that; for a
larger one, Dutchy replies with a private upload link. See
[Send the file privately](troubleshooting.md#send-the-file-privately).

**Report anonymously on GitHub** sends no file and no names. It opens the same form in your
browser with Dutchman's version, your system and the error's stage, upstream and code filled
in, and copies the file's anonymous report, which names no file and no folder. Paste it into
the form's **Anonymous report for this file** box and read it before you submit. Never attach
the design; a link to a file that is already public is welcome. The link stays under 2,000
characters so GitHub opens it while you are signed in.

The card takes the invite, the limits, and the message from a small policy built into the app.
Once a day at most, where update checks run, the app reads a newer copy from
<https://dutchman-cad.github.io/report-policy.json> in the background. The request says nothing
about your files. `DUTCHMAN_UPDATE_CHECK=0`, turning off automatic update checks, and source
builds all stop it, and the built-in copy is used.

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

## Diagnose a failed file

Choose **Diagnose this file** on a failed conversion card. The wizard runs the same local
narrowing as `dutchman diagnose --narrow`, showing numbered steps and feature progress.
Its checkpoint and reports live beside the source, in `<filename>.diagnosis/diagnose`.
After a crash or restart, choose the action for that same file again to resume. An open
wizard also has **Resume diagnosis** to rerun from its checkpoint.

Follow the recipe for your CAD program, then choose **Check my small file** and pick the
new file. The result says whether it fails the same way, converts fine, or differs.
**Make the report** becomes available after a matching or explicitly broad check. It writes
a local ZIP containing the small file, its signature and anonymous.json. **Open diagnosis
folder** lets you review it. The ZIP contains geometry; share it only if you are free to
share that small file. Nothing is sent by the wizard.

## Check a folder and matching STEP

Use **Check a folder** to run local folder diagnosis with concurrent-design and total
memory controls. Completed folder checks resume on the next run. Results and converted
designs are kept in a sibling `(Doctor)` folder. Only the top-level `report.json` is
anonymous; keep all other output private. **Add matching STEP** on a result card opens
local checking for that file. A same-name STEP is shown automatically; choose another
export if needed, set a numbered report target, then click **Start local checks**.
Comparisons are advisory measurements, not a rendered visual proof. Nothing is sent.

When the installed SolidCraft CLI supports snapshots, flagged comparisons show stored
source and matching STEP or converted geometry side by side. These are whole-design
views. On a file check, click **Target part N** to save the report target. Comparison
body numbers still need local resolution to the correct assembly part.
