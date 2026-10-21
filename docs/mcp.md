# The MCP server

`dutchman-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server. It lets
an AI assistant convert CAD files with Dutchman on your machine: it gives the assistant four
tools, and each returns the same JSON report as the matching `dutchman … --json` command
([json.md](json.md)).

It comes in every release archive and with `cargo install` (see the [guide](guide.md#install)).
It speaks JSON-RPC over stdio, one message per line, and takes no arguments: the client starts
it and talks to it.

## Set up a client

### Claude Code

```sh
claude mcp add dutchman -- dutchman-mcp
```

To have it check converted histories in SolidCraft, pass the path of `solidcraft-cli` if it is
not on your `PATH`:

```sh
claude mcp add dutchman --env SOLIDCRAFT_CLI=/path/to/solidcraft-cli -- dutchman-mcp
```

`claude mcp list` shows whether it connected.

### Codex

In `~/.codex/config.toml`:

```toml
[mcp_servers.dutchman]
command = "dutchman-mcp"
env = { SOLIDCRAFT_CLI = "/path/to/solidcraft-cli" }
```

Leave out the `env` line when `solidcraft-cli` is on your `PATH`.

### Other clients

Most clients (Claude Desktop, Cursor, Windsurf, and others) take an `mcpServers` entry:

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

If the client cannot find `dutchman-mcp`, give its full path as `command` (on Windows,
including `.exe`). `SOLIDCRAFT_CLI` is optional: without it, a converted history is checked
only when `solidcraft-cli` is on the server's `PATH`, and is reported unverified otherwise.

## Tools

| Tool | Arguments | Returns | Writes |
|---|---|---|---|
| `inspect` | `input` | `dutchman.inspect/1` | nothing |
| `convert` | `input`, `output`, and optionally `import_only`, `verify`, `solidcraft`, `verify_timeout` | `dutchman.convert/1` | the `output` file |
| `convert_all` | `inputs`, `out_dir`, and optionally `jobs`, `import_only`, `verify`, `solidcraft`, `verify_timeout` | `dutchman.convert-all/1` | inside `out_dir` |
| `formats` | none | `dutchman.formats/1` | nothing |

| Argument | Type | Meaning |
|---|---|---|
| `input` | string | Absolute path of a native CAD file: `.f3d`, `.f3z`, `.ipt`, `.iam` or `.sldprt`. |
| `output` | string | Absolute path of the `.solidcraft` file to write. An existing file is replaced. |
| `inputs` | array of strings, at least one | Absolute paths of files, and of folders to search (with their subfolders) for files Dutchman knows. |
| `out_dir` | string | Absolute path of the folder for the designs, keeping each input folder's layout. Created if missing. |
| `jobs` | integer, at least 1 | How many files to convert at once. Default: the machine's cores, at most 8. |
| `import_only` | boolean | Write the geometry as one non-parametric import, never the feature history. |
| `verify` | boolean | Check a converted history in SolidCraft. Default `true`; skipped, and the history marked unverified, when no `solidcraft-cli` is found. |
| `solidcraft` | string | Path of `solidcraft-cli` for the check. Default: `$SOLIDCRAFT_CLI`, else `solidcraft-cli` on `PATH`. |
| `verify_timeout` | integer, at least 1 | Seconds each SolidCraft check may run before the history is written unverified. Default 120. |

An unknown argument is refused (`unknown argument bogus`), as is a missing required one
(`output is required`).

Paths are on the machine running the server. Give absolute ones: a relative path is read
against the server's working directory, which the client chooses.

## Example calls and replies

Each reply carries the report twice: as `structuredContent`, and as the same JSON in a text
`content` item for clients that read only text. The replies below show `structuredContent`,
with `content` shortened to `…`.

Inspect a file:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"inspect","arguments":{"input":"/home/me/designs/solidworks_cube.sldprt"}}}
```

```json
{"jsonrpc":"2.0","id":2,"result":{"isError":false,"content":"…","structuredContent":{
  "schema":"dutchman.inspect/1","ok":true,
  "input":{"format":"solidworks_part","format_name":"SolidWorks part (.sldprt)","path":"/home/me/designs/solidworks_cube.sldprt"},
  "geometry":{"bodies":1,"faces":6},
  "mode":"import",
  "verification":{"status":"not_applicable","reason":"the design carries no feature history"},
  "history":{"features":[],"mapped":0,"total":0},
  "losses":[
    {"level":"Degraded","code":"sldprt/source.dialect-unverified","subject":"sldprt/source.dialect-unverified","message":"the document carries no swSolidWorks swVersion declaration, …"},
    {"level":"Degraded","code":"sldprt/config.inferred-without-native","subject":"sldprt/config.inferred-without-native","message":"1 configuration state(s) are inferred from geometry partitions without native configuration definitions."}]}}}
```

Convert one file:

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"convert","arguments":{"input":"/home/me/designs/synthetic_cube.ipt","output":"/home/me/designs/cube.solidcraft"}}}
```

```json
{"jsonrpc":"2.0","id":3,"result":{"isError":false,"content":"…","structuredContent":{
  "schema":"dutchman.convert/1","ok":true,
  "input":{"format":"inventor_part","format_name":"Inventor part (.ipt)","path":"/home/me/designs/synthetic_cube.ipt"},
  "output":{"format":"solidcraft/23","path":"/home/me/designs/cube.solidcraft"},
  "mode":"import",
  "verification":{"status":"not_applicable","reason":"the design carries no feature history"},
  "history":{"features":[],"mapped":0,"total":0},
  "losses":[
    {"level":"Degraded","code":"inventor/rse.segment-pair-untyped","subject":"inventor/rse.segment-pair-untyped","message":"Retained 1 structurally paired RSe segment(s) without record semantics."},
    {"level":"Info","code":"step/attribute.source-record-not-written","subject":"step/attribute.source-record-not-written","message":"7 source attribute record(s) were not written to STEP"},
    {"level":"Info","code":"step/native.source-record-omitted","subject":"step/native.source-record-omitted","message":"67 source-native record(s) were not represented in STEP"},
    {"level":"Info","code":null,"subject":"history","message":"flattened: the design carries no feature history"}]}}}
```

A conversion that fails is a tool error (`isError: true`) carrying its failure report:

```json
{"jsonrpc":"2.0","id":4,"result":{"isError":true,"content":"…","structuredContent":{
  "schema":"dutchman.convert/1","ok":false,
  "input":{"format":"solidworks_assembly","format_name":"SolidWorks assembly (.sldasm)","path":"/home/me/designs/a.sldasm"},
  "error":{"message":"SolidWorks assembly (.sldasm) files are not supported yet. Export a STEP file for SolidCraft to open, or follow the SolidWorks work in cadmpeg (https://github.com/cadmpeg/cadmpeg)."}}}}
```

Convert a folder. When any file fails, the whole call is a tool error, but the other files are
still converted and each has its own report (shortened here):

```json
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"convert_all","arguments":{"inputs":["/home/me/designs/project"],"out_dir":"/home/me/designs/converted","jobs":2}}}
```

```json
{"jsonrpc":"2.0","id":5,"result":{"isError":true,"content":"…","structuredContent":{
  "schema":"dutchman.convert-all/1","ok":false,"converted":3,"failed":1,
  "results":[
    {"ok":true,"input":{"path":"/home/me/designs/project/generated_unit_cube.f3d", …},"output":{"path":"/home/me/designs/converted/generated_unit_cube.solidcraft", …},"mode":"import", …},
    {"ok":false,"input":{"path":"/home/me/designs/project/sub/container_metadata_only.f3d", …},"error":{"message":"cannot read this design: no geometry could be read from this Fusion archive (.f3d) file"}},
    {"ok":true,"input":{"path":"/home/me/designs/project/sub/solidworks_cube.sldprt", …},"output":{"path":"/home/me/designs/converted/sub/solidworks_cube.solidcraft", …},"mode":"import", …},
    {"ok":true,"input":{"path":"/home/me/designs/project/sub/synthetic_cube.ipt", …},"output":{"path":"/home/me/designs/converted/sub/synthetic_cube.solidcraft", …},"mode":"import", …}]}}}
```

## Safety

- **Everything stays on your machine.** The server reads and writes local files only and
  sends nothing over the network. It runs `solidcraft-cli` locally for the history check.
- **What it writes.** `inspect` and `formats` write nothing. `convert` writes only its
  `output` file, which must end in `.solidcraft`; `convert_all` writes only inside `out_dir`.
  Nothing is written when a conversion fails, and a design is written only once it is
  complete. The server has no list of allowed folders: it can read and write wherever the
  account running it can. Point `output` and `out_dir` at a folder for converted designs, and
  check what your assistant asks for before you approve it.
- **Existing files.** `convert` replaces an existing `output` file, and `convert_all`
  replaces designs of the same name in `out_dir`. Use a fresh folder when that matters.
- **Caps.** Input files over 1 GiB are refused before they are read. One `convert_all`
  converts at most 10,000 files and searches folders at most 32 levels deep. An assembly
  brings in at most 1,000 part files and 100,000 placements, nested at most 32 deep. Each
  SolidCraft check stops after `verify_timeout` seconds. A message over 1 MiB is answered
  with an error and skipped.
- **Hostile files.** A damaged or unexpected file comes back as an error report, never as a
  crash of the server.
