//! Repeatable RESULTS-card evidence from the report-flow walk's saved reports.
#![cfg(feature = "test-support")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use dutchman_gui::{
    app::{Row, Status},
    App,
};
use eframe::egui::{self, ThemePreference};
use egui_kittest::Harness;
use std::path::PathBuf;

fn test_app(cc: &eframe::CreationContext<'_>) -> App {
    dutchman_gui::set_diagnostics_folder(
        std::env::temp_dir().join(format!("dutchman-results-tests-{}", std::process::id())),
    );
    App::new(cc)
}

fn assert_report_panel(h: &Harness<'_, App>, size: egui::Vec2) {
    use egui_kittest::kittest::Queryable;
    for label in [
        "Anonymous report",
        "GitHub issue",
        dutchman_gui::report_panel::PRIVACY,
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(rect.bottom() < size.y - 40.0, "{label}: {rect:?}");
    }
}

#[test]
fn results_evidence() {
    let Some(dir) = std::env::var_os("DUTCHMAN_RESULTS_SHOTS").map(PathBuf::from) else {
        return;
    };
    let cases = PathBuf::from(std::env::var_os("DUTCHMAN_REPORT_FLOW_CASES").unwrap());
    let reports = cases;
    let work = tempfile::tempdir().unwrap();
    let mut measurements = String::from("case\tsize-theme\taction\tleft\ttop\tright\tbottom\n");
    dutchman_gui::set_diagnostics_folder(work.path().join("diagnostics"));
    for (size, theme, suffix) in [
        (
            egui::vec2(1600.0, 1000.0),
            ThemePreference::Light,
            "1600-day",
        ),
        (
            egui::vec2(1280.0, 800.0),
            ThemePreference::Dark,
            "1280-night",
        ),
        (
            egui::vec2(1280.0, 800.0),
            ThemePreference::Light,
            "1280-day",
        ),
    ] {
        for name in ["full", "partial", "geometry", "failed", "slow"] {
            let extension = if name == "slow" { "f3z" } else { "f3d" };
            let input = format!("{name}.{extension}");
            let report = format!("{name}.json");
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(reports.join(report)).unwrap()).unwrap();
            let ok = report["ok"] == true;
            let output = ok.then(|| {
                report
                    .pointer("/output/path")
                    .and_then(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| reports.join(&input).with_extension("sc"))
            });
            let mut h = Harness::builder()
                .with_size(size)
                .build_eframe(|cc| test_app(cc));
            h.ctx.set_theme(theme);
            h.state_mut().set_checker_for_test(true);
            h.state_mut().set_previews_enabled(false);
            h.state_mut().set_file_manager(|_| Ok(()));
            h.state_mut().preview_result_for_test(Row {
                input: PathBuf::from(input),
                output,
                status: if ok {
                    Status::Converted
                } else {
                    Status::Failed
                },
                report: Some(report),
                progress: None,
                job: None,
                size: None,
            });
            h.run_steps(12);
            assert_report_panel(&h, size);
            if std::env::var_os("DUTCHMAN_RESULTS_ASSERT_LAYOUT").is_some() {
                use egui_kittest::kittest::Queryable;
                let mut actions = vec!["Save the full log…", "Report this file…"];
                if ok {
                    actions.extend(["Open in SolidCraft", "Show in folder"]);
                }
                for action in actions {
                    let rect = h.get_by_label(action).rect();
                    assert!(
                        rect.left() >= 0.0
                            && rect.right() <= size.x
                            && rect.top() >= 80.0
                            && rect.bottom() < size.y - 50.0,
                        "{name}-{suffix}: {action}: {rect:?}"
                    );
                    measurements.push_str(&format!(
                        "{name}\t{suffix}\t{action}\t{}\t{}\t{}\t{}\n",
                        rect.left(),
                        rect.top(),
                        rect.right(),
                        rect.bottom()
                    ));
                }
            }
            std::fs::create_dir_all(&dir).unwrap();
            h.render()
                .unwrap()
                .save(dir.join(format!("{name}-{suffix}.png")))
                .unwrap();
        }
    }
    if std::env::var_os("DUTCHMAN_RESULTS_ASSERT_LAYOUT").is_some() {
        std::fs::write(
            dir.parent().unwrap().join("layout-measurements.tsv"),
            measurements,
        )
        .unwrap();
    }
}

#[test]
fn result_actions_stay_visible_at_1280_and_preview_is_optional() {
    use egui_kittest::kittest::Queryable;
    let work = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    h.state_mut().set_checker_for_test(false);
    h.state_mut().preview_result_for_test(Row {
        input:PathBuf::from("assembly.f3z"), output:Some(work.path().join("assembly.sc")),status:Status::Converted,
        progress:None,job:None,size:None,
        report:Some(serde_json::json!({"ok":true,"mode":"partial_history","history":{"rebuilt":2,"needing":3,
            "parts":(1..=80).map(|n|serde_json::json!({"name":format!("Part {n}"),"mapped":2,"total":3})).collect::<Vec<_>>()},
            "losses":[{"level":"Degraded","code":"history/body-mismatch","message":"body reason"}]})),
    });
    h.run_steps(12);
    for action in [
        "Open in SolidCraft",
        "Show in folder",
        "Save the full log…",
        "Report this file…",
    ] {
        let rect = h.get_by_label(action).rect();
        assert!(
            rect.top() > 70.0 && rect.bottom() < 750.0,
            "{action}: {rect:?}"
        );
    }
    assert!(h.query_by_label("Converted preview").is_none());
    h.get_by_label("Show all 80").click_accesskit();
    h.run_steps(8);
    for action in [
        "Open in SolidCraft",
        "Show in folder",
        "Save the full log…",
        "Report this file…",
    ] {
        let rect = h.get_by_label(action).rect();
        assert!(
            rect.top() > 70.0 && rect.bottom() < 750.0,
            "{action}: {rect:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn preview_fills_in_after_the_card_and_reuses_the_cache_without_a_cli() {
    use egui_kittest::kittest::Queryable;
    use std::os::unix::fs::PermissionsExt;
    let work = tempfile::tempdir().unwrap();
    let design = work.path().join("part.sc");
    std::fs::write(&design, "synthetic test design").unwrap();
    let png = work.path().join("source.png");
    image::RgbaImage::from_pixel(48, 32, image::Rgba([60, 120, 160, 255]))
        .save(&png)
        .unwrap();
    let cli = work.path().join("snapshot");
    // All paths are discovered from the executable directory; no shell interpolation.
    std::fs::write(&cli,"#!/bin/sh\nroot=$(dirname \"$0\")\nwhile [ \"$#\" -gt 0 ]; do\n if [ \"$1\" = --out ]; then shift; output=$1; fi\n shift\ndone\nwhile [ ! -f \"$root/ready\" ]; do sleep 0.02; done\ncp \"$root/source.png\" \"$output\"\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
    let row = || Row {
        input: work.path().join("part.f3d"),
        output: Some(design.clone()),
        status: Status::Converted,
        progress: None,
        job: None,
        size: None,
        report: Some(serde_json::json!({"ok":true,"mode":"import","losses":[]})),
    };
    for cached in [false, true] {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| test_app(cc));
        h.state_mut().set_preview_cli_for_test(cli.clone());
        h.state_mut().preview_result_for_test(row());
        h.run_steps(2);
        if !cached {
            assert!(h.query_by_label("Converted preview").is_none());
            std::fs::write(work.path().join("ready"), "render").unwrap();
        }
        let started = std::time::Instant::now();
        while h.query_by_label("Converted preview").is_none()
            && started.elapsed() < std::time::Duration::from_secs(3)
        {
            std::thread::sleep(std::time::Duration::from_millis(30));
            h.run_steps(2);
        }
        h.get_by_label("Converted preview");
        for label in ["Open in SolidCraft", "Show in folder", "Report this file…"] {
            assert!(h.get_by_label(label).rect().bottom() < 750.0);
        }
        if !cached {
            std::fs::remove_file(&cli).unwrap();
        }
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    std::fs::remove_file(design.with_extension("preview.png")).unwrap();
    h.state_mut().set_preview_cli_for_test(cli);
    h.state_mut().preview_result_for_test(row());
    h.run_steps(4);
    std::thread::sleep(std::time::Duration::from_millis(100));
    h.run_steps(4);
    assert!(h.query_by_label("Converted preview").is_none());
    h.get_by_label("Report this file…");
    h.get_by_label("Converted");
    assert!(h.query_by_label("Failed").is_none());
}

#[test]
fn big_assembly_evidence() {
    let Some(dir) = std::env::var_os("DUTCHMAN_RESULTS_SHOTS").map(PathBuf::from) else {
        return;
    };
    for (size, theme, suffix) in [
        (
            egui::vec2(1600.0, 1000.0),
            ThemePreference::Light,
            "1600-day",
        ),
        (
            egui::vec2(1280.0, 800.0),
            ThemePreference::Dark,
            "1280-night",
        ),
        (
            egui::vec2(1280.0, 800.0),
            ThemePreference::Light,
            "1280-day",
        ),
    ] {
        let mut h = Harness::builder()
            .with_size(size)
            .build_eframe(|cc| test_app(cc));
        h.ctx.set_theme(theme);
        h.state_mut().set_checker_for_test(true);
        h.state_mut().set_previews_enabled(false);
        h.state_mut().preview_result_for_test(Row {input:PathBuf::from("Synthetic assembly.f3z"),output:Some(PathBuf::from("converted/assembly.sc")),status:Status::Converted,progress:None,job:None,size:None,
            report:Some(serde_json::json!({"ok":true,"mode":"partial_history","verification":{"status":"partial"},
                "history":{"rebuilt":160,"needing":240,"parts":(1..=80).map(|n|serde_json::json!({"name":format!("Part {n}"),"status":"unverified","rebuilt":2,"needing":3,"reason":"feature \"Chamfer 2\": blends on closed edges aren't supported yet"})).collect::<Vec<_>>()},
                "losses":[{"level":"Degraded","code":"history/profile-region-inferred","message":"A sketch region could not be checked"}]}))});
        h.run_steps(12);
        assert_report_panel(&h, size);
        std::fs::create_dir_all(&dir).unwrap();
        h.render()
            .unwrap()
            .save(dir.join(format!("big-assembly-{suffix}.png")))
            .unwrap();
    }
}

#[test]
fn real_preview_evidence() {
    use egui_kittest::kittest::Queryable;
    use std::io::Read;
    let Some(dir) = std::env::var_os("DUTCHMAN_RESULTS_SHOTS").map(PathBuf::from) else {
        return;
    };
    let cli = dutchman_solidcraft::usable_cli(&dutchman_solidcraft::Verification::Auto).unwrap();
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("Synthetic cube.f3d");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
        &input,
    )
    .unwrap();
    let design = work.path().join("Synthetic cube.sc");
    let writer = dutchman_solidcraft::SolidCraftWriter::new()
        .verification(dutchman_solidcraft::Verification::Off)
        .import_only(true);
    let report = dutchman::convert(&input, &design, &writer, false, &Default::default());
    assert_eq!(report["ok"], true, "{report}");
    // The STEP comes from Dutchman's own target container, never from native-file parsing.
    let mut container = zip::ZipArchive::new(std::fs::File::open(&design).unwrap()).unwrap();
    let mut step = Vec::new();
    for n in 0..container.len() {
        let mut entry = container.by_index(n).unwrap();
        if entry.name().unwrap().ends_with(".step") {
            entry.read_to_end(&mut step).unwrap();
            break;
        }
    }
    assert!(!step.is_empty());
    for paired in [false, true] {
        if paired {
            std::fs::write(input.with_extension("step"), &step).unwrap();
        }
        for (size, theme, suffix) in [
            (
                egui::vec2(1600.0, 1000.0),
                ThemePreference::Light,
                "1600-day",
            ),
            (
                egui::vec2(1280.0, 800.0),
                ThemePreference::Dark,
                "1280-night",
            ),
            (
                egui::vec2(1280.0, 800.0),
                ThemePreference::Light,
                "1280-day",
            ),
        ] {
            let mut h = Harness::builder()
                .with_size(size)
                .build_eframe(|cc| test_app(cc));
            h.ctx.set_theme(theme);
            h.state_mut().set_preview_cli_for_test(cli.clone());
            h.state_mut().preview_result_for_test(Row {
                input: input.clone(),
                output: Some(design.clone()),
                status: Status::Converted,
                report: Some(report.clone()),
                progress: None,
                job: None,
                size: None,
            });
            let label = if paired {
                "Original (STEP) preview"
            } else {
                "Converted preview"
            };
            let started = std::time::Instant::now();
            while h.query_by_label(label).is_none()
                && started.elapsed() < std::time::Duration::from_secs(45)
            {
                h.run_steps(2);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let preview = h.get_by_label(label).rect();
            assert!(h.get_by_label("Synthetic cube.f3d").rect().bottom() < preview.top());
            assert_report_panel(&h, size);
            for label in ["Open in SolidCraft", "Show in folder", "Report this file…"] {
                assert!(h.get_by_label(label).rect().bottom() < size.y - 50.0);
            }
            let kind = if paired { "paired" } else { "single" };
            h.render()
                .unwrap()
                .save(dir.join(format!("preview-{kind}-{suffix}.png")))
                .unwrap();
        }
    }
}
