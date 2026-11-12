# Dutchman for SolidCraft plugins

`crates/dutchman-plugin` is a streaming WebAssembly component front end to Dutchman's
normal CADIR → SolidCraft conversion pipeline. It reads Fusion `.f3d` and `.f3z`,
Inventor `.ipt` and `.iam`, SolidWorks `.sldprt` and `.sldasm`, and FreeCAD `.FCStd`.
The result contains ordinary SolidCraft features and geometry and works without the plugin.

This is an integration candidate for RFC 0002's importer kind, opening a new unsaved
design. It needs the official host's importer phase; it is not a command that modifies
the active design. The RFC's Decisions override the closed #4980 draft. The provided
RFC snapshot contains section 6a but no 6b; the owner specifies a generic `craft-plugin`
host crate, with `craft:plugin` versus `solidcraft:plugin` still open. No dependency on a
private host checkout is needed to build or test this component.

The manifest supplies native host forms: Keep editable history (true), Up axis
(`as_is`, default, or `stand_up`), Include hidden bodies (false). The boolean and string
enum schemas use the #2829 command contract field shapes and reject unknown fields.
Defaults retain the CLI's history, visibility and source-coordinate behavior. Include
hidden bodies bypasses Dutchman's source visibility filter, which can expose intermediate
FreeCAD shapes and consumed Fusion bodies. Stand up adds a +90° X rotation (+Y to +Z)
to a wrapper occurrence only after mapping and checks, preserving editable local features.
These two opt-in controls are plugin options; they do not add CLI flags.

There is no plugin UI code. The host draws the form, loss report, Open and Cancel controls
and asks for read access to the selected input's folder for assembly parts. There is no
network or clock permission and no write grant. The plugin resolves relative part paths and relocates stored absolute paths by their
longest safe suffix, trying folders nearest the referring assembly first, within the
granted input folder. SolidWorks and FreeCAD prefer a copy beside the referring assembly.
It never searches outside the granted folder, Inventor project libraries or the saving
machine's absolute paths. The host must enforce the folder capability and symlink rules;
unresolved parts are reported by the normal assembly reader. A missing-part message
asks you to keep the part in the input folder with its expected name and relative path.

Case-insensitive lookup must return canonical, case-preserving relative names. The
public `local_cubes.sldasm` fixture stores an uppercase part path for a lowercase file;
the CLI resolves it and derives body names from the actual filename. The provisional
`input.resolve` metadata call supplies this behavior without changing native CAD decoding.
The official host needs to review this extension: prefer exact names, then match Unicode
lowercase names as the CLI does, and return a bounded `/`-separated filename inside the
granted folder. The guest validates that returned name before reading it.

## Build and package

```sh
buildq run dutchman ~/builds/worktrees/dm-dutchman-plugin -- cargo build --release --target wasm32-wasip2 -p dutchman-plugin
```

Install the `wasm32-wasip2` Rust target first. Package `plugin.json` from the crate beside
`dutchman_plugin.wasm` from the build's `wasm32-wasip2/release` directory. Buildq uses a
worktree-specific target directory under `~/builds/buildq-target/dutchman/`
(or `~/.cache/buildq/target/dutchman/` without the build drive).
`.cargo/config.toml` applies `opt-level=z`, fat LTO, stripping and one codegen unit to
this target and its dependencies; native release settings stay unchanged.

The host tests are opt-in so the Wasmtime test compiler does not raise the normal
workspace's Rust 1.88 requirement. Use a toolchain supported by Wasmtime 50 for them.
They require an actual component and fail if it is absent:

```sh
DUTCHMAN_PLUGIN_COMPONENT=/absolute/path/dutchman_plugin.wasm \
  buildq run dutchman ~/builds/worktrees/dm-dutchman-plugin -- \
  cargo test --release -p dutchman-plugin --features host-tests --test component -- --nocapture
```

The harness uses a fresh instance per file, fuel, a 1 GiB limit per linear memory, and a
16 MiB Wasm stack with an 18 MiB async-stack budget (Wasmtime validates both even
for this synchronous harness). The production host must cap the sum across component memories.
It inherits no WASI folders, environment, arguments, stdio or network. It compares the
unpacked document and exact STEP payloads with the native CLI reader/writer pipeline
under identical verification and target-version settings. The fixtures are the existing
public synthetic files; no private design is needed. Output ZIP compression bytes are
not a geometric assertion. The harness uses an optimized native release build so the Wasmtime compiler itself is
optimized. The first `Component::from_file` call is timed without a compiled-code cache.

## Adapter contract and integration questions

All WIT, generated guest and harness bindings are in `src/adapter.rs`. The provisional
package is `solidcraft:plugin@1.0.0`, world `importer-plugin`. Replacing the package name
or adopting craft-plugin's reviewed WIT changes this file, not conversion code.

- `input.read(path, offset, len)` returns bytes; empty path selects the main file.
  `input.size(path)` supplies sizes of relative assembly members.
- `input.resolve(path)` resolves a relative member without regard to filename case and
  returns its actual case-preserving, folder-relative name, or none. The host enforces
  the folder/symlink capability. This metadata extension is needed for CLI assembly parity.
- `output.write(bytes)` appends the converted design. Calls use at most 1 MiB.
- `importer.manifest()` returns the packaged manifest; permissions must be approved
  from the static manifest before the host instantiates any code.
- `importer.import-design({name, size}, params)` converts and returns JSON loss data.
  This extends #4980 with a JSON options argument and structured JSON instead of prose.
- Failures use `failed`, `denied`, `limit` or `invalid` variants. Host file errors for
  missing assembly members become ordinary per-part conversion losses.

The manifest's `files: [{folder: "input", access: "read"}]`, entry `input`, and importer
surfaces are provisional until the generic host publishes its generated manifest schema.
The final host needs to settle package identity/version, options delivery, structured loss
report transport, canonical member resolution and limits. #4980's 16 MiB component cap is not assumed sufficient;
`component_mib: 64` is a requested budget, not a claim the current host accepts it.
The browser needs the same WIT through jco or another component host; browser execution
has not been validated here. This work adds no separate web runtime.

Limits are 1 GiB total input, 4096 file reads, 256 MiB output, 64 KiB report, and 16 KiB
options. Member lookups consider at most 64 path components and 4096-byte relative paths.
Oversize output or report fails before any output is sent. Native decoding still
materializes each input file and the writer builds the whole ZIP in memory; streaming
bounds host-call size, not total conversion memory. The host must independently enforce
its budgets, cancellation and fresh-instance behavior after traps.

## Verification seam and WASI constraints

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

WASI audit:

- `wasm32-wasip2` currently defaults to `panic="abort"` (`rustc --print cfg --target
  wasm32-wasip2`). Native `catch_unwind` guards remain in place, but cannot recover
  a codec panic on this target. Ordinary decoder errors return normally; an abort
  becomes a Wasmtime trap. The generic host must discard that call's streamed output,
  report the failed conversion and instantiate a fresh component for the next file,
  as RFC 0002 requires. Recovering a codec panic inside the guest needs a supported
  WASI unwind toolchain/runtime; no decoder guard or `verify.rs` is changed here.

- `wit-bindgen` emits unsafe canonical ABI exports (pointer marshalling and linker names).
  The new plugin crate uses `deny(unsafe_code)` instead of the workspace's `forbid`, with
  one `allow` around the generated `export!` invocation in `adapter.rs`. There is no
  handwritten unsafe code. Core, writer and the rest of the workspace keep their lints.
- External assembly resolution currently relies on `std::fs`: the new default-on
  `source-reader` core feature adds a caller-owned reader/resolver seam in `source.rs`
  and `locate.rs`. The reader receives the per-file byte cap and must refuse larger files
  before producing their bytes; the plugin checks host size metadata before reading parts.
  The seam accepts a `PartPositions` choice; the plugin passes `Smart` and uses one worker.
  Existing filesystem callers and filtering defaults remain unchanged.
- Core scoped workers and progress heartbeat threads need WASI serialization / guards;
  `eval-seam` is handling `parallel.rs` and `progress.rs`. The plugin requests one worker
  and does not construct a progress observer. Those files are not changed here.
- SolidCraft CLI discovery, subprocesses, replay and temporary files are unavailable in
  the closed sandbox. `eval-seam` owns `verify.rs`, `compare.rs`, writer integration and
  the evaluator trait. None of those files is changed here.
- Rust WASI panic behavior may trap instead of unwinding through the native cadmpeg
  guards. The host must discard a trapped instance; decoder error paths remain guarded.
- WASI clock and random imports from Rust/dependencies need the host's closed WASI
  implementation (fixed wall clock, no granted filesystem/network). They do not grant
  clock permission. Decode diagnostics can read monotonic time even without progress.

## Loss report schema: `dutchman.plugin.loss/1`

The return value is UTF-8 JSON, at most 64 KiB. Fields are additive within this version.
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

Linux builder, Rust 1.98.0, `wasm32-wasip2`, size flags above. The cold build used a
fresh Cargo target, no compiler cache, and one Cargo job. These are observed values,
not estimates for an uncapped build.

| Measurement | Result |
|---|---|
| Component | 39,449,156 bytes (37.62 MiB) |
| Cold release build | 31m 23.58s |
| Build scope peak | 12 GiB; the scope's 12 GiB cap was reached |
| Largest build process RSS | 11,368,060 KiB (10.84 GiB) |
| Wasmtime 50.0.1 first compilation | 31.703s |
| Wasmtime first-compilation peak process RSS | 2,034,016 KiB (1.94 GiB) |

The packaged component's SHA-256 is
`5f9842de3c769a1840818b0c2c206f12c799c07c52f384cfecad06867f82cb60`.
The exact requested release command also built successfully; the cold measurement
adds `--jobs 1` and disables the shared compiler wrapper so compiler children remain
in the measured scope. GNU time's RSS and the scope peak measure different things:
RSS is the largest individual process; the scope includes concurrent processes and
charged file cache. The successful build reached its cap, so this does not establish
an unconstrained peak below 12 GiB.

The Wasmtime values come from the optimized native harness, with a fresh Engine and
no compiled-code cache, fuel enabled, a 16 MiB Wasm stack and an 18 MiB async-stack
budget. Compilation is outside the guest linear-memory limit; its RSS must be
budgeted separately by the host. These are one-run observations on a shared builder.

Targeted validation passed: seven plugin unit tests, plugin Clippy with warnings
denied (including the optional host harness), and the Wasmtime component test. All
18 successfully converted public fixtures matched the CLI pipeline's unpacked
document and exact STEP payloads, including all three SolidWorks assemblies and the
FreeCAD assembly with sibling parts. Six remaining public fixtures were refused by
both pipelines. Verification status and reason matched the CLI, and the harness
also checked malformed options, truncated and oversized inputs, and a multi-chunk
stream. Workspace and GUI reruns were canceled at the owner's request; CI owns
those checks. Browser execution and the official host importer integration remain
unvalidated.
