### Formats

- Add a streaming SolidCraft importer component for Fusion, Inventor, SolidWorks and FreeCAD, with editable history and structured conversion losses.
- Implement SolidCraft's `craft:plugin` importer interface as a `wasm32-unknown-unknown` component with no WASI imports, declaring native conversion options and read-only access to the input's folder for assemblies.
- Give each conversion loss a plain title ("Parts of the file kept as-is", "Sketches carried over with changes") beside its code, and title the up axis options ("As in the file", "Stand up (Y-up to Z-up)").
- Check the plugin's editable history in SolidCraft itself through the host's `evaluate` import, so a history that matches the original is the visible model instead of a hidden unverified part.
