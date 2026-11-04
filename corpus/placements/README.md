# Placement goldens

Where every part of three public assemblies lands, as the "Placements" workflow
(`.github/workflows/placements.yml`) checks it on each pull request that changes the converter.
A golden maps an occurrence path (the occurrence names from the root, joined by `/`; a repeated
name among siblings gets `#2`, `#3`, ... in document order) to the world position of its origin
in millimetres, rounded to 0.001. These goldens record converter output; an independent source reference is needed to establish whether those positions are correct.
The check fails when a shared part moves more than 0.01 mm, or a part disappears or appears. The
"Editable history" stand-ins the converter adds beside a part are not listed.

| Golden | Source | Licence | Pinned file |
|---|---|---|---|
| `railcore.json` | RailCore II 300ZL, [railcore/parts](https://github.com/railcore/parts), `DXF/f3z/RailCore II 300ZL - Nov 2019.f3z` (the file now sits in `F3Z/` of that repository but is empty there; this commit still holds it) | CC BY 3.0 | commit `81ef3be6c712e56782142dbce4b736994c4a1477`, SHA-256 `eb4afd93e0a5bba9159090909532a91862385f8fcf17ee74c4b687fa45b434fe` |
| `robot-arm.json` | Robot arm with gripper, [Adriaeik/fusion2URDF](https://github.com/Adriaeik/fusion2URDF), `examples/Assem1_description/Assem1.f3z` | MIT | commit `4411ffbdffc0364dc6f5922c40b3e14fd7b642f8`, SHA-256 `3fd57697437949374b76df74dbc69342d286512f0ef8f4580b5dca7c1d6dfe44` |
| `voron.json` | Voron 2.4r2 assembly, [VoronDesign/Voron-2](https://github.com/VoronDesign/Voron-2), `CAD/Voron_2.4r2_Assembly.f3d` | GPL-3.0 | commit `a192410e27ea345644ae5c4b29b4c9c40cbe1a73`, SHA-256 `09ddd5561fed2f5f9cbcd13b9cda46b695d49af59240644ba164efa04ab0973f` |

The workflow downloads each file by its raw URL at the pinned commit and checks the SHA-256
before converting it with `--preset standard --no-verify`. The converted part names carry the
file's name, so the file must keep its original name. The Voron conversion needs about 15 GB, so
it runs only when `Cargo.lock` or `crates/dutchman-core` changes (or by hand).

## Check a design yourself

```
dutchman convert "RailCore II 300ZL - Nov 2019.f3z" -o railcore.sc --preset standard --no-verify
python3 scripts/placements.py railcore.sc --check corpus/placements/railcore.json
```

## Regenerate a golden

Regenerate only after checking why parts moved against an independent source, such as Fusion's
saved display scene, or a visual reference. A failing check is a finding to investigate first;
regenerating solely to make it pass records a fault as correct.

```
python3 scripts/placements.py railcore.sc --out corpus/placements/railcore.json
```

Commit the regenerated golden in the same pull request as the change that moved the parts, and say
in the description which parts moved and why.

RailCore and Voron were taken from Dutchman 0.2.5 conversions checked by eye. The original
robot-arm golden was also converter output checked visually, without an independent pose
reference; it pinned the old angular sign for aligned joints. Its replacement uses the fixed
second-connector rule, `C0 = C1 * T(offset) * Rz(+angle)` for aligned frames.

## Check against Fusion's saved scene

The robot-arm golden is checked against the root design's saved member
`FusionAssetName[Active]/OGS.BlobFolder/OGS/DefaultScene/world`. Its matrices are column-major
float32 4×4 transforms in centimetres. Absolute translation checks accept the rounded golden;
relative translation and rotation checks use the converted design's full, unrounded transforms.

```sh
dutchman convert Assem1.f3z --part-positions last-shown -o robot-arm.sc --preset standard --no-verify
python3 scripts/placements.py robot-arm.sc --out corpus/placements/robot-arm.json
python3 scripts/fusion_scene_placements.py Assem1.f3z corpus/placements/robot-arm.json \
  --mapping corpus/placements/robot-arm.scene.json --out robot-arm-absolute.json
```

The explicit last-shown mode matches all 38 independently mapped part origins within 0.1 mm;
the maximum error of the rounded golden is 0.000669 mm. Three assembly holders have no
independently assigned scene entry and remain explicitly unmatched (exit 4). Before selecting
current poses, all 38 mapped parts exceeded 0.1 mm, with a maximum of 597.217100 mm at
`dito_MIR:1`. The seven root parts' native `current_pose` records match within 0.000064 mm
and 1.25e-8 rad; their decoded stored transforms differ by 154.824–578.362 mm.

The scene also contains stored transforms. Matching an arbitrary `TransformAttribute` matrix
does not establish a displayed pose. Count matches against `OverrideTransformAttribute`
matrices instead:

```sh
python3 scripts/fusion_scene_placements.py Assem1.f3z robot-arm.sc --overrides \
  --out robot-arm-overrides.json
```

With the full preset, displayed-pose matches rise from 0/41 to 40/41. There are 39 overrides;
a holder shares a matching pose, so the match count can exceed the override count. These
checks require translation within 0.1 mm and axis entries within 1e-3. A match means a pose
Fusion displays. Overrides are not yet bound to named occurrences; identical parts could
swap unnoticed. This aggregate count always exits 4 to retain that limitation. It accepts a
SolidCraft design, whose unrounded transforms allow both translation and rotation checks.
The robot-arm golden explicitly uses `--part-positions last-shown`, preserving its check
against Fusion's displayed poses. With a decoder that supplies bound displayed poses, Smart uses them without a bounds veto.
When that arena is absent, Smart may retain first placements if its current-pose bounds
check fails. Use `--part-positions first-placed` (or the
`--stored-placements` alias) to ignore later poses.

Relative checks separate that arm movement from the gripper joint. The joint's first operand
is `griper:1/Assem1 component 1:1/frame:1`; its second is `Assem1 component 2:1/Part02:1`
(source occurrence `f3d:model:occurrence#25`). It is fixed, aligned, with angle +π/2 and zero offset.
Run both the gripper's internal check and its attachment check:

```sh
python3 scripts/fusion_scene_placements.py Assem1.f3z robot-arm.sc \
  --mapping corpus/placements/robot-arm.scene.json \
  --relative-to 'griper:1/Assem1 component 1:1/frame:1' \
  --path-prefix 'griper:1/Assem1 component 1:1/' --out robot-arm-internal.json
python3 scripts/fusion_scene_placements.py Assem1.f3z robot-arm.sc \
  --mapping corpus/placements/robot-arm.scene.json \
  --relative-to 'Assem1 component 2:1/Part02:1' \
  --path 'griper:1/Assem1 component 1:1/frame:1' --out robot-arm-attachment.json
```

Each check forms part-to-anchor transforms independently in the design and scene, then compares
translation and rotation. Shared assembly motion cancels. The rotation error is the angle between
the relative orientations, in radians; scene axes are orthonormalized to remove float32 rounding.
The tolerances are 0.1 mm and 0.001 rad. Repeat `--path` to select individual parts, or omit both
selection options to check every placement (including any unmatched holders). An empty selection
or an unmatched anchor cannot pass.

Measured with the same joint-emitting decoder for both builds:

| Relative comparison | Old rule: maximum mm / rad | Fixed rule: maximum mm / rad |
|---|---|---|
| 31 gripper parts relative to `frame:1` | 0.000084465 / 4.20e-8 | 0.000084465 / 4.20e-8 |
| `frame:1` relative to `Part02:1` | 0.000010043 / π | 0.000010043 / 2.87e-8 |
| 31 gripper parts relative to `Part02:1` | 99.999958 / π | 0.000078685 / 3.14e-8 |

For the last row, replace the attachment command's `--path` with
`--path-prefix 'griper:1/Assem1 component 1:1/'`. All 31 pass with the fix and all 31 fail with
the old rule. Both builds preserve the internal arrangement; the corrected joint orientation
is the independent evidence for regenerating the golden. The attachment frame's origin nearly
coincides in both builds, so checking translation alone would miss its half-turn error.

`robot-arm.scene.json` identifies 38 part matrices by byte offset, source occurrence identity,
and source/scene SHA-256. The reference identities come from the public file's native current
poses and positive-angle joint composition, corroborated against both rotation and translation
in the scene within 0.000064 mm. Duplicate entries must have identical translations for absolute
checks, and identical full transforms for relative checks. These mappings contain derived facts,
not CAD geometry.

The comparison also accepts a single `.f3d`, such as Voron 2.4. For another source, supply an
independently established mapping of occurrence paths to scene matrix offsets, bound to both
SHA-256 values, in the same format. It never assigns a part to whichever matrix happens to be
nearest its converted position: the scene includes stored transforms and current overrides,
so that would silently accept stale or unrelated entries. Without an independent mapping, a
present scene is reported as `no part mapping: not checked`.

Not every Fusion file stores this display scene. The pinned RailCore 2019 and Voron 2.4 inputs
have none: both absolute and relative checks report exactly `no scene: not checked` and exit
**3**, never a pass. Their goldens remain unchanged. Exit statuses are 0 for a complete match of
the selected placements, 1 for translation or rotation differences, 2 for an input error, 3 for
an absent scene, and 4 for unmatched parts or a missing mapping. A partial comparison also cannot
pass, even when all mapped parts are within tolerance.
