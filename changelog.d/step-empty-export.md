### Formats

- A STEP file that has a product but no solids, surfaces or curves now says the program that wrote it exported an empty design, and to check the design has bodies and export again, instead of "no geometry could be read". The JSON error keeps its `kind` and adds `"cause": "empty_export"`.
