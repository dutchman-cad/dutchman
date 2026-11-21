# Working on Dutchman with an AI agent

A task brief or issue that states its own rule for its work takes precedence over this file.
[CONTRIBUTING.md](CONTRIBUTING.md) is the full guide; this page is the short version for agents.

Format-specific guides: [Solid Edge](docs/agents/solid-edge.md).

## Build and test

Rust 1.88 or newer.

```sh
cargo build --release
```

Before opening a pull request, run the checks CI runs:

```sh
scripts/solidcraft-window.sh --print
python3 scripts/check-version-gates.py --base HEAD
python3 scripts/check-test-accept.py
python3 -m unittest discover -s scripts/tests -v
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The whole workspace build is large. On a small machine, test only the crates you changed
(`cargo test -p <crate>`) and let CI run the rest. SolidCraft round trips need a SolidCraft
release's `solidcraft-cli`:
`SOLIDCRAFT_CLI=/path/to/solidcraft-cli cargo test -p dutchman-solidcraft --test history --test assembly`.

## Code rules

- Never crash on a file: no `unwrap`, `expect`, `panic!` or slice indexing outside tests, no
  `unsafe`. Return `Result`, use `get()` and checked arithmetic, and cap sizes and counts read
  from a file.
- Messages are for people: say what happened and what to do next. Say "not supported yet" for a
  gap; never call a user's file corrupt or invalid.
- JSON reports only gain fields within a schema version; document new fields in
  [docs/json.md](docs/json.md).
- Native decoding belongs in cadmpeg. Reading any part of a native file's bytes is decoding.
- SolidCraft versions: keep the 0.18.2 floor and every existing writer check. New work targets
  only the releases `scripts/solidcraft-window.sh --print` lists; older targets get the
  original geometry with a named reason.
- Keep the default output: a change to what an existing file converts to is opt-in, and a golden
  or placement expectation changes only on purpose, with the reason in the pull request.

## Pull requests

Comment `/claim` before starting, then open a draft pull request within 24 hours, even rough.
After that, claims lapse after 5 inactive days unless waiting on a maintainer or an open
dependency. Maintainer claims never lapse; `/unclaim` releases a claim ([rules](docs/claiming.md)).

1. Branch from `main`; one topic per pull request, with tests for what changed.
2. Update the docs the change affects: [docs/guide.md](docs/guide.md),
   [docs/mcp.md](docs/mcp.md), [docs/json.md](docs/json.md), the format pages, and
   [skills/](skills/) when a tool or message the skill describes changes.
3. Add a [changelog.d/](changelog.d/) fragment (one allowed heading, then one to three short
   bullets for users); never edit `CHANGELOG.md`. Check with `python3 scripts/changelog.py check`.
4. Title and commit messages in the imperative, sentence case, saying what changes for the user.
5. Open it against `main` and make sure CI passes. If it needs an unmerged cadmpeg change,
   keep it a draft until Dutchman's pin includes that change.

## Test files

- Tests use synthetic files only: built in code, generated with cadmpeg's encoders, or copied
  from cadmpeg's public test suite, with the origin written in
  `crates/dutchman-core/tests/fixtures/README.md`.
- Never add third-party or private CAD files (model libraries, vendors, employers, customers,
  someone else's designs). A bug found on a private file gets a test with generic dimensions
  and synthetic geometry: never copy a private design's numbers, names or shapes.
- Never add material from another CAD program's source, SDK samples or help, or anything under a
  confidentiality agreement.
