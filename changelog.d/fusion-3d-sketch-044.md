### History

- Keep decoded 3D sketch dimensions, joins, axis directions and endpoint tangencies when
  targeting SolidCraft 0.44 or newer, without adding relations from endpoint proximity.
- Convert selected planar loops from 3D sketches into extrudes when the source body check confirms the result.
- Keep large 3D sketch relation lookups efficient and refuse planar profile copies that exceed the constraint budget.
