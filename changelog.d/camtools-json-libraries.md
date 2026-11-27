### Formats

- A `.json` Fusion tool library converts to `.camtools` exactly like a `.tools` one: `dutchman convert library.json -o tools.camtools`, `dutchman library.json`, and in `convert-all`, which now converts the `.tools` and `.json` tool libraries in a folder to `<name>.camtools` at the mirrored path. A `.json` is a tool library when it is an object whose `data` list holds only holders and Fusion tools; it may mix both, and may be UTF-8 or UTF-16 with a byte order mark. Any other `.json` is still not converted.
- Tool names made from the type and size no longer show float noise: an inch size is a common fraction where exact (`1/16"`, `3/16"`), else at most 4 decimals; a millimeter size at most 3.
- A library of only holders says "this library holds only holders (N), no tools to convert" instead of a count of 0 tools.
- A tool the library gives no cutting data at all is reported at Info with its placeholder speed and feed, not as degraded; a tool whose presets could not be used stays degraded.
