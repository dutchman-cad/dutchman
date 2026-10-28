# Releasing

A release is cut from `main` once CI is green there.

1. Run `python3 scripts/changelog.py release X.Y.Z` to assemble the fragments into a dated
   `CHANGELOG.md` entry (use `--date YYYY-MM-DD` to override today). Review it, add any known
   issue, and stage the deleted fragments. Set the workspace `version` in `Cargo.toml`, along
   with the `version` of the `dutchman-core` and `dutchman-solidcraft` workspace dependencies.
   Run `cargo build` so `Cargo.lock` follows. Leave the README's download table alone: it
   links the latest published release's version-less files (`releases/latest/download/`),
   which the release build uploads beside the versioned ones, so it follows a release only
   once that release is published (a test checks the table). Write `release-notes/X.Y.Z.md`: one intro paragraph,
   then `## What's new` with at most 7 bullets, each with a bold lead phrase. Use plain
   words and Oxford commas; describe the highlights rather than copying the CHANGELOG.
   Add `## Known issue` if needed. Images are kept for milestones, such as a design that now
   converts fully: commit one under `docs/images/releases/X.Y.Z/` (from a public, licensed
   file) and place it after the intro as
   `![description](https://raw.githubusercontent.com/dutchman-cad/dutchman/vX.Y.Z/docs/images/releases/X.Y.Z/<file>)`,
   so the release page and announcements show the file at the tag. The release-notes check
   fails when a shown image has no alt text or its file is not committed.
   Preview the full body with
   `python3 scripts/release-notes.py X.Y.Z`; it adds the downloads, install commands, and
   a link to the dated CHANGELOG entry at the tag.
2. Merge that, then tag the merge commit `vX.Y.Z` and push the tag.
3. Pushing the tag runs the `Release builds` workflow. It builds each platform's archive
   (`scripts/package-archive.sh`: `dutchman`, `dutchman-mcp` and `dutchman-gui`, plus `dutchman-web` on Linux, with LICENSE,
   NOTICE, README and a SHA-256), and the macOS disk image (`scripts/macos-dmg.sh`:
   Dutchman Convert.app, universal, signed ad hoc), as workflow artifacts. The two Mac targets
   build in parallel on separate Apple silicon runners (Intel is cross-compiled). A small
   packaging job checks and unpacks both archives, then uses their binaries to build the
   universal app with `lipo` and make the disk image, without compiling again. After all builds
   succeed, the workflow checks every SHA-256 and creates a draft GitHub release from `release-notes/X.Y.Z.md`, with the archives and
   checksums attached. It fails if a release for that tag already exists. A maintainer
   verifies the body and downloads, then publishes the release. Before publishing, check
   that the draft shows tag `vX.Y.Z`, not `untagged-…`.
   The workflow also runs on demand (`gh workflow run release-build.yml`) and for a pull
   request that changes the packaging, but those runs keep only workflow artifacts and
   do not create a draft. It does not run for every push to `main`.
   Publishing the release (not the tag or the draft) runs `Release image`
   (`release-image.yml`), which publishes `ghcr.io/dutchman-cad/dutchman:X.Y.Z` and `:latest`
   from the published release's checksum-verified Linux archives, with no second Rust build;
   a draft never reaches the registry. To republish a release's image, run it on demand with
   that tag (`gh workflow run release-image.yml -f tag=vX.Y.Z`). On-demand and
   packaging pull-request runs build both image architectures without logging in or pushing.
4. crates.io: releases ship as GitHub release archives only, until the core API settles. The
   crates.io `dutchman` crate stays the 0.0.1 placeholder. When crates are published, the
   order matters: `dutchman` depends on `dutchman-core` and `dutchman-solidcraft`, so those go
   first (`cargo publish -p dutchman-core`, then `-p dutchman-solidcraft`, then `-p dutchman`).
   Both are `publish = false` today, and publishing them makes their APIs public. Run
   `cargo publish --dry-run -p <crate>` before each.

## Packaging

Linux release and nightly archives include `dutchman-web`; it needs no desktop libraries.
Windows and macOS archives currently leave it out: native builds of the web server have not
yet been verified there. Build it from source on those platforms until that check is complete.

`scripts/macos-dmg.sh` builds the universal app and a compressed, read-only HFS+ disk
image. The installation window opens in icon view with about 640 × 440 points of content,
96-point app and Applications icons under the ends of a teal arrow, and the read-me below
them. The toolbar, status bar and sidebar are hidden.

The original background and its provenance live in `assets/dmg/`; regenerate both PNG
sizes from the SVG using the Rust example documented there. Packaging uses macOS `sips`
and `tiffutil` to combine @1x and @2x images into a hidden multi-resolution TIFF.
It needs no additional packaging dependencies.

macOS does not provide a standard headless writer for Finder's `.DS_Store` layout.
The script mounts a temporary writable image and asks Finder to save the view, positions
and background before detaching and compressing it. Finder needs a logged-in session and
permission for the build to control it (GitHub's macOS runners have both). Finder has a
30-second request timeout and an outer 45-second process limit. If styling fails, including when Finder is
unavailable or automation access is denied, the script discards that image and builds
the plain app, Applications link and read-me image from the untouched staging folder.
Image creation, signing, detaching and final `hdiutil verify` errors still stop packaging.

On macOS, mount the finished image afresh, check the icon positions and background in
both appearances and on Retina and standard-density displays, open the read-me, then
drag the app into Applications and launch it. Eject and remount to check persistence.
Also build with a failing `osascript` earlier on `PATH` to check that the plain fallback
still mounts, retains all three items and passes `hdiutil verify`.

## Stable and nightly

A stable release depends on cadmpeg at one commit of its `main` branch, pinned as the same
`rev` for every cadmpeg crate in the workspace `Cargo.toml`. Move the pin after a batch of
cadmpeg merges: set the new commit, run `cargo update -p cadmpeg-core -p cadmpeg-ir -p
cadmpeg-registry`, then the full tests, the SolidCraft round trip and the anonymous-report
leak check on a real library before merging it. A fix merged on cadmpeg's `main` after the
pinned commit reaches users through the nightly channel: the `nightly` pre-release, rebuilt
each day from Dutchman's `main` against cadmpeg's `main` (`scripts/use-cadmpeg-main.sh`, see
`.github/workflows/nightly.yml`). The stable release after the pin moves carries it.

Nightly and release macOS builds use the same release profile, target triples, packages, and
default features, with one shared Rust cache key per target (`macos-release-<target>`).
Main's nightly saves caches that tag builds can restore. The nightly restores the cache before
updating cadmpeg so its key reflects the same pinned manifests as a release from that commit;
Cargo still rebuilds dependencies changed by the nightly update. A different Rust toolchain,
changed dependencies, or an absent cache can still require a cold build.

All cadmpeg crates, including cadmpeg-registry, come directly from the pinned git revision.
`scripts/use-cadmpeg-main.sh <sha>` updates the dependencies and checks their lockfile sources.

## Container image

The Linux archive builds run in Debian bookworm so their glibc requirements fit the
`debian:bookworm-slim` runtime. `docker/release.Dockerfile` installs the CLI and MCP
server, plus `dutchman-web` whenever it is included in the archives, and retains the
licence files. Image licence metadata comes from `Cargo.toml`. Only the image publishing
jobs receive `packages: write`. The nightly image job waits for the nightly pre-release
publication to succeed, then publishes `ghcr.io/dutchman-cad/dutchman:nightly` from the
same run's Linux artifacts.

Check the image job in the workflow, then check the published manifest and executable:

```sh
docker buildx imagetools inspect ghcr.io/dutchman-cad/dutchman:X.Y.Z
docker run --rm ghcr.io/dutchman-cad/dutchman:X.Y.Z --version
```

The manifest must include `linux/amd64` and `linux/arm64`; test `--version` on each
architecture. On first publication, check that the GHCR package is public so anonymous
pulls work. See [Docker](docker.md) for conversion, MCP and Compose examples.
