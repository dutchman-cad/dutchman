# Machine-readable reports

`dutchman convert --json` and `dutchman inspect --json` print one JSON object on stdout and
nothing on stderr. The `dutchman-mcp` server's tools return the same objects, as structured
content and as text, with `isError` set when `ok` is false. The schema is named in `schema`. Within a schema version, fields are only
added: none is renamed, retyped or removed. A change that would break a reader gets a new
version (`dutchman.convert/2`).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | The design was converted (`convert`) or read (`inspect`). Losses do not change this. |
| 1 | It could not be: unsupported or unreadable input, no geometry, an output that could not be written. With `--json`, the report has `"ok": false`. |
| 2 | A usage error (unknown option, missing argument). clap's usage text is printed on stderr; there is no JSON. |

## `dutchman.convert/1`

```json
{
  "schema": "dutchman.convert/1",
  "ok": true,
  "input": {"path": "part.f3d", "format": "fusion_archive", "format_name": "Fusion archive (.f3d)"},
  "output": {"path": "part.solidcraft", "format": "solidcraft/23"},
  "mode": "history",
  "verification": {"status": "verified", "reason": null},
  "history": {
    "mapped": 2,
    "total": 2,
    "features": [
      {"name": "Base sketch", "kind": "sketch", "status": "mapped", "reason": null},
      {"name": "Block", "kind": "extrude", "status": "mapped", "reason": null}
    ]
  },
  "losses": [
    {"level": "Info", "code": "step/native.source-record-omitted", "subject": "step/native.source-record-omitted",
     "message": "45 source-native record(s) were not represented in STEP"}
  ]
}
```

| Field | Meaning |
|---|---|
| `ok` | `true` when the design was written. |
| `input.format` | `fusion_archive`, `fusion_multi_document`, `inventor_part`, `inventor_assembly`, `solidworks_part`, `solidworks_assembly`, or `null` for an unknown extension. `format_name` is the same for people. |
| `output.format` | The SolidCraft design format written. |
| `mode` | `history`: editable feature history, with the original geometry kept beside it, suppressed. `import`: the geometry alone. |
| `verification.status` | `verified`: the history was replayed in SolidCraft and matches the original. `unverified`: the history was written without that check (`reason` says why: no `solidcraft-cli`, a timeout, `--no-verify`). `flattened`: the design has a history but it was not written (`reason`: the first feature that does not map, or `replay differs (…)`). `not_applicable`: there was no history to write, or `--import-only`. |
| `history.features` | Every source feature in timeline order. `status`: `mapped`, `not_needed` (the target needs no feature, e.g. a principal datum plane), `left_out` (suppressed in the source), `blocked` (does not map; `reason` says why). |
| `history.mapped`, `history.total` | Mapped or not-needed features, out of all but left-out ones. |
| `losses` | What did not survive intact. `level` is `Info` (no effect on geometry), `Degraded` (carried in a weaker form) or `Dropped` (left out). `code` is cadmpeg's stable loss code when cadmpeg reported it, else `null`; `subject` says what was affected; `message` is for people. |

A failure carries `"ok": false`, `input` and `error.message`, and exits 1:

```json
{"schema": "dutchman.convert/1", "ok": false,
 "input": {"path": "assembly.sldasm", "format": "solidworks_assembly", "format_name": "SolidWorks assembly (.sldasm)"},
 "error": {"message": "SolidWorks assembly (.sldasm) files are not supported yet. …"}}
```

## `dutchman.convert-all/1`

`dutchman convert-all --json --out-dir DIR <files and folders>…` converts every file named, and
every file Dutchman knows in the folders named (with their subfolders; symbolic links are not
followed), several at a time (`--jobs N`, default: the machine's cores, at most 8). Each
design goes to `DIR`, keeping the folder layout below each folder named. A file that fails
does not stop the others.

```json
{"schema": "dutchman.convert-all/1", "ok": false, "converted": 2, "failed": 1,
 "results": [ {"schema": "dutchman.convert/1", "ok": true, "input": {…}, "output": {…}, …},
              {"schema": "dutchman.convert/1", "ok": false, "input": {…}, "error": {…}} ]}
```

`results` holds one `dutchman.convert/1` report per file (inputs that are neither a file nor a
folder come first), in the order found. `ok` is `true` only when every file converted; the
exit code is then 0, else 1.

## `dutchman.formats/1`

`dutchman formats --json` lists the native formats Dutchman knows:

```json
{"schema": "dutchman.formats/1", "ok": true, "formats": [
  {"id": "fusion_archive", "name": "Fusion archive (.f3d)", "extension": "f3d",
   "status": "converts", "converts": "geometry; feature history (…) when every feature maps; …"}
]}
```

`status` is `converts`, `coming` (in progress) or `not_supported`; `converts` says, for people,
what carries across.

## `dutchman.inspect/1`

The same fields as `convert` except `output`, plus `geometry`, and for an assembly
`assembly: {"parts": <part files read>, "placements": <parts placed>}`:

```json
{
  "schema": "dutchman.inspect/1",
  "ok": true,
  "input": {"path": "part.f3d", "format": "fusion_archive", "format_name": "Fusion archive (.f3d)"},
  "geometry": {"bodies": 1, "faces": 6},
  "mode": "import",
  "verification": {"status": "flattened", "reason": "feature \"Fillet1\": fillet is not mapped yet"},
  "history": {"mapped": 2, "total": 3, "features": [ … ]},
  "losses": [ … ]
}
```

`inspect` writes nothing and runs no SolidCraft, so a history that would be written reports
`verification.status` `unverified`. Both reports look at every feature, so every `blocked` one
is listed; `verification.reason` names the first.
