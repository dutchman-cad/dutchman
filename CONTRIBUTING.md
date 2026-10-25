# Contributing to Dutchman

Thank you for helping. Dutchman converts native CAD files into SolidCraft designs; reading the
native files is done by [cadmpeg](https://github.com/cadmpeg/cadmpeg). Bug reports, small
synthetic test files and pull requests are all welcome.

## Reporting a problem

Start with an anonymous report:

```sh
dutchman convert-all --out-dir converted --report-anon report-anon.json part.f3d
```

Replace `part.f3d` with your file or folder. Review `report-anon.json`, then open an issue
with the cause, counts, and versions it records. Ordinary logs, JSON output, and journals
contain file names and paths; keep them, the report key, and the salt private.

Do not attach design files publicly. If you are willing to share a small reproducer you
made yourself, say so in the issue and wait for a private way to send it. Do not share designs
from your employer or customers, or models downloaded from libraries. See
[Testing Dutchman on your library](docs/testing-your-library.md#5-report-what-you-found).

Always report it here, on Dutchman, even when the report names cadmpeg or SolidCraft (an
`upstream` field, or a `f3d/…`, `inventor/…` or `sldprt/…` loss code). Don't file it on cadmpeg
or SolidCraft yourself: we work out where the fix belongs, take it upstream, and tell you here
when a Dutchman release has it. [docs/triage.md](docs/triage.md) says how.

To test Dutchman on a whole library of your own without sharing a file, see
[Testing Dutchman on your library](docs/testing-your-library.md) and its issue form.

## Building and checking

You need Rust 1.88 or newer.

```sh
cargo build --release
```

Before you open a pull request, run the checks CI runs (`.github/workflows/ci.yml`):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI runs the tests on Linux, Windows and macOS, and checks that the workspace builds with
Rust 1.88. A further job opens every converted fixture in a released SolidCraft
(`scripts/solidcraft-roundtrip.sh`) and replays the history tests in it. It is informational
and does not block a merge. To run it locally, put a SolidCraft release's `solidcraft-cli`
where `SOLIDCRAFT_CLI` points:

```sh
SOLIDCRAFT_CLI=/path/to/solidcraft-cli cargo test -p dutchman-solidcraft --test history --test assembly
```

## Code style

- **Never crash.** A damaged or unexpected file must come back as an error, never a panic.
  The workspace lints deny `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`,
  `unreachable!` and slice indexing outside tests, and forbid `unsafe`. Use `Result`, `?`,
  `get()` and checked arithmetic. Every call into cadmpeg is guarded against panics.
- **Treat file contents as hostile.** Cap sizes and loop counts that come from a file.
- **Messages are for people.** Say what happened and what to do next, plainly, in sentence
  case. Say "not supported yet" for a gap in Dutchman or cadmpeg; don't call the user's file
  corrupt or invalid. Use "convert", not "import", for what Dutchman does.
- **JSON reports are stable.** Within a schema version (`dutchman.convert/1`), fields are only
  added. A change that would break a reader needs a new version. Update
  [docs/json.md](docs/json.md) with any field you add.
- **Tests use synthetic files only.** See [Test files](#test-files).

## Pull requests

1. Fork the repository and branch from `main`.
2. Keep each pull request to one topic, with tests for what changed.
3. Update the docs it affects: [docs/guide.md](docs/guide.md) for commands and messages,
   [docs/mcp.md](docs/mcp.md) for the MCP tools, [docs/json.md](docs/json.md) for report
   fields, the format pages, and [skills/](skills/) when a tool or message the skill
   describes changes.
4. Add a [changelog.d/](changelog.d/) fragment for the change; do not edit `CHANGELOG.md`.
   Run `python3 scripts/changelog.py check` and `python3 scripts/changelog.py preview`.
5. Open the pull request against `main` and make sure CI passes.

Write commit messages and pull request titles in the imperative, in sentence case, saying
what changes for the user: "Map patterns of features", "Convert SolidWorks parts".

## Licence

Dutchman is licensed under the [Apache License, Version 2.0](LICENSE). By contributing, you
agree that your contributions are licensed under the same terms (section 5 of the licence).
There is no separate contributor agreement or sign-off.

## Clean-room rule

Proprietary formats are decoded from public information and from files their owners made
themselves, never from leaked or confidential material, decompiled programs, or someone
else's private designs. Native decoding belongs in cadmpeg, not in Dutchman.

- Don't add code, test files or documentation that comes from another CAD program's source,
  SDK samples or help, or from material under a confidentiality agreement.
- Don't add third-party CAD files: no designs from model libraries, vendors, employers or
  customers.
- New test files are synthetic: built in code, or taken from cadmpeg's public test suite, with
  their origin written down in `crates/dutchman-core/tests/fixtures/README.md`.

## Test files

The files in [`crates/dutchman-core/tests/fixtures/`](crates/dutchman-core/tests/fixtures/)
are synthetic inputs from cadmpeg's test suite, or generated with cadmpeg's own encoders;
none is a real design. Their README records where each came from. Expected results are kept
as golden files in `crates/dutchman-solidcraft/tests/golden/`.

## Moving the cadmpeg pin

Dutchman pins cadmpeg to one commit of its `main` (the `rev` in the workspace `Cargo.toml`).
Move it after a batch of cadmpeg merges, never for one commit alone: run the tests, the
SolidCraft round trip and the anonymous-report leak check first (docs/releasing.md).

## Adding a format

Dutchman never parses a native format itself. A new format starts in cadmpeg: once cadmpeg
publishes a codec for it, Dutchman can convert it.

1. Turn the codec on in the `cadmpeg-registry` features in the workspace `Cargo.toml`.
2. Add the format to `Format` in `crates/dutchman-core/src/format.rs` (its id, extension,
   name, family and `codec_id`), and to `ALL`. Format ids are part of the JSON reports, so
   choose them once.
3. Add its row to `dutchman formats` in `crates/dutchman/src/formats.rs`, saying plainly what
   converts.
4. Add a synthetic fixture with its provenance, and tests: decoding in
   `crates/dutchman-core/tests/decode.rs`, the written design against a golden file in
   `crates/dutchman-solidcraft/tests/end_to_end.rs`. The hostile-input test
   (`crates/dutchman-solidcraft/tests/robustness.rs`) picks up every fixture in that folder
   by itself. Add the fixture to the round-trip job's list in `.github/workflows/ci.yml`.
5. Write `docs/<format>.md`: what converts, what does not yet, and which upstream gaps stand
   in the way. List the new extension in the README, [docs/guide.md](docs/guide.md),
   [docs/json.md](docs/json.md) (`input.format`) and the MCP tool descriptions.
