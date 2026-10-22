# Changelog

All notable changes to Dutchman are listed here. Versions follow [Semantic Versioning]; until
1.0, a minor version may change the command line, the Rust API or the JSON reports (a JSON
report's schema name changes when a field does).

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

## Unreleased

### FreeCAD

- Leaving out a FreeCAD document's hidden bodies no longer takes their sketches with them: a
  sketch's own wire body is hidden, and only the hidden bodies' own B-rep is left out now.
  Every FreeCAD history had stopped at its first sketch ("the sketch it binds is missing")
  once cadmpeg marks those bodies hidden.

### Feature history

- Circular feature patterns over a partial turn now map with copies at both ends of the span.
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
