### Desktop app

- The SolidCraft plugin asks for the file first, so the options that follow are the ones for
  that file, and File > Open and drag-and-drop go straight to them. A new **Folder with the
  parts** field lets you pick the folder that holds an assembly's parts, such as a Fusion
  export folder, for that one file. SolidCraft grants read access to it only when you pick it.
- The plugin remembers your last Keep editable history, Up axis, and Include hidden bodies
  choices in SolidCraft's per-user settings and offers them as the defaults for the next file.
  A conversion that fails keeps the earlier choices.
- This plugin needs a SolidCraft release that includes plugin importer support. Older
  SolidCraft releases refuse it at install and say which capability is missing.
- An Inventor or SolidWorks assembly whose stored part paths go through a hidden folder, such as
  a synced drive's `.shortcut-targets-by-id`, opens in the SolidCraft plugin: the plugin finds
  each part beside the assembly instead of stopping on SolidCraft's refusal of hidden names.
