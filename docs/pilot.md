# Pilot

`dutchman pilot` runs [Pilot](https://github.com/dutchman-cad/pilot), a local helper for
driving SolidCraft from plain-language requests through its public interfaces. Find
commands and example calls:

```sh
dutchman pilot how "round all edges 3 mm"
```

Start its model proxy with a chat-completions endpoint such as Ollama or LM Studio:

```sh
dutchman pilot model --upstream http://localhost:11434/v1
```

Connect SolidCraft to `http://127.0.0.1:11500/v1`. Install SolidCraft separately.
`dutchman pilot --help` lists Pilot's commands; `dutchman pilot model --help` lists the
model proxy's options. The default `pilot` Cargo feature includes this command.

Constant-radius FreeCAD AdditiveHelix and SubtractiveHelix features keep editable history
as sketch-profile coils when targeting SolidCraft 0.25 patch 1 or newer. Older targets report
`needs SolidCraft 0.25 patch 1 or newer` with the target version and flatten the history. The
profile, axis, pitch/height/turn
law, handedness, reversed travel, and Boolean operation carry across. Available feature
parameter bindings drive dimensions; dimensions without bindings keep their resolved
values with a Degraded loss. Spreadsheet expressions follow the usual value fallback.
Conical helices and flat spirals remain unsupported because SolidCraft's coil has no
cone-angle or radial-growth option. Replay verification applies as for other features.
