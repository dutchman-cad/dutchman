### Reports

- Name and compare SolidCraft by minor version in the desktop app and CLI. JSON
  `solidcraft.target` and `known_newest` visibly change from `x.y.z` to `x.y` strings;
  `found` keeps the installed full version. Patch-sensitive writer gates remain intact.
- Resolve and checksum-verify the latest stable patch of the newest SolidCraft minor
  during CI roundtrips. The oldest window release stays pinned.
