### Command line and install

- `dutchman sync FOLDER` converts a design again when its source file changed, using the
  options in the link record beside it, and replaces the `.sc` in place; a failed conversion
  keeps the previous design. `dutchman sync FOLDER --watch` keeps running and converts a source
  once a save has settled. Both print one line per design, or `--json` events. The link record
  now also holds the project and search folders and, in release builds, the commit.
