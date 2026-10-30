# Choosing what to keep

`convert`, `convert-all`, and `watch` accept these presets and narrowing switches:

| Preset | SolidCraft output |
|---|---|
| `fast` | Geometry at the stored placements; no history, reference sketches, or joints. No memory refusal. |
| `standard` (default) | History, sketches, and joints. History is left out when its estimate exceeds free memory and the history-free estimate fits. |
| `full` | History, sketches, and joints whenever they map; history is never left out to fit memory. |

`--no-history` (also `--import-only`), `--no-sketches`, and `--no-joints` only remove
choices from the preset. `--no-sketches` removes reference sketches, including those
kept beside an imported shape or after a blocked history; sketches needed to replay
retained history stay. `--no-joints` omits assembly joints and restores the source's
stored placements. An Info line under `joints` records that choice.
FreeCAD output (`--to freecad` or `--to fcstd`) honours the history choice only;
the sketch and joint choices do not control that writer.

```sh
dutchman convert assembly.f3z assembly.sc --preset fast
dutchman convert part.ipt part.sc --preset full --no-sketches
dutchman convert-all project/ --out-dir converted/ --preset standard --yes
```

Text conversions show `Estimate: about N GB (±R %), about M min (±T %)` before converting,
unless `--quiet` is set. Estimates below 0.1 GB read `under 0.1 GB`, and times below
60 seconds read `under a minute`; longer times round up to whole minutes. Where measurements
don't yet support a time band for the format, the time reads `roughly M min`.
Estimates use file size and format. For `.f3d`, `.f3z`, and
`.FCStd`, a bounded ZIP pre-read counts members and uses their total uncompressed size; for
Fusion archives it also reads each design's own directory, one design at a time, and never
decodes a design. Memory estimates add a fixed process cost to a content-dependent cost;
keeping history scales the content cost, not the fixed overhead. Tiny Inventor and SolidWorks
parts start at about 18 MB with every preset. The Fusion content term is about 52 bytes per
byte (57 with history). FreeCAD uses about 29 MB plus 150 bytes per byte of archive content,
with a 1.04 history multiplier on that content term. The small-file fit uses synthetic parts
and CC0 FreeCAD recipes from 0.21.2, 1.0.2 and 1.1.4, measured on Windows and macOS; the Mac
measurements used an older nightly decoder. Larger Fusion assemblies retain their earlier
calibration. Memory and time use separate history factors. Times are rounded up to whole seconds,
so subsecond conversions are especially approximate. They leave out the history check in
SolidCraft, which can add several minutes on a large assembly.
Separate assembly part files are not pre-read, so their cost can be underestimated.
An estimate is unknown for formats this build cannot convert, or when metadata cannot
be read, including ZIP directories exceeding 10,000 members or 16 MiB, and unreadable
archives.

RAM bands use every successful Windows and Mac measurement, re-evaluated with the
current coefficients: Fusion `.f3d` ±55%, multipart `.f3z` ±25%, Inventor parts ±30%,
SolidWorks parts ±35%, and FreeCAD ±50%. One percentage point of safety is added before
rounding up to a 5% step. These are empirical envelopes of this corpus, not guarantees for
unseen designs. Unmeasured formats retain their ±100% memory heuristic.
Time has its own band: multipart Fusion is ±20%; other time bands are uncalibrated.
Timing uses fractional `seconds_exact`; `seconds` remains its whole-second ceiling for
compatibility. Fixed startup is separate from content work: Windows has different fast,
history and verification startup terms; macOS has its own process floor and allows the
ten-second version-probe timeout for history without an explicit target version. The estimator
never runs SolidCraft to measure a probe. History may probe its version even with
`--no-verify`. Old Mac probe durations were not recorded, Windows startup varies between runs, and
FreeCAD assembly worker startup is poorly predicted by archive bytes, so these four formats keep uncalibrated time bands. Verification
replay and explicit-version/no-memory-check variants also retain rough timing; their
startup or replay costs are not generally calibrated. Bands are not widened to hide this.
JSON adds `memory_band` and nullable `time_band`. The existing `band` stays as the legacy
shared heuristic for compatibility; use the new fields for separate calibrated coverage.
For multipart Fusion `band` is the larger of the two calibrated bands (25%).

For Fusion archives with multiple readable embedded `.f3d` designs, the time estimate
uses the requested threads (0 means auto), limited by the converter's shared CPU, item
count and free-memory worker policy. A serial component remains. Before native decoding,
the embedded design count and the converter's 256 MiB minimum worker budget are proxies;
actual body counts can reduce conversion workers further. Single-design files keep their
existing time model. The worker adjustment changes time estimates only.

Available memory comes from Linux `MemAvailable` bounded by remaining cgroup v2
headroom (including ancestor limits), or safe system APIs through sysinfo on macOS and
Windows. Memory probes do not launch subprocesses. Unknown memory or an unknown estimate
does not block a run. Estimates use decimal GB and are approximate, not
reservations; other processes and simultaneous conversions can change available memory.

The memory check runs before decoding, using only the pre-read estimate. Standard
leaves history out for any design whose history estimate exceeds free memory when
its history-free estimate fits. That decision stays fixed through conversion; there
is no second refusal after decode. Full refuses before decode when its estimate
exceeds free memory. When Standard removes history, one Info line under `history` says:

```text
feature history left out: this conversion needs about N GB with history and M GB are free; use the full preset to keep it
```

If the selected choices exceed free memory, an interactive terminal offers the next
lighter preset and asks `y/N`; only `y` or `yes` switches. Without a terminal, with
`--yes`, or with `--json`, the conversion refuses with exit 2 and this message:

```text
this conversion needs about N GB and M GB are free; try --preset <lighter> (or --no-memory-check to run anyway)
```

`--yes` means no interactive preset switch. It does not bypass the memory guard.
`--no-memory-check` bypasses both the refusal and Standard's automatic history budget.
Existing decoder input limits and batch worker memory limits still apply. Batch
conversion checks each input; changing presets or narrowing switches also prevents a
journal from reusing a result made with different choices.

## Presets and estimates

The presets, narrowing switches, and memory guard are described above; `dutchman convert --help` lists them too.
