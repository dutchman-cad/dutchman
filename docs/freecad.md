# FreeCAD documents (`.FCStd`)

FreeCAD documents convert through cadmpeg's `fcstd` codec. Checked in SolidCraft, all 19
of the test documents (pads, pockets, revolves, holes, fillets, chamfers, shells and a
three-part assembly) come out at FreeCAD's volume. Before 0.1.5 FreeCAD documents converted
in the nightly builds only: cadmpeg 0.6.3 stored some faces as replicas of curves, kept each
feature's intermediate shape as a body of its own and could turn revolved faces inwards.

When a feature blocks SolidCraft history conversion, earlier editable features and resolved
sketches are kept in a hidden **Editable history (partial)** component. The original final
shape remains visible as an Import after the kept history. Show the hidden component to
continue editing it. The report names the blocker and counts retained source features;
`mode: partial_history` and verification `partial` distinguish this from full verified
history. The reason says whether retained features replayed or remain unverified. Later
sketches with unresolved planes or external geometry are reported and left out. A sketch
that fails replay is retried without constraints, with the loss reported. If there is
no body-building prefix, resolved sketches remain hidden beside the imported shape for
reference. A final shape mismatch preserves the mapped timeline for correction when it
replays; it does not identify which feature needs correction. SolidCraft's numerical eval
includes hidden bodies as well as the visible final shape.

## What converts

- **Geometry**, written as a SolidCraft import and checked in SolidCraft like any other.
  Only the bodies FreeCAD shows convert: a body cadmpeg marks hidden (`visible: false`, as it
  marks the shape of each step inside a PartDesign Body) is left out, with the geometry only
  it uses, and the report says how many were (`Info`). A curve or surface the shown bodies use
  keeps its construction, such as the full ellipse a trimmed elliptical edge is cut from, even
  when that construction is stored with a hidden step. A document that hides every body
  converts all of them, as stored.
- **Components**, when cadmpeg says which bodies each owns, as for a Fusion design.
  External `App::Link` references to Bodies and Parts in other `.FCStd` documents convert
  as geometry in component occurrences at their stored placements, including assemblies with
  no geometry in the assembly document itself. Link arrays place each element once.
  Dutchman looks for the referenced file beside the assembly first, then uses the shared
  [project and search folders](inventor.md#finding-the-part-files) (`--project`, `--search`).
  A missing or unreadable linked file is left out and reported by name; the other components
  still convert. Cyclic links are left out and reported, with nesting limited to 32 levels.
  Links that name only a document identifier, or select subelements, are not supported yet.
- **Grounded joints** hold their part fixed to the design where it is stored, as a rigid
  joint to the design; the assembly's revolute and slider joints keep their motion.
  Assembly-workbench joints use the same mapping as Fusion joints when cadmpeg resolves
  their operands and frames. Joints that cannot be carried are reported; the parts keep
  their stored placements.
- **Feature history**, when every feature maps: sketches, pads, simple, counterbored and
  countersunk holes, constant-radius fillets, and equal-distance chamfers, replayed and
  checked in SolidCraft like a Fusion history. Thicknesses (shells), counterbores or
  countersinks with a drill point, and
  revolutions short of a full turn are not mapped yet; a document holding one converts as
  geometry, and the report names the first feature that stopped it.

Hidden container visibility takes precedence over a child's own shown flag, including
nested Parts, groups, and PartDesign Bodies. A body hidden only by its source container can still supply a separately shown occurrence.

Unused binders and datum helpers do not block feature history. They are reported as
`not_needed`, with an Info note: `helper not converted: nothing uses it`. Suppressed later
features do not count as consumers. A used datum point or coordinate system needs no
separate feature when the dependent sketch carries its resolved plane or a pattern carries
resolved axes. Resolved datum constructions can carry earlier helpers in the same way.
Other uses keep the history flattened with a specific placement reason.
A simple binder over a resolved construction plane can likewise be carried in its attached
sketches. A binder used by a later feature remains a blocker when its source geometry
reference is not resolved; the report says `a binder used by a later feature is not mapped yet`.

MultiTransforms containing resolved mirrors require a target of SolidCraft 0.25.1 or newer
when a stage patterns or mirrors an earlier stage. Older targets report
`needs SolidCraft 0.25.1 or newer` and keep the original geometry. Single-stage mirrors
and patterns still map on every supported target. These MultiTransforms keep their stage
order as a chain of SolidCraft
mirror and pattern features. Each stage repeats the complete instance set from the previous
stage. Full-turn polar patterns with a repeated seed endpoint keep every distinct location
once. Replay checks body count, volume, and area against the stored original; a chain that
cannot reproduce it falls back to geometry with the replay reason. Unresolved mirror planes,
body-seeded composite mirrors, and progressively scaled stages remain unsupported.
An attempted replay that cannot finish also keeps the original geometry for these chains;
with verification off or no usable CLI, history remains unverified.

Some polar patterns referring to the document's Z axis currently receive another axis or
an incorrect endpoint from the decoder. These do not have verified editable history yet.
Replay differences and unfinished replay checks of repeated full-turn endpoints fall back
to geometry, as reported. With verification off or no usable CLI, history remains unverified.
The endpoint mapping does not repair a misdecoded axis or instance set.

Whole-sketch hole placements exclude construction geometry, including reference origin
points. Explicit entity selections can still use construction points or circle centres.

Fillets and chamfers need resolved named edges of their base feature's stored shape,
before the blend.
Dutchman preserves those edges when it removes the hidden intermediate shapes. It accepts
straight edges and open circular arcs only when their midpoint identifies exactly one edge
on SolidCraft's replay at that step, with matching ends and length. An ambiguous pick, a
missing base shape, a closed edge, or another curve stops the editable prefix, with the
reason reported. This requires a SolidCraft CLI to replay the history; `--no-verify` cannot
prove blend edges. Final-body edge numbers are never substituted for base-feature edges.

FreeCAD signed horizontal and vertical sketch distances convert to positive dimension
magnitudes at their solved positions. Their signed parameters and expressions remain
available; a dependent magnitude parameter drives each negative distance in SolidCraft.

## Spreadsheet parameters and expressions

Numeric spreadsheet cells become editable SolidCraft parameters. Aliases keep their names;
unaliased cells include their sheet name, with punctuation replaced by underscores
(`Sheet.A1` becomes `Sheet_A1`). Formula cells can remain live without a saved evaluated
value. Text and empty cells are omitted silently. An unused numeric cell is retained with
an `Info` note; the spreadsheet grid and formatting are not reproduced.

Expressions such as `Parameters.plate_w / 2` bind to the parameter owned by the named
object, including cell addresses and aliases. Renamed parameters remain linked through
formulas. Length units such as `mm` and `in`, angles in `deg`, arithmetic, `pi`, and supported
functions such as `min`, `max`, `abs`, `sqrt`, and degree-based trigonometry remain live.
Unsupported functions, conditionals (`? :`), unresolved or ambiguous references, and cycles
use the saved scalar value when available, with a specific reason. A formula with neither
a supported expression nor a scalar value is dropped with that reason.

Only bindings supplied by the decoder can be restored. Named sketch constraint bindings
saved as `.Constraints.width` are not projected by the current decoder; their dimensions
retain their evaluated values. Non-sketch feature dimensions such as pad depth are fixed
scalars in the current neutral model, without parameter links to their geometry inputs;
a live formula in the parameter table alone cannot drive those dimensions. Object-label
references also need an unambiguous decoded label identity. A verified initial volume checks the saved shape; it does not prove that
all source dependencies remain editable. Check the resulting geometry after editing a
parameter. A flattened design retains only parameters needed by its reference sketches.

Constant-radius AdditiveHelix and SubtractiveHelix features keep their sketch profile,
axis, pitch/height/turn law, handedness, reversed direction, and join/cut operation as
sketch-profile coils when the target is SolidCraft 0.25.1 or newer. Choose a target with
`--solidcraft-version x.y.z` on `convert`, `convert-all`, or `watch` (at least 0.18.2).
The default is the found CLI's version, even with `--no-verify`, else the newest release
known at build time (currently 0.25.1). The rolling 14-day window is currently
0.23.0–0.25.1. Older targets block the helix with
`needs SolidCraft 0.25.1 or newer` and the target version, preserving the original
geometry. Resolved feature parameter
bindings drive the coil dimensions; dimensions without bindings keep their values.
Spreadsheet parameters follow the usual expression/value fallback. Conical helices and
flat spirals are not supported yet: SolidCraft's sketch-profile coil has no cone angle or
radial growth. Replay verification still applies, including fallback when a profile or
Boolean operation cannot rebuild.

## Building with FreeCAD

FreeCAD support is the `freecad` cargo feature of `dutchman` and `dutchman-gui`, on by
default. A build without it says it leaves FreeCAD files out:

```sh
cargo build --release -p dutchman -p dutchman-gui --no-default-features
```

## Testing without FreeCAD

The FreeCAD fixtures in `crates/dutchman-core/tests/fixtures/` are cadmpeg's corpus
documents, authored in FreeCAD 1.1.1 by the cadmpeg project and dedicated to the public domain
(CC0-1.0). See that folder's README for where each came from.

## Writing rebuild macros

`--to freecad` writes a `.FCMacro` independently of `.FCStd` input support.
`--to fcstd` runs FreeCAD to save and verify a native document; `--to freecad --verify`
checks a macro without saving a native document. See
[FreeCAD output](guide.md#freecad-output-experimental) for coverage, fallback and execution.

The writer is Rust and emits calls to FreeCAD as an outside Python program. It does not
link or vendor FreeCAD. Its interface references are the public
[Sketcher scripting](https://wiki.freecad.org/Sketcher_scripting),
[Part scripting](https://wiki.freecad.org/Part_scripting),
[Pad](https://wiki.freecad.org/PartDesign_Pad),
[Revolution](https://wiki.freecad.org/PartDesign_Revolution),
[Linear pattern](https://wiki.freecad.org/PartDesign_LinearPattern),
[Group](https://wiki.freecad.org/Std_Group) documentation, and the
[public API reference](https://freecad.github.io/API/). No FreeCAD implementation source is
needed to generate or test the macro text.

Partial rebuilds retain the longest mapped prefix and every mapped sketch in a hidden
**Rebuilt history (partial)** group. When that prefix differs from the final body, the
embedded STEP is the visible result and the report says `rebuilt N of M features; kept
as reference`. Only the final shape participates in verification; reference features do
not add bodies or volumes. This output remains experimental; it is verified only when FreeCAD runs it headlessly
(`--to fcstd` or `--verify`).

The synthetic macro regression tests can also run in a prebuilt FreeCAD command-line
binary when `FREECAD_CMD` names that executable, including a final-volume check beside a
hidden partial rebuild.

Assembly output also follows the public [Assembly workbench](https://wiki.freecad.org/Assembly_Workbench),
[joint properties](https://wiki.freecad.org/Assembly_CreateJointFixed), and
[grounding properties](https://wiki.freecad.org/Assembly_ToggleGrounded) documentation.
The Assembly object, the `JointObject.Joint` and `JointObject.GroundedJoint` constructors, and
`solve()` were exercised through the installed FreeCAD 1.1.4 runtime on synthetic parts.
Only runtime calls, properties, and signatures were inspected; no FreeCAD source was read.
The unit tests include joint data assertions and a hinge golden. Set `FREECAD_CMD` to a
FreeCAD command-line executable (or a wrapper accepting `-c CODE`) to run the synthetic
macro checks for every mapped joint family, including hinge and slider motion about their
own axes, a sub-assembly solved inside its assembly, grounding, limit clamping, and volume
preservation. Joint coverage and specific fallback cases are listed in the guide.

Headless measurement and saving use the public [topological data scripting](https://wiki.freecad.org/Topological_data_scripting)
and [FreeCAD scripting basics](https://wiki.freecad.org/FreeCAD_Scripting_Basics) APIs:
`Shape.Solids`, `Volume`, `BoundBox`, document `recompute()` and `saveAs()`.
