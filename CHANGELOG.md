# Changelog

All notable changes to Dutchman are listed here. Versions follow [Semantic Versioning]; until
1.0, a minor version may change the command line, the Rust API or the JSON reports (a JSON
report's schema name changes when a field does).

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

## Unreleased

### History

- An extrude, revolve or other sketch feature whose profile cadmpeg gives as whole sketch
  regions no longer makes a design SolidCraft refuses as damaged ("invalid analytic offset
  topology or boundary reference"). The one loop of a one-loop sketch is written as every
  profile. Otherwise each boundary is written in the order SolidCraft finds it: counter-clockwise,
  from its leftmost point. This affected Inventor parts read with cadmpeg's sketch-region
  profiles.

## 0.2.0 (2026-10-25)

### Known issue

- Some SolidWorks parts that 0.1.5 read now decode empty, fail cadmpeg's edge checks, or come
  through with fewer bodies (reported as losses, not as a clean conversion); a fix is in
  progress in cadmpeg. If 0.1.5 worked better for a part, keep using it for that part for now.

### Formats

- Convert CATIA `.CATPart`, Creo View `.pvz`, Rhino `.3dm`, ACIS `.sat`/`.sab`/`.smt`/`.smb`,
  and STEP `.step`/`.stp` through the geometry path. Creo and NX parts
  (`.prt`, including numbered Creo saves) are recognised by contents and report when geometry
  reading is not supported yet. SolidWorks assemblies remain recognised without a reader.
- IGES `.igs`/`.iges` files are recognised and refused before decoding, with directions to
  open them in SolidCraft directly (File > Import). SolidCraft's own importer sews their
  faces into solids; cadmpeg does not yet. IGES refusals do not invite test files.
- The CLI, JSON reports and desktop app show whether each format reads geometry, opens
  without geometry, is recognised without a reader, or should be opened in SolidCraft directly.
  `dutchman formats` lists that status alongside what Dutchman converts. After a refused
  conversion (except IGES) or a history that did not fully map, one invitation links to
  test-file recipes and private help with wrong conversions.
  `--quiet` hides it; JSON output contains no invitation.
- Directory conversion keeps the newest numbered `.prt.N` save per part and reports older
  revisions as skipped. Numbered saves use the part's base output name and keep existing outputs.
- The cadmpeg pin moves from `63ebadd` to `66550bb`:
  Creo View geometry becomes available, and Fusion As-built joints carry limit owners,
  slider limits and rest positions from the reader.

### Docs

- The README and guide link the Dutchman CAD Discord for questions and help.

### Install

- Install with Homebrew (`brew install dutchman-cad/tap/dutchman`) or Scoop (`scoop install
  dutchman-cad/dutchman` after `scoop bucket add dutchman-cad
  https://github.com/dutchman-cad/scoop-bucket`); both follow each release.

### Command line

- Convert with `dutchman part.f3d`, optionally followed by an output path. `convert` also
  accepts no output: it derives the path beside the input from `--to`. Existing derived
  outputs are kept (exit 2) unless `--force`; explicit outputs still replace by default.
  `--force` conflicts with `--no-clobber`, and a default cannot replace the input.

- Run installed Dutchman CAD tools with `dutchman <tool>`: executables named
  `dutchman-<tool>` next to the command take precedence over those on `PATH`.
  Help lists installed tools and versions. An installed Pilot takes precedence over the
  built-in Pilot and accepts its own flags. The std-only `dutchman-launcher` crate supplies
  a tiny `dutchman` front command for Pilot-only installations.

- `dutchman pilot` runs [Pilot](https://github.com/dutchman-cad/pilot) 0.1.0-beta.1:
  `dutchman pilot --help` lists its commands, and `dutchman pilot how "round all edges 3 mm"`
  shows example calls.

### Reports

- A joint Dutchman does not convert is reported by the output that leaves it out. A FreeCAD
  macro (`--to freecad`, experimental) builds cylindrical, ball, distance and other joints
  SolidCraft output leaves out, and no longer reports them as dropped.

- Fillets and chamfers map on any prior history: their input edges are matched on the body SolidCraft rebuilds from the history before them (one replay per design), and are refused, saying why, without `solidcraft-cli`.
- Sheet-metal edge flanges map supported dimensions on unchanged rectangular base sheets using proven input-edge midpoint picks.
- Experimental FreeCAD macros keep a mapped history prefix and hidden reference sketches when a later feature cannot be rebuilt, retaining the final STEP shape.
- Experimental FreeCAD macros rebuild assembly joints in the Assembly workbench (the fixed, revolute, and slider joints SolidCraft gets, through sub-assemblies, plus cylindrical, ball, distance, parallel, perpendicular, and angle joints between directly placed parts), with connector frames, limits, and grounding; if solving fails, the parts keep their stored placements.
- Write experimental native FreeCAD documents with `--to fcstd`, or check macros with `--to freecad --verify`, using bounded headless execution and per-part volume and bounding-box verification.
- Map constant-radius fillets and equal-setback chamfers from proven feature input edges.
- A Fusion constant-radius fillet with a tangency weight of exactly 1 maps as a circular
  fillet, its blend being circular in section, with an Info note ("tangency weight 1.0 treated
  as a circular fillet"). Other weights are still refused.
- An assembly keeps the sketches of its own file, hidden for reference in the design itself,
  with the parameters their dimensions use; they were left out before without a note.

### cadmpeg

- The cadmpeg pin moves to 63ebadd: a Fusion parameter whose owner frame cadmpeg cannot
  read is kept native-only instead of failing the decode, Fusion as-built joints and joint
  freedoms read, a Fusion fillet whose tangency weight carries a unit stays native, Fusion
  extrude profiles resolve from circle-bounded and cap faces, ambiguous Fusion face
  appearances are reported instead of failing, Inventor thread annotations project as
  cosmetic threads and Thicken joins through selection lists, and Creo 11-13 order tables
  read.
- The cadmpeg pin moves to 892efad: joints carry their connector frame sense on their
  operands, Fusion components bound through an unregistered link entity place, edges that
  bound a selected face keep their geometry, a failed Fusion manifest says where its framing
  stopped, more ACIS curve families read reversed, and native CAD state is kept through the
  Fusion, SolidWorks, Creo and NX readers.
- The cadmpeg pin moves to 8a7ae67: a Fusion rectangular pattern of faces with two
  directions carries both, and a Fusion rectangular or circular pattern of features names the
  feature it repeats instead of a set of faces, so it maps when that feature does; STEP output
  writes face boundary curves on their edges' ranges, and a FreeCAD chamfer or fillet keeps its
  edge ends on its vertices.
- The cadmpeg pin moves to 8f8ff2a: Inventor sketches whose tangent relations key a hint
  code to either operand close again (two parts regressed under 0872c6d).
- The cadmpeg pin moves to 0872c6d: Inventor parts saved by Inventor 2019 and later carry
  their feature history, and newer Inventor assemblies place their parts; FreeCAD compounds
  of closed solids read as solid bodies; Fusion reads every context binding of a component,
  more parameter expressions, and a base flange's thickness from its sheet-metal rule.
- The cadmpeg pin moves to 525b961: Fusion rigid groups of any size (each member held to the
  first as built), and more Inventor sketches closed (solver hints on more relations, equal
  length).

### cadmpeg

- The cadmpeg pin moves to a2407a3 (CADIR 9): the geometry of a feature's historical input
  edges, rigid as-built joints in Fusion, evaluable procedural faces (a coil's swept face
  among them) written to STEP as sampled B-spline surfaces, and fixes to ASM intcurve
  pcurves and FreeCAD restoration.

### Desktop app

- When macOS blocks SolidCraft's first run, the CLI and desktop app explain how to allow
  it in Finder or System Settings and try the check again, without changing quarantine.
- The desktop app is named **Dutchman Convert**, part of Dutchman CAD: its window title and
  heading, and on macOS **Dutchman Convert.app** with a "Dutchman Convert" disk image volume.
  The commands (`dutchman`, `dutchman-gui`, `dutchman-mcp`), their subcommands, the settings
  and the download file names are unchanged. A Mac with 0.1.5 or earlier keeps
  `/Applications/Dutchman.app` until it is replaced.
- The macOS disk image opens on an installation window: Dutchman and Applications side by
  side with an arrow between them on Dutchman's own background, and the read-me below. If
  the window cannot be laid out while packaging, the image is built plain as before.
- A file that fails says what to do next: **Report this on GitHub** opens the **A file
  didn't convert** form filled in with Dutchman's version, the system, the error with its
  stage and upstream, and the file's anonymous report (copied to paste when it is too long
  for the link). The form had come back with its report and stage boxes empty.
- The issue forms' privacy box says that links to files that are already public are welcome;
  it asked for no file names at all, which kept a reporter from linking the public file that
  failed.
- The line naming the SolidCraft that checks history stays on one line (the whole of it on
  hover): a long path to `solidcraft-cli` wrapped onto several lines and pushed the Convert
  button out of view at the window's default size.

### Assemblies

- A joint whose connector frames point the same way (its CADIR operands' `frame_sense:
  "aligned"`) is written as a flipped SolidCraft joint, and a part a joint places meets its
  other part z to z rather than face to face. A Fusion robot arm's gripper mounted on its
  wrist this way had been turned over into the wrist.
- When the source grounds none of the parts a set of joints ties together, Dutchman grounds
  the fixed side of that set's first joint, so SolidCraft's solver holds the set where the
  source places it, and the report names the occurrence as inferred (`Info`, `joints`).
- A Fusion design's root component is grounded where it is: the document's own, and the
  root of each design inserted into it, held by the occurrence that inserts it. Both were left
  free to move.
- CADIR versions 8, 9 and 10 are read.

- The desktop app prints `--version` and `--help` without opening a window, shows its version
  in the title, and has a **Report a problem** link to the **A file didn't convert** form. An
  unreadable-file error links to that form too.
- Sweeps accept single-curve whole-sketch paths and guide rails, and report specific reasons for unsupported corner transitions, section laws, tangent extension, and simplification.
- Lofts map in more forms: surface lofts become SolidCraft surface lofts, and ruled lofts
  through two sections map. Loft rails share the sweep path reader, so a single-curve
  whole-sketch rail maps and each unsupported rail kind gives its own reason.
- Hole history keeps chamfered entries and ISO metric thread labels on tapped holes (thread
  details SolidCraft has no field for are reported, not blocking), and names the unsupported
  hole forms, placements, and ends it refuses.
- Revolves keep more of their history: unequal sides that add up to a full turn, surface
  revolves short of a full turn, and an axis drawn in the profile's sketch (kept as that
  line). Ends at a face, vertex or body and other face makers stop the history with a reason
  that names them.
- Patterns map named bodies, planar sketch paths, regular explicit offsets and angles, and two linear composite stages; unsupported pattern options receive specific refusals.
- Sketch history carries bounded conics, rational quadratic Béziers, equal-weight splines, and more geometric relations and dimensions; unsupported constraints retain counted, specific loss messages.
- A second issue form, **A file didn't convert**, reports one file: its format, the versions,
  the operating system, what was expected and what happened, and that file's anonymous
  report. Both forms list FreeCAD documents (`.FCStd`), and the agent skill does too.
- Body mirror patterns and body booleans (join, cut, or intersect, optionally keeping the
  tools) map to SolidCraft. Shells and drafts, shape-replacing mirrors, and threads without a
  resolved standard and pitch say why they are not mapped.
- Sheet-metal edge flanges and hems say why they are not mapped: an edge flange's edge is
  known only after the flange has bent it, and a hem's developed length or its rolled,
  teardrop, or unresolved form has no SolidCraft equivalent.
- Flattened designs keep mapped sketches with resolved planes, hidden for reference after the
  imported geometry, and report how many were kept or left out.

### FreeCAD output (experimental)

- `dutchman convert --to freecad` (and `convert-all`, the MCP tools and the desktop app's
  output choice) writes a FreeCAD rebuild macro (`.FCMacro`) instead of a SolidCraft design:
  it rebuilds the mapped history through FreeCAD's public scripting API and keeps the
  original geometry as embedded STEP, which it restores when the rebuild fails or differs.
  Dutchman does not run FreeCAD, so these histories are unverified. Geometry whose STEP uses
  replicated trimming curves (`CURVE_REPLICA`), which crash FreeCAD 1.1's reader, is refused.

## 0.1.5 (2026-10-22)

### Inventor history, and FreeCAD in stable builds

- Dutchman builds against cadmpeg at one tested commit of its `main` branch (60b6065), the
  same for every cadmpeg crate, instead of waiting for cadmpeg releases. That brings Inventor
  extrude profiles resolved to sketch profiles and regions (trimmed curves included), with
  arcs, polygons and more sketch constraints, so Inventor part histories replay; FreeCAD
  bodies read as FreeCAD shows them; Fusion component names and joint motion; FreeCAD
  spreadsheet values; and fixes to how ASM pcurves, torus pcurves and tolerant edges are
  read. The nightly build still takes cadmpeg's latest `main`. Building from source works as
  before, with `cargo install --git`. Reports name the cadmpeg version with its commit
  (`0.6.3+60b6065`).
- FreeCAD documents (`.FCStd`) convert in stable builds: the `freecad` feature of `dutchman`
  and `dutchman-gui` is on by default. A build without it (`--no-default-features`) says it
  leaves FreeCAD files out and where the release builds are.

### Feature history

- Holes: counterbores and countersinks at a sketch's points, through all or to a
  flat-bottomed depth; holes at a directed position without a sketch, on an axis through the
  cap of the extrusion before them, at selected sketch points and centres, with a drill-point
  form, and ending at that earlier cap. Tapered holes, and counterbores or countersinks with a
  drill point, are not mapped yet.
- Extrudes that start or end at a planar face of an earlier body, one-sided extrudes with a
  taper, and start offsets against the sketch normal; two-sided revolves with equal angles.
- Sweeps carry an end scale and a fixed-binormal orientation, and sketch profiles can sweep
  into surfaces; taper, corner transitions and changing sections are not mapped yet.
- Lofts end at a point or vertex, and several sections may be profiles of one sketch.
- Coils map to SolidCraft coils: along any axis, up or down it, of round or square section,
  inside, centred on or outside their path, turning either way. The report says why a tapered
  coil, a flat spiral or a coil of triangular section is not mapped: SolidCraft's coil has no
  such option. When cadmpeg's STEP writer leaves out a helix surface (the swept face of a
  coil or thread), the report says so in plain words (`Degraded`, `geometry`).
- Circular feature patterns over a partial turn map, with copies at both ends of the span.
- Selected sketch profiles that cross themselves map through the regions they split into;
  a native profile pick resolves when even-odd nesting gives one region with its holes.
- A solid line drawn across a sketch profile from one point of its boundary to another, such
  as the diagonals of an Inventor centre-point rectangle, is written as construction when the
  sketch's profile table takes that profile whole, and the report counts them (`Info`).

### FreeCAD

- A grounded joint in a FreeCAD assembly becomes a rigid joint to the design, holding its part
  where it is stored, so the assembly's other joints keep their motion.
- Leaving out a FreeCAD document's hidden bodies no longer takes their sketches with them.

### Assemblies

- An Inventor assembly that places no parts, as Inventor's own assembly templates do,
  converts as an empty design with a note ("the assembly places no parts"), verification
  `not_applicable`, instead of failing with "none of the assembly's parts could be found or
  read".

### Parameters and reports

- A design holding parameters and nothing else converts as a SolidCraft design of those
  parameters (verification `not_applicable`). A design with no bodies and no parameters fails
  up front, saying it has no geometry.
- A design none of whose features cadmpeg decodes has no history to write: it converts as
  geometry with verification `not_applicable`, not as a history flattened.

### Getting started

- A file that cannot be read as its format (cut short, damaged or misnamed) says how to
  recover: copy it again or check its extension, and report it anonymously if the original
  opens. The CLI and the MCP tools give the same advice.
- The desktop app says what to check when its window cannot start (a desktop session and
  working graphics drivers), and exits with that advice when graphics start stalls for 30
  seconds instead of waiting.
- The guide and CONTRIBUTING start problem reports from an anonymous report, explain the
  sample cube's expected warnings, and the MCP tool descriptions say exactly what
  `convert_all` writes and that a successful `inspect` does not mean a file converts.

### Feature history

- More holes map: at a directed position without a sketch, on an axis through the cap of the
  extrusion before them, at selected sketch points and centres, with a drill-point form, and
  ending at that earlier cap. A tapered hole is not mapped yet.

- The report says why a tapered coil, a flat spiral or a coil of triangular section is not
  mapped: SolidCraft's coil has no such option.

- Extrudes that start or end at a planar face of an earlier body, one-sided extrudes with a
  taper, start offsets against the sketch normal, and two-sided revolves with equal angles
  map to SolidCraft.

- Sweeps now carry end scale and fixed-binormal orientation, and sketch profiles can sweep into
  surfaces; taper, corner transitions and changing sections remain unmapped.

- Selected self-crossed straight sketch profiles map through their split regions; native sketch selections resolve when even-odd nesting identifies one region with its holes.

- Lofts map resolved point and vertex ends and retain separate profile selections for sections
  that share a sketch.

- Coils map to SolidCraft coils: along any axis, up or down it, of round or square section,
  inside, centred on or outside their path, turning either way, by turns and pitch, turns and
  height, or height and pitch. Tapered coils, flat spirals and triangular sections are not
  mapped yet.

- Circular feature patterns over a partial turn now map with copies at both ends of the span.
- Counterbore and countersink holes at a sketch's points map to SolidCraft holes, through all
  or to a flat-bottomed depth. One with a drill point at its bottom is not mapped yet.
- Coils map to SolidCraft coils: up the Z axis, of round or square section, inside, centred on
  or outside their path, turning either way, by turns and pitch, turns and height, or height and
  pitch. Tapered coils, flat spirals and triangular sections are not mapped yet.
- When cadmpeg's STEP writer leaves out a helix surface (the swept face of a coil or thread),
  the report says so in plain words (`Degraded`, `geometry`): the solid it bounds opens without
  it, with no volume.
- A solid line drawn across a sketch profile from one point of its boundary to another, such
  as the diagonals of an Inventor centre-point rectangle, is written as construction when the
  sketch's profile table takes that profile whole: solid, it would split the profile, and
  SolidCraft found no profile to match. cadmpeg does not yet say which Inventor sketch curves
  are construction. The report says how many lines were written so (`Info`).

### Parameters

- A design holding parameters and nothing else (no features, no geometry), as a Fusion
  design other designs take their parameters from, converts as a SolidCraft design of those
  parameters, with verification `not_applicable` ("the design holds 11 parameters and no
  geometry") and an `Info` line, instead of failing for want of geometry. A design with no
  bodies at all and no parameters now fails up front, saying it has no geometry, rather
  than writing an empty import SolidCraft cannot open.

### Reports

- A design none of whose features cadmpeg decodes, such as an Inventor part made of a base
  solid, has no history to write: it converts as geometry with verification `not_applicable`
  ("no feature history cadmpeg decodes: its one feature is kept in its native form") and an
  `Info` line, not as a history flattened. A native feature beside ones that map still
  flattens the history, as before.

  recover, in the CLI and the MCP tools alike.
- The desktop app says what to check when its window cannot start, and exits with that advice
  when graphics start stalls for 30 seconds.
- The guide and CONTRIBUTING start problem reports from an anonymous report and explain the
  sample cube's expected warnings; the MCP tool descriptions say exactly what `convert_all`
  writes and that a successful `inspect` does not mean a file converts.

## 0.1.4 (2026-10-22)

### Privacy

- **The anonymous report (`--report-anon`) of 0.1.3 can hold names: re-run with 0.1.4 before
  sharing one.** Two causes, both fixed. Codes of Dutchman's own losses were made from the
  loss's subject, which is a file name as often as a kind (`dutchman/<letters of a file
  name>`); a code is now `dutchman/` and one of a fixed set of words, else `dutchman/other`,
  and a cadmpeg code that is not a plain code reads `cadmpeg/other`. And paths relative to an
  Inventor project or the folders its project file names, and part names the report was never
  told of, were left in messages. Each conversion now records every name it meets (the paths
  and folders it reads from, and every name the design gives anything; `names` in the convert
  report), and the anonymous report takes each one out, with any word holding a path
  separator. A `--journal` written before this records no names, so its files are converted
  again rather than reported from it, whatever version wrote it.
- The anonymous report of a large batch is made in one pass over each message: one of about
  7,000 files had not finished after an hour and a half; 8,000 files now take seconds.

### FreeCAD

- Bodies a FreeCAD document keeps hidden, such as the shape of each step inside a PartDesign
  Body, are left out with the geometry only they use, and the report says how many (`Info`);
  history and structure are kept. It takes effect once cadmpeg marks those bodies hidden.

### Checks

- Sheet bodies (surfaces, open by design) are no longer reported as bodies inside out or
  open in SolidCraft's check: SolidCraft gives an open sheet a volume, of either sign, and
  warns that its shell is not closed. As many such findings as the design has sheets are put
  down to them, with an `Info` line saying how many surface bodies were not checked; solids
  inside out or open beyond that are still `Degraded`.

- A `solidcraft-cli` older than SolidCraft 0.18.2, the oldest that opens and checks Dutchman's
  designs, is not used: the history is written unverified, saying so and naming the version
  found, and the desktop app says it too. CI opens converted designs in SolidCraft 0.18.2 and
  0.19.0, and the nightly checks with 0.19.0. The guide says which versions are supported.

### Desktop app

- The macOS app icon is a white rounded square with the mark, as macOS icons are, and the Dock
  shows it while the app runs too: it showed the bare mark there, dark on the Dock's dark
  glass. Elsewhere the window icon stays the mark.

### Reports

- A pattern cadmpeg does not decode says so ("cadmpeg does not decode what it repeats or how
  yet", "… its layout yet", "patterns of faces are not mapped yet: cadmpeg does not resolve
  which") instead of "it patterns nothing", which now means a pattern that really repeats
  nothing.

- A design SolidCraft opens with faces missing says so plainly: "SolidCraft opens 52 of the 53
  faces converted: 1 face is missing there", in place of "SolidCraft shows 52 of its 53
  faces". The guide says what to do about it.

- An assembly says what it brought across: the JSON reports give the joints written by kind
  (`joints`), and the desktop app's card reads `41 components, 6 joints (5 revolute, 1 rigid)`,
  or `18 of 20 components placed, …` when some were left out.

### Feature history

- A profile cadmpeg keeps as a native selection maps when its sketch has one profile, which
  the selection can only be; in a sketch of several it says it is not mapped yet.

- Sheet-metal base flanges map onto SolidCraft base flanges, under a sheet rule of their
  thickness, on the sketch's side the source puts them. A flange thick on both sides of its
  sketch is not mapped yet.

- Fusion base features (bodies a timeline holds as they are, modelled directly or imported)
  map onto SolidCraft Import features at their place in the timeline, each holding its own
  bodies as STEP, so the features after them keep their history instead of the whole design
  being flattened. A base body that later features change is caught by the check against
  the original geometry.

- A component inserted, or a joint made, in a Fusion timeline is counted as not needed in the
  part's history, since the assembly carries the component and the joint, instead of
  stopping the history.

- A sketch profile with curves drawn across it, such as a rectangle with its diagonals, maps
  when the feature takes every profile of the sketch. Fusion walks such a profile along each
  inner curve both ways; Dutchman selected it by those curves and SolidCraft found no profile
  bounded by them, so the replay failed and the history was flattened. Any other such selection
  is reported as not mapped yet.

### Assemblies and components

- A joint to a component inside a design inserted into a Fusion archive takes its frame in
  that component's own space, as Fusion stores it, not in the inserted design's: the robot
  arm's gripper sat 52 mm off its wrist flange.

- A fixed joint whose parts' stored placements already meet, with its frames opposed or (as
  Fusion mates a flipped joint) aligned, holds them where they are, as revolute and slider
  joints already did, instead of turning one part over to mate the frames opposed. Fusion's
  files do not yet say which joints are flipped; a part only a flipped joint places, such as
  a design inserted into an archive, can still come out turned over.

## 0.1.3 (2026-10-21)

### Formats

- **FreeCAD** documents (`.FCStd`) convert in the nightly builds, through cadmpeg's FreeCAD
  codec: their geometry, and their components when cadmpeg says which bodies each owns.
  Stable builds say so instead, until a cadmpeg release reads FreeCAD solids right. FreeCAD
  support is the `freecad` cargo feature, which the nightly build turns on. See
  [docs/freecad.md](docs/freecad.md).

### Feature history

- Datum planes the source places (FreeCAD's PartDesign Plane) are counted as not needed:
  what is sketched on them keeps the same plane of its own.

- Nodes that only order the feature tree (folders, FreeCAD's PartDesign Body, comment and
  annotation containers) are counted as not needed instead of stopping the history.

- Simple holes drilled at a sketch's circle centres, arc centres or points map onto
  SolidCraft holes: through all, or to a depth with a flat end or a drill point. FreeCAD's
  Hole is one; counterbores, countersinks and holes placed on faces are not mapped yet.

### Desktop app

- After **Convert**, the window scrolls to the results as the run starts, and to the first
  file that failed or converted incomplete when it ends, so the first problem is on screen at
  any window size.

### Reading

- A part whose cadmpeg model fails cadmpeg's checks (a parameter name repeated in one scope,
  a parameter used before it is defined) is no longer refused. Its geometry is read again
  without the design's history, sketches and parameters, where such failures lie, and written
  as geometry, with a `Degraded` loss saying why. In an assembly, the part is placed instead of
  left out. When the geometry fails cadmpeg's checks too, the error says what each
  attempt found.

### Checks

- A history whose original geometry SolidCraft opens unsoundly (solids as empty shells,
  bodies inside out) is kept, unverified, saying so, rather than flattened onto that geometry:
  there is nothing sound to compare its replay with. A history that does not replay is still
  flattened.
- The check of geometry written without history also reads each body's volume in SolidCraft:
  a solid it gives no volume (an empty shell) or a body it reads inside out is reported as
  `Degraded: geometry`, as missing faces already were.
- Installing the SolidCraft app is enough to check histories: without `SOLIDCRAFT_CLI` or
  `solidcraft-cli` on `PATH`, Dutchman uses the CLI inside the newest installed SolidCraft app
  (macOS `SolidCraft*.app`, Windows `Program Files\SolidCraft`). The desktop app says which CLI
  it checks with. `dutchman_solidcraft::find_cli` is public.

### Releases

- macOS: each release has a disk image, `dutchman-VERSION-macos-universal.dmg`, holding
  **Dutchman.app** with its own icon, for Apple silicon and Intel, with the `dutchman` and
  `dutchman-mcp` commands inside. It is signed ad hoc, not with a Developer ID; the guide shows
  how to open it the first time. The archives stay as they were.

- The README, and each release's notes, start with a direct download for each platform, with
  its checksum, and how to tell which Mac you have.
- [Testing Dutchman on your library](docs/testing-your-library.md) is updated for this
  release: the macOS app, SolidCraft found in its app, Inventor project search, the incomplete
  count and `dutchman.log`, and when to try the nightly.
- A nightly build: Dutchman's main built against cadmpeg's main every day, when either has
  changed, and published as the `nightly` pre-release once every test and the SolidCraft round
  trip pass. It brings cadmpeg fixes that are merged but not yet released; stable releases keep
  using released cadmpeg only. See [Channels](docs/guide.md#channels-stable-and-nightly).

### Reports

- A file with more than 12 losses is summarised by component in the text reports: the design
  first, then each part with its counts and, when its history was flattened, why. `--verbose`
  prints every line. `convert --log FILE` writes the whole report, every loss in full, to a
  file, and `convert-all` writes one for the run to `dutchman.log` in its output folder (or
  `--log FILE`). The desktop app folds each part's losses under its own heading and saves the
  full log with **Save the full log…**.

### Assemblies and components

- An assembly's conversion says how complete it is, before you open it: `assembly: 18 of 20
  placements placed; 2 left out:`, then each cause with its file and count, counting through
  sub-assemblies as often as they are placed. `convert`, `convert-all` and `inspect` print it;
  the JSON reports carry it as `completeness`. The desktop app marks such a file
  **Incomplete**, shows the count and the causes on its card, and counts incomplete files in
  the run's summary. The anonymous report counts them as `incomplete`.
- Inventor assemblies find their parts as Inventor does: at the stored path, relative or with
  the saving machine's drive or root moved onto this one, else by name anywhere under the
  project folder (the nearest above holding an `.ipj`, the folder given to `convert-all`, or
  `--project DIR`), preferring the file in the stored path's folders. A project whose parts
  are in sibling folders (`../Parts`, `../SubAssembly`) converts where it lies. `--search DIR`
  adds folders searched after the project's, and files found are named as their folders list
  them. `dutchman_core::read_in` and `convert_traced` take a `PartSearch`.
- The desktop app names the part files an assembly could not find, and converts it again
  searching a folder, or a found file's folder, chosen there. Its **Inventor project folder**
  setting gives the project folder for a run.
- Revolute and slider joints become SolidCraft revolute and slider joints, revolutes with their
  limits, so the parts move as in the source; fixed joints keep their offset. Joint frames meet
  face to face, as in Fusion.
- Joined parts are placed where their joints put them, as Fusion can store a joined part where
  it was before its joints were solved.
- An Inventor assembly's part files are also searched for in the folders its Inventor project
  file (`.ipj`) names: the workspace, workgroup and library folders, and the
  imported-components folder. The project file is the nearest one in or above the project
  folder, so `--project` given a folder inside a larger project still finds the files
  elsewhere in it. The report lists the folders searched, and a part not found says how many
  were. Folders below the project folder that hold a project file of their own (Pack and Go
  copies) are no longer searched.
- A design inserted into a Fusion archive (`.f3z`) is placed as a sub-assembly, named as the
  archive names it, and a joint to a component inside it joins that design as a whole (its own
  joints are not carried yet). Such an archive converted as geometry alone before, without its
  joints.
- A revolute or slider joint whose parts already turn or slide on its axis where they are
  placed holds them there, at the angle or distance their placements give, instead of moving
  them to the value the source stores. Revolute limits that this angle lies outside are left
  out, with a loss saying so.
- A revolute joint's angle in the source is taken as the angular offset that aligns its frames,
  as a fixed joint's is, not as where it has turned to: Fusion stores no current angle, only
  the placements. Its limits hold from the aligned frames.

## 0.1.2 (2026-10-21)

### Testing on your own library

- `convert-all` converts each file in a process of its own: a file that runs past `--timeout`
  (default 900 s) or holds more than `--max-memory` (default 8192 MB) is stopped and fails
  alone, and one that crashes the converter no longer takes the batch down.
- `convert-all --journal FILE` records each file's result as it finishes, so a stopped run
  carries on where it stopped without converting files again.
- `convert-all --report-anon FILE` writes an anonymous report, safe to share: counts by format,
  every failure and loss by cause and stage, blocked features, timings and versions, with each
  file named only by a salted hash of its path. `--report-salt` keeps the ids from one run to
  the next and `--report-key` writes the private key from ids to paths. The MCP `convert_all`
  tool takes the same options.
- Every loss and failure in the JSON reports says its `stage` (`input`, `decode`, `project`,
  `map`, `write`, `solidcraft`) and, when it arose in cadmpeg or SolidCraft, its `upstream`.
  Each `convert-all` result gives its `elapsed_ms`, and the report counts skipped files by
  extension.
- Formats Dutchman does not convert yet point to Dutchman's issues rather than cadmpeg's.
- New guide, [Testing Dutchman on your library](docs/testing-your-library.md), with a
  library test report issue form; [docs/triage.md](docs/triage.md) says how reports are sorted
  and taken upstream.

### Feature history

- Lofts guided by one or two rails of sketch curves keep their rails. A loft along a centreline
  still flattens, with the reason.
- Mirrors of earlier features across a plane map onto SolidCraft mirrors.
- Sphere and torus primitives map onto SolidCraft's (a torus about the Z axis). Boxes and
  cylinder primitives still flatten, with the reason: cadmpeg does not yet say where they sit.

## 0.1.1 (2026-10-21)

### Desktop app

- `dutchman-gui`: drop design files or folders, choose an output folder and convert. Each file
  shows what was written and its losses, and opens in SolidCraft or in its folder. Day and Night
  themes, following the system's setting. It ships in the release archives.

### Feature history

- Lofts through profiles of two or more sketches map onto SolidCraft lofts, when they have no
  guide rails or centreline and are open, smooth and solid.
- Sweeps of a sketch profile along sketch curves map onto SolidCraft sweeps: along the whole
  path, square to it or held as drawn, optionally twisted or guided by one rail.

### Assemblies and components

- Fusion joints between components become SolidCraft rigid joints, written as built, so the
  parts they join stay together. Joints of other kinds are reported as Dropped.
- A component holding other components converts when it is placed more than once at one level;
  such designs were wrongly refused before.

### Checks

- When `solidcraft-cli` is found, geometry written without history is opened in SolidCraft too,
  and the report says `Degraded: geometry: …` when SolidCraft shows fewer faces than the source
  model has or warns about it.

### Command line

- `convert` first prints what it wrote where, `converted  IN -> OUT (geometry without
  history)`, as `convert-all` does for each file, now in plain words for both.
- Long loss notes are cut to their first sentence in the text reports, marked `…`; `--verbose`
  prints them whole. `--json` is unchanged.
- `convert-all` lists each converted file's losses beneath it, as `convert` does, and counts the
  files it skipped as not in a format it knows (`skipped` in `--json`).
- `inspect` says when converting a file would fail and why (`would write: nothing; …`, and
  `conversion` in `--json`), instead of predicting geometry it cannot write.
- `--no-clobber` (`overwrite: false` in the MCP tools, and a checkbox in the desktop app) keeps an
  existing `.solidcraft` file; that file's conversion fails instead. `--help` and the MCP tool
  descriptions now say that an existing file is otherwise replaced.

### MCP server

- A tool call with arguments the tool cannot take returns a failure report as structured
  content too, the same shape as a failed conversion, not only text.

### Reading and documentation

- Reads native files with cadmpeg 0.6.3.
- The release archive's README links to the docs of its own release; the getting-started guide
  has a complete install and sample download.

## 0.1.0 (2026-10-21)

The first release that converts designs. Native CAD files are read by
[cadmpeg](https://github.com/cadmpeg/cadmpeg) 0.6 and written as
[SolidCraft](https://github.com/tangency-labs/solidcraft) designs (`.solidcraft`, format
`solidcraft/23`).

### Formats

- **Fusion** archives (`.f3d`) and multi-document archives (`.f3z`, with the designs they
  reference merged in by cadmpeg).
- **Inventor** parts (`.ipt`) and assemblies (`.iam`), with their sub-assemblies. Part files
  are found at the stored path, else by name next to the assembly, ignoring case and either
  path separator.
- **SolidWorks** parts (`.sldprt`). SolidWorks assemblies are refused with a message.

### Feature history

- When every active feature maps, the design is written as editable SolidCraft history, with
  the original geometry kept beside it as a suppressed import. Otherwise it is written as the
  geometry, and the report says which feature stopped the history and why.
- Mapped:
  - planar sketches: points, lines, circles, arcs, whole ellipses, clamped uniform splines,
    with their constraints and dimensions (angles too), linked to parameters;
  - parameters, keeping their expressions where SolidCraft reads them the same way;
  - extrudes of whole sketches, profiles, profile regions or selected curves: one-sided,
    symmetric or two-sided, blind or through all, as new body, join, cut or intersect;
  - revolves that are symmetric or a full turn;
  - patterns of features, linear in one or two directions, or circular all the way round.
- A written history is replayed in `solidcraft-cli` and compared with the original geometry
  (body count and volumes). A history that differs is flattened. Without a CLI, or with
  `--no-verify`, it is written and reported unverified. `--import-only` never writes history.

### Assemblies and components

- Assemblies become SolidCraft components and occurrences: one component per part or
  sub-assembly, one occurrence per placement. Each part keeps its own feature history when it
  maps. Its parameters, and its body names, are renamed with its component's name where
  another part already uses them.
- Designs that define and place their own components (Fusion) convert as SolidCraft
  components when cadmpeg says which bodies each owns. Components inside components become
  sub-assemblies.

### Appearances

- Body colours, and planar face colours, carry across: through STEP for imports, and as
  SolidCraft looks for replayed bodies, including bodies of parts inside assemblies.

### Command line

- `dutchman convert`, `dutchman inspect` (what a conversion would carry across, writing
  nothing), `dutchman formats`, and `dutchman convert-all` (many files and folders into one
  output folder, several at a time).
- `--json` on every command prints one report with a stable schema (`docs/json.md`). Exit
  codes: 0 converted, 1 could not convert, 2 usage error.

### MCP server

- `dutchman-mcp` serves the same four commands as Model Context Protocol tools over stdio, for
  AI agents. Each tool returns the matching `--json` report. The README shows how to add it to
  a client.

### Robustness

- Hostile input comes back as an error, never a crash. Inputs over 1 GiB are refused before
  they are read, cadmpeg decodes under its resource limits, and a panic inside cadmpeg is
  caught wherever Dutchman calls it. A deterministic test runs every fixture, truncated and
  corrupted, through the whole conversion.
- Output is written only when the whole conversion succeeds.

### Known limits

- Fillets, chamfers, holes and partial one-sided revolves are not mapped yet: they wait on
  cadmpeg giving typed edge and face references and a revolve direction. Inventor and
  SolidWorks extrudes wait on cadmpeg resolving their profiles. See `docs/fusion.md`,
  `docs/inventor.md` and `docs/sldprt.md`.
- Assembly joints wait on cadmpeg's next IR version.

## 0.0.1

Name reservation on crates.io. No functionality. Later releases ship as GitHub release
archives; the crates.io crate stays this placeholder until the core API settles.
