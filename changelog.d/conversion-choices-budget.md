### Command line and install

- Add Fast, Standard, and Full conversion presets, with history, reference-sketch, and joint
  choices. Standard budgets assembly history against available memory; Full never drops it
  for memory. FreeCAD output honours the history choice only.
- Estimate conversion memory and time before decoding. Offer a lighter preset interactively,
  or refuse an over-budget run with exit 2; `--yes` disables prompts and `--no-memory-check`
  bypasses the guard. JSON reports include estimates and effective choices. MCP accepts the
  same choices and a `dry_run` pre-read that writes nothing.
