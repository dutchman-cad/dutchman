# Find what breaks in a large private file

A design with hundreds of parts can take hours to convert, run out of memory, or fail in one
part out of many. `dutchman diagnose` finds which parts, on your own machine, and writes a
report that names parts by number only, so you can share it without sharing the design. It
needs no AI assistant and uploads nothing.

## Step 1: diagnose

```sh
dutchman diagnose big.f3z --out findings
```

Options: `--time-limit` (the longest one part may take: seconds, or a number with `s`, `m`
or `h`; default 5 minutes), `--memory-limit GB` (the most memory the converter may hold while
it works on one part, the design it has read included; default four fifths of what is free),
`--project` and `--search` (where an assembly's part files are, as for `convert`), and
`--json`.

The design is read once, and its parts are then converted one by one, each timed and
measured, without writing any output. If a part runs past the time limit, takes the converter
past the memory limit or crashes it, that part is recorded as `timed_out`, `out_of_memory` or
`failed`, the converter is stopped and started again, and the run goes on with the next part.
A part never costs more than itself and the time to read the design again.

It writes three files in `findings/diagnose/`, as it goes (a run you stop half way leaves a
report with `status` `running` holding every part finished so far):

| File | What it is |
|---|---|
| `report.json` | The ranked report ([schema `dutchman.diagnose/1`](json.md#dutchmandiagnose1)). **Safe to share.** |
| `report.txt` | The same ranking as a plain table. **Safe to share.** |
| `local-key.json` | Part numbers to your part names. **Do not share.** |

`report.json` and `report.txt` hold part numbers, outcomes, the stage each part stopped at, the
error kind and check-finding codes, times and sizes, and nothing else: no part names, file
names, paths, or geometry.

Parts are numbered from 1 in the order of the design's structure (a sub-assembly, then the
parts inside it, then the next part). The numbers depend only on the structure, so they are the
same on every machine. They are ranked failures first (slowest first), then the parts that
converted, slowest first and biggest first among equals.

## Step 2: convert one part

```sh
dutchman convert big.f3z --part 17 -o part17.sc
dutchman convert big.f3z --part 4 --with-children -o assembly4.sc
```

Only that part is mapped and written, so the failure that took hours shows up in seconds
(the whole file is still read once). `--with-children` also converts the parts inside a
sub-assembly. A number the design does not have is refused with the range it does have.

## Step 3: make a reproduction

```sh
dutchman repro big.f3z --out findings
# or, with the report somewhere else:
dutchman repro big.f3z --out new-folder --from-diagnose findings
```

With a diagnose report to go on, `repro` goes straight to the first failing part instead of
converting the whole file, narrows that part to a body, feature or sketch, and
`kit/next-steps.md` names the part number and the feature kind to rebuild as a small file. See
[Make a minimum reproduction](minimum-reproduction.md).

## When the machine cannot finish

A conversion that needs more memory than the machine has free stops early, once the design is
read and its memory is measured, instead of running for hours. The message names the largest
parts by number:

```text
this conversion needs about 9.4 GB more memory and 6.1 GB are free (the design already takes
12.5 GB); parts 17 (about 2.9 GB), 4 (about 2.2 GB), and 9 (about 1.6 GB) are the largest. Run
`dutchman diagnose` on the file to see which parts fail, run slowly or take the most memory, or
convert one part with --part N (--no-memory-check converts anyway)
```

The exit code is 2. `--no-memory-check` converts anyway.

## With an AI assistant

Paste this into your assistant, and run the commands it asks for yourself. Give it
`report.json`, `report.txt` and `kit/anonymous.json` only, never `local-key.json` or the file.

```text
I have a Fusion file I cannot share. I ran `dutchman diagnose` and this is its report.json
(part numbers only). Tell me which part to look at first and why. Then give me the exact
`dutchman convert FILE --part N` command to confirm that part alone fails, and after that
`dutchman repro FILE --out FOLDER --from-diagnose FOLDER`. When I paste the kit's
anonymous.json, tell me in plain steps which feature to rebuild in my CAD program as a
small file with made-up dimensions, and how to check that the small file fails the same way
(same stage and same finding codes).
```
