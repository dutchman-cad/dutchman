# Dutchman for SolidCraft plugins

`crates/dutchman-plugin` is a streaming WebAssembly component front end to Dutchman's
normal CADIR → SolidCraft conversion pipeline. It reads Fusion `.f3d` and `.f3z`,
Inventor `.ipt` and `.iam`, SolidWorks `.sldprt` and `.sldasm`, and FreeCAD `.FCStd`.
The result contains ordinary SolidCraft features and geometry and works without the plugin.

It implements SolidCraft's importer interface: the `craft:plugin@0.1.0` package, world
`importer-plugin` (capability `formats@0.1`). SolidCraft opens the result as a new unsaved
design; the plugin never modifies the active design. `wit/craft-plugin.wit` is a copy of
SolidCraft's `crates/craft-plugin/wit/plugin.wit`. No host checkout is needed to build or
test this component.

The manifest supplies native host forms: Keep editable history (true), Up axis
(As in the file, `as_is`, the default, or Stand up (Y-up to Z-up), `stand_up`), Include
hidden bodies (false). The up axis lists its options as `oneOf` entries of `const` and
`title`, so the host's dropdown shows the titles and sends the value. The schemas use the
#2829 command contract field shapes and reject unknown fields.
Defaults retain the CLI's history, visibility and source-coordinate behavior. Include
hidden bodies bypasses Dutchman's source visibility filter, which can expose intermediate
FreeCAD shapes and consumed Fusion bodies. Stand up adds a +90° X rotation (+Y to +Z)
to a wrapper occurrence only after mapping and checks, preserving editable local features.
These two opt-in controls are plugin options; they do not add CLI flags.

There is no plugin UI code. The host draws the form, loss report, Open and Cancel controls
and asks for read access to the selected input's folder (`input_folder`) for assembly
parts. There is no network or clock permission and no write grant. The plugin resolves relative part paths and relocates stored absolute paths by their
longest safe suffix, trying folders nearest the referring assembly first, within the
granted input folder. SolidWorks and FreeCAD prefer a copy beside the referring assembly.
It never searches outside the granted folder, Inventor project libraries or the saving
machine's absolute paths. The host must enforce the folder capability and symlink rules;
unresolved parts are reported by the normal assembly reader. A missing-part message
asks you to keep the part in the input folder with its expected name and relative path.

Case-insensitive lookup must return canonical, case-preserving relative names. The
public `local_cubes.sldasm` fixture stores an uppercase part path for a lowercase file;
the CLI resolves it and derives body names from the actual filename. `source.resolve`
supplies this behavior without changing native CAD decoding: the host returns the exact
name, else the single case-insensitive match, else none. The guest validates that
returned name before reading it.

## Build and package

SolidCraft links only the `craft:plugin` interfaces: no WASI, no network. The component is
therefore built for `wasm32-unknown-unknown` and wrapped with `wasm-tools component new`,
with no WASI adapter:

```sh
buildq run dutchman ~/builds/worktrees/dm-dutchman-plugin -- \
  cargo +1.98.0 build --release --target wasm32-unknown-unknown -p dutchman-plugin
scripts/package-plugin.sh <target>/wasm32-unknown-unknown/release/dutchman_plugin.wasm <folder>
```

Install the `wasm32-unknown-unknown` Rust target and `wasm-tools` first (`WASM_TOOLS`
names another binary). The script writes `dutchman.wasm` (the manifest's `runtime.module`),
`plugin.json` and `dutchman-plugin.zip` with both files at its top, and fails unless the
component imports `craft:plugin` interfaces only.

`zip` turns on getrandom's JavaScript backend for every wasm32 target, so the core module
also imports a few wasm-bindgen functions: randomness for archive extraction and
encryption, which Dutchman never calls. The script binds each to a stub that traps. Choosing
getrandom's `unsupported` backend instead does not compile with getrandom 0.4.3 beside that
feature (rust-random/getrandom#875 fixes it); after a release with that fix, the backend
choice replaces the stubs. Buildq uses a worktree-specific target directory under
`~/builds/buildq-target/dutchman/` (or `~/.cache/buildq/target/dutchman/` without the build
drive). `.cargo/config.toml` applies `opt-level=z`, fat LTO, stripping and one codegen unit
to this target and its dependencies; native release settings stay unchanged.

On this target `std::time::Instant::now()` panics and threads cannot start. Dutchman reads
time through `dutchman_core::time::Instant`, which is `std::time::Instant` natively and a
clock that never advances on `wasm32-unknown-unknown`; cadmpeg's Fusion codec does the same (cadmpeg/cadmpeg#1283).
The plugin uses one worker and no progress heartbeat thread.

The host tests are opt-in so the Wasmtime test compiler does not raise the normal
workspace's Rust 1.88 requirement. Use a toolchain supported by Wasmtime 50 for them.
They require an actual component and fail if it is absent:

```sh
DUTCHMAN_PLUGIN_COMPONENT=/absolute/path/dutchman.wasm \
  buildq run dutchman ~/builds/worktrees/dm-dutchman-plugin -- \
  cargo +1.98.0 test --release -p dutchman-plugin --features host-tests --test component -- --nocapture
```

The harness uses a fresh instance per file, fuel, a 1 GiB limit per linear memory, and a
16 MiB Wasm stack with an 18 MiB async-stack budget (Wasmtime validates both even
for this synchronous harness). The production host must cap the sum across component memories.
Its linker holds only the `craft:plugin` imports, implemented in the test (source, design,
log, control, and a clock that answers 0), so a component that needs WASI fails to
instantiate. It compares the
unpacked document and exact STEP payloads with the native CLI reader/writer pipeline
under identical verification and target-version settings. The fixtures are the existing
public synthetic files; no private design is needed. Output ZIP compression bytes are
not a geometric assertion. The harness uses an optimized native release build so the Wasmtime compiler itself is
optimized. The first `Component::from_file` call is timed without a compiled-code cache.

## Adapter contract

All generated guest and harness bindings are in `src/adapter.rs`, from
`wit/craft-plugin.wit`. A change to the interface touches that file, not conversion code.

- The export `importer.import-file(entry, file: {name, size}, options)` converts. `entry`
  is the manifest entry (`convert`); `options` is JSON matching its `input_schema`.
- `source.read(path, offset, len)` returns at most 1 MiB; path `""` is the selected file,
  any other path is relative to its folder. `source.size(path)` gives member sizes and
  `source.resolve(path)` the folder's case-preserving name for a member.
- `design.write(bytes)` appends the converted SolidCraft design, at most 1 MiB per call.
- `control.progress` reports reading and writing; `control.cancelled` is checked before
  each member read and before the design is written.
- The result is the loss report below. Errors: an unsupported, corrupt or truncated file
  is `invalid-input` with field `file` (`options` and `entry` for bad options or entry
  names); size budgets are `limit-exceeded`; an error returned by a host import is passed
  back unchanged.

The browser needs the same WIT through jco or another component host; browser execution
has not been validated here.

Limits are 1 GiB total input, 4096 file reads, 256 MiB output, a report under 1 MiB, and 16 KiB
options. Member lookups consider at most 64 path components and 4096-byte relative paths.
Oversize output or report fails before any output is sent. Native decoding still
materializes each input file and the writer builds the whole ZIP in memory; streaming
bounds host-call size, not total conversion memory. The host must independently enforce
its budgets, cancellation and fresh-instance behavior after traps.

## Verification seam and sandbox constraints

The writer uses one worker, an explicit newest-known target version, verification off,
and `hide_unverified_history(true)`. It records the normal `Check::Unverified` reason and
keeps editable history hidden beside the visible original. It never labels an unchecked
history verified. Missing blend-prefix replay remains the writer's normal history blocker.

`TODO(eval-seam)` in the plugin conversion function is the integration point for
`SolidCraftEvaluator`. The parallel worktree currently declares two methods:
`evaluate(design, timeout, label) -> Result<Value, String>` and
`run(design, commands, timeout, label) -> Result<Vec<Value>, String>`. A stateless,
`Debug + Send + Sync` `HostEvaluator` in `crates/dutchman-plugin/src/evaluator.rs`
implements both method bodies over an isolated session's permission-filtered
`run(command, args)`. Its exact PR #580 trait implementation is behind the explicit
TODO gate until the trait lands. Unit tests exercise the method bodies now.

`evaluate` must evaluate an isolated archive and return the complete raw eval JSON,
combining `inspect.measure` body volumes, areas and bounds with `document.inspect`
timeline errors and warnings. `run` must start from that archive in an isolated context,
execute ordered `timeline.roll_to`, `document.inspect`, `model.edges` and `model.faces`
commands in the same context, and return every raw result, including command failures.
An evaluated refusal stays `{"evaluation_failed":"reason"}`; transport failure, timeout
or no answer is `Err`. Leave parsing, tolerances and source-coordinate comparisons in
the writer. Select the host's target release separately; the trait does not discover it.

The tested dispatcher translates the writer's `command`/`params` envelopes into host
`run(command, args)` calls, preserves ordered raw results and command failures, combines
measurements and timeline diagnostics, and caps commands/results. It forwards the archive,
whole-call timeout and diagnostic label to `IsolatedHost::with_design`; the host must
enforce the deadline without granting a guest clock. The module is currently compiled
for tests only. After #580 lands, compile it for the guest, implement `IsolatedHost` in
`adapter.rs`, remove the false cfg from the exact trait impl, and attach it through
`SolidCraftWriter::evaluator(Arc::new(HostEvaluator(host)))`.

The host must settle how `run` stages an archive without a filesystem write grant or a
single oversized JSON/base64 call, how an isolated evaluation context is addressed and
released, and how its deadline/output cap are enforced. Calling commands on the user's
active design is not a valid evaluator implementation. No subprocess, filesystem
preopen, temporary path or second tolerance policy belongs in the plugin. The current
contract deliberately does not import `run` before that implementation exists.

Sandbox audit:

- `wasm32-unknown-unknown` defaults to `panic="abort"`. Native `catch_unwind` guards
  remain in place, but cannot recover a codec panic on this target. Ordinary decoder
  errors return normally; an abort becomes a trap. The host discards that call's streamed
  output, reports the failed conversion and uses a fresh instance for the next file.
- `wit-bindgen` emits unsafe canonical ABI exports (pointer marshalling and linker names).
  The plugin crate uses `deny(unsafe_code)` instead of the workspace's `forbid`, with
  one `allow` around the generated `export!` invocation in `adapter.rs`. There is no
  handwritten unsafe code. Core, writer and the rest of the workspace keep their lints.
- External assembly resolution natively uses `std::fs`: the default-on `source-reader`
  core feature adds a caller-owned reader/resolver seam in `source.rs` and `locate.rs`.
  The reader receives the per-file byte cap and must refuse larger files before producing
  their bytes; the plugin checks host size metadata before reading parts. The plugin
  passes `PartPositions::Smart` and uses one worker.
- Core scoped workers run serially on wasm (`parallel.rs`). The plugin constructs no
  progress observer, so no heartbeat thread starts.
- SolidCraft CLI discovery, subprocesses, replay and temporary files are unavailable in
  the sandbox; verification is off until the host evaluator lands.
- The component imports nothing but `craft:plugin` interfaces: no clock, random,
  filesystem, environment or network. Time reads use the frozen clock described above.

## Loss report

The return value is UTF-8 JSON under 1 MiB, in the shape SolidCraft shows to the user:

| Field | Shape and meaning |
|---|---|
| `summary` | One line, at most 240 bytes, such as `Editable history: 12 of 14 features mapped; 3 losses` |
| `items` | At most 4096 rows, one per Dutchman loss: `severity` (`info`, `degraded`, `dropped`), `title` (the kind of loss in plain words, shared by every loss of that kind, such as "Parts of the file kept as-is" or "Sketches carried over with changes"; at most 120 bytes), `code` (the stable cadmpeg or Dutchman code, or empty; at most 128 bytes), `subject` (empty when it would only repeat the code; at most 256 bytes), `message` (at most 1024 bytes). SolidCraft shows one line per title with its count and keeps codes and messages under Details. |
| `details` | The full `dutchman.plugin.loss/1` report below. When the whole result would reach 1 MiB, its `losses` array is left out (the items carry them). |

### `details`: `dutchman.plugin.loss/1`

Fields are additive within this version.
The report describes the untransformed conversion, even when Stand up is selected. It may
contain design, file and feature names; it stays local and is not an anonymous report.
This schema projects `dutchman_core::Report`; CLI and MCP keep their existing shared
`dutchman.convert/1` serializer and behavior.

| Field | Shape and meaning |
|---|---|
| `schema` | Literal `dutchman.plugin.loss/1` |
| `stages` | Six rows in pipeline order: `stage`, `total`, `info`, `degraded`, `dropped` loss counts |
| `losses` | Ordered rows: `stage`, `severity`, `subject`, nullable stable `code`, `message`, nullable `upstream` (`cadmpeg` or `solidcraft`) |
| `outcome` | Nullable object: `mode` (`history`, `partial_history`, `import`), `verification`, `features` |
| `outcome.features` | Source feature rows: `name`, `kind`, `ordinal`, `variant`, `status` (`mapped`, `not_needed`, `left_out`, `blocked`), nullable `message`, `profile_unverified`, `reapplied`, `final_body` |
| `parts` | Assembly history rows: `name`, `verification`, `mapped`, `total`, `rebuilt`, `needing`, `reapplied` |
| `completeness` | Nullable object: `placed`, `placed_empty`, `left_out` rows (`file`, `message`, `stage`, `count`) |
| `joints` | Object mapping joint kind to count |

Every `verification` is `{status, message}`. `status` is `verified`, `partial`, `differs`,
`unverified`, `flattened` or `not_applicable`; `message` is null for verified, otherwise
Dutchman's reason. `stage` is `input`, `decode`, `project`, `map`, `write` or `solidcraft`.
Severity is `info`, `degraded` or `dropped`. Feature messages and stage losses preserve
Dutchman's existing reasons; `code` remains null where the core assigns no stable code.
Counts and ordinals are nonnegative integers; feature `name`, `ordinal` and `variant`
may be null. `profile_unverified`, `reapplied` and `final_body` are booleans.
A host should show feature blockers together with the relevant stage losses rather than
inventing a code from a feature name.

The placement regression test preserves a serialized fillet's local edge picks and radius
and fixes hidden occurrence and joint endpoint paths when adding the wrapper. A full fillet mapping/replay
assertion with Stand up waits for the host evaluator; it cannot pass with verification off.

For a cold Linux build measurement without buildq's shared compiler cache, run:

```sh
buildq run dutchman ~/builds/worktrees/dm-dutchman-plugin -- \
  cargo run --release --locked -p dutchman-plugin-measure
```

The std-only Cargo launcher runs `scripts/measure-plugin-build.sh` through buildq's
Cargo-only queue. The script uses a fresh temporary target folder and one compiler job, leaves the component
and `/usr/bin/time -v` output there, and prints their location. It also saves a ready-to-load
package in `target/dutchman-plugin/`, outside buildq's disposable target cache. This protects
the component from buildq's current cross-target directory trimming. The single-job setting
bounds compiler concurrency; the initial six-job build reached buildq's 12 GiB scope cap.
The scope's `memory.peak` measures concurrent build memory; time's maximum RSS measures
the largest individual process and should not be confused with that total.

## Measurements

Linux builder, Rust 1.98.0, `wasm32-unknown-unknown`, size flags above, Wasmtime 50.0.1.
One-run observations on a shared builder.

| Measurement | Result |
|---|---|
| Component (`dutchman.wasm`) | 38,567,054 bytes (36.78 MiB) |
| Release build through buildq, shared compiler cache | 11m 0s |
| Wasmtime first compilation | 7.4 s |
| Harness process peak RSS after first compilation | 3,584,892 KiB (3.42 GiB) |

The packaged component's SHA-256 is
`599ba31810436f1d88bf8e16c3d50c1c4502da48c632b1edb5a8dad660cc0e9c`.
The earlier cold `wasm32-wasip2` measurement took 31 minutes with up to 10.8 GiB in one
compiler process; `dutchman-plugin-measure` repeats a cold build for this target.
The Wasmtime values come from the optimized native harness, with a fresh Engine and
no compiled-code cache, fuel enabled, a 16 MiB Wasm stack and an 18 MiB async-stack
budget. The RSS figure is the whole test process, which also runs the native pipeline.

Targeted validation passed: seven plugin unit tests, Clippy with warnings denied for
the plugin, core and writer (with the optional host harness) and for the plugin on
`wasm32-unknown-unknown`, and the Wasmtime component test. All 18 successfully converted
public fixtures matched the CLI pipeline's unpacked document and exact STEP payloads,
including all three SolidWorks assemblies and the FreeCAD assembly with sibling parts.
Six remaining public fixtures were refused by both pipelines. Verification status and
reason matched the CLI, every loss report fit SolidCraft's shape, and the harness also
checked malformed options, an unknown entry, truncated and oversized inputs, and a
multi-chunk stream. The public fixtures include no `.f3z`. Browser execution remains
unvalidated.
