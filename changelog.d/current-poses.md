### Assemblies

- Change the Fusion assembly default to Smart part positions, accepting current poses only when joints, bounds, and repeated inserted designs agree. Use `--part-positions first-placed` to restore the old placement choice.
- Add Part positions choices Smart, As Fusion last showed them, and As first placed to the CLI, remembered desktop Advanced settings, and MCP. Keep `--stored-placements` as a first-placed alias.
- Report the selected mode, placement rule counts, and fallback reasons in JSON and an Info note.
- Compose current poses with the containing document's insertion and retain ambiguous decoded placements with reported counts.
- Prefer decoder-bound saved-scene displayed poses in Smart and As Fusion last showed them, without current-pose guards; skip ambiguous and unbound rows and support decoders without the arena.
- Keep joints closed at displayed poses by deriving missing holder placements from agreeing children, and report derived holders and conflicts.
