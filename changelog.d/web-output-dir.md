### Command line and install

- `dutchman-web --output-dir <dir>` also saves each converted design in a folder, such as a
  mounted network share, keeping its uploaded folders and name and numbering it rather than
  replace a file already there. Each result says where its design was saved.
- `dutchman-web --base-path /dutchman` serves every page and API route under that path, so it
  can share one address with other services behind a reverse proxy.
