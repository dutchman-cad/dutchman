### History

- A design whose history converts only partway now opens in SolidCraft with its converted features in the visible timeline, then a Remove of the bodies they make, then one Import named "Rest of the design (from <first feature not converted>)" holding the source's final shape. The result is exactly the original shape, with no hidden "Editable history (partial)" component. Roll the timeline marker back before Remove to edit the body the converted features build.
- The same layout is written for each such part of an assembly, in the part's own component.
- The report says which features are editable and where the imported body starts ("Features 1-7 are editable; from Hole 1 on, the part is one imported body."), in the desktop app, the plugin and `--json` (`history.rest_imported_from`).
- When SolidCraft's CLI is not available to name the bodies the converted features make, the features stay in the hidden "Editable history (partial)" component as before.
