# Releasing

A release is cut from `main` once CI is green there.

1. Move the `CHANGELOG.md` entry from "unreleased" to the release date, and set the workspace
   `version` in `Cargo.toml`, along with the `version` of the `dutchman-core` and
   `dutchman-solidcraft` workspace dependencies. Run `cargo build` so `Cargo.lock` follows.
2. Merge that, then tag the merge commit `vX.Y.Z` and push the tag.
3. The `Release builds` workflow on `main` keeps each platform's archive (`dutchman` and
   `dutchman-mcp`, with LICENSE, NOTICE, README and a SHA-256) as workflow artifacts. Attach
   them to a GitHub release for the tag, with the changelog entry as its notes.
4. crates.io: releases ship as GitHub release archives only, until the core API settles. The
   crates.io `dutchman` crate stays the 0.0.1 placeholder. When crates are published, the
   order matters: `dutchman` depends on `dutchman-core` and `dutchman-solidcraft`, so those go
   first (`cargo publish -p dutchman-core`, then `-p dutchman-solidcraft`, then `-p dutchman`).
   Both are `publish = false` today, and publishing them makes their APIs public. Run
   `cargo publish --dry-run -p <crate>` before each.

## Stable and nightly

A stable release depends only on cadmpeg versions published on crates.io; it never pins a
cadmpeg git commit. A fix that is merged on cadmpeg's `main` but not yet released reaches
users through the nightly channel: the `nightly` pre-release, rebuilt each day from Dutchman's
`main` against cadmpeg's `main` (see `.github/workflows/nightly.yml`). The stable release
that follows cadmpeg's next release carries it.
