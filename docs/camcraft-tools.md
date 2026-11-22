# Fusion tool libraries to CamCraft

```sh
dutchman library.tools                 # writes library.camtools beside it
dutchman convert library.json -o tools.camtools
```

Dutchman converts a Fusion tool library into [CamCraft](https://github.com/NeoTech/Camcraft)'s
`.camtools` file. In CamCraft, **Import library…** opens the result. The desktop app takes the same
files: add them with **Add designs** (the file dialog has a *Fusion tool libraries* filter) or drop
them on the window, and the result is written to the output folder as `<name>.camtools`.

- **Input:** a `.tools` export (a zip holding JSON) or a `.json` export. A `.json` file is taken as a
  tool library when it holds a `data` list of Fusion tools; any other `.json` is not touched.
  A `.tools` archive with several JSON files is read as one library, in file-name order.
- **Output:** `.camtools`, CamCraft's `camcraft-tools` file, lengths in millimetres. The tool library
  inside a CAM design (`.f3d`) is not read yet.
- **Report:** the usual `dutchman.convert/1` report. Every tool CamCraft cannot hold and every field it
  has no place for is a named loss; the rest of the library is still converted. `--json` adds
  `tools: {read, written, not_written}`.

CamCraft refuses a whole library for one tool it dislikes (a name twice, a tool that breaks its
geometry rules). Dutchman therefore checks each tool against CamCraft's rules and lists a tool that
would be refused, instead of writing a file CamCraft will not open.

## What the report says

| Level | Meaning |
|---|---|
| Dropped | The tool is not in the `.camtools`: a type CamCraft has no kind for, a missing size, a unit that is not millimeters or inches, or numbers CamCraft's rules refuse. The reason is given. |
| Degraded | The tool is written, but not exactly: a face mill written as a flat end mill that does not cut at its centre, a slot mill as a keyseat cutter, a center or spot drill as a drill, a countersink as a chamfer mill, a drill with no point angle (118 degrees), or a tool with no cutting data (placeholder 1000 rpm and 100 mm/min). |
| Info | Fusion fields with data CamCraft has no place for (plunge, ramp, lead-in, lead-out, retract and transition feeds, coolant, ramp spindle speed, tip diameter, shoulder length, holder gauge length, offsets that differ from the tool number, turret and so on); presets that match no CamCraft material; values derived rather than read; a name made unique; holder entries skipped. |

A Fusion field that is empty, zero, false or `disabled` is not reported. `guid`, `reference_guid`,
`expressions`, `version`, `GRADE` and a preset's `name` and `description` hold no tool data and are
not reported.

## Mapping

Lengths and feeds are converted from inches (x 25.4) when the tool's `unit` is `inches`. Angles are
degrees.

| Fusion field | CamCraft field | Notes |
|---|---|---|
| `description` | `name` | Without one, the name is made from the type and diameter. A name used twice becomes `Name (2)`. |
| `type` | `kind` | See the next table. |
| `geometry.DC` | `diameter` | Required. Turning tools also accept `IC`. |
| `geometry.LCF` | `flute_len` | Falls back to `LB`, then `OAL`, and says so. |
| `geometry.LB`, `OAL` | `shank_len` | The longer of the two, minus the flute length. |
| `geometry.SFDM` | `shank_d` | Defaults to the diameter. T-slot, keyseat, dovetail and lollipop cutters need a neck narrower than the head. |
| `geometry.RE` | `corner_r`; `nose_radius` for boring, turning and form tools | A ball nose takes half the diameter. |
| `geometry.SIG` | `tip_angle` | Drill point angle, countersink angle. |
| `geometry.TA` | `tip_angle` | Chamfer mill: twice `TA` (CamCraft stores the included angle). Dovetail: 90 minus `TA` (CamCraft stores the angle between flank and bottom face). |
| `geometry.TP` | `pitch` | Taps and thread mills. A threading insert with no width takes 0.866 x `TP`. |
| `geometry.thread-profile-angle` | `tip_angle` | Thread mill and threading insert; 60 when absent. |
| `geometry.W` | `width` | Grooving blade, threading insert, form tool. |
| `geometry.NOF` | `flutes` | 1 to 32. |
| `BMC` | `tool_material` | `carbide` and `hss`. |
| `product-id`, `vendor` | `part_number`, `vendor` | |
| `post-process.number`, `product-link`, `post-process.comment` | `notes` | `.camtools` stores no controller tool number, so it travels in the notes as `Fusion tool number 5`. |
| `holder.segments` | `holder_d`, `holder_len` | One cylinder: the widest segment and the total height. No holder writes 0 and 0. |
| `start-values.presets` | `rpm`, `feed`, `cutting_data` | The first preset for all materials (else the first preset) gives `rpm` (`n`) and `feed` (`v_f`; or `f_n` x `n`; or `f_z` x `NOF` x `n`). Other presets for `aluminum`, `steel`, `stainless steel`, `cast iron`, `brass` or `plastic` become one cutting row each, with `stepdown` and `stepover` when the preset uses them. |

| Fusion `type` | CamCraft `kind` |
|---|---|
| `flat end mill`, `ball end mill`, `bull nose end mill` | `end_mill`, `ball_nose`, `bull_nose` |
| `chamfer mill` | `taper` |
| `dovetail mill`, `lollipop mill` | `dovetail`, `lollipop` |
| `thread mill` | `thread_mill` |
| `drill` | `drill` |
| `tap right hand`, `tap left hand` | `tap` (left hand writes `thread_hand`) |
| `boring head`, `turning boring` | `bore` |
| `probe` | `probe` |
| `plasma cutter`, `water jet cutter` | `torch`, `waterjet` (spindle speed 0) |
| `turning general`, `turning grooving`, `turning threading`, `turning form` | `turning`, `groove`, `thread`, `form` |
| `face mill`, `slot mill`, `center drill`, `spot drill`, `counter sink` | `end_mill`, `keyseat`, `drill`, `drill`, `taper` (reported as degraded) |
| `tapered mill`, `radius mill`, `form mill`, `counter bore`, `reamer`, `laser cutter`, `marking`, `turning custom`, anything else | not written; named in the report |

Holder entries in a library (`type` `holder`) are not tools; the report counts them.

The `.camtools` version is 1, 2 when a left-hand tap is written, 3 when a cutting row has a
stepdown or stepover. The version is the oldest CamCraft reader that can take the file.

## Limits

- The lathe tool fields (`IC`, `LE`, `W`, `RE`) and the `TA` meaning for dovetails follow the
  structure of Fusion's exports; a library that disagrees shows up as a tool in the report, not as a
  wrong number. Send such a library if one fails.
- CamCraft's tool body (a drawn holder profile or a placed mesh) is not produced; the holder is a
  cylinder.

## Sources

The `.camtools` format is read from CamCraft's own repository (Apache-2.0), commit
`c5bb09ed13f8b63e691d3a5ed6fb0be6a64ee2aa`: the `camcraft-tools` envelope and its reader in
`crates/app/src/tool_library.rs`, the tool record in `crates/app/src/project/file.rs`, and the rules
CamCraft refuses a tool by in `crates/tools/src/tool_sanity.rs`. The Fusion side is the structure of
Fusion's exported libraries. No other tool-library converter was consulted.
