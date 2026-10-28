### Reports

- Reads and conversions show their stage, designs read when available, and elapsed time, with a
  still-working heartbeat every five seconds. Desktop cards show a reading progress bar
  when the design total is known. CLI progress goes to stderr, as JSON lines with `--json`,
  and is hidden by `--quiet`; stdout keeps its final report. Fusion `.f3z` member counts
  are not available yet.
