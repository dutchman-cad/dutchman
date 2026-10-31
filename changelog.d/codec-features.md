### Command line and install

- `dutchman-core` has a Cargo feature per input format (all on by default, in `formats`), so a
  build that reads only some formats, such as the SolidCraft importer plugin, links only their
  cadmpeg codecs.
