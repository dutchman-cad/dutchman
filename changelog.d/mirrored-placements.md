### Assemblies

- Mirrored placements retain reflected component geometry in shared mirrored definitions,
  including nested reflections. Mirrored copies keep imported geometry and report history
  and affected joints left out. Tiny placement drift is corrected; non-finite, scaled, or
  sheared placements beyond tolerance are left out with a note so the design can open.
  Mirrored geometry is written into the carriers (points, curves, surfaces, free-form
  poles) rather than as a reflecting transform, so SolidCraft keeps every mirrored body.
