# Install

With [Homebrew](https://brew.sh) on macOS or Linux:

```sh
brew install dutchman-cad/tap/dutchman
```

With [Scoop](https://scoop.sh) on Windows:

```powershell
scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket
scoop install dutchman-cad/dutchman
```

Both install `dutchman`, `dutchman-mcp`, and the desktop app (`dutchman-gui`).
Scoop also adds a Start menu shortcut.

- **Update:** `brew upgrade` or `scoop update dutchman` installs the newest package.
- **macOS app:** `brew install --cask dutchman-cad/tap/dutchman-convert` adds
  **Dutchman Convert.app** to Applications and Launchpad. Approve its
  [first run](guide.md#opening-the-app-the-first-time) once.
- **Homebrew commands:** ready-made bottles need no Xcode or Command Line Tools.
  Homebrew's command downloads are not quarantined and need no first-run approval.
- **Containers:** use [Docker](docker.md), with
  `docker pull ghcr.io/dutchman-cad/dutchman:latest`.
- **Source and nightly builds:** see [build instructions](building.md) and
  [release channels](guide.md#channels-stable-and-nightly).

## Direct downloads

<!-- downloads -->
| Your computer | Download | Checksum |
|---|---|---|
| Mac (any Mac): the desktop app | [dutchman-macos-universal.dmg](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.dmg) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.dmg.sha256) |
| Mac command line (any Mac) | [dutchman-macos-universal.tar.gz](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-macos-universal.tar.gz.sha256) |
| Windows, 64-bit | [dutchman-x86_64-pc-windows-msvc.zip](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-pc-windows-msvc.zip) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-pc-windows-msvc.zip.sha256) |
| Linux, x86_64 | [dutchman-x86_64-unknown-linux-gnu.tar.gz](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-unknown-linux-gnu.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-x86_64-unknown-linux-gnu.tar.gz.sha256) |
| Linux, ARM 64-bit (aarch64) | [dutchman-aarch64-unknown-linux-gnu.tar.gz](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-aarch64-unknown-linux-gnu.tar.gz) | [SHA-256](https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-aarch64-unknown-linux-gnu.tar.gz.sha256) |
<!-- /downloads -->

Each archive holds `dutchman`, `dutchman-mcp`, and the desktop app, Dutchman Convert
(`dutchman-gui`).
[Getting started](guide.md) shows how to check it, install it, and convert a sample in a
minute. Every release is on the [releases page](https://github.com/dutchman-cad/dutchman/releases).

## Check a download

Download the matching `.sha256` file from the table before extracting the archive.
On Linux, run `sha256sum -c FILE.sha256`; on macOS, run `shasum -a 256 -c FILE.sha256`.
On Windows, compare `Get-FileHash -Algorithm SHA256 FILE` with `Get-Content FILE.sha256`.
Replace `FILE` with the downloaded archive's name.

See the [archive walkthrough](guide.md#from-a-release-archive) for extraction and `PATH`
setup, or [build from source](building.md).
