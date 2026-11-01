### Reports

- Diagnosis now includes numbered rebuild recipes for Fusion, Inventor, SolidWorks and
  FreeCAD, using round starting sizes and the anonymous failure signature. Unknown feature
  kinds get a generic recipe. A versioned release list can identify exact known failures
  before further narrowing and show a workaround or next-version fix advice.
- Failed desktop cards now offer **Diagnose this file**, with step progress, checkpoint
  resume, the recipe, **Check my small file**, and local **Make the report**.
- MCP adds anonymous `diagnose`, `diagnose_narrow`, `convert_part`, `stop_at_feature`,
  `same_failure` and `make_report` tools, with a paste-in troubleshooting prompt. Reports
  stay local until the user chooses to share them.
