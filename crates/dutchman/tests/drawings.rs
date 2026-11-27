//! Drawing files: every one is accepted and gets a report; FreeCAD TechDraw pages become a
//! SolidCraft drawing. Inputs are synthetic or FreeCAD fixtures made for this project.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dutchman(args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(args)
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

fn json_of(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|err| {
        panic!(
            "{err}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A minimal file with the format's magic bytes, and the report's `drawing.format` for it.
fn synthetic(name: &str) -> (Vec<u8>, &'static str) {
    match name.rsplit('.').next().unwrap_or_default() {
        "idw" => ([CFB.as_slice(), &[0; 512]].concat(), "inventor-idw"),
        "slddrw" => ([CFB.as_slice(), &[0; 512]].concat(), "solidworks-slddrw"),
        "dft" => (
            [CFB.as_slice(), &[0; 64], &wide("SolidEdge"), &wide("Draft")].concat(),
            "solid-edge-dft",
        ),
        "f2d" => (b"PK\x03\x04synthetic".to_vec(), "fusion-f2d"),
        "dwg" => (b"AC1032\0\0\0\0\0\0\0synthetic".to_vec(), "autocad-dwg"),
        _ => (
            b"  0\nSECTION\n  2\nHEADER\n  0\nENDSEC\n  0\nEOF\n".to_vec(),
            "onshape-export",
        ),
    }
}

#[test]
fn every_drawing_format_is_accepted_and_reported_never_skipped() {
    let folder = tempfile::tempdir().unwrap();
    for name in ["a.idw", "b.slddrw", "c.dft", "d.f2d", "e.dwg", "f.dxf"] {
        let (bytes, id) = synthetic(name);
        let input = folder.path().join(name);
        std::fs::write(&input, bytes).unwrap();
        let output = folder.path().join(format!("{name}.sc"));
        let result = dutchman(
            &[
                "convert",
                input.to_str().unwrap(),
                output.to_str().unwrap(),
                "--json",
            ],
            &[],
        );
        assert!(!result.status.success(), "{name}: nothing was converted");
        let report = json_of(&result);
        assert_eq!(report["ok"], false, "{name}");
        let drawing = &report["drawing"];
        assert_eq!(drawing["format"], id, "{name}");
        assert_eq!(drawing["status"], "not_decoded", "{name}");
        assert_eq!(
            drawing["first_stop"]["code"], "drawing.format.not_decoded",
            "{name}"
        );
        assert_eq!(drawing["sheets"]["written"], 0);
        assert!(drawing["output"].is_null());
        assert!(!output.exists(), "{name}: no drawing without sheets");
        // The scoreboard's rule: one line, no path, no file name in a message.
        let root = folder.path().to_string_lossy().into_owned();
        let mut messages = vec![drawing["first_stop"]["message"].clone()];
        messages.extend(
            drawing["losses"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["message"].clone()),
        );
        for message in messages {
            let message = message.as_str().unwrap();
            assert!(!message.contains(['\n', '/', '\\']), "{name}: {message}");
            assert!(
                !message.contains(&root) && !message.contains(name),
                "{name}: {message}"
            );
        }
        // A synthetic file whose contents are not what its suffix says is still reported.
        let wrong = folder.path().join(format!("wrong-{name}"));
        std::fs::write(&wrong, b"plain text").unwrap();
        let report = json_of(&dutchman(
            &["convert", wrong.to_str().unwrap(), "--json"],
            &[],
        ));
        assert_eq!(report["drawing"]["status"], "not_decoded", "{name}");
        assert!(report["drawing"]["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["code"] == "drawing.format.content_mismatch"));
    }
}

#[test]
fn convert_all_reports_each_drawing_and_does_not_skip_it() {
    let folder = tempfile::tempdir().unwrap();
    for name in ["a.idw", "e.dwg"] {
        std::fs::write(folder.path().join(name), synthetic(name).0).unwrap();
    }
    let out = folder.path().join("out");
    let result = dutchman(
        &[
            "convert-all",
            folder.path().to_str().unwrap(),
            "--out-dir",
            out.to_str().unwrap(),
            "--json",
        ],
        &[],
    );
    let text = String::from_utf8_lossy(&result.stdout);
    assert!(
        text.contains("inventor-idw") && text.contains("autocad-dwg"),
        "{text}"
    );
    assert!(!text.contains("not supported"), "{text}");
}

fn solidcraft_cli() -> Option<PathBuf> {
    std::env::var_os("SOLIDCRAFT_CLI")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

#[test]
fn freecad_techdraw_pages_become_a_solidcraft_drawing() {
    let Some(cli) = solidcraft_cli() else {
        eprintln!("skipped: SOLIDCRAFT_CLI is not set");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("box.FCStd");
    std::fs::copy(fixture("techdraw.FCStd"), &input).unwrap();
    let output = folder.path().join("box.sc");
    let result = dutchman(
        &[
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--json",
        ],
        &[("SOLIDCRAFT_CLI", cli.to_str().unwrap())],
    );
    let report = json_of(&result);
    assert_eq!(report["ok"], true, "{report}");
    let drawing = &report["drawing"];
    assert_eq!(drawing["format"], "freecad-techdraw", "{drawing}");
    assert_eq!(drawing["status"], "partial", "{drawing}");
    assert_eq!(
        drawing["sheets"],
        serde_json::json!({"read": 1, "written": 1})
    );
    assert_eq!(drawing["views"]["read"], 2);
    assert_eq!(drawing["views"]["written"], 2);
    assert_eq!(drawing["dimensions"]["read"], 1);
    assert_eq!(drawing["dimensions"]["written"], 0);
    assert_eq!(drawing["annotations"]["read"], 1);
    let codes: Vec<_> = drawing["losses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["code"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        codes.contains(&"drawing.dimension.unsupported".to_owned()),
        "{codes:?}"
    );
    assert!(
        codes.contains(&"drawing.annotation.unsupported".to_owned()),
        "{codes:?}"
    );
    // The written drawing opens, with the sheet and view count as written.
    let (format, sheets, views) = dutchman_solidcraft::drawing::inspect(&output).unwrap();
    assert!(format.starts_with("solidcraft-drawing/"), "{format}");
    assert_eq!((sheets, views), (1, 2));
    let check = Command::new(&cli)
        .arg("eval")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
}
