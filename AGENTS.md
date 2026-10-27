# Instructions for AI contributors

Dutchman converts native CAD files (Fusion `.f3d`/`.f3z`, Inventor `.ipt`/`.iam`, SolidWorks
`.sldprt`, FreeCAD `.FCStd`) into SolidCraft designs (`.solidcraft`). It is pure Rust, Apache-2.0. Native files
are decoded by [cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman maps cadmpeg's neutral
model (CADIR) onto SolidCraft. [CONTRIBUTING.md](CONTRIBUTING.md) has the full rules for
everyone; this file is the short version, plus what an AI working in the repository needs.

## Read first

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the pipeline, the crates, the `Writer` seam,
  how history is mapped and checked.
- [docs/json.md](docs/json.md): the JSON reports. Field names are stable within a schema
  version.
- The format pages: [docs/fusion.md](docs/fusion.md), [docs/inventor.md](docs/inventor.md),
  [docs/sldprt.md](docs/sldprt.md).

## Layout

| Path | What is there |
|---|---|
| `crates/dutchman-core` | Format detection (`format.rs`), the one place cadmpeg is called (`decode.rs`), assemblies read from part files (`assembly.rs`), `Error` and `Report`, the `Writer` trait and `convert`. Knows nothing about SolidCraft. |
| `crates/dutchman-solidcraft` | The `.solidcraft` writer: the design container (`container.rs`, `document.rs`), the STEP restatement (`step.rs`), feature history (`history/`), assemblies (`assembly.rs`), the check in `solidcraft-cli` (`verify.rs`). Knows nothing about native formats. |
| `crates/dutchman` | The `dutchman` command (`main.rs`, `batch.rs` for `convert-all`, `formats.rs`), the `dutchman-mcp` server (`mcp.rs`, `bin/dutchman-mcp.rs`) and the JSON reports both return (`json.rs`). |
| `crates/dutchman-web` | The `dutchman-web` server: a page and an API that convert uploads with `dutchman::batch` (`lib.rs`), uploads and project zips (`upload.rs`), reports made fit for the browser (`report.rs`), the pages (`templates/`). |
| `crates/dutchman-serve` | Shared HTTP guards, private data folder checks (`hardening.rs`), and bounded zip extraction (`upload.rs`). |
| `crates/dutchman-core/tests/fixtures` | Synthetic test files, with their origin in the folder's README. |
| `crates/dutchman-solidcraft/tests/golden` | Expected `document.json` output. |
| `scripts/solidcraft-roundtrip.sh` | Opens converted fixtures in a SolidCraft CLI. |
| `docs/`, `skills/` | User docs, the MCP guide, and the agent skill for people converting files. |

## Checks

Run what CI runs (`.github/workflows/ci.yml`) before every commit:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI also builds with Rust 1.88 (`cargo check --workspace --locked`) and tests on Windows and
macOS, so don't assume a case-sensitive filesystem or `/` as the only path separator. The
history tests replay in SolidCraft when `SOLIDCRAFT_CLI` points at a `solidcraft-cli`.

## Rules

- **Never crash.** No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!`
  or slice indexing outside tests; no `unsafe` (the workspace lints enforce all of this). A
  hostile file comes back as an `Error`. Every call into cadmpeg goes through the existing
  guards, which turn a panic into an error. Cap sizes and counts that come from a file.
- **Keep the crates apart.** `dutchman-core` must not learn about SolidCraft;
  `dutchman-solidcraft` must not learn about native formats. Native decoding belongs in
  cadmpeg: if a format needs new decoding, the fix goes upstream there.
- **Stable reports.** Only add fields within a schema version, and document them in
  `docs/json.md`. The CLI's `--json` and the MCP tools return the same reports, built in one
  place (`crates/dutchman/src/json.rs`).
- **Messages.** Plain, specific, sentence case; say what to do next; "not supported yet" for
  our gaps, never "corrupt" or "invalid" for the user's file; "convert", not "import".
- **Docs move with the code.** A change to a command, flag, tool, report field or message
  updates `docs/guide.md`, `docs/mcp.md`, `docs/json.md` and `skills/` in the same pull
  request.

## Clean-room rule

Proprietary formats are decoded from public information and from files their owners made
themselves only.

- Never use leaked or confidential material, a CAD vendor's SDK samples or help text,
  decompiled programs, or another product's source code.
- Never add third-party CAD files to the repository: no models from libraries, vendors,
  employers or customers. Test files are synthetic (built in code, from cadmpeg's public
  test suite, or written with cadmpeg's encoders) and their origin goes in the fixtures
  README.
- Never upload a user's design files anywhere while working on a problem with them.
