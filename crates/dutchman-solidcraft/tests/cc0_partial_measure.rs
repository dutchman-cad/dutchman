//! Opt-in corpus measurement; paths are provided by the local runner, never inferred.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

#[test]
#[ignore = "requires the CC0 recipe corpus and two Dutchman binaries"]
fn measure_cc0_partial_history() {
    let lane = PathBuf::from(std::env::var_os("DUTCHMAN_MEASURE_LANE").unwrap());
    let corpus = PathBuf::from(std::env::var_os("DUTCHMAN_MEASURE_CORPUS").unwrap());
    let inventory: Value = serde_json::from_slice(
        &std::fs::read(std::env::var_os("DUTCHMAN_MEASURE_INVENTORY").unwrap()).unwrap(),
    )
    .unwrap();
    let cli = std::env::var_os("SOLIDCRAFT_CLI").unwrap();
    let mut records = Vec::new();
    for (label, binary) in [
        (
            "before",
            std::env::var_os("DUTCHMAN_MEASURE_BEFORE").unwrap(),
        ),
        ("after", std::env::var_os("DUTCHMAN_MEASURE_AFTER").unwrap()),
    ] {
        let dir = lane.join(label);
        std::fs::create_dir_all(&dir).unwrap();
        for item in inventory["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["cohort"] == "cc0")
        {
            let example = item["example"].as_str().unwrap();
            let report_path = dir.join(format!("{}.json", item["id"].as_str().unwrap()));
            if report_path.exists() {
                records.push(
                    serde_json::from_slice::<Value>(&std::fs::read(report_path).unwrap()).unwrap(),
                );
                continue;
            }
            let output = lane.join("tmp/measure.solidcraft");
            let run = Command::new(&binary)
                .arg("convert")
                .arg(corpus.join(example))
                .arg(&output)
                .arg("--json")
                .arg("--quiet")
                .arg("--solidcraft")
                .arg(&cli)
                .env("TMPDIR", lane.join("tmp"))
                .output()
                .unwrap();
            let report: Value = serde_json::from_slice(&run.stdout).unwrap_or_else(
                |_| json!({"ok": false, "stderr": String::from_utf8_lossy(&run.stderr)}),
            );
            let mut feature_types = std::collections::BTreeMap::<String, usize>::new();
            if output.exists() {
                let mut archive =
                    zip::ZipArchive::new(Cursor::new(std::fs::read(&output).unwrap())).unwrap();
                let mut text = String::new();
                archive
                    .by_name("document.json")
                    .unwrap()
                    .read_to_string(&mut text)
                    .unwrap();
                let doc: Value = serde_json::from_str(&text).unwrap();
                for feature in doc["features"].as_array().unwrap() {
                    if feature["suppressed"] != true && feature["type"] != "import" {
                        *feature_types
                            .entry(feature["type"].as_str().unwrap().to_owned())
                            .or_default() += 1;
                    }
                }
                std::fs::remove_file(&output).unwrap();
            }
            let record = json!({"pass": label, "example": example, "version": item["version"], "report": report, "editable_features": feature_types.values().sum::<usize>(), "feature_types": feature_types});
            std::fs::write(&report_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
            eprintln!(
                "{label}: {example}: {} {}",
                record["report"]["verification"]["status"], record["editable_features"]
            );
            records.push(record);
        }
    }
    std::fs::write(
        lane.join("measurements.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
    assert_eq!(records.len(), 698);
}
