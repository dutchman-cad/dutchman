### cadmpeg

- The cadmpeg pin moves from `2e01486` to `1923e49`. Fusion components own the bodies their
  body containers hold, and operation-table poses apply the same way to every occurrence of a
  definition, so assemblies are placed closer to the design. Inventor derived parts read their
  stored bodies, positional two-section lofts are read, and rectangular patterns keep their
  suppressed instances as skipped grid positions. SolidWorks parts with unusual edge topology
  are refused rather than read wrong.
- A rectangular pattern that skips grid positions is not mapped yet: SolidCraft's rectangular
  pattern fills the whole grid. The report names the pattern and how many positions it skips,
  and the part keeps its exact geometry.
