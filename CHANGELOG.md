# Changelog

All notable changes to Dutchman are listed here. Versions follow [Semantic Versioning]; until
1.0, a minor version may change the command line, the Rust API or the JSON reports (a JSON
report's schema name changes when a field changes incompatibly).

Unreleased changes live in [changelog.d/](changelog.d/).

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

## 0.2.5 (2026-10-30)

### Formats

- Convert FreeCAD TechDraw pages to DXF with `convert --to dxf`, one file per page,
  or select a page with `--page NAME`. Keep neutral view curves, visible and hidden
  line types, dimension labels and measurements, annotations, template frames, and
  resolved title-block text in millimetre page space. Report unsupported content
  and entity counts per page in JSON.

### Command line and install

- Target the installed SolidCraft release, even with verification off, or the newest release
  known at build time (currently 0.26.0). Dutchman is made for releases from the last two
  weeks, currently 0.18.2–0.26.0 (the window never starts below 0.18.2, the oldest accepted
  target). Features a target cannot open keep the original geometry with a named reason.
- Counterbores and countersinks write their explicit flat tip for targets 0.22.1 and newer,
  the first SolidCraft release that applies a drill-point angle to them.

### cadmpeg

- The cadmpeg pin moves from `2e01486` to `1923e49`. Fusion components own the bodies their
  body containers hold, and operation-table poses apply the same way to every occurrence of a
  definition, so assemblies are placed closer to the design. Inventor derived parts read their
  stored bodies, positional two-section lofts are read, and rectangular patterns keep their
  suppressed instances as skipped grid positions. SolidWorks parts with unusual edge topology
  are refused rather than read wrong.
- A rectangular pattern that skips grid positions is not mapped yet: SolidCraft's rectangular
  pattern fills the whole grid. The report names the pattern and how many positions it skips,
  and the part keeps its exact geometry.

### Docs

- The README's download table links the universal Mac command-line archive for Apple silicon
  and Intel, beside the universal desktop disk image, now that a release carries it.

## 0.2.4 (2026-10-29)

### History

- FreeCAD MultiTransforms containing mirrors keep editable history as ordered mirror and
  pattern features when replay matches the original, targeting SolidCraft 0.25.1 or newer.
  Older targets keep the original geometry with `needs SolidCraft 0.25.1 or newer`
  for chained stages; single-stage mirrors and patterns remain available.
  Full-turn circular patterns omit a
  repeated seed endpoint while retaining every distinct instance.
  An unfinished replay check keeps the original geometry for these patterns.
- Whole-sketch hole placements exclude construction geometry, so reference origin points
  do not create extra holes. Explicit entity selections retain construction locations.
- Constant-radius FreeCAD additive and subtractive helices keep their sketch profiles,
  axes, pitch/height/turn laws, handedness, and reversed travel as editable coils when
  targeting SolidCraft 0.25.1 or newer. Older targets preserve the original geometry and
  report the required version. Conical helices and spirals report the missing coil option.
- Choose the target release with `--solidcraft-version x.y.z` on `convert`, `convert-all`,
  or `watch`, or `solidcraft_version` in MCP. The default is the found verification CLI's
  version, else 0.18.2; with `--no-verify`, it defaults to 0.18.2. Targets below 0.18.2 are
  refused. Counterbore and countersink holes retain flat bottoms on newer targets too.
- Skip unused FreeCAD binders and datum helpers with an Info note. Carry used datum
  placements through resolved sketches, pattern axes, and datums, and explain unresolved uses.
  Keep helper Info notes when another feature or replay check flattens the history.
- Respect hidden FreeCAD containers when filtering bodies, including children marked shown.
- Preserve supported spreadsheet formulas and qualified parameter references as live SolidCraft
  expressions. Omit text cells silently, identify unused numeric cells, and report specific
  reasons when expressions must retain their evaluated values.
- FreeCAD sketches with negative horizontal or vertical distances replay with positive
  dimension magnitudes, preserving their solved positions and signed parameter expressions.
- Keep editable features before the first blocking feature, plus resolved sketches, hidden
  beside the visible imported final shape. Reports name the blocker, count retained features,
  and distinguish partial replay from verification of the full history.
- Retain a replayable timeline for correction when its final shape differs from the source.
- Retry sketches that fail replay without constraints, retaining their solved geometry and
  reporting the lost constraints.
- Keep resolved reference sketches in parts and assemblies even when no body-building
  feature or source timeline can be retained.
- Convert stored geometry as an imported base body, keeping later mappable features.
  Report imported solids as Info, without a flattened-history warning when no recipe exists.

### Assemblies

- Convert FreeCAD assemblies that link Bodies and Parts in other `.FCStd` documents, finding
  siblings first and reporting missing parts by name. Keep grounding and supported joints.
- Place FreeCAD link-array elements once, without duplicate LinkElement occurrences.
- Fusion components and bodies the design hides open hidden in SolidCraft: a hidden occurrence,
  or one whose part holds only hidden bodies, is placed hidden instead of shown, and can be
  shown again there. A part with shown and hidden bodies is still shown whole.
- Group each top-level design's features into a named timeline folder when an assembly
  contains multiple designs, keeping feature order and leaving joints outside folders.

### Desktop app

- **Report this on GitHub** opens for signed-in GitHub users again. A long filled-in link made
  GitHub answer "406 Not Acceptable"; the link now stays under 4,000 characters, a report that
  does not fit is copied for you to paste, and a very long error message is shortened in the
  form (the anonymous report keeps all of it).

### Reports

- A design whose faces SolidCraft merges (faces that share one surface) is no longer reported
  as missing faces. The history check compares area and volume, so an open-shell warning
  there is no fault when they agree; unmeshable faces, bodies inside out, solids without
  volume, and solids that geometry-only output opens as open shells are still reported.
- Reads and conversions show their stage, designs read when available, and elapsed time, with a
  still-working heartbeat every five seconds. Desktop cards show a reading progress bar
  when the design total is known. CLI progress goes to stderr, as JSON lines with `--json`,
  and is hidden by `--quiet`; stdout keeps its final report. Fusion `.f3z` member counts
  are not available yet.

### cadmpeg

- The cadmpeg pin moves from `e06fa52` to `2e01486`. Fusion designs whose sweeps or rotational
  supports failed cadmpeg's model checks convert now. Inventor holes are drilled the way the
  hole points (they used to replay outside the part and fall back to exact geometry), browser
  work planes are read as datum planes, and a split of a solid is read and reported as not
  mapped yet. SolidWorks parts keep their stored face senses.

### Docs

- The container image (`ghcr.io/dutchman-cad/dutchman:X.Y.Z` and `:latest`) is published when a
  release is published, from that release's own archives, instead of when its tag is pushed,
  so a draft release never reaches the registry.
- Build macOS release and nightly targets in parallel, reuse their checked archives for the
  universal disk image, and share per-target Rust caches between nightly and release builds.
- Mac command-line downloads include one universal archive for Apple silicon and Intel.
  Release notes link it alongside the universal desktop disk image; per-chip archives
  remain available for Homebrew.

## 0.2.3 (2026-10-28)

### Formats

- FreeCAD parts no longer lose faces bounded by trimmed elliptical (or other trimmed) edges:
  leaving out hidden intermediate shapes kept the edge's curve but dropped the construction
  and full curve it is cut from, so the edge, its loop and its face were left out of the
  geometry.

### History

- FreeCAD fillets and chamfers can use edges of their base feature's stored shape, with
  each edge proven on SolidCraft's replay before the blend. Hidden intermediate shapes
  remain left out of the converted geometry.

### Assemblies

- A Fusion joint from a part inside one copy of a component placed more than once is left out
  when another joint already holds that copy itself to the same occurrence, as a rigid group
  holding both stores. It held the copy a second time, with another frame, so one of the two
  joints could not hold.
- A Fusion joint between a component and a part inside it, as each rigid group stores one for
  every member, moves the inner part and keeps the component where the design places it.
  Inserted designs anchoring their own rigid groups were moved to meet their parts, losing
  their placement in the assembly.
- A Fusion joint between a copy of a component placed more than once and a part inside that
  copy is the component's own joint, written through its first copy like the part. It joined
  the copy to the first copy's part, so every later copy moved onto the first.
- Joints that reach nothing placed, such as a design placed where it is stored that holds its
  parts in a rigid group, are placed from the outermost occurrence they join, which stays
  where the file puts it. They started from whichever joint came first, so a part-to-part
  joint could fix both parts where they were stored and leave the design's own joints
  unmet.
- A joint that moves a component leaves the parts inside it that other joints already placed
  where those joints put them. In a Fusion rigid group listing a design's parts before the
  component holding them, the later joint moved the parts away from the earlier ones.
- Fusion components the file places at the pose it shows them in stay there: their joints
  are checked where they are instead of moving them back to where they were inserted. A
  printer assembly's linear carriages, idler washers, and electronics now open where the
  design shows them.

### cadmpeg

- The cadmpeg pin moves from `fb1cd0a` to `e06fa52`. SolidWorks parts that 0.1.5 read and 0.2.0
  to 0.2.2 read empty, with fewer bodies, or failing cadmpeg's edge checks read again: bytes
  inside other records no longer hide or refuse a part's bodies, arcs stored past the seam keep
  their range, and solid snapshots stored with features are no longer read as sketches. Fusion
  reads rigid groups and their joints in full, with members mated aligned, and binds joint
  operands in other designs, in nested components and in merged member designs to the exact
  occurrence; places designs inserted at the top of an assembly at their current pose, not
  where they were first inserted; reads construction planes from their solved placement, typed
  local component occurrences, qualified and legacy external placement frames, edge flanges
  that meet at a corner, localized hole scopes, and through-all hole extents with drill tips;
  shares one definition for each merged external design; and reads fillet face groups, sketches
  placed on entities, parameters with detached owners, and pipe scopes. Inventor projects
  attached plates with their bends, sheet-metal corners that change nothing, mirrors about a
  work plane, single moves and rotations of bodies (which Dutchman reports as not mapped yet),
  and more revolves. NX reads change streams against their embedded schema. FreeCAD applies
  part and link placements once and counts a shell as closed when each edge is used as often
  each way. STEP output recovers edge curves a model leaves out, keeps the faces it can bound
  as surfaces when a body cannot be written whole, writes solids as advanced B-rep
  representations, and leaves a collapsed edge without a curve out of its loop. Fusion also
  places local occurrences at the root of a design, and external occurrences, at the pose the
  design shows them in, from the records that store that pose, instead of where they were
  inserted; when those records disagree, the ones that list the most owners decide. Inventor
  rectangular feature patterns with fixed spacing, in one or two directions, convert as
  SolidCraft rectangular patterns; Combine is read as a Boolean of bodies, which Dutchman
  reports as not mapped yet. SolidWorks parts that fall back to their display meshes keep them,
  and report that the meshes carry no exact geometry.

### Known issue

- A very large Fusion assembly can need more than 24 GB of memory to convert
  ([#227](https://github.com/dutchman-cad/dutchman/issues/227)); on a machine with less, the
  system stops the conversion before it writes anything.

## 0.2.2 (2026-10-26)

### Known issue

- Some SolidWorks parts that 0.1.5 read still decode empty, fail cadmpeg's edge checks, or come
  through with fewer bodies (reported as losses, not as a clean conversion); a fix is in
  progress in cadmpeg. If 0.1.5 worked better for a part, keep using it for that part for now.

### History

- Merged designs that reuse a parameter name keep their own references: an expression follows
  the parameters it depends on to their new names instead of naming another design's
  parameter of the same name. Such renames are reported as information, and only names
  SolidCraft cannot use are reported as changed.
- The SolidCraft replay check allows for the error in SolidCraft's measuring mesh, which grows
  with each body's area and size, and also compares body areas. Curved parts such as lofts and
  revolves that rebuild exactly are now verified instead of flattened.

### Assemblies

- A design's own component that holds no bodies and no other components is placed as an empty
  component, when every body of the design belongs to a component, instead of the whole design
  converting as stored geometry.
- A joint between two parts inside one copy of a component placed more than once is converted
  once, as that component's own joint, instead of being left out. A joint from such a part to
  something outside its copy holds the whole copy, with a note.
- A joint to a part inside a design inserted into a component, which Fusion names by the
  part's key alone, joins that part instead of the component that holds it.
- A component that is empty in the source is written as an empty component, so the joints to
  it are kept.

### Desktop app

- On macOS the app is shown as **Dutchman** in Launchpad, the Dock, Finder and the menu bar, so
  Launchpad no longer cuts its name short. Its file stays `Dutchman Convert.app`.

### Command line and install

- On macOS, `brew install --cask dutchman-cad/tap/dutchman-convert` installs the Dutchman
  Convert app in Applications.
- Homebrew installs pour a ready-made bottle on macOS and Linux, so they no longer need Xcode
  or current Command Line Tools.
- Dutchman Web can link converted designs directly to the SolidCraft web app with
  `--solidcraft` (`DUTCHMAN_WEB_SOLIDCRAFT`), using a same-origin path. Cards show
  **Open in SolidCraft**, and API outputs include `open`; FreeCAD macros keep only their download links.
- `dutchman-web --output-dir <dir>` also saves each converted design in a folder, such as a
  mounted network share, keeping its uploaded folders and name and numbering it rather than
  replace a file already there. Each result says where its design was saved.
- `dutchman-web --base-path /dutchman` serves every page and API route under that path, so it
  can share one address with other services behind a reverse proxy.

### Docs

- Add a Compose example for SolidCraft, Dutchman Web, and a watched NAS inbox on one origin.
- The release and nightly workflows download only Dutchman's own archives, so the container
  image's build record no longer stops the draft release.

## 0.2.1 (2026-10-25)

### Known issue

- Some SolidWorks parts that 0.1.5 read still decode empty, fail cadmpeg's edge checks, or come
  through with fewer bodies (reported as losses, not as a clean conversion); a fix is in
  progress in cadmpeg. If 0.1.5 worked better for a part, keep using it for that part for now.

### Formats

- Show cadmpeg's published headline level beside reader status in the CLI, MCP formats
  report, desktop format list, README and guide.
- A STEP file of one part goes into the SolidCraft design as it is, instead of being read by
  cadmpeg and written again as STEP: nothing is lost on the way, and the report no longer lists
  the writer's STEP losses. A STEP assembly of several parts still converts part by part into
  components.

### History

- A fillet or chamfer on edges a sheet-metal base flange generated maps the same way as one on
  an extrusion's generated edges: a sketch point's side edge runs the flange's thickness on the
  side it is drawn, and each edge is proven on SolidCraft's rebuilt body before the blend is
  written.
- A fillet or chamfer left unmapped for its edges says which case it is: a selection cadmpeg
  does not resolve to edges yet, edges only partly resolved in its input topology, or edges of
  the final body.
- A fillet or chamfer on edges an extrusion generated maps when cadmpeg names them by the
  sketch entity that made them, as it does for Inventor: a sketch point's side edge, or a sketch
  line's edge where the extrusion starts or ends. Each edge is proven on SolidCraft's rebuilt
  body before the blend is written.
- A counterbore, countersink or chamfered hole with a drill point at its bottom says why it is
  not mapped: SolidCraft holes take a drill point only on a plain drilled hole, and ignore it
  on one with an entry recess, which would leave the cone out of the geometry.
- An extrude, revolve or other sketch feature whose profile cadmpeg gives as whole sketch
  regions no longer makes a design SolidCraft refuses as damaged ("invalid analytic offset
  topology or boundary reference"). The one loop of a one-loop sketch is written as every
  profile. Otherwise each boundary is written in the order SolidCraft finds it:
  counter-clockwise, from its leftmost point. This affected Inventor parts read with cadmpeg's
  sketch-region profiles.

### Command line and install

- Each release is also published as a container image, `ghcr.io/dutchman-cad/dutchman`, for
  amd64 and arm64, tagged with its version and `latest` (nightly builds as `nightly`). It holds
  the same binaries as the Linux downloads; [docs/docker.md](docs/docker.md) covers pulling,
  converting a mounted folder, the MCP server, and updating. Linux downloads are built on
  Debian bookworm, so they run on systems with glibc 2.36 or newer.
- Run Dutchman in Docker: the `Dockerfile` builds an image with `dutchman` and `dutchman-mcp`
  for x86_64 and ARM64 Linux, optionally with SolidCraft to check histories
  ([docs/docker.md](docs/docker.md)).
- Convert from a browser with `dutchman-web`, a web server for a self-hosted Dutchman: drop
  design files or a project zip, read each file's report, and download the designs. Uploads are
  converted as `convert-all` converts them and deleted after a while; a password and upload
  limits are settings. The Docker image includes it, and `compose.yaml` runs it on port 8080
  ([docs/web.md](docs/web.md)).
- `dutchman watch <inbox> --out-dir <dir>` converts files dropped into a folder, for a
  self-hosted Dutchman on a shared or network folder. It checks the folder on an interval,
  waits until a file stops changing, converts it into the output folder with its report beside
  it, and never converts the same content twice (also after a restart). A file that fails
  gets a `.failed.json` report and is tried again only once it changes. The inbox is never
  written to. `--once` converts what is ready and exits, for cron and scripts.
- Dutchman Web shares zip limits across each upload, limits uploads being received, and
  refuses new jobs when saved data or available disk space reaches its configured limit.
  Data folders are checked for private access on Unix, and browser uploads check their origin.
  The web guide explains HTTPS setup for HTTP basic authentication.
- Linux release and nightly archives include `dutchman-web` alongside the command, MCP server
  and desktop app.

### Reports

- The geometry-only retry leaves out parameters as well as history and sketches before it checks
  the model, so a design whose parameters repeat a name (as merged Fusion multi-document
  archives can) converts as geometry; the report says parameters were left out.

### cadmpeg

- The cadmpeg pin moves from `66550bb` to `fb1cd0a`. Fusion keeps nested local component
  placements, more resolved face selections, and compact coil handedness; reads designs whose
  exact edge curves end slightly away from their vertices (as in some large multi-part
  assemblies) instead of refusing them; gives each design merged into a multi-document
  archive its own parameter scope; and assigns sketches to their components. Inventor reads
  more extrusion and hole extents, full revolves about a work axis, center-point holes,
  spotfaces and hole end targets, oriented chamfers, circular sweeps, unguided two-section
  lofts, closed sketch splines, stored-body base features, and bend-free sheet-metal plates as
  base flanges, and resolves more extrusion profiles and fillet and chamfer edge selections.
  SolidWorks corrects compact-curve edge orientation. Creo preserves plane axes and boundary
  orientation, avoids ambiguous operation states, reads circle radii from type-4 dimensions,
  and binds two-cap circular sweeps. FreeCAD reads point symmetry and improves mesh placement,
  spline domains, joint angles, Body visibility, and document restoration. STEP keeps
  unclaimed bodies, corrects replica transformations, and writes reflected conic pcurves as
  exact rational splines.

### Docs

- Each pull request adds a changelog fragment; release preparation assembles the fragments
  into one ordered entry, and CI checks fragment headings and repeated release headings.
- Tagged builds now create a draft release with short highlights, downloads, and a link to the
  full changelog.

## 0.2.0 (2026-10-25)

### Known issue

- Some SolidWorks parts that 0.1.5 read now decode empty, fail cadmpeg's edge checks, or come
  through with fewer bodies (reported as losses, not as a clean conversion); a fix is in
  progress in cadmpeg. If 0.1.5 worked better for a part, keep using it for that part for now.

### Formats

- Convert CATIA `.CATPart`, Creo View `.pvz`, Rhino `.3dm`, ACIS `.sat`/`.sab`/`.smt`/`.smb`,
  and STEP `.step`/`.stp` through the geometry path. Creo and NX parts (`.prt`, including
  numbered Creo saves) are recognised by contents and report when geometry reading is not
  supported yet. SolidWorks assemblies remain recognised without a reader.
- IGES `.igs`/`.iges` files are recognised and refused before decoding, with directions to open
  them in SolidCraft directly (File > Import). SolidCraft's own importer sews their faces into
  solids; cadmpeg does not yet. IGES refusals do not invite test files.
- The CLI, JSON reports, and desktop app show whether each format reads geometry, opens without
  geometry, is recognised without a reader, or should be opened in SolidCraft directly.
  `dutchman formats` lists that status alongside what Dutchman converts. After a refused
  conversion (except IGES) or a history that did not fully map, one invitation links to
  test-file recipes and private help with wrong conversions. `--quiet` hides it; JSON output
  contains no invitation.

### History

- Constant-radius fillets and equal-setback chamfers map from proven feature input edges on any
  prior history: their input edges are matched on the body SolidCraft rebuilds from the history
  before them (one replay per design), and are refused, saying why, without `solidcraft-cli`.
- A Fusion constant-radius fillet with a tangency weight of exactly 1 maps as a circular
  fillet, its blend being circular in section, with an Info note ("tangency weight 1.0 treated
  as a circular fillet"). Other weights are still refused.
- Sheet-metal edge flanges map supported dimensions on unchanged rectangular base sheets using
  proven input-edge midpoint picks. Other edge flanges say why they are not mapped when their
  edge is known only after bending. Hems say why their developed length or rolled, teardrop, or
  unresolved form has no SolidCraft equivalent.
- Sweeps accept single-curve whole-sketch paths and guide rails, and report specific reasons
  for unsupported corner transitions, section laws, tangent extension, and simplification.
- Lofts map in more forms: surface lofts become SolidCraft surface lofts, and ruled lofts
  through two sections map. Loft rails share the sweep path reader, so a single-curve
  whole-sketch rail maps and each unsupported rail kind gives its own reason.
- Hole history keeps chamfered entries and ISO metric thread labels on tapped holes (thread
  details SolidCraft has no field for are reported, not blocking), and names the unsupported
  hole forms, placements, and ends it refuses.
- Revolves keep more of their history: unequal sides that add up to a full turn, surface
  revolves short of a full turn, and an axis drawn in the profile's sketch (kept as that line).
  Ends at a face, vertex, or body and other face makers stop the history with a reason that
  names them.
- Patterns map named bodies, planar sketch paths, regular explicit offsets and angles, and two
  linear composite stages; unsupported pattern options receive specific refusals.
- Sketch history carries bounded conics, rational quadratic Béziers, equal-weight splines, and
  more geometric relations and dimensions; unsupported constraints retain counted, specific
  loss messages.
- Body mirror patterns and body booleans (join, cut, or intersect, optionally keeping the
  tools) map to SolidCraft. Shells and drafts, shape-replacing mirrors, and threads without a
  resolved standard and pitch say why they are not mapped.
- Flattened designs keep mapped sketches with resolved planes, hidden for reference after the
  imported geometry, and report how many were kept or left out.

### Assemblies

- A joint Dutchman does not convert is reported by the output that leaves it out. A FreeCAD
  macro (`--to freecad`, experimental) builds cylindrical, ball, distance, and other joints
  SolidCraft output leaves out, and no longer reports them as dropped.
- An assembly keeps the sketches of its own file, hidden for reference in the design itself,
  with the parameters their dimensions use; they were left out before without a note.
- A joint whose connector frames point the same way (its CADIR operands' `frame_sense:
  "aligned"`) is written as a flipped SolidCraft joint, and a part a joint places meets its
  other part z to z rather than face to face. A Fusion robot arm's gripper mounted on its wrist
  this way had been turned over into the wrist.
- When the source grounds none of the parts a set of joints ties together, Dutchman grounds the
  fixed side of that set's first joint, so SolidCraft's solver holds the set where the source
  places it, and the report names the occurrence as inferred (`Info`, `joints`).
- A Fusion design's root component is grounded where it is: the document's own, and the root of
  each design inserted into it, held by the occurrence that inserts it. Both were left free to
  move.
- Experimental FreeCAD macros rebuild assembly joints in the Assembly workbench (the fixed,
  revolute, and slider joints SolidCraft gets, through sub-assemblies, plus cylindrical, ball,
  distance, parallel, perpendicular, and angle joints between directly placed parts), with
  connector frames, limits, and grounding; if solving fails, the parts keep their stored
  placements.

### Desktop app

- When macOS blocks SolidCraft's first run, the CLI and desktop app explain how to allow it in
  Finder or System Settings and try the check again, without changing quarantine.
- The desktop app is named **Dutchman Convert**, part of Dutchman CAD: its window title and
  heading, and on macOS **Dutchman Convert.app** with a "Dutchman Convert" disk image volume.
  The commands (`dutchman`, `dutchman-gui`, `dutchman-mcp`), their subcommands, the settings
  and the download file names are unchanged. A Mac with 0.1.5 or earlier keeps
  `/Applications/Dutchman.app` until it is replaced.
- The macOS disk image opens on an installation window: Dutchman and Applications side by side
  with an arrow between them on Dutchman's own background, and the read-me below. If the window
  cannot be laid out while packaging, the image is built plain as before.
- A file that fails says what to do next: **Report this on GitHub** opens the **A file didn't
  convert** form filled in with Dutchman's version, the system, the error with its stage and
  upstream, and the file's anonymous report (copied to paste when it is too long for the link).
  The form had come back with its report and stage boxes empty.
- The line naming the SolidCraft that checks history stays on one line (the whole of it on
  hover): a long path to `solidcraft-cli` wrapped onto several lines and pushed the Convert
  button out of view at the window's default size.
- The desktop app prints `--version` and `--help` without opening a window, shows its version
  in the title, and has a **Report a problem** link to the **A file didn't convert** form. An
  unreadable-file error links to that form too.

### Command line and install

- Install with Homebrew (`brew install dutchman-cad/tap/dutchman`) or Scoop (`scoop install
  dutchman-cad/dutchman` after `scoop bucket add dutchman-cad
  https://github.com/dutchman-cad/scoop-bucket`); both follow each release.
- Convert with `dutchman part.f3d`, optionally followed by an output path. `convert` also
  accepts no output: it derives the path beside the input from `--to`. Existing derived outputs
  are kept (exit 2) unless `--force`; explicit outputs still replace by default. `--force`
  conflicts with `--no-clobber`, and a default cannot replace the input.
- Run installed Dutchman CAD tools with `dutchman <tool>`: executables named `dutchman-<tool>`
  next to the command take precedence over those on `PATH`. Help lists installed tools and
  versions. An installed Pilot takes precedence over the built-in Pilot and accepts its own
  flags. The std-only `dutchman-launcher` crate supplies a tiny `dutchman` front command for
  Pilot-only installations.
- `dutchman pilot` runs [Pilot](https://github.com/dutchman-cad/pilot) 0.1.0-beta.1: `dutchman
  pilot --help` lists its commands, and `dutchman pilot how "round all edges 3 mm"` shows
  example calls.
- Directory conversion keeps the newest numbered `.prt.N` save per part and reports older
  revisions as skipped. Numbered saves use the part's base output name and keep existing
  outputs.

### FreeCAD output (experimental)

- `dutchman convert --to freecad` (and `convert-all`, the MCP tools, and the desktop app's
  output choice) writes a FreeCAD rebuild macro (`.FCMacro`) instead of a SolidCraft design: it
  rebuilds the mapped history through FreeCAD's public scripting API and keeps the original
  geometry as embedded STEP, which it restores when the rebuild fails or differs. By default,
  Dutchman does not run FreeCAD, so these histories are unverified. Geometry whose STEP uses
  replicated trimming curves (`CURVE_REPLICA`), which crash FreeCAD 1.1's reader, is refused.
- Experimental FreeCAD macros keep a mapped history prefix and hidden reference sketches when a
  later feature cannot be rebuilt, retaining the final STEP shape.
- Write experimental native FreeCAD documents with `--to fcstd`, or check macros with `--to
  freecad --verify`, using bounded headless execution and per-part volume and bounding-box
  verification.

### cadmpeg

- The cadmpeg pin moves to `66550bb`. Fusion keeps parameters whose frames cannot be read as
  native-only instead of failing, reads more parameter expressions and every component context
  binding, places components bound through unregistered links, and reports ambiguous face
  appearances instead of failing. As-built joints carry slider limits, their limit references,
  and rest positions; rigid as-built joints and rigid groups of any size read, with each group
  member held to the first. Fillets with unit-bearing tangency weights stay native, extrude
  profiles resolve from circle-bounded and cap faces, rectangular face patterns keep both
  directions, feature patterns name the feature they repeat, and base flanges read thickness
  from their sheet-metal rule. Failed manifests say where framing stopped. Inventor 2019 and
  later parts carry history, newer assemblies place parts, and more sketches close through
  tangent-relation hints on either operand, other solver hints, and equal-length relations;
  thread annotations become cosmetic threads, and Thicken joins through selection lists.
  SolidWorks keeps native CAD state, as do Fusion, Creo, and NX. Creo 11–13 order tables read,
  and Creo View geometry becomes available. FreeCAD compounds of closed solids read as solids,
  fillets and chamfers keep edge ends on vertices, and document restoration improves. STEP/ACIS
  gains face boundary curves on their edges' ranges, more reversed ACIS curve families, fixes
  to ASM intcurve pcurves, and evaluable procedural faces (including a coil's swept face)
  written as sampled B-spline surfaces. Across formats, feature input edges carry their
  historical geometry, edges bounding selected faces keep their geometry, and joints carry
  connector frame sense on their operands.
- CADIR versions 8, 9, and 10 are read.

### Docs

- The README and guide link the Dutchman CAD Discord for questions and help.
- The issue forms' privacy box says that links to files that are already public are welcome; it
  asked for no file names at all, which kept a reporter from linking the public file that
  failed.
- A second issue form, **A file didn't convert**, reports one file: its format, the versions,
  the operating system, what was expected and what happened, and that file's anonymous report.
  Both forms list FreeCAD documents (`.FCStd`), and the conversion skill does too.

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
