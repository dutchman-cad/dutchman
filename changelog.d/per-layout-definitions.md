### Assemblies

- A component whose instances hold different components, or the same components at different
  placements, now converts as an assembly instead of one flat import. It is written as one
  definition per layout (`<component>`, `<component> (2)`, ...), each instance at its own
  placement, and the report notes how many components were written as how many definitions.
  Instances with the same layout, within numeric noise, still share one definition, and mirrored
  placements combine with it.
