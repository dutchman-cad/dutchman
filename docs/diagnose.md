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

## Let Dutchman do all of it: `--narrow`

```sh
dutchman diagnose --narrow big.f3z --out findings
```

This runs the whole narrowing by itself, in five steps, and needs no AI assistant:

1. **Rank the parts**, as `dutchman diagnose` does.
2. **Isolate a failing part**: alone, then with the parts either side of it. Up to five failing
   parts are tried, in rank order, until one fails again. If every part converts alone, the
   whole design is tried.
3. **Narrow its history**: the part is converted again with its history cut short, halving the
   range each time, until feature N fails the same way and feature N - 1 does not. A part with
   80 features takes about seven tries.
4. **Record the failure signature**: the kind of feature, its options, the shape of its profile
   and the error code. It holds no names, paths or real dimensions.
5. **Write the results.**

It says where it is as it goes:

```text
dutchman: step 3 of 5: narrowing part 17 (feature 41 of 80)
```

The design is read once, and one converter takes every try, so narrowing does not read a large
file again for each one. A try that runs past its limits, or crashes the converter, counts as
that failure, and a new converter reads the design for the next try.

**If it stops**, run the same command with the same `--out` folder. `narrow-state.json` is
written after every step and every try, so a crash or a reboot costs at most the try it was
on. The folder must be about the same file; another file is refused. A folder that already has a
finished `dutchman diagnose` report for this path starts at step 2.

**Limits.** Each step has its own. `--time-limit` and `--memory-limit` hold for one part in the
ranking and for each try in steps 2 and 3, so a part stopped by a limit is stopped by the same
limit when it is tried again. `--step-time-limit` is the most steps 2 and 3 may take each, in
all (default 4 hours; it counts across resumes). Describing the feature in step 4 has 10 minutes.
A step past its limit stops, and the run reports what it found: when that is step 3, the range
the feature is in.

It writes these in `findings/diagnose/`, beside the ranking:

| File | What it is |
|---|---|
| `narrow.json` | What was found ([schema `dutchman.narrow/1`](json.md#dutchmannarrow1)). **Safe to share.** |
| `signature.json` | The failure signature ([schema `dutchman.signature/1`](json.md#dutchmansignature1)). **Safe to share.** |
| `narrow-state.json` | The checkpoint: numbers, categories and a hash of your file. Nothing to share. |
| `kit/anonymous.json` | The anonymous report. **Safe to share.** |
| `kit/next-steps.md` | The steps to rebuild the failure as a small file. |
| `kit/local-key.json` | The numbers back to your part and feature names. **Do not share.** |

**When no feature is named.** A conversion writes a part's stored shape as well as its history,
and cutting the history leaves the stored shape as it is. So a part that fails with no history
at all fails in its stored shape, and the run says so instead of naming a feature. A
design that cannot be read has a signature with no feature too. Failures of a conversion that
finished, such as a history that stopped early and kept its imported shape, are not failures here.

Narrowing uses the same writing as `dutchman diagnose`, without the check in SolidCraft, so a
failure that shows only in that check does not show here.

To try one cut yourself:

```sh
dutchman convert big.f3z --part 17 --stop-at-feature 41 -o part17.sc
```

### Check your small file

Rebuild the failure as `kit/next-steps.md` says, then:

```sh
dutchman diagnose --same-failure small.f3d --out findings
```

The small file goes through the same narrowing and its signature is compared with yours. Sizes
and names do not matter; the feature's kind and options, its profile's shape and the error do.
You get one of:

| Verdict | What it says |
|---|---|
| `same` | It fails the same way: this is the one to send. |
| `converts_fine` | It converts fine. Try again, closer to what the failure needs. |
| `different` | It fails, but differently, and it names what differs. |
| `too_broad` | Same stage and error category, but the failure has no feature or check code, so that cannot prove it is the same failure. You can still make the report. |

Your original file is refused here: it is not a small rebuild.

### Make the report

```sh
dutchman diagnose --make-report small.f3d --out findings
```

This makes `findings/diagnose/dutchman-report.zip`, holding `anonymous.json`, `signature.json` and
your small file as `small.f3d`, and prints a link that opens the GitHub issue form filled in; the
link is in `issue-link.txt` too. It works only for a file that passed `--same-failure`, and never
for your original.

**Nothing is sent.** The zip stays on your machine until you send it. On GitHub, open the link,
paste `anonymous.json` and drag the zip in, if you are free to share the small file publicly.
With no GitHub account, or if the small file must stay private, send the zip to Dutchy in a direct
message on the [Dutchman Discord](https://discord.gg/yTcseBEdeu); see
[Send the file privately](troubleshooting.md#send-the-file-privately).

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

What the design holds is measured. What it would need on top is an estimate, three quarters of
that, which has not been calibrated against real peak memory yet.

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
