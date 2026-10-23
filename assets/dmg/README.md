# Disk image background

Original artwork drawn for Dutchman, licensed under the [Apache License, Version
2.0](../../LICENSE). No third-party artwork is used. The drafting grid, transfer arrow
and lantern accent use the ink (`#0E1A24`), teal (`#1F7A6A`) and amber (`#D98E2B`)
from [Dutchman's mark](../logo/dutchman-mark.svg), on a muted light ground that stays
legible alongside either macOS appearance.

`background.svg` is the single source. Regenerate the checked-in 640 × 480 @1x and
1280 × 960 @2x PNGs with:

```sh
cargo run --locked -p dutchman-gui --example dmg-background -- assets/dmg
```

The packaging script combines them into a multi-resolution TIFF using macOS `sips`
and `tiffutil`. Finder uses the representation appropriate for the display's scale.
The window shows about 640 × 440 points: 96-point icons at (180, 190) and (460, 190),
with the arrow between them, and the read-me below at (320, 320). The background is 40
points taller than that, so a shorter title bar on older macOS shows more ground instead
of a blank strip, and Finder needs no scroll bar. Keep the SVG geometry and
Finder positions in `scripts/macos-dmg.sh` together when changing the layout.
