# Doctor calibration

Doctor compares stored geometry with converted bodies in the conversion's selected
assembly pose. Keeping joints uses joint-solved placements; omitting joints uses
stored placements. Placement sanity uses that same choice. Comparing different poses
can report movement and can pair the wrong bodies, creating volume and area findings.

The coverage sample contains 19 public designs: three RailCore assemblies, the robot
arm, and 15 other designs. Fast output converts 17 of them. Fourteen designs supply
measured comparisons (356 complete and nine incomplete body rows); three comparisons
are unavailable and two designs do not convert. These coverage outcomes are separate
from the false-positive counts below. Measurements use the same SolidCraft 0.29.0
kernel for both sides of each comparison.

Two other RailCore variants retain six coincidence advisories. The written designs
repeat the transforms of the referenced definitions. These variants have no placement
goldens, so the advisories remain for review and are excluded from the known-good
false-positive rate.

## Observed false positives

The two [placement-golden assemblies](../corpus/placements/README.md) have
408 matching occurrences: 367 in RailCore ZL
and 41 in the robot arm. None moved more than the golden's 0.01 mm allowance, appeared,
or disappeared. For shape-only output that keeps joints (`--import-only --no-sketches`),
their advisory body comparisons
have these observed false-positive counts:

| Assembly | Evaluated body rows | False flags with a stored-pose reference | False flags with the selected-pose reference |
|---|---:|---:|---:|
| RailCore ZL | 270 | 62 | 0 |
| Robot arm | 59 | 49 | 0 |
| Total | 329 | 111 (33.7%) | 0 (0%) |

Rates count flagged body rows, not individual finding codes or occurrence placements.
Two robot rows have incomplete volume/area measurements; their placement comparisons
are included in the row count, but they are not complete geometry passes. The other
327 rows have complete measurements. Both assemblies retain all their golden placements. At design level, the observed
false-positive rate falls from 2/2 assemblies (100%) to 0/2 (0%).
These are observed rates for this sample and comparison mode, not an estimate for all
CAD files or for native geometry decoding.

## Numerical allowances

Volume and area comparisons allow 1% relative error. Bounding boxes and placement
centres allow 0.1% of the body diagonal, clamped to 0.1–0.5 mm. The ceiling keeps a
1 mm translation detectable on metre-scale bodies. Synthetic comparisons cover a
1 mm translation and a floating part at three body sizes; SolidCraft evaluations also
check unchanged, moved, and deliberately different stored/joint poses.

Across 15 additional public designs, standard output supplied 25 complete and seven
incomplete measured body rows. The largest observed relative volume difference was
0.003992%; the largest area difference was 0.004572%. Box and centre differences were
below 0.000001 mm. Fast output supplied 37 complete and seven incomplete rows, with
zero volume/area differences and position differences below 0.000001 mm. Neither mode
produced numerical body findings. These results give no reason to widen the 1% allowance.

Replay retains its existing mesh-error allowance and area tolerance. Only two full
histories in the 15-design sample were verified; five were partial, three flattened,
three had no applicable history, and two designs did not convert. Partial or flattened
history is not a verified full replay. Missing geometry, conversion limits, unavailable
comparisons and incomplete measurements are excluded from claims of a passing check.
No independent matching-STEP accuracy rate is established by these measurements.

## Missing torus trims

`geometry_untrimmed_torus` flags a toroidal face with no trim loops when it shares a
shell with other faces. A whole torus in its own shell is allowed, including a separate
cavity shell. This structural check runs even when kernel measurements are unavailable;
it can catch a missing boundary reproduced in both the reference and converted geometry. Findings remain advisory and need review before reporting a defect.
