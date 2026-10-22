# Releasing

A release is cut from `main` once CI is green there.

1. Move the `CHANGELOG.md` entry from "unreleased" to the release date, and set the workspace
   `version` in `Cargo.toml`, along with the `version` of the `dutchman-core` and
   `dutchman-solidcraft` workspace dependencies. Run `cargo build` so `Cargo.lock` follows, and
   `scripts/downloads.py X.Y.Z README.md` so the README's download table links the new
   archives (a test checks it does).
2. Merge that, then tag the merge commit `vX.Y.Z` and push the tag.
3. Pushing the tag runs the `Release builds` workflow. It keeps each platform's archive
   (`scripts/package-archive.sh`: `dutchman`, `dutchman-mcp` and `dutchman-gui`, with LICENSE,
   NOTICE, README and a SHA-256), and the macOS disk image (`scripts/macos-dmg.sh`:
   Dutchman.app, universal, signed ad hoc), as workflow artifacts; both Mac targets build on
   one Apple silicon runner. Attach them to a GitHub release for the tag. Its notes start with
   the download table, `scripts/downloads.py X.Y.Z`, then the changelog entry. The workflow
   also runs on demand (`gh workflow run release-build.yml`) and for a pull request that
   changes the packaging, but not for every push to `main`.
4. crates.io: releases ship as GitHub release archives only, until the core API settles. The
   crates.io `dutchman` crate stays the 0.0.1 placeholder. When crates are published, the
   order matters: `dutchman` depends on `dutchman-core` and `dutchman-solidcraft`, so those go
   first (`cargo publish -p dutchman-core`, then `-p dutchman-solidcraft`, then `-p dutchman`).
   Both are `publish = false` today, and publishing them makes their APIs public. Run
   `cargo publish --dry-run -p <crate>` before each.

## Stable and nightly

A stable release depends on cadmpeg at one commit of its `main` branch, pinned as the same
`rev` for every cadmpeg crate in the workspace `Cargo.toml`. Move the pin after a batch of
cadmpeg merges: set the new commit, run `cargo update -p cadmpeg-core -p cadmpeg-ir -p
cadmpeg-registry`, then the full tests, the SolidCraft round trip and the anonymous-report
leak check on a real library before merging it. A fix merged on cadmpeg's `main` after the
pinned commit reaches users through the nightly channel: the `nightly` pre-release, rebuilt
each day from Dutchman's `main` against cadmpeg's `main` (`scripts/use-cadmpeg-main.sh`, see
`.github/workflows/nightly.yml`). The stable release after the pin moves carries it.
