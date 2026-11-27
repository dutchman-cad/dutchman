# Dutchman for SolidCraft plugins

`crates/dutchman-plugin` is a streaming WebAssembly component front end to Dutchman's
normal CADIR → SolidCraft conversion pipeline. It reads Fusion `.f3d` and `.f3z`,
Inventor `.ipt` and `.iam`, SolidWorks `.sldprt` and `.sldasm`, and FreeCAD `.FCStd`.
The result contains ordinary SolidCraft features and geometry and works without the plugin.

It implements SolidCraft's importer interface: the `craft:plugin@0.2.0` package, in the world
`dutchman-importer` (`wit/dutchman.wit`), which is SolidCraft's `importer-dialog-plugin` plus
`user-storage` and `folders`. The manifest requires the capabilities `formats@0.2`,
`evaluate@0.2`, `dialog@0.1`, `folders@0.1` and `user-storage@0.1`, and states
`min_host_version`. It needs a SolidCraft release that includes plugin importer support
(SolidCraft #20425: file first, folder access, minimum version, per-user settings). SolidCraft opens the result as a new unsaved
design; the plugin never modifies the active design. `wit/craft-plugin.wit` is a copy of
SolidCraft's `crates/craft-plugin/wit/plugin.wit` (change it there first). No host checkout is
needed to build or test this component, or to run its unit tests.

Released SolidCraft refuses this package at install: a component's WIT imports are static, so
`folders` and `user-storage` cannot be optional, and a release without them (and without the
`min_host_version` and `file_first` manifest fields) reports the capability or field it lacks.
`min_host_version` is the oldest release that serves all of them, 0.50.0, the first release that
includes #20425; the previous package, built against
`craft:plugin@0.1.0`, is what SolidCraft 0.44 to 0.49.1 can install.

Open Fusion, Inventor, SolidWorks or FreeCAD files with **File > Open**. Before reading
native geometry, the plugin checks bounded archive indexes and the desktop app's measured
memory/time estimate. The memory ceiling is 1,700 MB (`preflight::PEAK_BYTES`, which a test
keeps below the manifest's 2048 MiB guest memory) and the time ceiling five minutes. A refusal
is one line that names the limit that fired, what the design needs and what the plugin has
(`preflight::too_large`), then points to the [Dutchman app download](https://github.com/dutchman-cad/dutchman/releases/latest).
There is no cap on the number of parts or components: the only counts are the archive's
design files (documents) in the Fusion work estimate below, which the message labels as such, and
the host's 4096 file reads per import.

An Inventor or SolidWorks assembly's own file only lists its parts, so its size does not stand
for the design: the old estimate multiplied the 5 MB top-level `.iam` of a 67-part public
design as if it were one 577 MB part and refused it. The plugin now judges time and work from the
assembly file, and its memory as the parts are read: `preflight::PartsLoad` adds each part's size
(the host reports it before any byte is read) to what has been read so far, and refuses before
reading a part when the projected memory (100 MB plus 60 bytes per file byte, an upper bound) does
not fit. The core reads one part at a time and frees its bytes once it is decoded, so only
the decoded design grows. The guest's memory limit is 2048 MiB.
The module itself declares that 2048 MiB maximum and reserves a 16 MiB stack, so hosts that cannot impose a limit, such as web browsers, enforce the same ceiling.
The app can convert larger designs and save a `.sc` to open in SolidCraft. Compressed inner archive checks are bounded to 16 MiB in total; larger checks also
direct you to the app. The estimate is
approximate; it is not a guarantee that every smaller design fits. SolidCraft 0.45's importer
API has no dialog call: refusal errors appear in its status area; the Import report dialog
is available only after a successful conversion.

Fusion archives also have an explicit fuel estimate, independent of part count:

```text
estimated fuel = 3 * (500,000,000 + 16,000 * B + 100,000,000 * E + 500,000,000 * D)
```

`B` is the larger of the outer archive's expanded bytes and the sum of its inner
archives' expanded bytes. `E` counts outer archive entries; `D` counts indexed `.f3d`
documents, with a minimum of one. These come from bounded ZIP metadata, without reading
features or B-rep payloads. Documents are not placed parts: the 41-part arm has only two
indexed documents. Arithmetic saturates, so overflowing work estimates are refused.
The manifest requests **100,000,000,000 fuel**, SolidCraft's default **Max plugin fuel**
ceiling, so a default install loads the plugin. An estimate above that request directs the
design to the app before native decoding. SolidCraft runs a package with exactly the fuel its
manifest declares and the guest interface has no call to read the host's ceiling, so raising
**Max plugin fuel** cannot give the plugin a larger budget and the message does not suggest it. Other formats retain the existing memory/time checks.

Calibration used the component from `f174dac3`, before this preflight update, in Wasmtime 50.0.2,
with editable history enabled and real CLI host evaluation. The fuel count includes
instantiation, conversion and guest work during verification:

| Public design | Expanded bytes `B` | Entries `E` | Documents `D` | Consumed fuel |
|---|---:|---:|---:|---:|
| 41-part robot arm | 11,718,465 | 4 | 2 | 167,740,144,095 |
| 250 mm MIC6 bed | 261,515 | 17 | 1 | 4,710,741,081 |
| Mini power inlet plate | 260,519 | 17 | 1 | 3,350,956,153 |
| Mini power switch plate | 198,568 | 17 | 1 | 2,412,586,136 |

The arm is the MIT-licensed `examples/Assem1_description/Assem1.f3z` example from
[`fusion2URDF`](https://github.com/Adriaeik/fusion2URDF). Calibration uses observations
only; these public design files are not added to the test fixtures.

The formula before its 3x multiplier bounds all four observations; that multiplier
reserves room for runtime differences and unseen feature complexity. The declared request
is SolidCraft's default 100-billion ceiling (it was 4x the arm's measured fuel, which default
installs refused at load). The 41-part arm's estimate is above it, so it opens in the app.
The RailCore Mini (110,045,607 expanded bytes, 45 entries, 43 documents) and 300ZL
(74,341,274 bytes, 71 entries, 69 documents) exceed both the fuel envelope and the retained
memory gate. They are refused from their outer indexes before inner archive inflation.
This is a conservative heuristic, not a guarantee for every feature mix or runtime.
SolidCraft 0.45 uses Wasmtime 48.0.8; its real-host behavior must also be checked.
For comparison, the rc.9 component with host evaluation refused used
188,397,757,861 fuel on the arm, exceeding its old 100-billion request.

After the update, the rebuilt component used 167,744,341,712 fuel for the arm,
including real host evaluation, with 240,254,976 bytes of peak guest linear memory.
That measurement is above the current request, which is why the arm is directed to the app. A synthetic archive
with 10,000 small entries also confirmed that the compiled component refuses work
from the fuel estimate independently of the memory estimate.
In one headless SolidCraft 0.45.0 session, `file.import` opened all 41 arm parts in
62.6 seconds, including first compilation and host evaluation. The subsequent
RailCore Mini import was refused in 0.81 seconds with the existing app message;
this timing uses the already compiled component. No GUI step is needed for these checks.

The manifest supplies native host forms: Keep editable history (true), Up axis
(Automatic, `auto`, the default: stand up only when the source states Y-up, otherwise keep the
file's axes; As in the file, `as_is`, never rotates; or Stand up (Y-up to Z-up), `stand_up`), Include
hidden bodies (false), and an optional Folder with the parts (see below).
The entry sets `"file_first": true`: SolidCraft picks the file before it shows the form, and
File > Open and drag-and-drop, which already have a path, go straight to the form. The up axis lists its options as `oneOf` entries of `const` and
`title`, so the host's dropdown shows the titles and sends the value. The schemas use the
#2829 command contract field shapes and reject unknown fields.
Defaults retain the CLI's history and visibility behavior; the automatic up axis keeps the file's axes unless the source states Y-up, which the plugin's host-read files do not yet. Include
hidden bodies bypasses Dutchman's source visibility filter, which can expose intermediate
FreeCAD shapes and consumed Fusion bodies. Stand up adds a +90° X rotation (+Y to +Z)
to a wrapper occurrence only after mapping and checks, preserving editable local features.
These two opt-in controls are plugin options; they do not add CLI flags.

There is no plugin UI code. The host draws the form, loss report, Open and Cancel controls
and asks for read access to the selected input's folder (`input_folder`) for assembly
parts.

Two ways to give the plugin an assembly's parts:

- `input_folder`, approved once at install: the parts are in the folder of the file.
- The `parts_folder` property (`"format": "directory"`, `"x-solidcraft-grant": "read"`), which
  shows a folder field with **Browse...** in the form. The person picks the folder that holds
  the parts, such as a Fusion export folder, and SolidCraft grants read access to it for that
  one import. The grant is the property key, so the guest reads parts with
  `folders.read("parts_folder", path, ...)`, `folders.size` and `folders.resolve`, relative to the
  picked folder; the main file is still read through `source`. The value in the options is only
  a switch: the plugin never opens that path. A default or saved value grants nothing, and a
  script, `solidcraft-cli` or MCP call that names a folder gets no grant either: SolidCraft
  allows a non-empty folder value there only when an install approval already covers it, and an
  importer manifest cannot declare one (`runtime.permissions.files` is refused for importers).
  So scripts keep using `input_folder`.

The folder field only changes where parts come from; everything else (limits, budgets, the
resolver's suffix search, the case-insensitive lookup) is the same. The parts are looked up
from the root of the picked folder, so pick the folder that holds the assembly's parts at the
paths the assembly stores (the export folder itself, not a folder above it).
With the field set, the plugin first asks for the grant (`folders.resolve("parts_folder", "")`)
before reading anything. A refusal names the missing grant, the field to use and what it is
for (`notice::parts_folder_access`); when `input_folder` is the one missing, the message names
that instead (`notice::folder_access`).

Per-user settings (`user-storage@0.1`) keep the three dialog options. After an import,
`adapter.rs` sets the key `options` to `{"editable_history", "up_axis", "include_hidden_bodies"}`
(`settings::encode`); SolidCraft publishes it only when it accepts the design, so a failed or
canceled import saves nothing. The dialog hook (`dialog@0.1`, `dialog-provider.update-fields`) reads
that key when a file is chosen and replies with a `default` for each option the stored value
holds validly (`settings::field_defaults`); nothing stored, a damaged value or a value the form
does not offer leaves the manifest's defaults. The parts folder is never stored. The hook reads
only its own settings, runs no conversion and gets no folder grant. Scripts and MCP pass options
directly and never run the hook, but an accepted scripted import still saves the options it
used. There is no network or clock permission and no write grant. The plugin resolves relative part paths and relocates stored absolute paths by their
longest safe suffix (a suffix through a hidden name such as `.shortcut-targets-by-id` is skipped, because SolidCraft refuses hidden names in a folder read), trying folders nearest the referring assembly first, within the
granted input folder. SolidWorks and FreeCAD prefer a copy beside the referring assembly.
It never searches outside the granted folder, Inventor project libraries or the saving
machine's absolute paths. The host must enforce the folder capability and symlink rules;
unresolved parts are reported by the normal assembly reader. For `.iam` and `.sldasm` the
adapter first calls `source.resolve` on the opened file's own name; a permission refusal there
stops the import before any read, with a message naming the `input_folder` grant and what it is
for (`notice::folder_access`). A refusal on a later part read gets the same message. Fuel and
memory ceilings are SolidCraft preferences the plugin cannot read or change; a fuel trap is
raised by the host and cannot be reworded by the guest. A missing-part message
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

Install the `wasm32-unknown-unknown` Rust target and the reviewed packager with
`cargo install --locked wasm-tools --version '=1.263.0'` first (`WASM_TOOLS`
names another binary). Release builds pin that exact version; update it deliberately and
check the `wasm-tools print` import parsing in `scripts/package-plugin.sh` when upgrading.
The script writes `dutchman.wasm` (the manifest's `runtime.module`),
`plugin.json` and `dutchman-plugin.zip` with both files at its top, and fails unless the
component imports `craft:plugin` interfaces only, or if `dutchman.wasm` is over 67,000,000
bytes (`PLUGIN_WASM_MAX_BYTES`; 1.2x the 55.7 MB of 0.3.0). SolidCraft compiles the module
when a file is first opened, so it must not grow unnoticed.

Where the bytes are (0.3.0 core module, 55.8 MB: 54.0 MB code in 78,950 functions, 1.7 MB
data, no debug or name sections): cadmpeg_ir 19.2 MB (serde `Deserialize` code over
`serde_json`), cadmpeg_codec_f3d 7.3 MB, `core` 6.2 MB, serde_json 4.3 MB, the SolidWorks
codec 3.3 MB, Inventor 2.7 MB, serde 2.7 MB. `wasm-opt -Oz` (binaryen, installable with
`cargo install wasm-opt`; limited to the features rustc emits, since `-all` adds GC types
the host may not accept) takes it to 48.4 MB, but first open stayed within noise
(46.2 s to 44.6 s, 5.0 to 6.2 GB peak), so release builds do not run it. A real cut needs less
code in cadmpeg_ir's serde impls.

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
clock that never advances on `wasm32-unknown-unknown`; cadmpeg's Fusion codec does the same
(cadmpeg/cadmpeg#1283, included in Dutchman's cadmpeg pin).
The plugin uses one worker and no progress heartbeat thread.

The host tests are opt-in so the Wasmtime test compiler does not raise the normal
workspace's Rust 1.88 requirement. `tests/solidcraft_host.rs` runs the package on a real
SolidCraft instead of this crate's stand-in host: `plugin check` (registration), `plugin test`
cases (a file handed over directly, an assembly with and without the `input_folder` approval,
a folder named by a script) and `exec file.import` with a settings folder (settings saved by
an accepted import only). It needs `SOLIDCRAFT_CLI` (a build from a commit with SolidCraft #20425)
and `DUTCHMAN_PLUGIN_COMPONENT`, and fails when either is missing. The picked folder itself is granted
only by the desktop dialog, which no command line can answer, so `tests/component.rs` covers
it with its stand-in host: parts read through `folders`, a refusal without the grant, and the
saved and offered settings. Use a toolchain supported by Wasmtime 50 for them.
They require an actual component and fail if it is absent:

```sh
DUTCHMAN_PLUGIN_COMPONENT=/absolute/path/dutchman.wasm \
  buildq run dutchman ~/builds/worktrees/dm-dutchman-plugin -- \
  cargo +1.98.0 test --release -p dutchman-plugin --features host-tests --test component -- --nocapture
```

The harness uses a fresh instance per file, fuel, a 2 GiB limit per linear memory (the manifest asks for 2048 MiB), and a
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
- `dialog-provider.update-fields(entry, context)` returns `{"fields": {...}}` with the person's
  saved defaults (see above); `user-storage.get/set` and `folders.read/size/resolve` are the
  other additions to this contract.
- `source.read(path, offset, len)` returns at most 1 MiB; path `""` is the selected file,
  any other path is relative to its folder. `source.size(path)` gives member sizes and
  `source.resolve(path)` the folder's case-preserving name for a member.
- `design.write(bytes)` appends the converted SolidCraft design, at most 1 MiB per call.
- `control.progress` reports the main file read, each part read (5-35%), each SolidCraft
  history check (40-90%) and writing, in steps of at least 1% and never going back, so
  SolidCraft 0.48's operations row shows how far a conversion has got. `control.cancelled` is
  checked before each member read, each history check and before the design is written;
  mapping runs without host calls, so Cancel takes effect at the next of those steps.
- The result is the loss report below. Errors: a file that is not supported yet or cannot be read
  is `invalid-input` with field `file` (`options` and `entry` for bad options or entry
  names); size budgets are `limit-exceeded`; an error returned by a host import is passed
  back unchanged. A member read, size, or resolution failure aborts the whole conversion,
  even when other members are readable; no further input calls or output writes follow it.
  A missing member (`source.resolve` returns none) remains an ordinary assembly loss.

The browser needs the same WIT through jco or another component host; browser execution
has not been validated here.

Limits are 1 GiB total input, 4096 file reads, 256 MiB output, a report under 1 MiB, and 16 KiB
options. Member lookups consider at most 64 path components and 4096-byte relative paths.
Oversize output or report fails before any output is sent. Native decoding still
materializes each input file and the writer builds the whole ZIP in memory; streaming
bounds host-call size, not total conversion memory. The host must independently enforce
its budgets, cancellation and fresh-instance behavior after traps.

## Verification in the host

The plugin checks its history in SolidCraft itself, through the host's `evaluate` import
(capability `evaluate@0.2`, SolidCraft's importer PR follow-up). `HostEvaluator` in
`crates/dutchman-plugin/src/evaluator.rs` implements #580's `SolidCraftEvaluator` over it:
for each check `adapter.rs` stages the design (`evaluate.stage`, 1 MiB at a time), runs the
writer's commands one at a time (`evaluate.run`: `inspect.measure`, `document.inspect`,
`timeline.roll_to`, `model.edges`, `model.faces`), and discards it (`evaluate.reset`). The
host opens each staged design in a private session apart from every open design and every
file, and enforces the import's deadline and cancellation; the plugin needs no clock, file
or network permission for it.

The writer uses one worker and an explicit newest-known target version, with the evaluator
and `hide_unverified_history(true)`. A history that matches the original geometry is
written as the visible model; one the host can't check (an older SolidCraft without
`evaluate`, a refused command, a reply over 1 MiB) records the normal `Check::Unverified`
reason and stays hidden beside the visible original, as before. It never labels an
unchecked history verified. A command that fails in the host is an answer
(`{"error": ...}`) the writer compares like the CLI's; only a transport failure is `Err`.
Parsing, tolerances and source-coordinate comparisons stay in the writer, and stand-up
is applied after the checks.

The Wasmtime harness refuses evaluation for ordinary fixture comparisons, keeping them
comparable with the CLI without verification. Its Stand up regressions require a real
`solidcraft-cli` (discovered normally or selected with `SOLIDCRAFT_CLI`): they map and
replay the public FreeCAD fillet fixture and evaluate repeated hidden-body world names.
Each staged evaluation uses a private CLI archive; earlier session commands are replayed
before the next command, preserving timeline state.

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
  the sandbox; checks run through the host's `evaluate` import instead (see above).
- The component imports nothing but `craft:plugin` interfaces: no clock, random,
  filesystem, environment or network. Time reads use the frozen clock described above.

## Loss report

The return value is UTF-8 JSON under 1 MiB, in the shape SolidCraft shows to the user:

| Field | Shape and meaning |
|---|---|
| `summary` | One line, at most 240 bytes, such as `Partly converted; Losses: 2 Info, 1 Degraded` |
| `items` | At most 4096 rows, with dropped losses first, then degraded losses, then informational losses (source order within each severity). One per retained Dutchman loss: `severity` (`info`, `degraded`, `dropped`), `title` (one plain sentence about the change, at most 120 bytes; a note Dutchman has no plain sentence for keeps its own message here), `code` (the stable cadmpeg or Dutchman code, or empty; at most 128 bytes), `subject` (empty when it would only repeat the code; at most 256 bytes), `message` (at most 1024 bytes). Notes that change nothing the person sees (records STEP has no place for, a placement statistic of nothing) are not in `items`; they stay in `details.losses`. SolidCraft shows one line per title with its count and keeps codes and messages under Details. |
| `details` | The `dutchman.plugin.report/2` report below. When the whole result exceeds 1 MiB, its `losses` array is left out; items are reduced further if needed, retaining the severity priority. |

When losses exceed the item or byte limit, a reserved final item has code
`dutchman.report.truncated`, title `Conversion report shortened`, severity `info`, and
subject `report`. Its message says how many losses are omitted from items and whether
full loss details were omitted. The notice counts toward the 4096-item limit. A
`details.truncation` object records `omitted_items` (a nonnegative count) and
`details_losses_omitted` (a boolean). This additive field is absent in complete reports.
Loss text still has the per-field limits above; an oversized report that cannot fit even
without loss rows fails before output is sent.

### `details`: `dutchman.plugin.report/2`

Version 2 replaces the earlier machine-oriented `dutchman.plugin.loss/1` projection in
this host-facing surface. CLI and MCP `dutchman.convert/1` reports are unchanged.
The report describes conversion before Stand up. It stays local and may include readable
part and feature names; archive member paths are replaced with plain descriptions.

| Field | Shape and meaning |
|---|---|
| `schema` | Literal `dutchman.plugin.report/2` |
| `result` | `Converted`, `Partly converted`, `Converted, no history`, or `Incomplete`, using the desktop app's wording |
| `help` | "Is there a problem? Is something missing?", followed by the Discord invite, DM Dutchy, and GitHub issue links, in that order |
| `shape_check` | Nullable `{result, reason}` with plain descriptions such as "Not checked" or "No editable history to check". A history that was not checked, or only partly, also has `meaning`: it was written without SolidCraft replaying it and comparing the result with the original shape, which is not a warning that the history is wrong, and how to check it (Dutchman Convert with **Check feature history in SolidCraft** turned on, and a STEP export of the same design). |
| `features` | Nullable list of `{name, feature, result, reason}` in source order; results are `Converted`, `Not needed`, `Left out`, or `Not supported yet` |
| `assembly` | Nullable `{placed_parts, empty_parts, unplaced_parts, placement_summary, missing_parts}`; `unplaced_parts` rows contain `{part, placements}` for unreadable archive positions and dependent children. These do not count in `placed_parts` or `empty_parts`. `placement_summary` is the plain unreadable-position sentence (or `null`); the headline is `Incomplete`. Missing rows contain `{part, reason, placements}` |
| `parts` | List of `{name, shape_check, converted_features, source_features}` |
| `losses` | Every note, as `{level, part_or_feature, reason, summary, shown_by_default}`; `level` is `Info`, `Degraded`, or `Dropped`; `summary` is the plain sentence and `shown_by_default` is `false` for notes that change nothing the person sees |
| `truncation` | Present only when shortened: `omitted_items` count and `details_losses_omitted` boolean |

With Keep editable history off, the reason is `Keep editable history is turned off`.
Failure notices use the desktop app's help wording, with Discord before GitHub. Neither
notices nor report details expose internal archive paths. Links are text in the host's
report; opening or copying them depends on the host UI.

Placement regressions check component parents, hidden occurrence and joint endpoint
paths, and hidden-body world names when adding the wrapper. The component harness also
converts `freecad-1.1.4-P04/P04a.FCStd` with Stand up through the real host evaluator, requires a
mapped fillet, and replays the resulting placed design in SolidCraft. Fatal member read,
size and resolve errors (permission and limit errors), the aggregate input limit, and
oversized member metadata are tested after a good member; each must return the original
error without writing a partial design.

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

Linux builder, stable Rust 1.99.1, `wasm32-unknown-unknown`, size flags above, Wasmtime 50.0.2,
cadmpeg main `f1338aa2`.
One-run observations on a shared builder.

| Measurement | Result |
|---|---|
| Component (`dutchman.wasm`) | 51,622,915 bytes (49.23 MiB); SolidCraft accepts importers up to 64 MiB |
| Release build through buildq, three jobs, shared compiler cache | 13m 51s (25m 31s on the first build in a worktree); largest process 14.6 GiB |
| Wasmtime first compilation | 17.3 s on the shared builder under load |
| Harness process peak RSS after first compilation | 4,804,720 KiB (4.58 GiB) |
| Guest peak linear memory, public fixtures (`peak_guest_memory` lines) | 3.3 to 15.2 MiB per file (inputs of 0.6 to 15 KB) |
| Native peak RSS, one 68 MB Fusion archive, `dutchman convert --threads 1 --no-verify` | 3.8 GiB (3,894,656 KiB), 2 min 51 s |

A public 94-placement Inventor assembly (an FRC swerve module, `SDS MK4i Swerve Module, NEOs.0002.iam`,
4.4 MB, with its part files) converts natively in 7.9 s at 572 MB peak RSS, but in the plugin it
used all of the manifest's 100,000,000,000 fuel after 12.4 s, with the guest's linear memory at
179 MiB at that point. For assemblies of this size the work budget, not memory, is the limit that
fires on released SolidCraft; the plugin cannot ask for more fuel there (see the manifest
notes above), so the host reports the exhausted fuel and the Dutchman app converts the design.

The manifest asks for 2048 MiB: the public fixtures need under 16 MiB, and the native run of
a very large archive shows memory growing to about 57 times the input. These observations
do not establish an admissible compressed file size; the preflight limits above also bound
work. wasm32 cannot address more than 4 GiB, so the largest archives stay with the command line.

The earlier cold `wasm32-wasip2` measurement took 31 minutes with up to 10.8 GiB in one
compiler process; `dutchman-plugin-measure` repeats a cold build for this target.
The Wasmtime values come from the optimized native harness, with a fresh Engine and
no compiled-code cache, fuel enabled, a 16 MiB Wasm stack and an 18 MiB async-stack
budget. The RSS figure is the whole test process, which also runs the native pipeline.

Targeted validation passed: eleven plugin library tests, four core source tests, and three
component tests. Clippy passed with warnings denied for the plugin, core, and writer
(including the optional host harness), and for the plugin on `wasm32-unknown-unknown`.
All 19 successfully converted public fixtures matched the CLI pipeline's
unpacked document and exact STEP payloads, including the three SolidWorks assemblies and
the FreeCAD assembly with sibling parts. Six remaining fixtures were refused by both
pipelines. Verification status and reason matched the CLI, every loss report fit SolidCraft's
shape, and the harness checked malformed options, an unknown entry, truncated and oversized
inputs, and a multi-chunk stream. Eight fatal member-input scenarios returned the original
error with no output after a good member. The real CLI verified and replayed P04a's fillet
with Stand up, preserving its geometry and rotating its bounds, and evaluated both hidden
world names of a repeated part. The public fixtures include no `.f3z`. Browser execution
remains unvalidated.
