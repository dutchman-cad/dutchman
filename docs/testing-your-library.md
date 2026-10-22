# Testing Dutchman on your library

This guide is for anyone willing to run Dutchman over their own CAD library and tell us what
broke, without giving away a single file. You convert everything on your own machine, and
Dutchman writes an **anonymous report**: what converted and what did not, by cause, with no
file names, paths, part names or geometry in it. You read it, then share it or the parts of
it you choose.

Fusion (`.f3d`, `.f3z`) libraries are the main target. Inventor (`.ipt`, `.iam`) and
SolidWorks (`.sldprt`) files are welcome too, and FreeCAD (`.FCStd`) ones with a nightly build,
including ones you cannot open any more.

## Everything stays on your machine

- Dutchman makes **no network connections**. It reads your files, writes the designs and
  reports where you tell it to, and starts only two kinds of program: itself (one copy per
  file, so one bad file cannot stop the run) and, if you have it, SolidCraft's command-line
  tool to check the results. On macOS and Windows it also runs `ps` or `tasklist` to measure
  how much memory each file takes.
- Your files are never changed. The converted designs go to a folder you name.
- Nothing is sent anywhere unless you send it yourself.

## 1. Install

1. Install Dutchman 0.1.3 or newer, as [the guide](guide.md#install) shows: on a Mac, the
   disk image (`Dutchman.app`, with the `dutchman` command inside it, which the guide shows
   how to put on your `PATH`); elsewhere, the release archive. Check that `dutchman --version`
   prints the version.
2. Optional, recommended: install SolidCraft 0.18.2 or newer (Dutchman is tested with 0.18.2
   and 0.19.0). With it, Dutchman checks every converted design
   in SolidCraft: whether a feature history replays to the same shape, and whether the
   geometry shows whole, with a volume. Dutchman finds the command-line tool inside an
   installed SolidCraft app (`SolidCraft.app` on macOS, under Program Files on Windows). If
   you have only the tool, `solidcraft-cli` from a
   [SolidCraft release](https://github.com/tangency-labs/solidcraft/releases), put it on your
   `PATH` or tell Dutchman where it is:

   ```sh
   export SOLIDCRAFT_CLI=/path/to/solidcraft-cli          # macOS, Linux
   ```

   ```powershell
   $env:SOLIDCRAFT_CLI = "C:\path\to\solidcraft-cli.exe"  # Windows
   ```

### Stable or nightly

Use the latest stable release. When an issue you filed says the fix is "merged, not released
yet", or a fix depends on a cadmpeg change not released yet, you can try it at once with the
[`nightly`](https://github.com/dutchman-cad/dutchman/releases/tag/nightly) pre-release:
Dutchman's and cadmpeg's latest code, rebuilt each day once every test passes. Run the same
batch with it (a new `--journal`, since a different version converts everything again), and
say in the issue which you used: its notes give the commits.

## 2. Run the batch

Pick a folder for the results **outside** your library, then run (one line on Windows,
without the `\`):

```sh
dutchman convert-all ~/CAD \
  --out-dir ~/dutchman-test/designs \
  --journal ~/dutchman-test/journal.jsonl \
  --report-anon ~/dutchman-test/report-anon.json \
  --report-key ~/dutchman-test/report-key.json
```

- `~/CAD` is your library; give several files or folders if you like. Subfolders are searched;
  files Dutchman does not read are skipped and counted.
- A thousand files take from minutes to a few hours, depending on their size and whether
  SolidCraft checks them. `--jobs N` sets how many convert at once (default: your cores, at
  most 8); use fewer if the machine is busy.
- A file that takes over 15 minutes or 8 GB of memory is stopped and counted as failed; change
  that with `--timeout SECONDS` and `--max-memory MB`.
- **Stopped part way?** Run the same command again. The journal remembers every file already
  done, and only the rest are converted.
- Inventor assemblies find their part files as Inventor does: at the paths they store, else
  in the project the nearest Inventor project file (`.ipj`) above them describes, with its
  workspace, library and imported-components folders. A shared library kept elsewhere is
  searched too with `--search DIR` (give it more than once for several). See
  [Finding the part files](inventor.md#finding-the-part-files).
- As it runs, Dutchman prints each file's result, with what it lost grouped by component for
  assemblies; an assembly with parts left out is counted as converted but **incomplete**.
  Every loss of every file, in full, goes to `dutchman.log` in the `--out-dir` folder.
- When it ends, Dutchman prints the report's salt. Keep it to yourself (it is also in the key
  file). To make the next run's ids match this one's, add `--report-salt` with that salt.

## 3. Read the report

`report-anon.json` is plain JSON; any text editor shows it. The parts that matter most:

- `totals` and `formats`: how many files converted, how (editable history, checked or not, or
  geometry only), how many converted incomplete (assemblies with parts left out), how many
  failed.
- `stages`: where the trouble was. `decode` and `project` are cadmpeg reading the file, `map`
  and `write` are Dutchman, `solidcraft` is SolidCraft checking the result, `input` is the file
  itself (missing parts beside an assembly, say).
- `errors`: every distinct reason a file failed, most files first.
- `losses`, `blocked_features` and `verification`: what converted files lost, which features
  kept a history from being written, and why histories were not checked.
- `timing` and `slowest`: how long files took.

Each cause lists up to 20 file ids (`f3a9c0e1d2b4f5a6`). Look one up in `report-key.json` to
see which of your files it is. [The report's fields](report-anon.md) are all described.

## 4. What is safe to share

| File | Share? |
|---|---|
| `report-anon.json` | **Yes**, after reading it. It holds no names, paths or geometry. If you find a name in it, leave that part out and tell us: it is a bug. |
| Parts of it (one cause, a few ids) | Yes. |
| `report-key.json`, the salt | **No.** They turn ids back into your file paths. |
| The journal, `dutchman.log`, `--json` output, the printed results | No. They hold your file paths and names. |
| The converted designs, your files | No, unless you choose to (below). |

## 5. Report what you found

File every report on **Dutchman's issues**,
<https://github.com/dutchman-cad/dutchman/issues>, with the **Library test report** form.
Even when the report says `upstream: cadmpeg` or `upstream: solidcraft`, file it on Dutchman,
not there: we work out where the fix belongs and take it upstream ourselves, then tell you in
the issue when a Dutchman release fixes it.

- **One issue per distinct cause**: one entry of `errors`, `losses` or `blocked_features`.
  Ten files failing for one reason are one issue.
- In each: the format and the version that made the files (Fusion's, Inventor's or
  SolidWorks' year) if you know it, the cause as the report gives it (message or code, stage,
  upstream), the counts, a few file ids, and the `versions` block.
- Attach the whole `report-anon.json` to one issue (or the first) if you are happy to.
- **Never attach a design file to an issue.** If you are willing to let a maintainer see one
  file privately, say so in the issue; we will reply with a private way to send it. That is
  entirely your choice. Without files, a simple test part that shows the same problem, made
  for the report, is just as useful.

## Let your own Claude do the legwork

If you use Claude (Claude Code, or Claude with the `dutchman-mcp` server and the
`dutchman-converting` skill from this repository), paste this prompt. It runs the batch,
groups what went wrong and drafts the issues **for you to review**; it does not post anything.

```text
I am testing Dutchman (https://github.com/dutchman-cad/dutchman) on my CAD library. My files
must never leave this machine: do not upload, paste or attach any design file or its contents
anywhere, and do not post anything to GitHub or elsewhere yourself.

1. Run Dutchman's batch over my library at <LIBRARY FOLDER>, writing to <RESULTS FOLDER>
   (outside the library), with a journal, an anonymous report and a key:
   dutchman convert-all <LIBRARY FOLDER> --out-dir <RESULTS FOLDER>/designs
     --journal <RESULTS FOLDER>/journal.jsonl
     --report-anon <RESULTS FOLDER>/report-anon.json
     --report-key <RESULTS FOLDER>/report-key.json
   (or the dutchman-mcp convert_all tool with the same options). If it stops, run it again;
   the journal carries on. Do not show me the salt or the key's contents in your summary.
2. Read report-anon.json only (not the key, not the journal, not dutchman.log). Summarise totals, formats,
   stages and the slowest files in a few lines.
3. Group the causes in errors, losses (Degraded and Dropped only) and blocked_features into
   distinct issues, most files first. Merge entries that are clearly one cause.
4. For each, draft a GitHub issue for dutchman-cad/dutchman in the "Library test report"
   template's shape: title, format, stage, upstream, the cause as the report gives it, counts,
   up to five file ids, and the versions block. Use only text from report-anon.json. Check
   each draft for anything that looks like a file name, path, part or company name, and
   remove it.
5. Save the drafts as Markdown files in <RESULTS FOLDER>/issues/ and list them for me. I will
   read them and post the ones I choose.
```

## Questions

Ask on Dutchman's issues; anything you would rather not ask in public, say so there and we
will find a private way.
