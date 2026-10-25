### Command line and install

- `dutchman watch <inbox> --out-dir <dir>` converts files dropped into a folder, for a
  self-hosted Dutchman on a shared or network folder. It checks the folder on an interval,
  waits until a file stops changing, converts it into the output folder with its report beside
  it, and never converts the same content twice (also after a restart). A file that fails
  gets a `.failed.json` report and is tried again only once it changes. The inbox is never
  written to. `--once` converts what is ready and exits, for cron and scripts.
