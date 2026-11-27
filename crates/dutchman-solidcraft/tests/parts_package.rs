//! A placed-parts package (`dutchman.parts/1`) read by Dutchman's package reader and written as a
//! SolidCraft design: parts as components and named occurrences, overlays as sheet bodies.
//!
//! The SolidCraft check runs when `SOLIDCRAFT_CLI` names a SolidCraft CLI. Set
//! `DUTCHMAN_PARTS_PACKAGE` to a package folder (with its `manifest.json`) to check a package
//! a module wrote; `DUTCHMAN_PARTS_OUT` then names where to keep the design.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput};
use cadmpeg_registry::{build_encoder, Format};
use dutchman_core::package::{read_package, PARTS_SCHEMA};
use dutchman_core::{Report, Writer};
use dutchman_solidcraft::{SolidCraftWriter, Verification, CLI_ENV};
use serde_json::{json, Value};
use zip::ZipArchive;

fn cube_step() -> Vec<u8> {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let encoder = build_encoder(Format::Step);
    let (_, target) = encoder.targets().default().unwrap();
    let plan = encoder
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(target.id.as_str()),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    bytes
}

fn document(bytes: &[u8]) -> Value {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap()
}

fn evaluate(cli: &Path, design: &[u8]) -> Vec<Value> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
    let output = dir.path().join("eval.json");
    std::fs::write(&input, design).unwrap();
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    result["bodies"].as_array().unwrap().clone()
}

fn write(dir: &Path, manifest: &[u8]) -> (Vec<u8>, Report) {
    let mut report = Report::default();
    let model = read_package(dir, manifest, &mut report).unwrap();
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(&model, &mut bytes, &mut report)
        .unwrap();
    (bytes, report)
}

fn cli() -> Option<PathBuf> {
    std::env::var_os(CLI_ENV).map(PathBuf::from)
}

#[test]
fn a_package_becomes_components_occurrences_and_overlay_sheets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("board.step"), cube_step()).unwrap();
    std::fs::write(dir.path().join("part.step"), cube_step()).unwrap();
    let square = |x: f64, y: f64, s: f64| json!([[x, y], [x + s, y], [x + s, y + s], [x, y + s]]);
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "name": "Synthetic board",
        "units": "mm",
        "models": [{"id": "board", "file": "board.step"}, {"id": "cube", "file": "part.step"}],
        "parts": [
            {"name": "Board", "model": "board", "transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0]]},
            {"name": "R1", "model": "cube", "transform": [[1,0,0,10],[0,1,0,0],[0,0,1,0]]},
            {"name": "R2", "model": "cube", "transform": [[0,-1,0,20],[1,0,0,0],[0,0,1,0]]},
        ],
        "overlays": [
            {
                "name": "Copper top", "kind": "copper", "finish": "masked", "side": "top",
                "part": "Board", "z": 1.001,
                "areas": [{"outer": square(0.0, 0.0, 0.9), "holes": [[[0.2, 0.2], [0.2, 0.7], [0.7, 0.7], [0.7, 0.2]]]}],
            },
            {
                "name": "Silkscreen top", "kind": "silkscreen", "side": "top",
                "part": "Board", "z": 1.01, "color": "#ffee00",
                "areas": [{"outer": square(0.1, 0.1, 0.1), "holes": []}, {"outer": square(0.5, 0.5, 0.1), "holes": []}],
            },
        ],
        "notes": [],
    });
    let (bytes, report) = write(dir.path(), manifest.to_string().as_bytes());
    let doc = document(&bytes);
    let names: Vec<&str> = doc["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Board", "R1", "R2", "Copper top", "Silkscreen top"]);
    // One component for each model and one for each overlay: the cube is one definition.
    assert_eq!(doc["components"].as_array().unwrap().len(), 4);
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| loss.severity != dutchman_core::Severity::Dropped),
        "{:?}",
        report.losses()
    );
    // The overlays' colours reach the design: solder mask green and the silkscreen's own.
    let mut archive = ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let mut steps = String::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        if file.name().unwrap().starts_with("blobs/") {
            let mut text = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut text).unwrap();
            steps.push_str(&String::from_utf8_lossy(&text));
        }
    }
    assert!(
        steps.contains("COLOUR_RGB('',0.05,0.45,0.22)"),
        "mask green"
    );
    assert!(steps.contains("COLOUR_RGB('',1.,0.93"), "silkscreen colour");
    if let Some(cli) = cli() {
        let bodies = evaluate(&cli, &bytes);
        let named: Vec<&str> = bodies.iter().filter_map(|b| b["name"].as_str()).collect();
        // SolidCraft names an imported body `Body (<component>)`.
        let has = |what: &str| named.iter().any(|name| name.contains(what));
        assert!(has("Copper top"), "{named:?}");
        assert!(has("Silkscreen top"), "{named:?}");
        // Board, two resistors, and the two overlay sheets.
        assert_eq!(bodies.len(), 5, "{bodies:?}");
    }
}

#[test]
fn a_package_a_module_wrote() {
    let Some(dir) = std::env::var_os("DUTCHMAN_PARTS_PACKAGE").map(PathBuf::from) else {
        eprintln!("skipped: set DUTCHMAN_PARTS_PACKAGE to a package folder");
        return;
    };
    let manifest = std::fs::read(dir.join("manifest.json")).unwrap();
    let (bytes, report) = write(&dir, &manifest);
    for loss in report.losses() {
        println!("{loss}");
    }
    let doc = document(&bytes);
    println!(
        "{}",
        serde_json::to_string_pretty(&doc["occurrences"]).unwrap()
    );
    // The one-resistor package: R1's body (model box x -0.8..0.8, y -0.4..0.4, z 0..0.45) lands
    // at x 24.2..25.8, y 14.6..15.4, z 1.6..2.05 mm.
    let mut again = Report::default();
    let model = read_package(&dir, &manifest, &mut again).unwrap();
    let assembly = model.assembly.unwrap();
    let r1 = assembly
        .placements
        .iter()
        .find(|p| p.name.as_deref() == Some("R1"));
    if let Some(r1) = r1 {
        let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
        for x in [-0.8, 0.8] {
            for y in [-0.4, 0.4] {
                for z in [0.0, 0.45] {
                    for axis in 0..3 {
                        let row = r1.transform[axis];
                        let v = row[0] * x + row[1] * y + row[2] * z + row[3];
                        low[axis] = f64::min(low[axis], v);
                        high[axis] = f64::max(high[axis], v);
                    }
                }
            }
        }
        for (got, want) in low
            .iter()
            .chain(high.iter())
            .zip([24.2, 14.6, 1.6, 25.8, 15.4, 2.05])
        {
            assert!((got - want).abs() < 0.01, "R1 box {low:?}..{high:?}");
        }
    } else {
        eprintln!("no part R1 in this package; the R1 position check is skipped");
    }
    if let Some(out) = std::env::var_os("DUTCHMAN_PARTS_OUT") {
        std::fs::write(out, &bytes).unwrap();
    }
    if let Some(cli) = cli() {
        let bodies = evaluate(&cli, &bytes);
        println!("{}", serde_json::to_string_pretty(&bodies).unwrap());
    }
}

#[test]
fn a_part_colour_reaches_the_design_as_a_body_look() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("part.step"), cube_step()).unwrap();
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "models": [{"id": "cube", "file": "part.step"}],
        "parts": [
            {"name": "Painted", "model": "cube", "color": "#336699"},
            {"name": "Plain", "model": "cube", "transform": [[1,0,0,5],[0,1,0,0],[0,0,1,0]]},
        ],
    });
    let (bytes, report) = write(dir.path(), manifest.to_string().as_bytes());
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| loss.severity != dutchman_core::Severity::Dropped),
        "{:?}",
        report.losses()
    );
    let doc = document(&bytes);
    assert!(
        doc["appearances"]
            .to_string()
            .to_lowercase()
            .contains("336699"),
        "{}",
        doc["appearances"]
    );
}

#[test]
fn a_mask_opacity_and_every_part_colour_reach_the_design_looks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("part.step"), cube_step()).unwrap();
    let square = json!([[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]]);
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "models": [{"id": "cube", "file": "part.step"}],
        "parts": [
            {"name": "Board", "model": "cube", "color": "#c8b27a"},
            {"name": "Plain", "model": "cube", "transform": [[1,0,0,5],[0,1,0,0],[0,0,1,0]]},
        ],
        "overlays": [
            {"name": "Mask", "kind": "mask", "part": "Board", "z": 1.0, "opacity": 0.85,
             "color": "#1f7a3d", "areas": [{"outer": square, "holes": []}]},
            {"name": "Silk", "kind": "silkscreen", "part": "Board", "z": 1.0,
             "areas": [{"outer": square, "holes": []}]},
            {"name": "Copper", "kind": "copper", "part": "Board", "z": 1.0,
             "areas": [{"outer": square, "holes": []}]},
        ],
    });
    let (bytes, _) = write(dir.path(), manifest.to_string().as_bytes());
    let doc = document(&bytes);
    let bodies = doc["appearances"]["bodies"].as_object().unwrap();
    let look = |colour: [u64; 3]| -> Vec<&Value> {
        bodies
            .values()
            .filter(|look| look["color"] == json!(colour))
            .collect()
    };
    let mask = look([0x1f, 0x7a, 0x3d]);
    assert_eq!(mask.len(), 1, "{bodies:?}");
    assert!((mask[0]["opacity"].as_f64().unwrap() - 0.85).abs() < 0.005);
    let board = look([0xc8, 0xb2, 0x7a]);
    assert_eq!(board.len(), 1, "{bodies:?}");
    assert_eq!(board[0]["opacity"].as_f64().unwrap(), 1.0);
    // The silkscreen and the copper have a look of their own, opaque.
    assert!(bodies.len() >= 4, "{bodies:?}");
    // No two imports share a default body name, so a look cannot land on another part's body.
    let mut names = Vec::new();
    for feature in doc["features"].as_array().unwrap() {
        if let Some(listed) = feature["body_names"].as_array() {
            names.extend(listed.iter().map(|n| n.as_str().unwrap().to_owned()));
        }
    }
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    // Every uncoloured part's import names its body too, not only the coloured one.
    let imports = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "import")
        .count();
    assert!(
        names.len() >= imports - 1,
        "{names:?} for {imports} imports"
    );
}

#[test]
fn an_island_inside_a_pours_hole_is_written_as_its_own_face() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("part.step"), cube_step()).unwrap();
    let hole = json!([[0.1, 0.1], [0.1, 0.9], [0.9, 0.9], [0.9, 0.1]]);
    let island = json!([[0.3, 0.3], [0.6, 0.3], [0.6, 0.6], [0.3, 0.6]]);
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "models": [{"id": "cube", "file": "part.step"}],
        "parts": [{"name": "Board", "model": "cube"}],
        "overlays": [{
            "name": "Pour", "kind": "copper", "part": "Board", "z": 1.0,
            "areas": [
                {"outer": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], "holes": [hole]},
                {"outer": island, "holes": []},
            ],
        }],
    });
    let (bytes, report) = write(dir.path(), manifest.to_string().as_bytes());
    assert!(
        report
            .losses()
            .iter()
            .all(|l| l.severity != dutchman_core::Severity::Dropped),
        "{:?}",
        report.losses()
    );
    let doc = document(&bytes);
    let pour = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Pour")
        .unwrap();
    let blob = pour["step_blob"].as_str().unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut archive.by_name(blob).unwrap(), &mut text).unwrap();
    assert_eq!(text.matches("ADVANCED_FACE").count(), 2, "{text}");
}

/// SolidCraft's own account of the looks, on the CLI's version: the Board and the mask carry
/// their colour and opacity on the body, with no face look painting over them.
#[test]
fn the_cli_reads_the_board_colour_and_mask_opacity_back() {
    let Some(cli) = cli() else {
        eprintln!("skipped: set {CLI_ENV} to a SolidCraft CLI");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("part.step"), cube_step()).unwrap();
    let square = json!([[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]]);
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "models": [{"id": "cube", "file": "part.step"}],
        "parts": [{"name": "Board", "model": "cube", "color": "#c8b27a"}],
        "overlays": [
            {"name": "Mask", "kind": "mask", "part": "Board", "z": 1.0, "opacity": 0.85,
             "color": "#1f7a3d", "areas": [{"outer": square, "holes": []}]},
        ],
    });
    let (bytes, _) = write(dir.path(), manifest.to_string().as_bytes());
    let design = dir.path().join("design.solidcraft");
    std::fs::write(&design, &bytes).unwrap();
    let script = dir.path().join("list.json");
    std::fs::write(&script, r#"[{"command":"appearance.list","params":{}}]"#).unwrap();
    let out = dir.path().join("looks.json");
    let status = std::process::Command::new(&cli)
        .arg("run")
        .arg(&script)
        .arg("--in")
        .arg(&design)
        .arg("--results")
        .arg("--json-out")
        .arg(&out)
        .arg("--quiet")
        .status()
        .unwrap();
    assert!(status.success());
    let looks: Value = serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
    let bodies = looks[0]["bodies"].as_object().unwrap();
    assert!(
        bodies
            .values()
            .any(|l| l["color"] == "#c8b27a" && l["opacity"] == 1.0),
        "{bodies:?}"
    );
    assert!(
        bodies.values().any(
            |l| l["color"] == "#1f7a3d" && (l["opacity"].as_f64().unwrap() - 0.85).abs() < 0.01
        ),
        "{bodies:?}"
    );
    assert!(looks[0]["faces"].as_array().unwrap().is_empty(), "{looks}");
}

#[test]
fn a_thick_overlay_is_a_closed_solid_of_the_expected_volume() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("board.step"), cube_step()).unwrap();
    let hole = json!([[1.0, 1.0], [1.0, 4.0], [4.0, 4.0], [4.0, 1.0]]);
    let outer = json!([[0.0, 0.0], [5.0, 0.0], [5.0, 5.0], [0.0, 5.0]]);
    let island = json!([[10.0, 10.0], [12.0, 10.0], [12.0, 12.0], [10.0, 12.0]]);
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "models": [{"id": "board", "file": "board.step"}],
        "parts": [{"name": "Board", "model": "board"}],
        "overlays": [
            {"name": "Copper top", "kind": "copper", "side": "top", "part": "Board", "z": 2.0,
             "thickness": 0.035, "areas": [{"outer": outer, "holes": [hole]}]},
            {"name": "Copper bottom", "kind": "copper", "side": "bottom", "part": "Board",
             "z": -2.0, "thickness": 0.05, "areas": [{"outer": outer, "holes": []}]},
            {"name": "Copper pour", "kind": "copper", "side": "top", "part": "Board", "z": 3.0,
             "thickness": 0.1,
             "areas": [{"outer": outer, "holes": [hole]}, {"outer": island, "holes": []}]},
        ],
    });
    let (bytes, report) = write(dir.path(), manifest.to_string().as_bytes());
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| loss.severity != dutchman_core::Severity::Dropped),
        "{:?}",
        report.losses()
    );
    // An overlay with one area needs no join; the pour's two areas are joined into one body.
    let joins: Vec<Value> = document(&bytes)["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "combine")
        .cloned()
        .collect();
    assert_eq!(joins.len(), 1, "{joins:?}");
    assert_eq!(joins[0]["operation"], json!("join"));
    assert_eq!(joins[0]["tools"].as_array().unwrap().len(), 1);
    // The piece joined away keeps no look of its own; the body it joined keeps its look.
    let looks = &document(&bytes)["appearances"]["bodies"];
    let tool = joins[0]["tools"][0].as_str().unwrap();
    let target = joins[0]["target"].as_str().unwrap();
    assert!(looks.get(tool).is_none(), "{looks}");
    assert!(looks.get(target).is_some(), "{looks}");
    let Some(cli) = cli() else {
        eprintln!("skipped: set SOLIDCRAFT_CLI to check the solids");
        return;
    };
    let bodies = evaluate(&cli, &bytes);
    let find = |what: &str| {
        let found: Vec<&Value> = bodies
            .iter()
            .filter(|b| b["name"].as_str().is_some_and(|n| n.contains(what)))
            .collect();
        assert_eq!(found.len(), 1, "{what}: {bodies:?}");
        found[0]
    };
    // (25 - 9) mm2 times 0.035 mm, and 25 mm2 times 0.05 mm: the top prism spans z 2 to 2.035,
    // the bottom one z -2.05 to -2.
    for (what, want, faces) in [
        ("Copper top", 16.0 * 0.035, 10),
        ("Copper bottom", 25.0 * 0.05, 6),
    ] {
        let body = find(what);
        assert_eq!(body["closed"], json!(true), "{what}: {body}");
        assert_eq!(body["faces"], json!(faces), "{what}: {body}");
        let volume = body["volume_mm3"].as_f64().unwrap();
        assert!(
            (volume - want).abs() < 1e-6,
            "{what}: volume {volume}, want {want}: {body}"
        );
    }
    let top = find("Copper top");
    assert!((top["bbox"]["min"][2].as_f64().unwrap() - 2.0).abs() < 1e-9);
    assert!((top["bbox"]["max"][2].as_f64().unwrap() - 2.035).abs() < 1e-9);
    let bottom = find("Copper bottom");
    assert!((bottom["bbox"]["max"][2].as_f64().unwrap() + 2.0).abs() < 1e-9);
    // Two areas, (25 - 9) and 4 mm2, each 0.1 mm thick, are two solids in one body, the
    // overlay's: every piece keeps its volume, so the design holds the board (a 10 mm cube) and
    // the sum of area times thickness of every overlay.
    assert_eq!(bodies.len(), 4, "{bodies:?}");
    let pour = find("Copper pour");
    assert_eq!(pour["faces"], json!(10 + 6), "{pour}");
    assert!((pour["volume_mm3"].as_f64().unwrap() - 20.0 * 0.1).abs() < 1e-6);
    assert!(
        bodies.iter().all(|b| b["closed"] == json!(true)),
        "{bodies:?}"
    );
    let total: f64 = bodies
        .iter()
        .map(|b| b["volume_mm3"].as_f64().unwrap())
        .sum();
    let want = 1000.0 + 16.0 * 0.035 + 25.0 * 0.05 + (16.0 + 4.0) * 0.1;
    assert!(
        (total - want).abs() < 1e-6,
        "volume {total}, want {want}: {bodies:?}"
    );
}

#[test]
fn metallic_and_roughness_reach_the_design_look_as_pbr() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("board.step"), cube_step()).unwrap();
    let square = json!([[0.0, 0.0], [0.5, 0.0], [0.5, 0.5], [0.0, 0.5]]);
    let manifest = json!({
        "schema": PARTS_SCHEMA,
        "models": [{"id": "board", "file": "board.step"}],
        "parts": [{"name": "Board", "model": "board", "color": "#c8b27a",
                   "metallic": 0.0, "roughness": 0.5}],
        "overlays": [
            {"name": "Copper", "kind": "copper", "part": "Board", "z": 1.0,
             "metallic": 0.0, "roughness": 0.45, "areas": [{"outer": square, "holes": []}]},
            {"name": "Silk", "kind": "silkscreen", "part": "Board", "z": 1.0,
             "areas": [{"outer": square, "holes": []}]},
        ],
    });
    let (bytes, report) = write(dir.path(), manifest.to_string().as_bytes());
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| loss.severity != dutchman_core::Severity::Dropped),
        "{:?}",
        report.losses()
    );
    let doc = document(&bytes);
    let bodies = doc["appearances"]["bodies"].as_object().unwrap();
    let pbr_of = |what: &str| {
        let found: Vec<&Value> = bodies
            .iter()
            .filter(|(name, _)| name.as_str() == what)
            .map(|(_, look)| look)
            .collect();
        assert_eq!(found.len(), 1, "{what}: {bodies:?}");
        found[0]["pbr"].clone()
    };
    assert_eq!(
        pbr_of("Body (Copper)"),
        json!({"metallic": 0.0, "roughness": 0.45, "dielectric_ior": 1.5})
    );
    assert_eq!(
        pbr_of("Body"),
        json!({"metallic": 0.0, "roughness": 0.5, "dielectric_ior": 1.5})
    );
    assert!(pbr_of("Body (Silk)").is_null(), "no values, no pbr");
    if let Some(cli) = cli() {
        assert_eq!(evaluate(&cli, &bytes).len(), 3);
    }
}
