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
history and verification startup terms; macOS has its own process floor. A possible
version-probe timeout is not included as normal conversion work. The estimator
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
existing time model. Automatic workers are capped at two; detailed parts and retained
preparations can reduce that further after reading.

`threads.used` is the largest bounded Dutchman part-worker limit selected across conversion
stages and embedded members. It excludes threads inside SolidCraft child processes and does
not measure simultaneously busy CPU cores. Failed worker tasks can retry serially.

Available memory comes from Linux `MemAvailable` bounded by remaining cgroup v2
headroom (including ancestor limits), or safe system APIs through sysinfo on macOS and
Windows. Memory probes do not launch subprocesses. Unknown memory or an unknown estimate
does not block a run. Estimates use decimal GB and are approximate, not
reservations; other processes and simultaneous conversions can change available memory.

The memory check runs before decoding, using only the pre-read estimate. Standard
leaves history out for any design whose history estimate exceeds free memory when
its history-free estimate fits. That retention decision stays fixed through conversion.
After reading, a conversion can also stop when the resident design leaves too little
memory for mapping and writing. Full refuses before decode when its estimate
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
`--no-memory-check` bypasses the refusal, Standard's automatic history budget, and
history-check skipping.
Existing decoder input limits and batch worker memory limits still apply. Batch
conversion checks each input; changing presets or narrowing switches also prevents a
journal from reusing a result made with different choices.

## Presets and estimates

The presets, narrowing switches, and memory guard are described above; `dutchman convert --help` lists them too.

History checking stays enabled by default, independently of the retention preset. Its
second build needs memory on top of conversion: measured peaks for a 256-placement
assembly were 8.6 GB in 22 seconds with checking off, 3.5 GB in 25 seconds with one worker
and checking off, and over 17 GB with checking on before the run was stopped. The check
needed about 9 GB extra. These measurements justify limiting automatic workers to two
and budgeting dense single-body parts from their entity counts, with a separate allowance
for all retained preparations. Small imported parts use serial SolidCraft preparation: a synthetic 256-cube
assembly took about 2.9 seconds with either one or sixteen workers, while sixteen used
about 14 MiB more peak memory. Explicit thread requests can still prepare those parts
in parallel within the memory budget.

The peak estimate includes the requested check. Conversion preflight and Standard's
history budget use conversion-only memory so an expensive check does not discard history.
Once read, the check allowance is three times the resident set plus the 15% mapping margin.
Free memory is checked again before SolidCraft rebuilds geometry. If the allowance will
not fit, that design skips checking, keeps the original shape visible and retained editable
history unverified, and reports `History check skipped: needs about N GB, M GB free`.
Unknown memory measurements allow checking. This is a conservative estimate, not a memory
reservation; simultaneous work can change what is free.

`--no-verify` keeps checking off. `--no-memory-check` forces requested checking even if
memory is low. Desktop Advanced offers the same override as **Convert and check history
even if memory is low**. CLI, desktop, web conversions and MCP use the same policy.
