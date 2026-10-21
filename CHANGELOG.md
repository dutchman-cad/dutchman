# Changelog

All notable changes to Dutchman are listed here. Versions follow [Semantic Versioning]; until
1.0, a minor version may change the command line, the Rust API or the JSON reports (a JSON
report's schema name changes when a field does).

[Semantic Versioning]: https://semver.org/spec/v2.0.0.html

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
