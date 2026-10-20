# Dutchman architecture

Dutchman converts native CAD design files into SolidCraft designs (`.solidcraft`). It starts
as a command-line tool and becomes a SolidCraft plugin once SolidCraft has plugins. The
design rule is low coupling, high cohesion: one self-contained conversion core, and a thin
SolidCraft adapter that can later sit behind a plugin interface unchanged.

## Pipeline

```
native file ──cadmpeg decode──▶ CADIR (neutral model) ──Writer──▶ target file
  .f3d .ipt .sldprt …            exact B-rep, history,             .solidcraft
                                 product structure
                    └──────────── Report (losses) ─────────────┘
```

Decoding belongs to [cadmpeg](https://github.com/cadmpeg/cadmpeg). Dutchman never parses a
native format itself; it drives cadmpeg's codecs and maps cadmpeg's neutral model, CADIR
(`cadmpeg_ir::CadIr`), onto a target. Format work lands in cadmpeg, so every cadmpeg
consumer benefits and Dutchman stays small.

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `dutchman-core` | Format gate, input caps, the `Writer` seam, the `Report`, the `convert` pipeline. Owns the cadmpeg decode call. | `cadmpeg-ir` (later `cadmpeg-registry`) |
| `dutchman-solidcraft` | Implements `Writer` for `.solidcraft`. Knows the SolidCraft design format and nothing about native formats. | `dutchman-core` |
| `dutchman` | The CLI: `dutchman convert <in> <out.solidcraft>`. Argument parsing and printing only. | both of the above, `clap` |

`dutchman-core` and `dutchman-solidcraft` are `publish = false` until their APIs settle. The
`dutchman` crate is the published CLI (`cargo install dutchman`); `dutchman-cad` stays a
reserved placeholder for a future library facade.

### Why CADIR is the model

CADIR is already a format-neutral, exact B-rep model with feature history, sketches,
parameters, appearances and assemblies, in millimetres and radians. A second Dutchman-owned
model would duplicate it and double the mapping work. `dutchman_core::Model` is an alias for
`cadmpeg_ir::CadIr`; if a target ever needs a different shape, the adapter derives it.

## The plugin seam

```rust
pub trait Writer {
    fn extension(&self) -> &'static str;
    fn write(&self, model: &Model, out: &mut dyn Write, report: &mut Report) -> Result<(), Error>;
}
```

The SolidCraft adapter depends only on this trait, CADIR and the SolidCraft file format.
When SolidCraft grows a plugin interface, a small shim crate implements that interface by
calling `dutchman-core` to decode and `dutchman-solidcraft` to build the design (or hands the
design to SolidCraft in memory instead of as bytes). Neither crate changes.

## Depending on cadmpeg

- From crates.io, semver-pinned (`cadmpeg-ir = "0.6"`). cadmpeg publishes `cadmpeg-ir`,
  `cadmpeg-core`, `cadmpeg-registry` and the codec crates; no git or path dependency is
  needed, so CI and `cargo install` work without a cadmpeg checkout.
- `cadmpeg-registry` with `default-features = false` and only the codec features Dutchman
  ships (`f3d`, then `inventor`, …), to keep the build small.
- Unreleased cadmpeg work is tried locally with a `[patch.crates-io]` path override that is
  never committed.
- Minimum Rust follows cadmpeg's (1.88 today).

## Writing SolidCraft designs

A `.solidcraft` file is JSON (`{"format":"solidcraft/N", "name", "units", "features", …}`);
SolidCraft also reads a zip container of the same document. The adapter writes the JSON
itself with `serde_json` rather than linking `solidcraft-doc`: that crate pulls in
SolidCraft's kernel and renderer, needs edition 2024, and would build against unpatched
dependencies outside SolidCraft's workspace.

The adapter pins one format version (`solidcraft/26`). SolidCraft upgrades older formats on
open and refuses newer ones, so writing a slightly old format is always safe. A test in CI
round-trips a written design through SolidCraft's reader once that can be done without
building SolidCraft in Dutchman's CI (open question 4).

## Errors and losses

- **`Error`** stops a conversion: I/O, input over the size cap, unknown or not-yet-supported
  format, wrong output extension, a decode or encode failure. Nothing is written when the
  input cannot be read.
- **`Report`** lists what a successful conversion could not carry across, each `Loss`
  tagged `Info` (no effect on geometry), `Degraded` (carried in weaker form, such as history
  flattened to a body) or `Dropped`. cadmpeg's own `DecodeReport` / `LossNote`s are folded
  into the same report, so the user sees one list.
- The CLI prints losses to stderr and exits 0 on success, 1 on a conversion error, 2 on a
  usage error.

## Never crash

Hostile input comes back as an error, never a panic: workspace lints deny `unwrap`,
`expect`, `panic!`, `todo!`, `unreachable!` and slice indexing outside tests; `unsafe` is
forbidden. Inputs are size-checked before opening (`MAX_INPUT_BYTES`), and cadmpeg decodes
run under its `DecodePolicy` resource limits (salvage mode, service-profile caps).

## v0.1: one format, end to end

1. Decode **Fusion `.f3d`** through `cadmpeg-codec-f3d`, cadmpeg's most complete native
   codec (connected B-rep and extrude/fillet/chamfer history). Inventor `.ipt` follows as
   soon as its cadmpeg support is level with it (open question 1).
2. Write each solid body as SolidCraft's non-parametric **`import` feature**: CADIR B-rep →
   STEP text through cadmpeg's STEP encoder → `{"type":"import","step":…}`. This is exactly
   what SolidCraft builds when it opens a STEP, IGES or FreeCAD file, so the result behaves
   like any import.
3. Report everything not carried (history, sketches, parameters, appearances) as losses.
4. Tests on synthetic fixtures only: CADIR built in code, and files cadmpeg itself
   publishes as test fixtures. No customer or downloaded models.

## After v0.1

- **History**: map CADIR features (sketch, extrude, revolve, fillet, chamfer, hole) and
  parameters to SolidCraft's parametric features, falling back to an `import` body per
  feature that cannot be mapped. This is the reason Dutchman exists beyond STEP export.
- **Assemblies**: CADIR occurrences → SolidCraft components and occurrences.
- **Appearances and names**.
- More formats as cadmpeg's codecs mature.
- The SolidCraft plugin shim.

## Open questions

1. **First format.** cadmpeg rates Fusion `.f3d` at L4 and Inventor at L1 (B-rep transfers,
   suppression and dependency state not resolved). Start with `.f3d`, or Inventor first?
2. **Licensing of commercial-format codecs.** SolidCraft deliberately builds only cadmpeg's
   open-format codecs; Dutchman exists partly to host the commercial ones (f3d, ipt,
   sldprt) outside SolidCraft. Is shipping those in a public Apache-2.0 binary acceptable?
3. **Crate names.** CLI as `dutchman`, library facade later as `dutchman-cad`: right split?
4. **Output check.** Should CI validate output against SolidCraft's own reader (heavy build),
   or is a schema/golden-file check enough until the plugin exists?
5. **Container or JSON.** Write plain JSON (simplest), or SolidCraft's zip container (STEP in
   blobs, smaller files)?
