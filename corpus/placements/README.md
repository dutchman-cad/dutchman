# Placement goldens

Where every part of three public assemblies lands, as the "Placements" workflow
(`.github/workflows/placements.yml`) checks it on each pull request that changes the converter.
A golden maps an occurrence path (the occurrence names from the root, joined by `/`; a repeated
name among siblings gets `#2`, `#3`, ... in document order) to the world position of its origin
in millimetres, rounded to 0.001. The positions of a public design are facts about that design.
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

Only when a part is meant to be somewhere else, and only after a person has opened the converted
design and looked at the picture: the parts sit where the source puts them, nothing is scattered,
floating or stacked at the origin. A failing check is a finding to fix first; regenerating to make
it pass records the fault as correct.

```
python3 scripts/placements.py railcore.sc --out corpus/placements/railcore.json
```

Commit the regenerated golden in the same pull request as the change that moved the parts, and say
in the description which parts moved and why.

The goldens were taken from conversions checked by eye: RailCore and Voron from Dutchman 0.2.5,
the robot arm with its gripper in place.
