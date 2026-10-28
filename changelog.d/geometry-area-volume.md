### Reports

- A design whose faces SolidCraft merges (faces that share one surface) is no longer reported
  as missing faces. The history check compares area and volume, so an open-shell warning
  there is no fault when they agree; unmeshable faces, bodies inside out, solids without
  volume, and solids that geometry-only output opens as open shells are still reported.
