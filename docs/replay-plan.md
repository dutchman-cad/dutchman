# Replay plans

`dutchman-replay` is the shared data model for script writers. It builds one ordered,
target-neutral plan per part from Dutchman's existing mapped history. It does not add a
command, select an output format, or change existing conversion output.

Typed quantities describe geometry in millimetres and angles in degrees. An emitter
decides which steps it supports and lowers the supported prefix into the target
program's units and calls. Emitters supply a fixed runtime script to read those calls as
data; the shared crate does not execute scripts. Text arguments are encoded as data.

## Plan document

The JSON schema identifier is `dutchman.replay-plan/1`. Fields and step kinds may be
added within version 1; incompatible changes require a new schema version. Readers
retain untyped options; typed records without an extra map refuse unknown fields.
Emitters refuse unknown step kinds and any option they do not handle, with a reason
identifying the feature and target program. Emitters decide whether feature names must
be unique.

The top-level fields, in serialization order, are:

| Field | Meaning |
| --- | --- |
| `schema` | `dutchman.replay-plan/1` |
| `generator` | Dutchman's name and version |
| `part` | Part name |
| `units` | `{"length":"mm","angle":"deg"}` |
| `source` | Format word, nullable `program_version`, nullable input `sha256` |
| `parameters` | Written parameters; source order when names are unchanged, otherwise name order |
| `steps` | Ordered rebuild steps; suppressed source features are omitted |
| `sketches` | Mapped reference sketches, including those beyond a history stop |
| `stop` | Null, or `{step_index, feature, reason}`; the index is zero based |
| `fallback` | `{"step_file":"original.step"}` |
| `expected` | Null, or independent expected geometry measurements |
| `outcomes` | Source feature name, kind, status and nullable reason |

A parameter has `name`, `expr`, nullable numeric `value`, `kind` (`length`, `angle` or
`number`), and nullable `source`. `name` is the name written by the mapper; `source` is
the original parameter name, or null when the mapper does not expose an unambiguous
source link. If any source parameter is renamed, duplicated, or absent from the written
values, all parameter source links are null and expressions use the mapped text.
Otherwise expressions retain the source text. Every quantity parameter link resolves in
`parameters`. Outcome statuses are `mapped`, `not_needed`, `left_out` and `blocked`;
names can be null when a source feature has no name.

Each step has a numeric `id`, string `kind`, `name`, `source: {id, ordinal}`, its kind's
fields, and an `extra` object. Every untyped row field stays in `extra`. Untyped parts
of sketch and axis records stay under `extra.sketch` and `extra.axis`; `to_row` merges
them back into their records. Emitters must declare handled options per step kind using
dotted paths, such as `extent.start_offset` or `curves.rho`. The adapter checks semantic
support and every extra map before lowering, including nested records and reference
sketches. Undeclared options are refused with the feature name and option path; the
shared writer always runs this check. A source link is filled only when the step name
uniquely matches a source outcome: then `ordinal` is that outcome's ordinal. Otherwise
both link fields are null. Readers must not rely on `source.id`; it is currently null
for mapped plans.

In-memory models without a recognised source format use `cadir` as the format word.
Current mapped plans leave `source.program_version`, `source.sha256` and `expected`
null. An adapter that has the original input bytes can fill the source digest. The plan
retains embedded STEP text only in untyped import rows; the complete fallback STEP is
written separately in the package. An expected measurement, when supplied, has `bodies`,
`volume_mm3`, `area_mm2`, `bbox_mm` (minimum and maximum 3D corners), and `by`
identifying the measuring program and version.

## Steps and stable references

| Kind | Typed fields |
| --- | --- |
| `sketch` | `plane`, `points`, `curves`, `constraints` |
| `extrude` | `sketch`, `profile`, `operation`, `extent`, optional `bodies` |
| `revolve` | `sketch`, `profile`, `operation`, `axis`, `angle`, optional `symmetric`, `bodies` |
| `hole` | Optional `sketch`, `points`; `form`, `diameter`, `extent`, `direction` |
| `fillet` | `radius`, `edges` |
| `chamfer` | `distance`, `edges` |
| `pattern` | `features`, `layout` |
| `mirror` | `features`, `plane` |
| `shell` | `faces`, `thickness`, `direction`, optional `body`, `tangent_chain` |
| `loft` | `sections`, `operation`, optional `bodies` |
| `sweep` | `sketch`, `profile`, `operation`, `path_sketch`, `path`, optional `bodies` |
| `coil` | `base`, `coil_axis`, `pitch`, `turns`, `operation`, optional sketch/profile or diameter/section fields |
| `sphere` | `center`, `radius`, `operation`, optional `bodies` |
| `torus` | `center`, `major`, `minor`, `operation`, optional `bodies` |

Options such as sweep rails and orientation stay in `extra` until explicitly handled.
Other emitted kinds, including `surface_revolve`, `surface_loft`, `surface_sweep`,
`box`, `cylinder`, `cone`, `thread`, `sheet_base`, `sheet_flange`, `delete_face`,
`offset_face`, `combine` and `split`, retain their row fields in `extra` for explicit
emitter support.

Step references use the mapper's numeric feature ids. Pattern and mirror seeds use those
ids instead of feature names. Within a sketch, points, curves and constraints use the
ids already supplied by the mapper, such as `origin`, `p1`, `l1` and `k1`. Curves name
points by id; constraints name points or curves by id. The fixed sketch origin is
`origin`. An out-of-range row index stops the plan with `feature X: a sketch reference
is out of range`.

A point is `{id, at: [x,y], fixed}`. A curve has `id`, `type` (`line`, `circle`, `arc`,
`ellipse`, `spline`, `conic`), its point references (`a`, `b`, `c`, `m` or `pts`),
optional radius `r` and spline `degree`, and `construction`. A conic names its `apex`
point by id and retains numeric `rho` as an untyped option. A pattern-point constraint
names its `center` by point id; its angle (radians), offset (millimetres) and directions
remain untyped options. Constraints have `id`, `type`, references such as `line`, `a`,
`b`, `c`, `p`, `q`, optional quantity `value`, and optional `driven`. Numeric text
constraint values carry `value_is_text: true`, so row conversion restores the JSON
string. Other constraint text retains its expression unchanged.

Planes are `{type:"origin", name:"XY"|"XZ"|"YZ"}` or `{type:"frame", origin:[x,y,z],
x_dir:[x,y,z], y_dir:[x,y,z]}` in part coordinates, or `{type:"offset", base:<plane>,
distance:<quantity>}`. Profiles are `{type:"all"}`, `{type:"loops", loops:[[curve id]]}`
or `{type:"regions", regions:[{outer:[curve id], holes:[[curve id]]}]}`, or
`{type:"points", points:[[x,y]]}` for interior picks in sketch coordinates. Revolve axes
are `{type:"line", origin:[x,y,z], dir:[x,y,z]}` or `{type:"sketch_line", curve:<curve
id>}`. Only unknown axis metadata stays in `extra.axis`. Coil directions are separate
vectors in `coil_axis`, restored as `axis` in mapper rows. Operations use `join`, `cut`,
`new_body` or `intersect`.

Extrusion extents use `type` (`distance`, `symmetric`, `two_sided` or `through_all`),
`direction` (`positive` or `negative`), and optional `distance` and `distance2`
quantities. A symmetric extent has a positive direction. Other mapper extent options are
preserved for explicit handling. A hole extent is `distance` with a `depth` quantity, or
`through_all`. Hole forms are `simple`, `drilled` (with `tip_angle`), `counterbore`
(`cb_diameter`, `cb_depth`) or `countersink` (`cs_diameter`, `cs_angle`). Rectangular
(`type:"rectangular"`) pattern layouts have `dir1`, `count1`, `spacing1`, and optional
second-direction fields. Path (`type:"path"`) layouts retain `path_sketch` (feature id),
`path` (curve ids) and `spacing` (mapper quantity text) in their extra map, with typed
`count`. Composite patterns map to rectangular layouts or sequences of pattern and
mirror steps. Circular layouts have `origin`, `axis`, `count` and `angle`. Typed counts,
angles and rectangular spacings are quantities, keeping the mapper's expression text as
well as the evaluated number; path spacing remains text in the layout extra map.

Entity references must identify exactly one entity in the target program. An edge
reference uses `at` (a point on the edge), optional `ends` and `length`, and a `proof`
word. A face reference uses `at`, `normal` and `proof`. A runtime must report a missing
or ambiguous match, for example `the edge at (x, y, z) was not found` or `the edge at
(x, y, z) matched 2 edges`. It must never choose the nearest entity.

`Step::to_row` returns `Result` and refuses unknown referenced ids. It restores the
mapper's vocabulary, quantity text and sketch array indices. For mapped rows, converting
to a plan step and back preserves the JSON value, including unknown row fields. Steps
created by hand must supply valid referenced ids.

## Quantities and partial history

A quantity is `{value, expr, param}`. `value` is a nullable finite number in millimetres
or degrees. `expr` keeps the row text exactly; `param` names a directly referenced
parameter, including a negated parameter, or is null.

Evaluation accepts a number with no unit or with `mm`, `cm`, `m`, `in`, `ft`, `deg` or
`rad`; enclosing parentheses; a leading minus; a parameter name; or one division of two
such quantities. Parameter resolution is bounded to 64 levels and 4,096 recursive
evaluations, and refuses cycles. Other expressions, division by zero and non-finite
results have a null value. Angle-valued constraints (`angle`, `line_angle`, `arc_sweep`)
retain their original radian number with a ` rad` suffix in `expr`, and give `value` in
degrees. Emitters must refuse quantities they cannot evaluate.

When mapping fails, Dutchman disables the source suffix beginning at the first blocked
outcome and maps the remaining prefix again. The original arenas and topology remain
available for resolving references. It retains the recovered steps and records the stop.
The emitter then accepts steps in order until its first refusal. Retained outcomes say
`rebuilt as reference in partial history`; later building outcomes become blocked.
Unretained sketches stay mapped with `kept as a hidden reference sketch`, unless
conversion or support fails, in which case their outcome records the reason. If mapping
fails without a blocked outcome, the complete model is mapped again. Partial output adds
`rebuilt N of M features; kept as reference` as a degraded note. The shared writer
reports `history`, `partial_history` or `import` according to the emitted prefix, and
leaves verification to the runtime script.

Plans and sketches are bounded to 100,000 entries; sketch entity arrays have the same
cap. Row strings use the project input size limit (1 GiB); nesting is capped at 64.

## Script data and packages

`steps.tsv` is UTF-8 with CRLF line endings. Readers must accept LF or CRLF. The adapter
writes the first unnumbered header record: `dutchman-plan-sha256/1<TAB><64 lower-case
hexadecimal digits>`. The digest is SHA-256 of the exact serialized `plan.json`,
including its trailing newline. The emitter receives the same digest as the third
argument of `lower(plan, upto, digest)`. Its TSV starts with a second header record,
`dutchman-steps/1<TAB><target>`. This crate supplies the encoding and adapter; each
emitter supplies its runtime.

Each following line is `<n><TAB><OP><TAB><arg>…`, numbered from 1. Operation names are
fixed uppercase identifiers supplied by the emitter. A bad operation name has its own
identifier error. Text arguments percent-encode `%`, `=`, `+`, `-`, `@`, all bytes below
0x20, and all bytes above 0x7E, using uppercase hex for each UTF-8 byte. For example,
tab becomes `%09`, newline `%0A`, percent `%25`, and `ü` becomes `%C3%BC`. Numbers use
Rust's shortest round-trip plain decimal representation with a dot and no exponent;
negative zero is written as `0`. Flags are `1` or `0`, and an absent argument is `-`;
the text `-` is `%2D`. Non-finite numbers are refused. The encoder allows at most
200,000 calls and 8,000 bytes per line, excluding its CRLF terminator. Failed pushes
append nothing.

The ZIP entries are written in this order:

1. `README.txt`, supplied by the emitter.
2. The fixed runtime script, with the emitter's file name.
3. `steps.tsv`.
4. `plan.json`, pretty-printed in struct field order with a trailing newline.
5. `original.step`, restated with Dutchman's guarded STEP encoder.
6. Any `Lowered.extra_files`, in the emitter's supplied order.

`Lowered` is non-exhaustive; use `Lowered::new(tsv)` and append extra files as needed.

Entries use deflate and a fixed modification time of 1980-01-01 00:00:00. With the same
build, equal entry names, bytes and order produce identical ZIP bytes. Entry names are
unique single file names; paths, colons and control characters are refused. Total input
bytes are capped at 512 MiB across all entries, including additional files.
`package::sha256` computes the lower-case hexadecimal SHA-256 of the exact `plan.json`
bytes, including its trailing newline; that digest identifies the plan in a runtime
result.

## Runtime result

`ScriptResult::read` reads `dutchman.script-result/1` JSON objects up to 65,536 bytes.
It checks every required field's type and caps strings at 2,000 Unicode characters.
Strings cannot contain control or bidirectional formatting characters. The plan digest
is exactly 64 lower-case hexadecimal digits. Volume and area are finite and nonnegative.
Unknown fields are ignored, with nesting bounded to 64 levels throughout the file.
Refusals use `That result file isn't one Dutchman can read: <field>.`

| Field | Type or values |
| --- | --- |
| `schema` | `dutchman.script-result/1` |
| `kind` | `rebuild` |
| `target`, `program_version`, `script_version`, `plan_sha256` | Strings |
| `verdict` | `pass` or `fail` |
| `check` | `passed`, `bodies`, `volume`, `area`, `bbox`, `not_run` |
| `steps_total`, `steps_done` | Unsigned 32-bit counts; done cannot exceed total |
| `failed` | Null or `{line, op, step_kind, message}` |
| `measured`, `original` | Null or `{bodies, volume_mm3, area_mm2, bbox_mm}` |
| `seconds` | Finite nonnegative number |

All fields are required, including those whose value can be null. `kind`, `verdict` and
`check` have typed Rust enums with these JSON spellings. Failure messages may contain
feature names and the program's error text. This crate only reads and validates results;
it supplies no runtime, measurement comparison, thresholds or result-file writing.

## Test data

The plan goldens use cadmpeg's public `examples::unit_cube` with synthetic circle,
extrusion, revolve, pattern and unsupported-suffix histories authored in the tests. The
plate example is hand-written geometry and measurements. Round trips cover all
SolidCraft document goldens and the exact rows extracted from the four FreeCAD macro
goldens. Additional synthetic models exercise sketch-line revolves, cosmetic threads,
sheets, face edits, body edits, imports and spatial sketches. Hand-written rows
supplement those models with deliberate unknown options, malformed references and
expression spellings that the mapper does not emit. No native design files or external
CAD models are added.

To regenerate the plan goldens deliberately, run the crate's tests with `BLESS=1`.
Review the changed plan bytes before accepting them.
