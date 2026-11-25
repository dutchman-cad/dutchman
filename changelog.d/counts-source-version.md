### Reports

- The counts-only report (`--report-counts --report-anon`) now counts the Inventor release each file was saved with (`inventor_2016` to `inventor_2027`, `inventor_older`, `inventor_newer`). Other formats stay `unknown`.
- `dutchman convert --json` reports `source_version` for an Inventor file: the saving application's version as `major.minor.revision`, read from the file's own numbers.
