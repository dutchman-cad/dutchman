### Assemblies

- A placed-parts package overlay may carry an optional `thickness` (0 to 1 mm). With it the overlay becomes closed solids, one prism for each area with its holes, extruded from `z` up for a top overlay and down for a bottom one, instead of a zero-thickness sheet. A thickness out of range refuses that overlay by name, and an overlay that would pass the package's 250,000-face cap is drawn as a sheet with a note. The field is in docs/modules.md.
