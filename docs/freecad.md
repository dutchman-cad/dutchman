# FreeCAD documents (`.FCStd`)

FreeCAD documents convert through cadmpeg's `fcstd` codec. Checked in SolidCraft, all 19
of the test documents (pads, pockets, revolves, holes, fillets, chamfers, shells and a
three-part assembly) come out at FreeCAD's volume. Before 0.1.5 FreeCAD documents converted
in the nightly builds only: cadmpeg 0.6.3 stored some faces as replicas of curves, kept each
feature's intermediate shape as a body of its own and could turn revolved faces inwards.

## What converts

- **Geometry**, written as a SolidCraft import and checked in SolidCraft like any other.
  Only the bodies FreeCAD shows convert: a body cadmpeg marks hidden (`visible: false`, as it
  marks the shape of each step inside a PartDesign Body) is left out, with the geometry only
  it uses, and the report says how many were (`Info`). A document that hides every body
  converts all of them, as stored.
- **Components**, when cadmpeg says which bodies each owns, as for a Fusion design.
- **Grounded joints** hold their part fixed to the design where it is stored, as a rigid
  joint to the design; the assembly's revolute and slider joints keep their motion.
- **Feature history**, when every feature maps: sketches, pads, and simple, counterbored and
  countersunk holes, replayed and checked in SolidCraft like a Fusion history. Fillets,
  chamfers, thicknesses (shells), counterbores or countersinks with a drill point, and
  revolutions short of a full turn are not mapped yet; a document holding one converts as
  geometry, and the report names the first feature that stopped it.

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

`--to freecad` writes a `.FCMacro` independently of `.FCStd` input support. See
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
not add bodies or volumes. This output remains experimental and unverified.

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
