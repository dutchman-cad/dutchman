# Releasing

A release is cut from `main` once CI is green there.

1. Move the `CHANGELOG.md` entry from "unreleased" to the release date, and set the workspace
   `version` in `Cargo.toml`, along with the `version` of the `dutchman-core` and
   `dutchman-solidcraft` workspace dependencies. Run `cargo build` so `Cargo.lock` follows.
2. Merge that, then tag the merge commit `vX.Y.Z` and push the tag.
3. The `Release builds` workflow on `main` keeps each platform's archive (`dutchman` and
   `dutchman-mcp`, with LICENSE, NOTICE, README and a SHA-256) as workflow artifacts. Attach
   them to a GitHub release for the tag, with the changelog entry as its notes.
4. crates.io: the `dutchman` crate depends on `dutchman-core` and `dutchman-solidcraft`, so
   those are published first, in that order (`cargo publish -p dutchman-core`, then
   `-p dutchman-solidcraft`, then `-p dutchman`). Both are `publish = false` today; publishing
   them means removing that line and accepting their APIs as public. Run
   `cargo publish --dry-run -p <crate>` before each.
