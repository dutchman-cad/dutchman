### Assemblies

- Slice component models in parallel and reuse identical empty assembly holders. Prepare
  SolidCraft assembly parts in parallel, including per-part replay checks, while
  preserving design bytes and report order. Bound workers with `--threads N` on `convert`,
  `convert-all`, and `watch`, or MCP `threads`; `0` selects automatically and `1` runs serially.
  Part count and available memory cap workers; JSON reports record requested and used counts.
  Failed worker preparations retry serially. Available memory also respects Linux cgroup
  limits and is detected on Windows.
