# Preflight

`dutchman preflight part.f3d --json` reads and plans a design without writing
output. The default target gets an evidence-backed history forecast when work
fits the budgets. Exit 0 includes unknown results; exit 1 means the input could
not be read. Unknown never means convertible.

Pre-flight forecasts small parts. With the default 750 ms and 64 MiB budgets,
large designs, including assemblies and `.f3z` archives over a few MB, commonly
report unknown rather than a guess. Compressed file size alone cannot establish
whether a design fits: decoding can exhaust the work budget or overrun the
cooperative deadline. Unknown is the intended result when evidence cannot be
collected within those budgets.

There are two tiers:

- `prefix` checks regular-file size, suffix policy and a bounded content prefix.
  It establishes definite refusals and format hints. Prefix recognition alone
  cannot prove readability, source version, document contents or history.
  Shared part suffixes accept only their existing reader families. A strong
  incompatible identity is refused as `format_mismatch`; an unresolved identity
  stays unknown.
- `planned` decodes at design depth, validates the neutral model, resolves
  assembly parts through the existing reader and runs the default writer's
  convertibility check and history planner. Design depth includes geometry;
  process data is skipped. No design container is generated or written.

The planned forecast reports full history, partial history with mapped and total
active feature counts and the first blocking feature's kind and reason, solid
only, or refused. Suppressed features are excluded from active counts; features
that need no target row count as mapped. Leaves without exportable geometry
contribute no timeline outcomes, matching conversion. Kind lists come from the mapper,
rather than a duplicated support table. Version, topology, operands and selections can
make two features of the same kind map differently. Missing facts stay null.
A source-kind census stays unknown when the mapper covers only a subset of the
decoded rows; per-target driving kinds still describe its own evidence.
Source version remains unknown because no stable source-version accessor is
available in the common model.

The forecast targets the newest release known at build time, reported as
`target_version`; it does not discover an installed executable.

The other three target ids remain present with unknown forecasts: their writers
do not expose an equivalent planning-only path. In particular a default-target
history mapping cannot establish a macro's supported operations or drawing-page
encoding. A failed design decode does not establish whether drawing-only output
can be produced. Preflight does not probe target installations or run external
replay. A forecast describes static mapping with ordinary output resources
available; final replay, output encoding, free disk space and write permissions
remain conversion checks. It cannot promise successful geometry evaluation.

Library callers use `dutchman_core::preflight::{scan, scan_bytes, prepare,
Budget}` for prefix or decoded evidence, and `dutchman::preflight::preflight`
for target forecasts. Defaults are 64 MiB of source reads, 750 milliseconds and
10000 features. These are work ceilings, not measured performance estimates.
Prefix reads additionally stop at the detector's 128 KiB ceiling. Planning
charges the prefix and every part-file read, including repeated source reads;
a file that fits the budget alone can therefore still exceed it. Expansion and
retention use the existing decoder resource limits, additionally capped by the
byte budget per decode. Existing entity and nesting limits remain in place.
Features are counted immediately after guarded decoding, before model
validation and mapping; this is not an allocation-time feature limit. Aggregate
model feature counts are checked again after local component extraction.

Time is checked before and after decoding, between assembly files, before and
after planning, and between mapped history features and reference sketches.
An elapsed budget sets
`stop_reason: time_budget` and discards all forecasts. Calls into native readers,
filesystem operations, model validation and individual mapping operations cannot
be interrupted. The deadline is cooperative: a slow operation may overrun it.
Hosts needing a hard deadline must isolate the call. Byte and feature exhaustion
also return unknown forecasts with explicit stop reasons. Decoder resource
exhaustion uses `resource_limit`, keeping it distinct from an unreadable file.

`scan_bytes` accepts borrowed upload bytes for the core library's memory-only
prefix tier. The host bounds the upload. The repository has no browser plugin
binding shim; the planned filesystem/clock API needs binding and host clock
support before browser exposure. The web server still produces batch conversion
reports. No web endpoint or MCP tool is added here.

See [JSON reports](json.md#preflight) and the separate
[cheaper bounded reader follow-up](https://github.com/cadmpeg/cadmpeg/issues/1662).

## Existing seams and cheap evidence

| Location | Existing behavior | Preflight use |
| --- | --- | --- |
| `dutchman-core/src/decode.rs` | Guarded detection, decode, validation and resource limits | Prefix detection and design-depth decode |
| `dutchman-core/src/format.rs` | Suffix policy and shared-suffix content resolution | Suffix table plus bounded detection |
| `dutchman/src/choices.rs` | Measured size and memory estimates | Estimates alone do not establish history |
| `dutchman/src/lib.rs` | Inspect reads the model and calls the writer planner | Same mapping path, with deadline checks |
| `dutchman/src/batch.rs` | Inventory, assembly grouping and output planning | Existing conversion reports remain separate |
| `dutchman/src/json.rs` | Shared CLI and MCP report serialization | Advisory preflight serialization |
| `dutchman-web/src/report.rs` | Scrubs paths and adds downloads | Presentation after conversion |

Cheap header evidence can establish physical size, a strong format identity and
suffix conversion policy. Format-specific inspectors sometimes expose dialect
and container entry metadata. No common bounded reader exposes ordered feature
rows, source version or a trustworthy part/assembly classification. Those need
more reading today. Readable geometry, resolved operands, assembly references and
actual history mapping require design decoding; successful replay requires full
conversion checks. Compressed size does not bound expanded content.
