### Desktop app

- When the SolidCraft plugin cannot convert a file, its message now carries the full failure
  report, the same `dutchman.convert/1` report the Dutchman app writes to `<name>.failed.json`,
  with its reason codes, and says how to get that file from the app. SolidCraft gives a plugin
  no clipboard or file to write, so the report travels in the message text.
- An Inventor or SolidWorks assembly is no longer refused for the size of its own file, which
  only lists parts. The plugin counts the parts it reads and refuses before reading one that
  would not fit, in one line that names the limit, what the design needs, and what the plugin
  has, and points to the Dutchman app.
