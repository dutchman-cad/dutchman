# Getting started

The desktop Designs list shows one complete estimate sentence for a file, folder, or selection.
Folder and selection summaries use the highest per-file memory estimate and sum the displayed
time estimates; hover over the summary for each file’s sentence. Folder paths wrap and show
the full path on hover. The path and estimate share one paragraph that wraps as needed,
keeping the complete sentence visible at every window width. The compact header keeps the
default window’s controls in view. Estimate values in commands, tools and JSON reports are unchanged.

Onshape Derived inserts from captured, pinned Part Studios become linked Insert Design
components when targeting SolidCraft 0.44 or newer, with supported body booleans kept editable.
Select target 0.44 explicitly when the existing default is older.
Keep the generated `derived-<sha256>.sc` companion designs beside the main result;
edit a companion and use SolidCraft's Update Links to rebuild the host. If conversion
finds an edited companion, choose another output folder; that companion is kept even
when overwriting is enabled. Parallel conversions safely share identical companions.
Earlier targets
report `design/reference.frozen` and keep frozen derived bodies. Missing references,
part subsets, configurations or transformed inserts name the reason under
`onshape/history.not-supported`; fetch a new bundle for missing references.

Derived Boolean conversion requires an explicit stored `keepTools` boolean. Missing or
contradictory choices retain `onshape/history.not-supported`; later records still convert.
Captured studio responses are checked against their own manifest microversion, independently
of the root studio. Fetch a new bundle when the reported reference evidence is missing.

See [Onshape bundles](onshape.md) for the supported reference forms.

Onshape history dependencies follow consumed operands in their stored order. Deterministic
body, face and edge selections need a unique earlier producer proved by rollback
transitions; unresolved or ambiguous producers retain `onshape/history.not-supported`.
A created-by query may name the proved creator of an entity that later features
modified; its dependency still follows the latest modification. Missing creation
proof, unrelated producers and IDs reused after disappearance remain refused.
Every present scope query list is validated, while an empty list keeps implicit scope.
Thread depth follows explicit `isTappedThrough` and `tappedDepth`, independently of bore depth.
A record the reader cannot convert becomes a named loss and the remaining records continue.
Unreadable or oversized members use `onshape/member.not-supported`; malformed features
remain Native with `onshape/history.not-supported`. Container admission and exhausted
caller resource budgets can still stop conversion. Inspect the reported losses before
relying on editable history.
Open, one-sided thin walls retain directed left/right data in the reader; writing
those sides is not supported yet, so the final STEP geometry remains.

Onshape sketch constraint losses name the source constraint kind and why it could not
be mapped. The solved sketch geometry remains; inspect these losses even when body
verification succeeds. Origin coincidences and midpoint construction carriers retain
editable relations. See [Onshape bundles](onshape.md).

The desktop app also accepts pasted files, paths, Onshape links, and confirmed HTTPS
downloads; see [pasting files and links](desktop.md#paste-files-and-links).
**Onshape connected ✓** opens the account dialog with the Part Studio link field.
This adds no CLI flags, MCP tools, or report fields.

Desktop Onshape jobs keep the `.onshape-bundle` beside the `.sc`; **Show bundle**
reveals it and **Send to Dutchman** opens private report review with it attached.
To share a document with us, send the .onshape-bundle; never send your keys.

Features whose source records that they changed no geometry are reported as `left_out`
with reason "changes no geometry in the source"; later features can keep editable history.

If a reader supplies only CADIR v11 design history, the final geometry is kept and
the report says its ordered selections and evaluation states are not mapped yet.
See [the port audit](cadir-v11.md) for the pinned reader contract.

Local Onshape bundles (`.onshape-bundle`) convert through the same command:
`dutchman convert bracket.onshape-bundle bracket.sc`. The reader works offline on
one Part Studio, including sketch-only bundles without STEP. Supported history includes
built-in-plane sketches, tangent chains, construction-line revolve axes,
counterbore/countersink holes, midpoint relations, assigned variables and symmetric
blind extrudes. Variables become SolidCraft parameters that keep sketch dimensions,
extrude depths and revolve angles driven. Up-to-next extents map when exact intersections with stored input faces prove coverage
and the first termination over the complete closed profile. Face/cap sketch attachments
need a stored complete frame; unresolved cases report `onshape/history.not-supported`.
Up-to-face extrudes retain their selected planar input face. A face
at a slant to the sketch needs SolidCraft 0.44 or newer as the target. Older targets
retain a named loss and the final geometry. Unresolved selections and unsupported
expressions retain named losses.
Ellipses and elliptical arcs keep editable geometry. Variables keep their units and
expressions, including when conversion falls back to STEP with no usable sketch.
Thin walls map from stored thicknesses, side and direction controls, and a proved
sole input-body selection for modifying operations. Missing or contradictory controls
retain the final STEP geometry with `onshape/extrude.thin-not-supported`.
Repeated ellipse carriers use solved chord endpoints and intersection records to retain distinct trimmed regions.
Asymmetric chamfers explain the missing input reference-face ordering.
Unproven open dividers and exhausted cumulative region-recovery budgets produce
`onshape/history.not-supported` before writing, including with verification off.
Simplify the sketch or split the bundle when the recovery budget is exhausted;
the final STEP shape remains available. Native symmetric distances and parameter
fallback defaults stay unchanged.

Through-all holes omit the drill-point angle because no bottom forms, for every entry form and
every target down to SolidCraft 0.18.2. This preserves the geometry without a loss note. Blind
and to-face counterbores, countersinks, and chamfered entries retain their point on SolidCraft
0.22.1 and newer; older targets retain the existing refusal and original geometry.

Holes ending at a proven earlier planar cap can retain both an entry countersink and
a drill point. For Fusion Extents To, the cylindrical shoulder reaches the face and
the tip extends beyond it. An unproven target still blocks body-building history.

Onshape quantities need a dimensioned expression or explicit supported units.
Malformed or repeated Boolean operands, unresolved latest variable bindings,
face/cap sketch attachments without a stored complete frame, up-to-next extents without
exact stored-target coverage, ambiguous directed region boundaries
and missing required hole facts retain `onshape/history.not-supported` with a specific
reason. A dropped sketch dimension instead uses
`onshape/sketch.constraint-not-supported`, preserving the solved geometry.
Blind holes need an explicit unique tip angle; angled bottoms also need Boolean
`depthToTip`. Hole standards need explicit hand and modeled/cosmetic facts.
Inspect these losses before relying on editable history; the final STEP shape remains.

See [Onshape](onshape.md) for editable features, required rollback
responses, and the STEP fallback when a feature is not supported yet.
Bundles marked `consistent: false` are refused; fetch again or use a version link.
Older v1 bundles without consistency metadata still convert. Default-plane sketches
and explicit sketch-region extrudes also accept the API's compressed query records;
ambiguous boundaries retain a specific history loss and the final STEP geometry.
Custom FeatureScript features name why history stops: bundle format 1 lacks complete
per-feature body geometry to prove a frozen step. Use the retained final STEP shape;
fetching another v1 bundle does not add the missing shapes. Later body-building features
remain blocked.

## Public share links

Paste a public Fusion or Google Drive **file** share link into the desktop app's link
field and choose **Add link**, paste it when no text field is focused, or drop a saved
`.url` or `.webloc` link shortcut into the window. The file downloads in the background,
then joins the same queue as local files. Choose an output folder and convert as usual.
URI text drops also work when delivered by a desktop integration. Some desktop/browser combinations do not deliver
raw URL drops through the window toolkit; paste the link in that case.

The CLI accepts the same links:

```sh
dutchman convert 'https://drive.google.com/file/d/FILE_ID/view' -o design.sc
```

Quote links in a shell. With no output path, the CLI writes in the current folder using
the downloaded file's name; an existing default output still requires `--force`.
Project ZIP links use the existing bounded project extraction and batch conversion.

Only public files needing no login work. Google Drive file links use
`https://drive.google.com/uc?export=download&id=FILE_ID`, including Drive's virus-scan
confirmation page. Drive folders and Google Docs documents are not file downloads.
Fusion links from `a360.co` or `*.autodesk360.com` work when the share page exposes a
public download URL or download link. If downloads are disabled or require login,
Dutchman asks you to download the design yourself and drop the file into the app.
Private and expired links get the same plain explanation.

Downloads use HTTPS, a 200 MB cap matching the web app's default upload limit, a two-minute
total timeout across DNS, connection, TLS and response streaming, and at most twelve
requests. Every redirect and confirmation action is checked before contacting its host.
Resolved addresses must be public unicast: local, private,
link-local, CGNAT, reserved and cloud metadata destinations are refused, including
IPv4-mapped IPv6 forms. Each connection uses the screened DNS answer without a second
lookup. Drive downloads stay on `drive.google.com`,
`drive.usercontent.google.com`, or `googleusercontent.com` and its subdomains; Fusion
stays on `a360.co`, `autodesk360.com` and its subdomains, or `autodesk.com` and its subdomains. Other download hosts require a manual download.

The original share link is never included in reports or sent to another service as a
referrer. Dutchman uses no browser login or credentials and does not load the page's
scripts, images, or other resources. Downloaded files live in a private temporary folder;
failed downloads are removed immediately. The CLI removes its download after conversion,
errors, Ctrl-C, and intentional exits (including a closed output pipe); the app retains downloads while queued files or result cards need them, then removes
them when cleared or when the app closes. Converted outputs stay in your output folder.

## Ask your assistant about a saved desktop result

A desktop conversion saves `<stem>.report.json` beside its design (`.failed.json` for a
failure), using the CLI's full per-file JSON schema. These reports contain private local
paths. A converted card with losses (and a failed card) has **Ask your AI about this file**: it
copies a short prompt, shown under the button too, naming the original, the design, the saved
report and the run's log. Paste it into an assistant with local file and command access.
It sends the assistant to the installed `AGENTS.md` (or its GitHub copy when absent), which
explains how to read them and the `dutchman` commands to use in a fresh scratch folder. It does
not require a server connection and asks before sending anything. See [desktop usage](desktop.md#ask-your-ai-about-this-file).

## Project inputs

Files whose names start with `~$` are lock files. They are ignored in project planning
and excluded from job, failure and skipped-file counts.

ZIP directory data and ZIP64 extensible records are limited to 16 MiB each before
the ZIP directory is allocated; entry budgets count every declared entry.

Folders and project ZIPs can be passed directly to `dutchman convert` or `convert-all`.
The default output is a new `Project (SolidCraft)` folder beside the input. Project planning
selects top-level Inventor and SolidWorks assemblies by their references, converts them
with parts resolved inside the project, then converts parts no assembly uses. Same-name adjacent STEP exports are fallbacks
for failed native conversions. See [Project inputs](command-line.md#project-inputs).

Dutchman Convert turns native CAD files into [SolidCraft](https://github.com/tangency-labs/solidcraft)
designs (`.sc`). This page takes you from install to an open design in four steps: install, convert, read the report, and open the result; the
[docs list](../README.md#docs) has everything else.

What changed in each release: the [0.2.6 release notes](../release-notes/0.2.6.md) and the
[changelog](../CHANGELOG.md).

A design stores its geometry compressed, so a large assembly stays small: SolidCraft refuses design files over its limit (256 MB by default).

## Install

- **Mac:** `brew install --cask dutchman-cad/tap/dutchman-convert` for the app, or
  `brew install dutchman-cad/tap/dutchman` for the commands.
- **Windows:** the [per-user installer](install.md#windows-the-installer) adds the app to Start
  and the commands to your user PATH without administrator rights. Or `scoop bucket add dutchman-cad https://github.com/dutchman-cad/scoop-bucket`,
  then `scoop install dutchman-cad/dutchman`.
- **Any system:** a [release archive](install.md#from-a-release-archive), [Docker](docker.md),
  or [from source](install.md#from-source).

[Install](install.md) has every option, checksums, and the one-time
[first-run approval](install.md#opening-the-app-the-first-time) on macOS. Install
[SolidCraft](install.md#solidcraft-optional-recommended) too: Dutchman uses it to check each
converted history. The supported SolidCraft window is currently 0.40–0.45.

## Your first conversion

You need a native CAD file. Dutchman has no public sample designs yet, but its tests use
small synthetic files that are fine to try: they are in
[`crates/dutchman-core/tests/fixtures/`](../crates/dutchman-core/tests/fixtures/), with
where each came from in that folder's README. None is a real design. `solidworks_cube.sldprt`
is a 10 mm cube; `generated_unit_cube.f3d` and `synthetic_cube.ipt` are the same cube as
Fusion and Inventor files. To download them:

```sh
fixtures=https://raw.githubusercontent.com/dutchman-cad/dutchman/main/crates/dutchman-core/tests/fixtures
curl -fLO "$fixtures/solidworks_cube.sldprt"
curl -fLO "$fixtures/generated_unit_cube.f3d"
curl -fLO "$fixtures/synthetic_cube.ipt"
```

```sh
dutchman convert solidworks_cube.sldprt cube.sc
```

```text
SolidWorks .sldprt: reads geometry
converted  solidworks_cube.sldprt -> cube.sc (geometry without history)
dutchman: Degraded: sldprt/source.dialect-unverified: the document carries no swSolidWorks swVersion declaration, so no declared identity was verified. …
dutchman: Degraded: sldprt/config.inferred-without-native: 1 configuration state(s) are inferred from geometry partitions without native configuration definitions.
dutchman: Info: step/design.parametric-records-omitted: 1 parametric/design record(s) were not represented in STEP: configuration=1
dutchman: Info: step/native.passthrough-record-omitted: 2 uninterpreted passthrough record(s) were not represented in STEP
dutchman: Info: step/native.source-record-omitted: 1 source-native record(s) were not represented in STEP
dutchman: Info: history: flattened: the design carries no feature history
dutchman: (repeated notes are grouped and long notes shortened; --verbose prints every note in full)
Want to help Dutchman read SolidWorks better? If you have SolidWorks, you can make a few small test files from short recipes: https://dutchmancad.xyz/fixtures/ or type /volunteer in our Discord (https://discord.gg/yTcseBEdeu).
```

The synthetic SolidWorks cube lacks native version and configuration metadata, so those
warnings are expected for this sample. The internal records omitted from STEP do not change
its shape: the written geometry should be one 10 mm cube (1,000 mm³). The sample has no
editable feature history. Check the result in SolidCraft as [below](#open-the-result-in-solidcraft).

The exit code is 0, and the first line, on stdout, says what was written where: here the
geometry, as this file has no feature history (else `editable history, verified in SolidCraft`
or `editable history, unverified`). After progress updates, the lines on stderr are the loss report, explained
[below](#reading-the-loss-report). Long notes are cut to their first sentence, marked `…`;
`--verbose` prints them whole. An explicitly named existing output file is
replaced (`--no-clobber` keeps it, and the conversion fails instead), and nothing is written
when the conversion fails.

Inventor attached sheet-metal plates and their bends can keep editable history on an unchanged
L-shaped or notched base. The plate must span one proven straight profile edge at 90 degrees;
see [sheet-metal limits](history.md#sheet-metal). Unproven picks or base perimeters with curved,
open, crossing, disconnected, or collinear edges keep geometry and name the blocked feature.

SolidCraft output accepts `.sc`, `.solidcraft`, and `.solvecraft`; each uses the same container.
Other targets use their own extension.
To get one machine-readable report on stdout instead
of the text, add `--json` ([json.md](json.md) describes every field):

```sh
dutchman convert --json generated_unit_cube.f3d cube.sc
```

```json
{"history":{"features":[],"mapped":0,"total":0},
 "input":{"format":"fusion_archive","format_name":"Fusion archive (.f3d)","path":"generated_unit_cube.f3d"},
 "losses":[
  {"code":"f3d/material.not-transferred","level":"Degraded","message":"Materials and appearances were not carried over.","subject":"f3d/material.not-transferred"},
  {"code":"step/attribute.source-record-not-written","level":"Info","message":"7 source attribute record(s) were not written to STEP","subject":"step/attribute.source-record-not-written"},
  {"code":"step/native.source-record-omitted","level":"Info","message":"45 source-native record(s) were not represented in STEP","subject":"step/native.source-record-omitted"},
  {"code":null,"level":"Info","message":"flattened: the design carries no feature history","subject":"history"}],
 "mode":"import","ok":true,
 "output":{"format":"solidcraft/23","path":"cube.sc"},
 "schema":"dutchman.convert/1",
 "verification":{"reason":"the design carries no feature history","status":"not_applicable"}}
```

(Printed on one line; wrapped here.)

SolidWorks parts convert in their saved configuration when its body membership is known.
An Info note counts bodies left out from other configurations and names the saved configuration.

SolidWorks assemblies (`.sldasm`) convert with their external part files. Keep parts beside
the assembly or give `--project DIR` and `--search DIR`. Missing or unreadable occurrences
are reported and left out; unavailable referenced configurations use the saved active
configuration with a Degraded loss. See [SolidWorks assemblies](sldprt.md#assemblies-sldasm).

Before a history is built, Dutchman checks each extrude's profile region against the
source. When the design's only geometry-building feature is a blind one-sided extrude whose
end face survives in the stored model, the area of the region the extrude sweeps must match
that face's area (within 1 mm² or 3%, which absorbs arcs measured as fine polygons); a mismatch blocks the extrude, and the history stops
there. An extrude whose region cannot be checked this way stays kept and is marked
`profile_unverified` in the JSON report (see [json.md](json.md)).

Face extrusions on imported bases require a matching final source result after the
input face is proved. An intermediate result, an unavailable check, or a measured
mismatch keeps the final imported shape and reports
`a face extrude on an imported base is not supported yet without a matching source result check`.

History checks stay on by default and fit themselves to free memory. If a second build
will not fit after reading the design, conversion still saves a usable original shape
with retained history unverified; the card and report say
`History check skipped: needs about N GB, M GB free`. Peak estimates include checking.
The runtime guard uses the same calibrated extra check figure as the desktop estimate,
without multiplying the memory already held by the decoded design.
Geometry-only conversions also check the converted shape by default. Their allowance is
calibrated separately; **Check in SolidCraft** can turn checking off in the desktop.
Use `--no-verify` to keep checks off or `--no-memory-check` to force requested checks.
Desktop Advanced has the matching **Convert and check history even if memory is low**
control. Automatic assembly preparation uses at most two workers and reduces that count
for detailed parts or large retained preparations. See [presets](presets.md).

The desktop shows the same estimate in the file row and starts converting when you press
Convert; see [the desktop guide](desktop.md).

## Reading the loss report

An assembly's conversion first says how complete it is, under the line saying what was
written:

```text
converted  top.iam -> out/top.sc (geometry without history)
assembly: 18 of 20 placements placed; 2 left out:
  pin.ipt ×2: not found at its stored path or in the project folder /work/Robot
```

Empty leaf parts are counted separately as `placed empty`, and make the desktop and
web result card Incomplete. For example, `67 of 88 placements placed; 13 placed empty;
8 left out` means 13 component placements retain their joints but have no bodies.
Each empty leaf adds a Degraded line naming its anonymous part number, as used by
`diagnose` and `--part`. An assembly holder of children is not an empty leaf.
When a solid region becomes an open sheet, a Degraded geometry line counts the
affected regions. Intended surface bodies are not reported as degraded for being open.
Doctor includes these findings and the assembly's placement counts.

Mirrored assembly placements use a shared mirrored copy of the component's geometry;
ordinary copies keep the original. An Info `placements` note counts the converted
reflections. Mirrored copies retain imported geometry only, with a history note when
needed. Non-finite, scaled, or sheared placements beyond tolerance are left out with a
Dropped `placements` note naming the occurrence index and kind. Check those placements
in the source and convert again. Tiny numeric drift is corrected.

The count takes in every placement the assembly and its sub-assemblies record, each
sub-assembly as often as it is placed. Each cause names the file and how many placements it
cost. A part not found usually means its project folder is elsewhere: give it with
`--project`, or a library with `--search` (see
[Finding the part files](inventor.md#finding-the-part-files)). `inspect` says the same as
`would place: …`.

Every conversion lists what it could not carry across intact. A design with more than 12
losses, an assembly's usually, gets them by component instead, the design itself first, then
the parts with the most that matters:

```text
losses: 182 in 14 components
  the design: 1 Degraded, 3 Info
  pin.ipt: 1 Dropped
  base.ipt: history flattened (feature "Shell1": a native face reference is not supported yet: cadmpeg does not resolve it to a model face); 2 Degraded, 9 Info
  …
```

`--verbose` prints every line, and `--log FILE` writes them all to a file. In the desktop app,
each part's losses fold away under its own heading, and **Save the full log…** writes them
out.

Each loss line reads `LEVEL: SUBJECT: MESSAGE`:

| Level | Meaning | What to do |
|---|---|---|
| `Info` | No effect on the geometry. Something was left out that SolidCraft does not use, or the line just says what happened. | Nothing. |
| `Degraded` | Carried across in a weaker form. | Check that part of the design in SolidCraft. |
| `Dropped` | Left out of the design. | Compare with the original; redraw what is missing, or convert again when Dutchman or cadmpeg supports it. |

When cadmpeg's model fails its checks, Dutchman retries with geometry and assembly structure
alone. It removes history, sketches, parameters, and associated design records before checking
again, including parameter links on joints. A `Degraded: history` loss says these were left out
and gives the original check failure. Duplicate parameter names therefore cannot block this
retry; geometry checks still apply.

Repeated stderr notes of the same kind are grouped with a count. `--verbose`, `--log`,
and JSON retain each individual loss. Fusion’s material warning can also appear for a
default material: the decoded model does not establish that no source appearance was lost.

Appearance or attribute bindings for one placed instance or an evaluation state are
left out with a specific loss: Dutchman has no per-occurrence override for them.
Definition-level appearance bindings still colour every placement of that definition.

A Dropped definition-level appearance binding names its body or face target.
“Its target is absent from the decoded model” means no decoded geometry can receive it; “its target exists in the decoded
model but has no placed component geometry” means the target was decoded but is not placed.
Bindings to consumed or removed Fusion geometry have a separate explanation. “Its body has
no geometry the STEP writer can write” means the decoded target exists, but its shape cannot
be carried through STEP. Compare the named part with the source design when reviewing these losses.

Lines from the examples above, in plain words:

- `Info: history: flattened: the design carries no feature history`: the file has no
  history to convert, so the design is the solid geometry alone.
- `Info: step/native.source-record-omitted: 45 source-native record(s) were not represented
  in STEP`: internal records of the native file that have no place in the converted geometry.
  The shape is not affected.
- `Degraded: f3d/material.not-transferred: Materials and appearances were not carried over.`:
  the decoder could not confirm that materials carried over. Set any missing materials
  again in SolidCraft.
- `Degraded: f3d/sketch.dimension-native: 1 sketch dimension(s) retain native operands …`:
  a sketch dimension could not be tied to the sketch geometry it measures. The sketch is at
  the right size, but that dimension may not drive it.

`Dropped` lines name what was left out. For example, a part in an Inventor assembly whose
file cannot be found is reported as `Dropped: <part>: its part is not in a separate file
Dutchman can find`, and the rest of the assembly still converts.

The first part of a subject such as `f3d/material.not-transferred` is a stable code from
cadmpeg; in `--json` it is the `code` field, which scripts can match on.

Sketch dimensions keep parameter expressions when their operands map, including
point-to-line distances and parallel-line spacing. A loss saying a historical thread face
has no input surface geometry means the thread annotation could not be placed safely;
see [Feature history](history.md#feature-by-feature). Check size edits on the editable body
when the result also contains the original imported shape.

Clamped sketch splines with nonuniform spacing or repeated interior knots convert as chains
of exact spline pieces, up to 200 pieces, instead of leaving out the sketch. Periodic
splines are clamped first. Uniform splines stay one spline. Endpoints are shared, and
profiles and sweep paths select the whole chain. Constraints that cannot apply to a chain
are left out with an Info message; the solved geometry remains exact. A loft through such a
chain is not mapped yet (more edges in a section would change the lofted surface).

Fusion Split Body keeps editable history when its tool is a resolved datum plane or
input faces whose boundaries prove they lie on one plane. The faces' plane extends through
the target body. Curved or noncoplanar tools, incomplete input selections, and final-body
face selections receive specific refusal reasons; conversion keeps the original geometry.
A body kept only earlier in the timeline is imported from the source's history body before
the Split. When a Base Feature already imported that same body, the Split reuses it.
The new Split history body imports and input-face tools need SolidCraft 0.40 or newer.
Existing Base Feature imports retain their version support.
Older targets keep the original geometry with a named version reason; existing datum-plane splits remain available. Targeting an installed 0.25–0.39 release also shows the usual older-version notice.

A hole on a planar face can retain its history even when that face has a rounded corner or
an earlier circular hole. Dutchman checks the source boundary and asks SolidCraft to prove
that face exists before drilling. A placement inside an existing hole, outside the face,
or facing out of it stays blocked with a specific reason; conversion keeps the original
shape. This proof needs SolidCraft replay; when replay is unavailable or disabled, the
original shape stays visible.
This extension requires a target of SolidCraft 0.40.0 or newer; older targets retain the
original geometry with a named reason.

## Open the result in SolidCraft

In the SolidCraft app, use File, Open, or start it with the design:

```sh
solidcraft cube.sc
```

From the command line, `solidcraft-cli eval` opens the design, rebuilds its timeline and
prints its bodies as JSON:

```sh
solidcraft-cli eval cube.sc
```

Check that `body_count` is what you expect, that no `timeline` entry has an `error` or a
meshing warning, and compare each body's box position, box bounds, `area_mm2`, and
`volume_mm3` with the original design. History position and box checks allow
the larger of 0.01 mm and 0.02% of the larger box diagonal, for the centre distance and each
minimum/maximum coordinate; volume and area retain their measuring mesh tolerances.
A face-count difference alone does not establish a defect. An AI agent can do the same through SolidCraft's MCP server: open the design with
`solidcraft-cli mcp --in cube.sc` and call its `measure` tool.

For a source consisting of one solid body with disconnected lumps, verification can
compare the imported pieces together against one replayed body. Every source region
must remain present; their summed volume and area and combined bounds must agree.
Single-lump and multiple-source-body comparisons retain their existing checks. See
[How a history is checked](history.md#how-a-history-is-checked).

Conversion JSON includes `verification_checks` when a final history comparison is attempted.
It records each body's measurements, counts, closure and centre of mass against source
geometry, plus source parameter values and quantity kinds after replay. Geometric, closure
and parameter checks say `pass`, `fail` or `not_checked` with evidence or a reason. A failed check can cause conversion to retain the
original geometry. Raw kernel face, edge and shell counts are informational: `info` when equal and
`differs` when unequal. These raw count differences alone never reject a history. Omitted or renamed parameters are `not_checked`. Manifoldness remains explicitly unchecked because
there is no evaluator result proving it. See [JSON reports](json.md#source-comparison-evidence).

Final SolidCraft history verification also requires matching available merged face
counts per body (`merged_faces` in `verification_checks`). Merged edge counts
(`merged_edges`) are informational: mirror and full-turn revolve operations can split
edges differently across SolidCraft releases while preserving the faces and shape.
Face counts merge adjacent pieces of one surface and periodic seams. Bodies with
free-form or mesh surfaces skip merged totals, since equivalent pieces can have different
surface records. Available analytic surface-kind counts (`analytic_faces`) must still
match for planes, cylinders, cones and spheres; mesh bodies skip that check too. A mismatch
retains the imported final shape, even when volume and area pass. Raw kernel counts remain
informational.

Which SolidCraft a design is written for, and how Dutchman finds the installed one, is in
[SolidCraft versions](command-line.md#solidcraft-versions).

## Where next

- [The desktop app](desktop.md): presets, estimates, the SolidCraft version, and updates.
- [The command line](command-line.md): options, `inspect`, `contents`, whole folders, watching a folder.
- [Choosing what to keep](presets.md): Fast, Standard, Full, and the memory and time estimate.
- [Formats](formats.md): what converts from each program.
- [Feature history](history.md): what maps onto SolidCraft, and what stops a history.
- [Troubleshooting](troubleshooting.md): common problems and diagnostic logs.

## Moved sections

This page used to hold everything. Each part now has its own page:

### Assembly workers

[Assembly workers](command-line.md#assembly-workers)

### Choosing what to keep

[Choosing what to keep](presets.md)

### Homebrew or Scoop

[Homebrew or Scoop](install.md#homebrew-or-scoop)

### macOS: the app

[macOS: the app](install.md#macos-the-app)

### Opening the app the first time

[Opening the app the first time](install.md#opening-the-app-the-first-time)

### From a release archive

[From a release archive](install.md#from-a-release-archive)

### Channels: stable and nightly

[Channels: stable and nightly](install.md#channels-stable-and-nightly)

### From source

[From source](install.md#from-source)

### Docker

[Docker](docker.md)

### Dutchman Web, in a browser

[Dutchman Web, in a browser](install.md#dutchman-web-in-a-browser)

### SolidCraft (optional, recommended)

[SolidCraft (optional, recommended)](install.md#solidcraft-optional-recommended)

### Desktop app

[Desktop app](desktop.md)

### Options

[Options](command-line.md#options)

### Look before you convert: inspect

[Look before you convert: inspect](command-line.md#look-before-you-convert-inspect)

### List what a file holds: contents

Assembly-owned features and parameters are included under their component paths,
with records already present in child parts counted once.

[List what a file holds: contents](command-line.md#list-what-a-file-holds-contents)

Contents reports what maps for the newest SolidCraft release known to this build,
without looking for or launching an installed SolidCraft CLI.

### Progress during conversion

[Progress during conversion](command-line.md#progress-during-conversion)

### Convert a whole folder: convert-all

Folder runs in the desktop app and CLI keep paths relative to the input folder:
`in/ProjectA/part.f3d` becomes `out/ProjectA/part.sc`. Designs with the same name in
other subfolders stay separate. Linked design companions and desktop reports stay
beside their design. Repeating a run into an explicit output folder reuses those
paths and replaces designs unless `--no-clobber` is set; edited linked companions
are kept and reported as a conflict. Single-file output paths are unchanged.

[Convert a whole folder: convert-all](command-line.md#convert-a-whole-folder-convert-all)

### Watch a folder

[Watch a folder](command-line.md#watch-a-folder)

### Formats

[Formats](formats.md)

### Pilot

[Pilot](pilot.md)

### Feature history and the SolidCraft check

Body moves require SolidCraft 0.45 or newer. This requirement applies only to designs
with a body move; it does not trigger a general history-loss notice for older targets.
Dutchman keeps resolved translations
and axis-angle rotations, including explicit pivots, when every selected body has
a proven earlier name. CADIR supplies millimetres and radians; the writer never
infers a transform from final geometry. Unresolved body selections, unresolved
point-to-point motion, and copied body result identities remain blockers. Bodies
available only as final geometry also require a captured pre-move body. Older
targets keep the original final geometry with a named version refusal.

[Feature history and the SolidCraft check](history.md)

Cosmetic thread conflicts name the designation's nominal diameter and pitch alongside the
secondary numeric values. ISO metric table designations tolerate decimal rounding and
floating-point unit-conversion noise. UNC/UNF values in millimetres or TPI are checked, but
separate UNC/UNF cosmetic Thread output is not supported yet by SolidCraft. See
[cosmetic threads](history.md) for details.

Separate modeled Thread features keep their helical shape in the imported final geometry
and stop editable history. When reached in the timeline, a Degraded `history/modeled-thread-retained` loss
preserves the designation when available and explains that CADIR does not resolve hand, tolerance class or the exact
groove profile. SolidCraft 0.45's modeled option does not recover those missing facts.
The designation remains in the report; no cosmetic face selection is guessed on the
threaded final body. See [thread history](history.md).

One stored sketch loop does not prove a native profile selection when other solid
curves remain, including curves needed by a later trimmed cut. Dutchman requires
source-confirmed inference in that case; otherwise it preserves the final shape
and reference sketches instead of rebuilding extra regions. A later fillet may
therefore be reported behind the earlier unresolved profile.

Contour flanges map when cadmpeg supplies an open sketch chain of lines and arcs, a blind
sweep along the sketch normal, and material on one side. Their thickness, bend radius and
supplied K-factor become a sheet rule. Unsupported contour forms keep the original geometry
and report the reason; see [sheet-metal history](history.md#sheet-metal).

A trimmed profile region, including a revolve's, is selected by a point inside its boundary. When a
native profile selection can't be resolved, Dutchman tries the selected curves and then the whole
sketch, and keeps the feature only when SolidCraft's body check confirms it; the loss
`history/profile-region-inferred` says so. Otherwise the feature stays blocked with its reason.
When neither confirms, Dutchman measures every region of the sketch the selection can name (up to
24) in SolidCraft against the source, and keeps the feature only when exactly one region matches;
the loss `history/profile-region-measured` says so. A last fillet or chamfer whose edges can't be
proven is measured the same way, choosing among the rebuilt edges within 2 mm of each source edge.
Such features are marked `inferred` in the JSON report, never exact. A tie, or no match, keeps
the feature blocked. New inferences against the final shape also require a measured surface
distance; an unavailable comparison or a part with multiple final bodies keeps the refusal.
New measured regions are tried only at the last body-building feature, against the original
solid with both face counts. Every source and replayed body must have finite volume, area,
complete bounds and derived placement; missing measurements keep the refusal. Regions reported
only by centroids are refused because a centroid may lie outside its own region. One 120 s
feature budget covers region discovery (including `sketch.inspect`), mapping and all candidate
evaluations; late results keep the refusal.

Closed circular input edges can become editable fillets and equal-setback chamfers when
targeting SolidCraft 0.44.1 or newer. Their source edge must reference an exact
stored circle with centre, axis, radius and a full-turn parameter range. Its stored topology must identify one earlier named owning body;
only that body supplies replay candidates, even when another body has a coincident rim.
Missing carrier or ownership evidence, ellipses, and transformed source carriers remain
refused. The rebuilt rim may be split into an edge path or have a different seam. SolidCraft must also confirm the circular carrier's centre, axis, radius,
and analytic length through `inspect.measure`; seam samples alone cannot establish its plane.
Gaps, overlaps, ambiguous bodies, and other closed curves remain blockers.
A missing source carrier reports `its closed input edge has no resolved historical curve carrier, which is not supported yet`.
This means the source geometry needed for selection is unavailable; it does not establish
that the curve is noncircular. The editable prefix and original final shape are retained.
Use `--solidcraft-version 0.44.1` to select this patch release. The exact target stays in
effect through input preparation and geometry-only fallbacks; JSON displays its minor, `0.44`.
Older targets report `blends on closed input circles need SolidCraft 0.44 patch 1 or newer`
and keep the original geometry. The existing history status and reason fields report refusals.

Chamfers with unequal distances or a distance and an angle other than 45 degrees keep
editable history when the source's final body proves which face takes the distance.
Straight edges need a retained planar chamfer face; closed circles need a plane and a
coaxial cylinder in the replay and both resulting circles in the source. Each body's
chamfer is replayed in both face orders when needed and kept only where its cut matches.
Missing evidence keeps the original geometry with a specific history reason. Existing
history statuses, counts and report fields are unchanged.

For SolidCraft 0.44 and newer, extrude joins, cuts and intersections can keep a
selected body scope at the final body-building feature when the reader names
modified bodies and replay matches the final source geometry. The Info note `history/body-scope-confirmed`
records this confirmation. Intermediate body centres cannot confirm body scope,
so intermediate features keep the previous mapping. Without confirmation the
previous mapping applies; older targets keep their existing output.

On SolidCraft 0.44 and newer, a consumed boolean tool with no final body row
keeps a name only when its identity is proved: the source retains exactly one
history body with `produced_by` matching its single-output producer, or the
complete replay matches the final source geometry. Replay candidates keep
their names only after that check passes; intermediate body centres cannot
confirm them. An output ID alone is not proof. Without proof, selecting the
tool stops editable history with "it selects a body that an earlier feature
has not named". Older targets keep the previous mapping.

Constant-radius fillets and equal-setback chamfers whose proven input edges span several
bodies become one SolidCraft blend per body, with numbered names from the same source
feature. The report keeps one `mapped` feature outcome for that source feature.

For SolidCraft 0.44 and newer, sweep paths may select a connected open chain of proven input
edges on one earlier body. Missing source geometry, ambiguous matches, topological guide rails, and older
targets keep the original geometry and report a specific history blocker. Closed sweep
paths, including loops assembled from open edges, still require a proven start and traversal
direction and remain refused. An unavailable exact curve measurement blocks only the feature
that needs it; earlier straight-edge blends remain editable. These proofs
require a SolidCraft replay for body-edge selections; `--no-verify` cannot establish those
edges. A stored sketch wire instead resolves to its owning earlier sketch when every edge
matches one entire mapped curve, including the midpoint and length of an arc.

A history using these edge paths stays hidden beside the
original unless the full comparison succeeds. This also applies when checking is off
or the original cannot be measured soundly. The original position and shape remain
visible, and the report explains the unverified check. Other histories keep their
existing verification behavior.

Offset datum planes need no building feature when their support resolves or all their
consumers carry independent resolved sketch placements. Otherwise the history reason
names the unresolved plane reference; resolved skipped planes count as `not_needed`.

Offset faces targeting SolidCraft 0.45 or newer bind their historical face points to the
unique earlier named body in the part's history. Multiple source names for that same body
count as one body. Circular boundaries use points clear of
both outer trims and holes. An offset-face body blocker means ownership is not proven;
selecting among several input bodies is not supported yet. The original geometry is kept.
Older targets keep the previous point-only representation.

Shells require removed faces from their own historical input topology, a resolved
thickness and inward or outward direction, and one earlier named body. A shell input-face
blocker means the source selection or historical boundary is not supported yet; final-body
faces cannot establish which face existed before shelling.

A body made by a new-body feature is named from the source; when the source leaves it
unnamed, it is named "Body 1", "Body 2" and so on (the first number not yet taken), so later
features can still target it. An extrude stored as a join before any feature has built a
body makes that first body. A through-all cut with no earlier named body (no earlier
new-body feature mapped) is `blocked` before replay, with a reason asking for its
body-producing feature to resolve first. Source storage order does not
change replay order: the neutral ordinal controls it.

### Diagnostic logs for unfinished runs

[Diagnostic logs for unfinished runs](troubleshooting.md#diagnostic-logs-for-unfinished-runs)
include UTC time, Dutchman version and result in automatic filenames. Explicit
`--diagnostics PATH` files keep their chosen path. Reusing a log from another version shows
a notice and still converts. Desktop result cards show or copy their own run's log.

### Troubleshooting

[Troubleshooting](troubleshooting.md)

### FreeCAD output (experimental)

[FreeCAD output (experimental)](freecad.md#freecad-output-experimental)

### Conversion details

[Conversion details](command-line.md#solidcraft-versions)

### Presets and estimates

[Presets and estimates](presets.md#presets-and-estimates)

Anonymous diagnostic logs include timed decoder preparation phases and, on failure, the
failure stage and error category. Model check failures add check-code counts and available
referenced curve-kind counts. A geometry-only recovery rechecks the decoded model in memory
only when failing checks involve history, sketches, or parameters; geometry failures stop
without repeating the decode. Fusion archives also record anonymous `member_classified`
timings for nested scanning, layer classification and reference metadata extraction before
the first member decode starts. See [diagnostic fields](json.md#dutchmandiagnostics1).

## Make a minimum reproduction

When a failing design must stay private, run:

```sh
dutchman repro part.f3d --out reproduction
```

Use a fresh output folder each time. This runs the default SolidCraft conversion with
local diagnostics and writes `reproduction/kit/anonymous.json`, `local-key.json` and
`next-steps.md`. `--json` returns the summary, kit paths and anonymous content. Exit 0
means the kit was written, including when conversion failed; exit 1 means kit creation
failed. A successful conversion says **no failure to reproduce**.

Only `anonymous.json` is intended for sharing. `local-key.json` maps anonymous ordinals
to your own names and is marked **Do not share**. Follow `next-steps.md` in your CAD
program to rebuild a small design and compare the failure stage and finding-code set.
Temporary conversion output and diagnostic logs are removed after the command returns.
No files are uploaded. Narrowing uses retained CADIR subsets and stops where an independent
subset cannot reproduce the same structured check codes; it does not regenerate features.
The [kit schema](json.md#dutchmanrepro1) describes these limits.

## Rebuild a small failing example

`dutchman diagnose --narrow FILE --out RESULTS` now includes a numbered rebuild recipe for
Fusion, Inventor, SolidWorks and FreeCAD in `RESULTS/diagnose/kit/next-steps.md`. Recipes
use round starting sizes and the signature's options and profile counts. Unknown feature
classes get a generic recipe. An exact match in the release's known-problem list gives a
workaround or next-version fix advice before further narrowing when enough fields are known.

In the desktop app, choose **Diagnose this file** on the failure card for progress, resume,
the recipe, **Check my small file**, and **Make the report**. The report ZIP stays local
and contains the small file's geometry. Review it before choosing to share it.
The [MCP tools](mcp.md#anonymous-diagnostic-tools) provide the same steps for an assistant.

## Check a folder locally

`dutchman doctor designs --out checked --jobs 2 --memory-mb 8192 --json` plans
folders and project ZIPs with the same rules as `convert-all`: top-level assemblies,
their referenced parts, and unused parts. It converts in isolated workers, then runs
part diagnosis. `--memory-mb` is the total converter budget, divided across concurrent
designs; CPU availability caps `--jobs`. `--time-limit 5m` limits each diagnose part.
Run the same command again to resume. Completed checks are reused when the project
inventory, choices, budgets and matching STEP metadata agree. Keep the output outside
the input folder. Default output is a stable sibling named `designs (Doctor)`.

Only `checked/report.json` is anonymous. Converted designs, `local-key.json` and
`private-checkpoints/` stay private. Findings are grouped locally by signature with
counts. A broad error category is a candidate group, not proof of the same defect.
Nothing is sent. Review advisory findings before making a report.

`dutchman diagnose big.f3z --step big.step --out checked` compares stored geometry
against an export of the same design. An unambiguous same-name `.step` or `.stp`
beside a native file is found automatically, including inside a folder or ZIP.
`doctor --step reference.step` accepts a project with one conversion job. Use
`diagnose --part 17` or `doctor --part 17` to record a report target. This does not
change the geometry being compared or replace `convert --part`.

STEP results use evaluated **body numbers**, which can differ from diagnose part
numbers for assemblies and repeated occurrences. Matching uses geometry, placement
and stable order, never names. Results are worst first and contain volume, area,
bounding box, face counts where available, and placement differences. Current
thresholds are 1% volume/area. Bounds/placement allow 0.1% of the body diagonal,
with a 0.1 mm floor and a 0.5 mm ceiling, so a 1 mm move still flags on large bodies.
Face counts alone never flag a difference. Missing measurements are explicit. Checks remain advisory; unavailable comparisons do not count as passes.
No automatic report is sent. The observed false-positive rate for the two
placement-golden assemblies is 0/329 evaluated body rows (0%);
[calibration results and limits](doctor-calibration.md) give the coverage and denominators.
The STEP itself stays local; use the private Discord steps if you choose to share it.

The structural checks also flag `geometry_untrimmed_torus`: a toroidal face without
trim loops sharing a shell with other faces. Review its boundary before reporting a
defect. A whole torus in its own shell is allowed, including a separate cavity shell.
An isolated part cannot be called floating without an independent placement reference;
the comparison checks detect movement relative to stored geometry or a matching STEP.

Every successful SolidCraft result also gets an advisory body comparison against the
source's stored geometry, using the same joint setting and assembly pose as the
conversion. Partial-history outputs skip this comparison because eval
includes intentionally hidden editable bodies; the normal history verification still
runs. A measured replay difference removes the unconfirmed edited body and uses
the imported final shape. The report names the failed position, box, volume, or area
check and its measured difference. Source imports before the unconfirmed edit and
independent sketches remain available. Where supported, SolidCraft writes private
whole-design snapshot pairs for flagged comparisons under `diagnose/preview/`. Their PNGs and staged designs carry
geometry. Review them locally; they are excluded from anonymous tool results.

Memory supervision includes kernel subprocesses on Linux, macOS and Windows; a stopped
worker stops its process group/tree too. These are sampled resident-memory limits,
not an operating-system reservation. The folder planner runs before worker supervision.
Resume fingerprints include resolved external assembly references (up to 10,000 files);
an incomplete dependency inventory is checked afresh.

Real SolidCraft controls on the three published synthetic cube fixtures (Fusion,
Inventor and SolidWorks) produced 0 flags in 3 comparisons against matching STEP.
A planted 1 mm move in a two-instance assembly was detected, and snapshot pairs were
produced. These small controls establish basic behavior, not a false-positive rate
for large assemblies or arbitrary designs.

<a id="report-consent"></a>
In wide windows, **Is there a problem? Is something missing?** stays pinned below the
output controls, even with **Advanced** and output choices expanded. Input and output
controls scroll above it; long report reviews scroll below the question. Narrow windows
keep one scrolling page. **Report this file…** selects that file in the panel, and the
top-bar **Report a problem** opens the same panel.
Partial history says **Partly converted**; geometry-only or flattened results say
**Converted, no history**, in amber. Full history keeps **Converted** in green unless
degraded content needs review.
With one result and compact output controls, the panel sits directly below **Output folder**;
several results or expanded choices keep it pinned and visible.
After a conversion, **Output folder** folds into a summary. Click its folder path or
**Change…** to reopen all output settings. **Clear** removes the designs and results and
restores the expanded settings.

In narrow desktop windows, **Report a problem…** (or **Report this design…** with a result
selected) stays above **Convert** and scrolls to the full numbered cards in the page.
See [reporting a problem](desktop.md#reporting-a-problem).

Report consent is per report. In the desktop app, Discord is the first route, with a small
**GitHub issue** backup link below the three steps. **Anonymous report** is under **More**,
with AI help and diagnosis. It opens a review for the chosen file
with bundle or summary. A bundle is selected when available; otherwise
only the anonymous summary is offered and selected: version, OS, format, source app and
problem signature only. Converted results use `converted-with-problem` and never
reuse a prior failure bundle. The review displays the exact metadata and sharing choices.
Descriptions are not supported in the anonymous envelope; describe the problem privately
or in the GitHub form without private names or paths. Whole files go privately to Dutchy on Discord. A separate box,
**You may use this report in a public test case**, starts unticked.
Nothing is sent before clicking Send. A report is sent only when a matching receipt
arrives. A temporary failure gets one short retry, then keeps the report locally;
retrying later requires clicking **Retry**, which appears after a failure. A daily cap
asks you to try again tomorrow.
**Anonymous report** sends to the cloud inbox after review. **GitHub issue** opens the
prefilled public issue form under your GitHub username. Never attach a geometry bundle
there. For a converted result the form asks what differs from the original, without claiming conversion failed.

For the CLI, first make a local preview (always printed as JSON):

```sh
dutchman report --folder reports/converted --report-id f0123456789abcdef --source-format f3d --signature converted-with-problem
```

Replace the example id with the file id from your diagnostic log or private report key.
Use a fresh report folder for this converted result. No design is opened or uploaded.
Read the preview's `metadata`, then send that summary explicitly:

```sh
dutchman report --folder reports/converted --send-summary --yes
```

`--public-test` separately permits a public test case and defaults off. The same command
retries a failed submission only when you run it again. Saved desktop report folders can
also be reviewed with `dutchman report --folder FOLDER`. MCP offers the same path through
[`report_preview` and `report_submit`](mcp.md#report-preview-and-sending).

Release builds send to Dutchman's report service; `DUTCHMAN_REPORT_ENDPOINT` at compile
time replaces it. A debug build has no default: clicking Send keeps the report locally
and offers the same fallbacks, and development tests use a loopback HTTP stub.

Report summaries identify the authoring app from the native format, such as Fusion or
Inventor. Exchange formats and unclassified `.prt` use `unknown` because their authoring
app cannot be inferred. The build version is included alongside this information.

Desktop anonymous reports take `versions.solidcraft` and `run.verify` from the saved
conversion report, so changing the installed CLI or verification setting afterward does
not rewrite what checked that file. No schema fields change.

In wide desktop windows, the report question stays visible below the input and output
controls. Its body has its own scrollbar, so **More** and long reviews remain reachable.
The input controls and results scroll separately; narrow windows use one scrolling page.
Each reporting step shows its own copied-message notice beside the action.
**Check a folder** keeps the app header; **Part to check** selects a part for local checks.

`dutchman diagnose part.f3d --bundle --out diagnosis` cuts a private reproduction
bundle without narrowing. Ordinary diagnosis, narrowing, and doctor also try to save a
bundle when cadmpeg's decoded model fails a check. A cut must reproduce the same
failure signature; Dutchman widens it if needed and keeps no bundle if proof fails.
A refused cut, including a closure wider than the current fragment byte limit, is a
normal no-bundle result; choose summary or the private Discord steps.
Shrinking runs for up to ten seconds (or the shorter time limit). Creating
`diagnose/cancel-bundle` in the output folder stops shrinking between candidates and
keeps the last proven cut.

The report folder `diagnose/repro` holds `bundle.json.gz`, `manifest.json`, and private
report metadata. The manifest lists retained arenas, row counts, sizes, and purposes;
previews never show their contents. Bundles contain numeric geometry. Review them locally
before choosing to send. A bundle is the default when available. The service caps the entire multipart request at 25 MiB, including metadata and
upload overhead. Dutchman keeps gzip bundles at least 64 KiB below that cap and checks the complete
size before enabling Send. If it will not fit, use the
private Discord steps or send a summary. Decode refusals have no bundle yet; choose
an anonymous summary or the private Discord steps. Nothing is sent by diagnosis.

## Assembly placements

**Part positions** offers **Smart** (default), **As last shown in the original program**, and
**As first placed**. Smart accepts current poses only when touching joints close, geometry
stays within first-placement assembly bounds grown by 10%, and repeated inserted designs
move together. It keeps first placements when a check fails or cannot be established.
Last-shown accepts available current poses without those checks. First-placed ignores later
moves and restores the old placement choice. Joints work from the selected placements.

Use CLI `--part-positions smart|last-shown|first-placed`, MCP `part_positions`, or desktop
**Advanced → Part positions** (remembered). `--stored-placements` remains a first-placed
alias. Smart and last-shown prefer decoder-bound displayed poses when available, without
current-pose guards. Ambiguous and unbound rows are skipped; an absent arena supplies none.
Fallback current poses may be stale. An Info note counts the selected rules. JSON includes
`choices.part_positions` and `part_positions` counts and fallback reasons.

Missing assembly holder poses are inferred bottom-up from displayed children only when
their world-pose estimates agree within 0.001 mm and 1e-6 rotation entries and reproduce
every displayed child. A conflicted holder keeps its first placement, even inside a holder
that moves, and is reported. Smart also keeps the first placement of a displayed holder (and
everything inside it) when a part below it would leave the bounds grown by 10%. JSON `part_positions` adds `derived_holders`
and `holder_conflicts`; `bound_displayed` and `derived_holders` partition `displayed`, preserving the displayed/current/first-placement total.

Local CLI and desktop progress names the running part or stage and its elapsed time, the
feature a design's history is on ("Feature 29 of 37 · Extrude 1"), including “Still …” notices and full-sentence check-limit consequences. Durations use one
format, such as `1 min 1 s` or `2 min`. Desktop folder runs keep the longest-running file,
its stage and step visible below the completed count, with the number of other active files. See
[progress during conversion](command-line.md#progress-during-conversion). MCP final reports
are unchanged; running names are excluded from anonymous diagnostic logs.

When cadmpeg reports mesh-body members for a Fusion design with no decoded B-rep bodies
or placed components, Dutchman says "it has mesh bodies (N), which Dutchman
doesn't convert yet, and no solid bodies could be read" and asks you to convert the meshes to
solids in Fusion, then save a new copy. The count comes from typed mesh selections; an
undecoded mesh container or an unresolved source alone cannot supply a count, so the
existing read error remains.
Designs with both B-rep and mesh bodies still convert their B-rep bodies and report reader
losses for meshes it could not decode.

## Fetch your own Onshape document

`dutchman onshape fetch <document-url> --out snapshot.onshape-bundle --max-calls 50`
uses the keys saved by `dutchman onshape login` (or Connect Onshape in the app), or `ONSHAPE_ACCESS_KEY`/`ONSHAPE_SECRET_KEY` for scripts, and prints the HTTP
attempt count. `--version VERSION_ID` selects an immutable version; `--json` prints
`dutchman.onshape-fetch/1`. This fetcher saves API responses without converting CAD.
To use OAuth instead, pass `--bearer-token-file PATH` (a private UTF-8 file, mode 600 on
Unix), or set `DUTCHMAN_ONSHAPE_BEARER_TOKEN` in the process environment. The file wins
if both token sources are set. Remove API-key environment variables and saved keys
(`dutchman onshape logout`) first: mixed authentication is refused before any request.
Never pass the token value in command arguments. The desktop's **Use bearer token file**
option and MCP's `bearer_token_file` use the same authentication. HTTP 401/403 means the
token expired or lacks access; obtain a fresh token with read access, replace it, and retry.
Token files are opened once on Unix, with symlinks and special files refused and ownership
and permissions checked on that handle. On Windows, file privacy cannot be checked yet:
clear the token file selection and use `DUTCHMAN_ONSHAPE_BEARER_TOKEN` instead.
Credentials stay out of logs and reports, including `--json`.
Selecting a token file alongside API-key environment variables reports the authentication
conflict before reading the file on every platform.
Part Studios include features, stored sketch placements, needed input topology,
variables/configuration, and final STEP; assemblies include their same-document
Part Studios. Version links use version paths throughout. Workspace links pin supported reads to a microversion,
use workspace paths for STEP/variables, and recheck the head before writing. A
change triggers one refetch; a second change stops with “the document changed while
fetching; try again, or use a version link”. Successful manifests record per-file
path kinds/microversions and `consistent: true`. See [Onshape](onshape.md#fetching-a-document) for keys,
limits, caching, and privacy. Each uncached Part Studio now needs one additional
sketch-information call. Fetch older bundles again when a face-attached sketch
reports a missing attachment frame; conversion keeps the final shape with
`onshape/history.not-supported` until that frame is stored.

Meeting 3D curve endpoints do not create relations without a decoded source relation.

Decoded 3D sketches retain supported neutral joins, dimensions, axis directions
and endpoint tangencies on SolidCraft 0.44 and newer; dimensions keep their
parameters. Other relations are reported as not carried. A selected planar line
loop can supply an extrude through a separate planar profile sketch only after
the source body check confirms it. A message saying its planar 3D sketch profile
has not been confirmed means the original shape is retained. Earlier targets
keep their existing output. See [Fusion conversion](fusion.md) for the
remaining spatial constraints and profile limits.

A planar 3D sketch profile copy carries at most 400,000 constraints, including
repeated line occurrences across all relations. If it exceeds that copy limit,
reduce repeated relations and convert again; the original shape is retained.

## SolidCraft plugin component

The [plugin front end](plugin.md) converts files through the same core and SolidCraft writer,
with host-drawn conversion options and a structured loss report. It implements SolidCraft's
`craft:plugin` importer interface; see that page for the build. To install it, see
[Dutchman in SolidCraft](solidcraft-plugin.md).

The plugin aborts on host input failures before writing output. Its bounded loss report
prioritizes dropped and degraded losses and includes an explicit truncation notice and
`details.truncation` counts when shortened; see [the plugin contract](plugin.md).

A new-body extrude, revolve, sweep or loft can still be reported as `mapped`
when its required profile or path is absent from the source and its stored result
is written as a named Import so later features can continue. This fallback runs only
after ordinary mapping and measured inference fail. An unresolved selection in an
existing sketch, an unsupported option or an unconfirmed measurement keeps its
usual refusal. The Degraded loss `history/body-fallback-imported` explains the
absent input. Recover the original external design to recover the editable recipe.
Cuts and joins cannot use this fallback.

Each history row in `dutchman.convert/1` and `dutchman.inspect/1` now includes
`output_ids` and `output_match`, linking it to features in the saved design. A split
can have several ids; dropped or merged rows have none. Inspection returns empty ids
with `not_written`. These fields are additive; schemas and existing fields are unchanged.
See [the JSON report reference](json.md) for the matching values.

## Counts-only anonymous reports

Use `--report-counts --report-anon FILE` with `convert` or `convert-all` to write
`dutchman.report-counts/1`. For MCP `convert_all`, set `report_counts: true` and
`report_anon` to the destination. This opt-in profile holds counts and fixed categories;
it has no messages, file IDs, names, paths, dimensions or geometry. It cannot use
`report_salt` or `report_key`. Source version bands remain `unknown` when unavailable.
Review the exact file before sharing. This option writes locally and does not authorize
sending; retain the existing user confirmation before sharing a report.
See the counts-only schema in [JSON reports](json.md).

## Scan before converting

Run `dutchman preflight part.f3d` (or add `--json`) for bounded input evidence.
The planned tier reads geometry and history, then forecasts the default target
without writing a design. Partial results include retained feature counts and
the first blocker. Other targets and budget overruns stay unknown. Exit 0
includes unknown; it does not promise that conversion will succeed.
See [preflight](preflight.md) for bounds and available facts.

Preflight also refuses suffixes the current build cannot convert, following the
existing converter gate before reading payload bytes. A shared part suffix stays
unknown until its contents identify a reader.

Library planning outcomes used by preflight keep empty output ids with
`not_written`; preflight saves no design.

## Orient a Y-up design

Use `--stand-up` with `convert` or `convert-all` to rotate a Y-up design +90°
about +X, so its +Y axis becomes SolidCraft's +Z axis. Source axes are preserved
by default; `--no-stand-up` explicitly keeps them. For MCP `convert` and
`convert_all`, use `stand_up: true` or `no_stand_up: true`; the two options
cannot be combined. This changes the saved design's orientation, while history
checks use the source coordinates.

Parallel directed hole placements on one entry plane can supply complete locations
without a source sketch. For SolidCraft 0.44.1 or newer, Dutchman writes an editable
location sketch and a multi-point hole. The hole's form and termination must also
be supported. Each placement still passes the single-hole entry-face checks. Different drilling directions, different entry planes, and
multiple unsigned axes remain not supported yet; the original geometry is retained.
Older targets report `multiple spatial hole locations need SolidCraft 0.44 patch 1 or newer`.
Choose that patch explicitly with `--solidcraft-version 0.44.1`.

Generated blend-edge identities are opaque CADIR references. An identity without
resolved sketch-entity geometry reports `its generated edge identity has no resolved
source geometry, which is not supported yet`. A numeric feature-local path alone
does not prove which rebuilt edge to select. The editable prefix and original final
geometry are retained; a generated reference with missing or blank fields is still refused.

When no final source shape is available, conversion keeps the editable history before the
failed feature, leaves out its dependent suffix, and checks the remaining design again.
The report names the omitted features and the rebuild reason. Conversion stops only if
no usable history remains or the remaining design cannot be made to open.

When SolidCraft reports a feature that fails to rebuild, conversion removes that feature and
its dependent body history and keeps the final source shape with the editable prefix before the
failure. The report names the feature and SolidCraft's reason, even when the source shape could
not be measured. Before omission, refused STEP imports with excess shared edge uses get identical edge
copies for uses beyond the first two and are rechecked. Faces, curves, vertices, names, and
placements are retained; `geometry/sheet-edges-split` records the repair. A final opening
check also covers geometry-only output, hidden history, and
assemblies. If SolidCraft also refuses the available final source shape, omission is the last
resort: `geometry/import-failed` says the body geometry is **MISSING** and names the feature
and reason. A combined import may leave out the whole part. Review these losses before using
the result. If a named failure cannot be removed, conversion stops without writing it. An
evaluator error or nonzero exit without named failed features means verification is
unavailable: the design is still written, marked unverified, with the reason. Verification
turned off remains unchecked.

Variable-radius fillets with an unproven chain direction report `variable fillets need
a proven chain direction: their radius stations are not bound to a source start vertex,
which is not supported yet`. Mapping a radius law requires
an ordered, directed input chain with its radius positions bound to that traversal.
Carrier sample order alone does not supply that binding. The editable prefix and original
final geometry are retained when the direction is unresolved.

The SolidCraft plugin checks estimated memory and time before native decoding. When it
says the design is too large to open inside SolidCraft, use the
[Dutchman desktop app](https://github.com/dutchman-cad/dutchman/releases/latest) to save a
`.sc` and open that in SolidCraft. Its Import report uses the app's result wording and
"Is there a problem? Is something missing?" help, with Discord and Dutchy before GitHub.
See [the plugin guide](plugin.md).

### Batch report

After a folder run finishes in the desktop app, choose **Batch report** in the
bottom bar, after the converted and failed counts. The button is hidden before
a run and while conversion is running. Writing runs in the background; the
status then says `<converted> converted, <failed> failed · Report saved to <output folder name>/` or shows a folder
permission error.

For a fresh CLI run, use `dutchman convert-all designs --out-dir converted --batch-report`.
For saved results, run `dutchman batch-report converted`. Both write
`dutchman-batch-report.md` and `dutchman-batch-report.csv` in the output folder.
Markdown starts with totals for full history, partial history, geometry only,
and failed files, including zero counts. CSV has a header and one row per file,
with no summary rows. Each file has its relative input path, output path,
outcome (Full history, Partial N of M, Geometry only, or Failed), and first reason. Partial counts use
rebuilt features and features needing rebuilding, with older mapped/total counts
as a fallback. These local summaries include names and paths.

The [public fixture score workflow](fixture-score.md) checks public fixture
history on GitHub runners each week or on demand. It records conversion and
SolidCraft reopen evidence for the fixture scoreboard.

Extrudes and resolved revolves accept a profile that explicitly names an earlier active
planar sketch feature with a resolved sketch binding. This selects the whole bound sketch
and keeps the existing source geometry checks. Otherwise the report says "its feature
profile has no earlier active resolved planar sketch mapping; this is not supported yet".
Resolve the producer's sketch binding or provide an explicit neutral profile selection;
a nearby sketch or the final body's contours do not prove that selection.

Profiles from sketch features require SolidCraft 0.45 or newer. Older targets retain
the original geometry and report "profiles from sketch features require SolidCraft 0.45
or newer; choose a newer target to keep this feature editable".

A resolved sketch frame alone does not prove profile geometry. A feature reference whose
sketch contains only points or reference axes is refused with "its sketch feature has no
resolved non-construction profile curves; this is not supported yet". The decoder must
supply the actual profile curves. An empty profile table can still use the complete bound
sketch when its non-construction curves are decoded; no loop or contour is guessed.

## Claim work before starting

Comment `/claim` on the issue first: claim, then a draft pull request within 24 hours,
even rough. Comment `/unclaim` to release it. After a linked pull request exists, claims
lapse after 5 days without activity from you. The bot reminds you at 12 hours without a
pull request, and a day before an idle claim lapses. A ready pull request waiting on a
maintainer or an open dependency keeps the claim active; maintainer claims never lapse.
On lapse, pull requests stay open for others to continue with the original author credited
in a `Co-authored-by:` trailer. See [claiming an issue](claiming.md) for the full rules.
