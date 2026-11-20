These are hand-written synthetic API responses, shaped from the supplied Onshape v17
OpenAPI schemas (`BTFeatureListResponse-2457`, `BTMSketch-151`, `BTMFeature-134`,
`BTMIndividualQuery-138`, `BTAssemblyDefinitionInfo`, and `BTAssemblyPartInfo`).
They were never downloaded from Onshape and describe no owner's design.
`partstudio.json` has a sketch, extrude, and fillet; `assembly.json` has two parts
from one external Part Studio and a revolute mate. IDs and dimensions are invented.

`variables.json` and `configuration.json` follow `BTVariableTableInfo` and
`BTConfigurationResponse-2019`. Export-job responses in the tests follow
`BTTranslationRequestInfo` (`ACTIVE`, `DONE`, and `FAILED`). `export.step` is a
hand-written, empty STEP exchange envelope used only as an opaque binary transport
payload; it is not a geometric conversion fixture. These fetch tests prove raw
response preservation, not CAD decoding or geometry verification.
