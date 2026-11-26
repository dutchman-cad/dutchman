### Command line and install

- A design is stood up (+Y becomes +Z) by default when its source states that it is Y-up, as a Fusion export's manifest now does. Z-up designs and designs whose up axis is unknown keep their axes. `--stand-up` always rotates, `--keep-axes` (also `--no-stand-up`) never does, and the app and the plugin's `up_axis` option offer the same three choices. The report says which up axis was found, where it came from and what was done.
