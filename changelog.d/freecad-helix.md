### History

- Constant-radius FreeCAD additive and subtractive helices keep their sketch profiles,
  axes, pitch/height/turn laws, handedness, and reversed travel as editable coils when
  targeting SolidCraft 0.25.1 or newer. Older targets preserve the original geometry and
  report the required version. Conical helices and spirals report the missing coil option.
- Choose the target release with `--solidcraft-version x.y.z` on `convert`, `convert-all`,
  or `watch`, or `solidcraft_version` in MCP. The default is the found verification CLI's
  version, else 0.18.2; with `--no-verify`, it defaults to 0.18.2. Targets below 0.18.2 are
  refused. Counterbore and countersink holes retain flat bottoms on newer targets too.
