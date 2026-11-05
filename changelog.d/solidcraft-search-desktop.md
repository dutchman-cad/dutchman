### Desktop app

- The desktop app now finds SolidCraft the way the command line does. A SolidCraft that was
  slow to answer when the app opened (a first start after installing) is no longer treated as
  missing for the whole session: opening the SolidCraft version window searches again. A
  remembered `solidcraft-cli` that no longer answers is ignored in favour of the automatic
  search, and the window lists the places that were searched when nothing is found.
