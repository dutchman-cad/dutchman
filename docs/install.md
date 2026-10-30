# Install

Dutchman installs as three programs: `dutchman` (the command line), `dutchman-mcp` (the MCP server), and `dutchman-gui` (the desktop app, Dutchman Convert). Release archives, the macOS disk image, Homebrew, and Scoop install all three; `cargo install` and Docker leave out the desktop app.

## Homebrew or Scoop

On macOS or Linux with [Homebrew](https://brew.sh):

```sh
brew install dutchman-cad/tap/dutchman
```

On Windows with [Scoop](https://scoop.sh):

```powershell
scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket
scoop install dutchman-cad/dutchman
```

Either installs `dutchman`, `dutchman-mcp`, and the desktop app, `dutchman-gui` (Scoop also adds
a **Dutchman Convert** Start menu shortcut), on your `PATH`. `brew upgrade` or `scoop update`
brings each new release, usually within an hour of it. Homebrew pours a ready-made bottle, so
it needs no Xcode or Command Line Tools. On macOS the commands Homebrew installs open without
the first-run approval below, because Homebrew's downloads are not quarantined.

On macOS, Homebrew also installs the app, **Dutchman Convert.app**, in Applications and
Launchpad:

```sh
brew install --cask dutchman-cad/tap/dutchman-convert
```

Homebrew quarantines the apps it installs, so the app needs the
[first-run approval](#opening-the-app-the-first-time) once, as from the disk image.
[Pilot](https://github.com/dutchman-cad/pilot) installs the same way, as `dutchman-pilot`
(`brew install dutchman-cad/tap/dutchman-pilot`, `scoop install dutchman-cad/dutchman-pilot`).

## macOS: the app

From 0.1.3 on, each release has `dutchman-VERSION-macos-universal.dmg`: **Dutchman
Convert.app** (named **Dutchman.app** in 0.1.5 and earlier), for Apple silicon and Intel Macs
alike, with the `dutchman` and `dutchman-mcp` commands inside it.

1. Open the disk image and drag **Dutchman Convert** onto **Applications**.
2. Open it from Applications or Launchpad, where it is shown as **Dutchman** (the file is
   `Dutchman Convert.app`).

### Opening the app the first time

Dutchman Convert is not signed with an Apple Developer ID, so the first time you open it macOS
stops it with this box:

<img alt="The macOS box: “Dutchman” Not Opened. Apple could not verify “Dutchman” is free of malware that may harm your Mac or compromise your privacy. Buttons: Move to Trash, Done." src="images/macos-not-opened.png" width="260">

Click **Done**, not Move to Trash, then:

1. Open **System Settings** › **Privacy & Security**.
2. Scroll down to **Security**. It says “Dutchman” was blocked to protect your Mac.
3. Click **Open Anyway**, enter your password, then click **Open Anyway** in the box that
   follows.

<img alt="System Settings, Privacy & Security, Security: “Dutchman” was blocked to protect your Mac, with an Open Anyway button." src="images/macos-open-anyway.png" width="450">

macOS remembers the choice; from then on Dutchman Convert opens like any other app. Right-click ›
Open, which older macOS versions offered, no longer gets past the box.

Or clear the download's quarantine flag once, in Terminal:

```sh
xattr -dr com.apple.quarantine "/Applications/Dutchman Convert.app"
```

The app is signed ad hoc, so macOS never calls it “damaged”. If it does, the download was
cut short or changed: download it again and check it against its `.sha256`
(`shasum -a 256 -c dutchman-VERSION-macos-universal.dmg.sha256`).

The commands inside the app (`dutchman`, `dutchman-mcp`) are covered by the same approval.
Run before it, from Terminal, they meet the same box: approve the app once, as above, and they
run too.

To use the commands from Terminal, link them onto your `PATH`:

```sh
sudo ln -sf "/Applications/Dutchman Convert.app/Contents/Helpers/dutchman" /usr/local/bin/dutchman
sudo ln -sf "/Applications/Dutchman Convert.app/Contents/Helpers/dutchman-mcp" /usr/local/bin/dutchman-mcp
```

With 0.1.5 or earlier, write `/Applications/Dutchman.app` in these commands instead.

## From a release archive

Each [release](https://github.com/dutchman-cad/dutchman/releases) has archives for the
platforms below. Each archive holds `dutchman` (the command), `dutchman-mcp` (the
[MCP server](mcp.md)), `dutchman-gui` (the desktop app, from 0.1.1 on), `LICENSE`, `NOTICE`,
`README.md` and the font licences, with a `.sha256` file beside it.

| Platform | Archive |
|---|---|
| Linux, x86_64 | `dutchman-VERSION-x86_64-unknown-linux-gnu.tar.gz` |
| Linux, ARM64 | `dutchman-VERSION-aarch64-unknown-linux-gnu.tar.gz` |
| macOS, Apple silicon or Intel | `dutchman-VERSION-macos-universal.tar.gz` |
| macOS, Apple silicon (per-chip archive) | `dutchman-VERSION-aarch64-apple-darwin.tar.gz` |
| macOS, Intel (per-chip archive) | `dutchman-VERSION-x86_64-apple-darwin.tar.gz` |
| Windows, x86_64 | `dutchman-VERSION-x86_64-pc-windows-msvc.zip` |

The universal macOS archive runs on both Apple silicon and Intel. Per-chip archives remain
available for Homebrew and other installations that select a processor. The README's download
table links the latest release's universal archive and disk image.

Download the archive and its `.sha256` file, check one against the other, then unpack it and
put the programs on your `PATH`. On Linux x86_64, for release 0.2.5 (set `VERSION` and the
target for another release or machine):

```sh
VERSION=0.2.5 TARGET=x86_64-unknown-linux-gnu
base=https://github.com/dutchman-cad/dutchman/releases/download/v$VERSION
curl -fLO "$base/dutchman-$VERSION-$TARGET.tar.gz"
curl -fLO "$base/dutchman-$VERSION-$TARGET.tar.gz.sha256"
sha256sum -c dutchman-$VERSION-$TARGET.tar.gz.sha256
tar xzf dutchman-$VERSION-$TARGET.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 755 dutchman-$VERSION-$TARGET/dutchman* "$HOME/.local/bin/"
export PATH="$HOME/.local/bin:$PATH"
dutchman --version
```

The `export` lasts for this shell; add the same line to your shell's profile (`~/.bashrc`,
`~/.zshrc`) to keep it. The `dutchman*` pattern installs the desktop app, `dutchman-gui`, too.

On Linux, launch the desktop app with `dutchman-gui` from a terminal. It needs an X11 or
Wayland desktop session with working OpenGL. See [Troubleshooting](troubleshooting.md) if no
window appears.

On macOS, `shasum -a 256 -c` takes the place of `sha256sum -c`. The programs are not signed
with a Developer ID, so macOS blocks each the first time, as it does
[the app](#opening-the-app-the-first-time); allow it in System Settings › Privacy & Security,
or clear the quarantine flag:

```sh
xattr -d com.apple.quarantine dutchman dutchman-mcp dutchman-gui
```

On Windows, in PowerShell, compare the hash with the one in the `.sha256` file, then unpack
the archive and add its folder to your `PATH`:

```powershell
Get-FileHash dutchman-VERSION-x86_64-pc-windows-msvc.zip -Algorithm SHA256
Get-Content dutchman-VERSION-x86_64-pc-windows-msvc.zip.sha256
Expand-Archive dutchman-VERSION-x86_64-pc-windows-msvc.zip -DestinationPath .
```

## Channels: stable and nightly

- **Stable** releases (`v0.2.5`, …) are the ones to use. Each is built against one tested
  commit of cadmpeg, the library that reads the native files.
- **Nightly** is one pre-release, [`nightly`](https://github.com/dutchman-cad/dutchman/releases/tag/nightly),
  rebuilt each day from Dutchman's latest code against cadmpeg's latest code, and published
  only when every test passes. Its notes give both commits and the date. Its archives are
  named `dutchman-nightly-TARGET` and install the same way. Use it when an issue says a fix is
  merged but not in a release yet; go back to stable once a stable release has it.

## From source

With Rust 1.88 or newer:

```sh
cargo install --git https://github.com/dutchman-cad/dutchman dutchman
```

This installs both `dutchman` and `dutchman-mcp`. Check the install:

```sh
dutchman --version
```

## Docker

Each release is published as a container image with `dutchman`, `dutchman-mcp` and
`dutchman-web`, without the desktop app, for amd64 and arm64:
`docker pull ghcr.io/dutchman-cad/dutchman:latest`. The repository's `Dockerfile` builds the
same from source. [Docker](docker.md) says how to get it and convert with it.

## Dutchman Web, in a browser

`dutchman-web` serves a page that converts uploaded designs, for a Dutchman hosted on one
server and used from any browser. [Dutchman Web](web.md) says how to run it.

## SolidCraft (optional, recommended)

When a design's feature history converts, Dutchman checks it by replaying it in SolidCraft's
command-line tool, `solidcraft-cli`, which comes with every
[SolidCraft release](https://github.com/tangency-labs/solidcraft/releases). Installing the
SolidCraft app is enough: Dutchman finds the tool inside it (on macOS, the newest
`SolidCraft*.app` in `/Applications` or `~/Applications`; on Windows, `Program Files\SolidCraft`, else the install folder the installer recorded in the
registry).
`--solidcraft PATH`, `SOLIDCRAFT_CLI`, or `solidcraft-cli` on your `PATH` come first. The
desktop app says which one it checks with. Without one, conversions still work; a converted
history is marked unverified.

On macOS, let SolidCraft itself run once before the first check: open SolidCraft from
Applications, click **Done** when macOS stops it, then click **Open Anyway** in **System
Settings > Privacy & Security**, the same steps as for Dutchman Convert. Dutchman reads the
app's quarantine attribute and its user-approved flag without changing them. The desktop app
retries a blocked check when its window regains focus or you start a conversion. Until then
it says so under the check option:

<img alt="Dutchman Convert with macOS's “SolidCraft” Not Opened box over it; under Check feature history in SolidCraft it reads: macOS has not yet allowed SolidCraft to run, with the steps to allow it." src="images/macos-solidcraft-blocked.png" width="450">

Dutchman is made for the current SolidCraft (any release from the last two weeks);
older versions get simplified features, or pass `--solidcraft-version x.y.z` on
`convert`, `convert-all`, or `watch`. The current window is 0.18–0.27; roundtrip
checks cover its oldest and newest releases. Each design targets the found CLI's version,
even with `--no-verify`, else the newest release known at build time (currently 0.27).
Versions below 0.18.2 and malformed versions are refused. Older targets remain accepted
down to 0.18.2. Features that need a newer release block history with a named reason
and preserve the original geometry as for other blockers.
A design opens in its target release and newer releases. A `solidcraft-cli` older than
0.18.2 is not used: the conversion says so ("… is older
than SolidCraft 0.18 patch 2, the oldest that opens and checks Dutchman's designs; install a newer
SolidCraft") and writes the history unverified. SolidCraft 0.19.0 saves designs in a newer
format that older versions do not open, so a design you save again in 0.19.0 opens in 0.19.0
or newer only.

## Building and packaging

```sh
cargo build --release
cargo test --workspace
```

For packagers: the std-only `dutchman-launcher` crate provides a tiny front command for
installations that ship with Pilot alone. Install its binary as `dutchman`;
`dutchman --version` identifies it as `dutchman launcher <version>`. Install Dutchman Convert
for the conversion commands.
