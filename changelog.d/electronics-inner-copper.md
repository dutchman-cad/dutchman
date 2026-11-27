### Formats

- Circuit boards can include their inner copper layers: `--copper all` (default `outer`), the `copper` option in the MCP tools, and "Inner copper layers" under Advanced in the app. Each inner layer becomes its own body named like "Inner copper (In1.Cu)", placed by its height and thickness. With an older electronics module that cannot do this, the outer layers are read and the report says so.
- A board overlay with a side other than top, bottom or inner is left out with a Degraded note, and the rest of the board converts. A board package can now carry up to 64 overlays.
- A board package can group its parts and overlays with an optional `group` text: each group becomes a folder in the design (for example "Components top" and "Layers"), and anything without a group stays at the top level.
