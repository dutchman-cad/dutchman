### Command line and install

- New `dutchman diagnose FILE`: for a large design that takes hours, runs out of memory, or
  fails in one part, it converts the design part by part on your machine, each part under a
  time and memory limit (`--time-limit`, `--memory-limit`), and writes a ranked report that
  names parts by number only (`diagnose/report.json`, `report.txt`; `local-key.json` maps the
  numbers back to your part names and stays private). A part that hangs, crashes or runs out of
  memory costs only that part, and the report is written as it goes.
- `dutchman convert FILE --part N` (with `--with-children` for a sub-assembly) converts only
  part N, as diagnose numbers it, so a failure that took hours to reach shows in seconds. A
  number the design does not have is refused with the range it does have.
- A conversion that cannot fit in the machine's free memory now also stops once the design is
  read, instead of running for hours, naming the largest parts by number; `--no-memory-check`
  still converts anyway.
- `dutchman repro` goes straight to the failing part of a diagnose report (found in `--out`, or
  given with `--from-diagnose`), and `next-steps.md` names the part number and the kind of
  feature to rebuild.
