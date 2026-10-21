<h1>
  <img alt="Dutchman" src="assets/logo/dutchman-lockup.png#gh-light-mode-only" width="252">
  <img alt="Dutchman" src="assets/logo/dutchman-lockup-night.png#gh-dark-mode-only" width="252">
</h1>

Dutchman ferries CAD designs between formats: it converts native CAD designs into open,
editable models. Reading the native file is
done by [cadmpeg](https://github.com/cadmpeg/cadmpeg); Dutchman maps cadmpeg's model onto a
target format, starting with [SolidCraft](https://github.com/tangency-labs/solidcraft) designs.

```sh
dutchman convert part.f3d part.solidcraft
```

Early development. Fusion (`.f3d`, `.f3z`) designs, Inventor parts (`.ipt`), Inventor
assemblies (`.iam`, keep the part files next to the assembly) and SolidWorks parts (`.sldprt`)
convert today:

- When every feature in a design's history maps onto SolidCraft (sketches and extrudes so
  far), the SolidCraft design keeps that history, editable, with the original geometry kept
  beside it. If `solidcraft-cli` is installed, Dutchman replays the history in it and checks
  it against the original before keeping it.
- Otherwise the design converts as solid geometry without history.
- Every conversion lists what it could not carry across, as Info, Degraded or Dropped.

`dutchman formats` lists what converts from each format; `dutchman convert --help` lists the
options.

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

Convert a whole project folder, four files at a time:

```sh
dutchman convert-all --json --jobs 4 --out-dir converted/ project/
```

Exit codes: `0` converted, `1` could not convert (the JSON report has `"ok": false` and
`error.message`), `2` usage error. A written history is checked in SolidCraft when
`solidcraft-cli` is found: set `SOLIDCRAFT_CLI` to its path, or pass `--solidcraft PATH`.
[docs/json.md](docs/json.md) describes every field; field names are stable.

### As an MCP server

`cargo install` also installs `dutchman-mcp`, a [Model Context Protocol](https://modelcontextprotocol.io)
server on stdio. It gives an agent four tools, each returning the same report as the matching
`--json` command:

| Tool | Arguments | Returns |
|---|---|---|
| `inspect` | `input` | `dutchman.inspect/1` |
| `convert` | `input`, `output`, and optionally `import_only`, `verify`, `solidcraft`, `verify_timeout` | `dutchman.convert/1` |
| `convert_all` | `inputs` (files and folders), `out_dir`, and optionally `jobs` and the convert options | `dutchman.convert-all/1` |
| `formats` | none | `dutchman.formats/1` |

A conversion that fails comes back as a tool error carrying its failure report. Paths are on
the machine running the server, so give absolute ones. `convert` writes only its `output`
file, and `convert_all` writes only inside `out_dir`.

For Claude Code:

```sh
claude mcp add dutchman -- dutchman-mcp
```

For other clients, the usual configuration entry is:

```json
{
  "mcpServers": {
    "dutchman": {
      "command": "dutchman-mcp",
      "env": {"SOLIDCRAFT_CLI": "/path/to/solidcraft-cli"}
    }
  }
}
```

`SOLIDCRAFT_CLI` is optional: without it, a written history is checked only when
`solidcraft-cli` is on the server's `PATH`, and is reported unverified otherwise.

## Building

```sh
cargo build --release
cargo test --workspace
```

## Licence

Licensed under the [Apache License, Version 2.0](LICENSE).
