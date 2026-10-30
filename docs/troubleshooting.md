# Troubleshooting

For a question, or to talk a problem through first, post in the Convert help forum on the
[Dutchman CAD Discord](https://discord.gg/yTcseBEdeu).

For a failed conversion, or a design that looks wrong in SolidCraft, start with an anonymous
report (replace `part.f3d` with your file or folder):

```sh
dutchman convert-all --out-dir converted --report-anon report-anon.json part.f3d
```

In the desktop app, **Report this on GitHub** on the failed file does this for you. Review
`report-anon.json`, then paste it into the
[**A file didn't convert**](https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml)
form on Dutchman's issues (for a whole library, the **Library test report** form). Keep
ordinary logs, JSON output, the journal, the report key, and the salt private: they contain
names, paths, or a way to identify files. Do not attach design files publicly. If you want to share a
small reproducer you made yourself, offer it privately in the issue. See
[Testing Dutchman on your library](testing-your-library.md#5-report-what-you-found).

On Linux, run `dutchman-gui` from a terminal and confirm that your X11 or Wayland desktop
session has working OpenGL and graphics drivers. Include the terminal diagnostics when
reporting startup trouble. A graphics startup error prints that advice and exits with
status 1, as does a start that stalls for 30 seconds. The command-line `dutchman convert` works without
a desktop session.

Every error is one line on stderr, and the exit code is 1 (2 for a mistake on the command
line). A failed file creates no design; a batch can still write successful designs, logs,
and requested reports.

| Message | What to do |
|---|---|
| `x.step: output must be a .sc file` | Name the output `something.sc`. |
| `a.txt: Dutchman does not convert this kind of file. It converts …` | Choose one of the formats listed by `dutchman formats`. STEP exchange files convert too. |
| `Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import).` | Open the `.igs` or `.iges` file in SolidCraft using File > Import. |
| `… reading geometry is not supported yet. Export a STEP file and convert that instead.` | The format is recognised, but its reader has no geometry path yet. Export STEP from the program that made the design and convert that. |
| `cannot read this design: its contents are inventor data, not Fusion archive (.f3d); rename it to match` | The file's extension does not match its contents. Rename it with the right extension. |
| `cannot read this design: no geometry could be read from this Fusion archive (.f3d) file` | The file holds no geometry Dutchman can read yet, for example a design saved without bodies, or one whose geometry is stored in a form cadmpeg does not read yet. |
| `cannot read this design: malformed container: …` | Download or copy the file again, or check its extension. If the original opens in the program that made it, use the anonymous reporting route above. |
| `cannot convert this design: the model has no solids or surfaces SolidCraft can import` | The file was read but held nothing SolidCraft can hold as a solid or surface: no body, or no face whose bounds can be written. When only some faces of a body can be bounded, those faces convert as surfaces and the report lists the rest. |
| `missing.f3d: No such file or directory (os error 2)` | Check the path. |
| `part.f3d: the file is … bytes, over Dutchman's 1073741824-byte limit` | Dutchman opens files up to 1 GiB. |
| `history: …; unverified: …` | Not an error: the history was written without the SolidCraft check. Install `solidcraft-cli`, or point `--solidcraft` or `SOLIDCRAFT_CLI` at it, and convert again. |
| `history: flattened: replay differs (…)` | Not an error: the history did not rebuild the same shape in SolidCraft. A retained editable timeline is hidden beside the imported final shape when possible. `--no-verify` keeps the history, unchecked. |

## Diagnostic logs for unfinished runs

Enable diagnostics **before** reproducing a conversion that hangs or stops unexpectedly:

```sh
dutchman convert --diagnostics-default part.f3d converted.sc
dutchman convert-all --diagnostics-default --out-dir converted parts/
dutchman watch inbox --diagnostics-default --out-dir converted
```

Each invocation creates one JSON Lines file named by UTC start time, such as
`2026-10-29_201500.jsonl`; a counter suffix keeps simultaneous starts unique. Input names
never enter the log's filename. The default folder is:

| System | Folder |
|---|---|
| Windows | `%LOCALAPPDATA%\Dutchman\diagnostics\` |
| macOS | `~/Library/Logs/Dutchman/` |
| Linux | `$XDG_STATE_HOME/dutchman/diagnostics/`, or `~/.local/state/dutchman/diagnostics/` when unset |

The newest ten automatically placed logs are kept. Starting a run removes older logs
whose filenames match this timestamp pattern; unrelated files are left alone.
`--diagnostics /absolute/path/run.jsonl` instead appends to an explicit file, without
retention. Choose either `--diagnostics-default` or `--diagnostics PATH`.
The MCP `convert` and `convert_all` tools accept `diagnostics` (an explicit path) or
`diagnostics_default: true`.

In the desktop app's settings, select **Save a diagnostic log (no names or contents from
your file)** before starting. **Open diagnostics folder** opens the default folder.
After completion or Stop, **Show the log** opens the log's folder; its path is shown too.
On launch, an unfinished conversion in the newest default log shows **The last run didn't
finish. Its diagnostic log is here:** with **Open folder** and **Dismiss** buttons.

Default diagnostic filenames use UTC on every platform, formatted from the system clock
without launching a timestamp command. Diagnostic OS and total-memory metadata are read
only when logging is enabled. On Windows the OS version field contains `windows`.
Available memory, total memory, and heartbeat process memory and CPU time are read through
safe system APIs on Windows and macOS, without launching memory-probe commands. Heartbeat
resource fields are available on Linux, Windows, and macOS; a measurement that cannot be
read is null. Linux keeps its `/proc` readers and honours cgroup memory limits when
checking available memory.


Every record is appended and flushed immediately, with a disk sync at least every five
seconds and on every stage change. A background heartbeat every 30 seconds records the
stage, elapsed time, process memory, CPU time, and last decoder member id when available.
Fusion archives include anonymous member sizes; per-member records appear when the decoder
reports them, including re-decodes and fixed failure categories. No progress bar total is
inferred from these decode attempts. A stopped process leaves its complete lines usable;
an interrupted final line may be incomplete. A supervised batch worker that times out or
is cancelled gets an error `end` record; killing the supervising process can leave no `end`.

The schema is [`dutchman.diagnostics/1`](json.md#dutchmandiagnostics1). It holds versions,
machine resources, selected conversion choices, input and archive byte sizes, timings,
salted ids, and loss codes with counts. It contains no file or design names, paths, geometry,
file contents, or loss/error messages. Salts use the anonymous report's random source and
hashing scheme; when `convert-all` also requests `--report-anon`, both share its salt and
file ids, including `--report-salt` when supplied. Keep that salt and any report key private.

Review the JSONL file, then attach it to Dutchman's **A file didn't convert** issue or send
it with your problem report. No design file is needed. Diagnostics work while conversion
is still running; [the anonymous report](report-anon.md) adds a batch summary after it ends.
Ordinary `--log`, JSON conversion reports, and journals contain names and stay private.
