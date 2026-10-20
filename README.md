<h1>
  <img alt="Dutchman" src="assets/logo/dutchman-lockup.png#gh-light-mode-only" width="252">
  <img alt="Dutchman" src="assets/logo/dutchman-lockup-night.png#gh-dark-mode-only" width="252">
</h1>

Dutchman converts native CAD designs into open, editable models. Reading the native file is
done by [cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman maps cadmpeg's model onto a
target format, starting with [SolidCraft](https://github.com/tangency-labs/solidcraft) designs.

```sh
dutchman convert part.f3d part.solidcraft
```

Early development. Fusion (`.f3d`) designs, Inventor parts (`.ipt`), Inventor assemblies
(`.iam`, as placed geometry; keep the part files next to the assembly) and SolidWorks parts
(`.sldprt`) convert today:

- When every feature in a design's history maps onto SolidCraft (sketches and extrudes so
  far), the SolidCraft design keeps that history, editable, with the original geometry kept
  beside it. If `solidcraft-cli` is installed, Dutchman replays the history in it and checks
  it against the original before keeping it.
- Otherwise the design converts as solid geometry without history.
- Every conversion lists what it could not carry across, as Info, Degraded or Dropped.

`dutchman convert --help` lists the options.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

## Use from scripts and AI agents

Install from source (Rust 1.88 or newer):

```sh
cargo install --git https://github.com/dutchman-cad/dutchman dutchman
```

See what would carry across, without writing anything:

```sh
dutchman inspect --json part.ipt
```

Convert, with one JSON report on stdout:

```sh
dutchman convert --json part.f3d part.solidcraft
```

Exit codes: `0` converted, `1` could not convert (the JSON report has `"ok": false` and
`error.message`), `2` usage error. A written history is checked in SolidCraft when
`solidcraft-cli` is found: set `SOLIDCRAFT_CLI` to its path, or pass `--solidcraft PATH`.
[docs/json.md](docs/json.md) describes every field; field names are stable.

## Building

```sh
cargo build --release
cargo test --workspace
```

## Licence

Licensed under the [Apache License, Version 2.0](LICENSE).
