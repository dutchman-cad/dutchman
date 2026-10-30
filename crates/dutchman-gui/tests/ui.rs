//! The app driven as a person would: designs added, an output folder set, Convert pressed, and
//! the results read back from the window.
//!
//! `screenshots` renders the window in both themes; it needs a GPU, so it is ignored by default:
//! `DUTCHMAN_GUI_SCREENSHOTS=<dir> cargo test -p dutchman-gui --test ui -- --ignored`.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dutchman_gui::app::Status;
use dutchman_gui::App;
use eframe::egui::{self, ThemePreference};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

fn harness() -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 680.0))
        .build_eframe(|cc| App::new(cc))
}

/// Presses Convert and runs the window until the run is over.
fn convert(harness: &mut Harness<'static, App>) {
    harness.run();
    harness.get_by_label("Convert").click();
    harness.step();
    let started = Instant::now();
    while harness.state().running() || harness.state().rows().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the run did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.step();
    }
    harness.run();
}

#[test]
fn the_window_offers_the_file_problem_form_and_names_its_version() {
    let mut harness = harness();
    harness.run();
    assert!(harness.query_by_label("Report a problem").is_some());
    assert_eq!(
        dutchman_gui::WINDOW_TITLE,
        format!("Dutchman Convert {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        dutchman_gui::REPORT_PROBLEM_URL,
        "https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml"
    );
}

#[test]
fn designs_convert_from_the_window() {
    let out = tempfile::tempdir().unwrap();
    // Keep Convert and all three results visible without scrolling.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 1280.0))
        .build_eframe(|cc| App::new(cc));
    harness.state_mut().add_inputs([
        fixture("generated_unit_cube.f3d"),
        fixture("synthetic_cube.ipt"),
        fixture("solidworks_cube.sldprt"),
    ]);
    harness.state_mut().set_out_dir(out.path().to_path_buf());
    harness.state_mut().set_verify(false);
    convert(&mut harness);

    let rows = harness.state().rows();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.status == Status::Converted));
    assert!(out.path().join("generated_unit_cube.solidcraft").is_file());
    assert!(out.path().join("synthetic_cube.solidcraft").is_file());
    assert!(out.path().join("solidworks_cube.solidcraft").is_file());
    assert!(harness.query_by_label("3 converted, 0 failed").is_some());
    assert_eq!(harness.query_all_by_label("Open in SolidCraft").count(), 3);
}

#[test]
fn the_format_list_shows_headlines_beside_reader_status() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 1800.0))
        .build_eframe(|cc| App::new(cc));
    harness.run();
    harness.get_by_label("Formats").click();
    harness.run();
    assert!(harness.query_by_label("Fusion archive (.f3d)").is_some());
    assert!(harness.query_all_by_label("L5").count() >= 1);
    assert!(harness.query_by_label("IGES (.igs, .iges)").is_some());
    assert!(harness.query_by_label("L9").is_some());
    assert!(harness
        .query_by_label("open it in SolidCraft directly")
        .is_some());
    assert!(harness.query_by_label("Creo or NX part (.prt)").is_some());
}

#[test]
fn a_file_that_cannot_convert_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.f3d");
    std::fs::write(&broken, b"not a design").unwrap();
    let mut harness = harness();
    harness.state_mut().add_inputs([broken]);
    harness.state_mut().set_out_dir(dir.path().join("out"));
    convert(&mut harness);

    let rows = harness.state().rows();
    assert_eq!(rows[0].status, Status::Failed);
    assert!(harness.query_by_label("Failed").is_some());
    assert!(harness.query_by_label("0 converted, 1 failed").is_some());
    assert!(!dir.path().join("out/broken.solidcraft").exists());

    // The failed row says to report it, and its button opens the form filled in, without
    // the file's name.
    harness.get_by_label("Report this on GitHub").click();
    harness.step();
    let opened = harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::OpenUrl(open) => Some(open.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        opened.url.starts_with(dutchman_gui::REPORT_PROBLEM_URL),
        "{}",
        opened.url
    );
    assert!(opened.url.contains("&stage=stage%3A%20"), "{}", opened.url);
    assert!(opened.url.contains("&report-anon="), "{}", opened.url);
    assert!(!opened.url.contains("broken"), "{}", opened.url);
}

#[test]
fn a_file_converts_again_searching_a_folder_chosen_for_it() {
    let out = tempfile::tempdir().unwrap();
    let mut harness = harness();
    harness
        .state_mut()
        .add_inputs([fixture("synthetic_cube.ipt")]);
    harness.state_mut().set_out_dir(out.path().to_path_buf());
    harness.state_mut().set_verify(false);
    convert(&mut harness);

    let parts = out.path().join("Parts");
    let ctx = harness.ctx.clone();
    harness.state_mut().search_for_row(0, parts.clone(), &ctx);
    assert!(harness.state().running());
    let started = Instant::now();
    while harness.state().running() {
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(20));
        harness.step();
    }
    harness.run();
    let row = &harness.state().rows()[0];
    assert_eq!(row.status, Status::Converted);
    assert_eq!(row.job.as_ref().unwrap().search.also, [parts]);
    assert!(harness.query_by_label("1 converted, 0 failed").is_some());
}

#[test]
fn convert_waits_for_designs_and_a_folder() {
    let mut harness = harness();
    harness.run();
    assert!(harness.query_by_label("Add designs to convert.").is_some());
    harness
        .state_mut()
        .add_inputs([fixture("synthetic_cube.ipt")]);
    harness.run();
    assert!(harness.query_by_label("Choose an output folder.").is_some());
}

#[test]
#[ignore = "renders with a GPU; set DUTCHMAN_GUI_SCREENSHOTS to a folder"]
fn screenshots() {
    let Some(dir) = std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").map(PathBuf::from) else {
        panic!("set DUTCHMAN_GUI_SCREENSHOTS to a folder");
    };
    std::fs::create_dir_all(&dir).unwrap();
    // The designs, copied where their paths say nothing about this machine.
    let work = tempfile::tempdir().unwrap();
    let designs = work.path().join("Designs");
    std::fs::create_dir_all(&designs).unwrap();
    let inputs: Vec<PathBuf> = [
        "solidworks_cube.sldprt",
        "synthetic_cube.ipt",
        "container_metadata_only.f3d",
    ]
    .into_iter()
    .map(|name| {
        let copy = designs.join(name);
        std::fs::copy(fixture(name), &copy).unwrap();
        copy
    })
    .collect();
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 680.0))
            .build_eframe(|cc| App::new(cc));
        harness.ctx.set_theme(theme);
        harness.state_mut().updater.preview(dutchman_gui::update::Release {
            tag: "v0.2.6".into(),
            notes: "Dutchman 0.2.6\n\nImproved desktop conversion progress and clearer reports.\nUpdated native format readers.".into(),
            download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
        });
        harness.run();
        harness
            .render()
            .unwrap()
            .save(dir.join(format!("{name}-update-pill.png")))
            .unwrap();
        for (found, case, update, width) in [
            (Some("0.27.0"), "current", false, 1120.0),
            (Some("0.28.0"), "newer", false, 1120.0),
            (Some("0.28.0"), "newer-update", true, 1120.0),
            (Some("0.18.1"), "too-old", false, 1120.0),
            (Some("0.24.0"), "older-history", false, 1120.0),
            (None, "none-found", false, 1120.0),
            (Some("0.28.0"), "narrow", true, 760.0),
        ] {
            let mut preview = Harness::builder()
                .with_size(egui::vec2(width, 680.0))
                .build_eframe(|cc| App::new(cc));
            preview.ctx.set_theme(theme);
            preview.state_mut().preview_solidcraft(found);
            if update {
                preview
                    .state_mut()
                    .updater
                    .preview(dutchman_gui::update::Release {
                        tag: "v0.2.6".into(),
                        notes: "Release notes".into(),
                        download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6"
                            .into(),
                    });
            }
            preview.run();
            let label = if width < 850.0 {
                found.unwrap_or("not found").to_owned()
            } else {
                found.map_or_else(
                    || "SolidCraft not found".into(),
                    |v| format!("SolidCraft {v}"),
                )
            };
            preview.get_by_label(&label).click();
            preview.run();
            assert!(preview
                .query_by_label_contains(match case {
                    "newer-update" | "narrow" => "Update Dutchman",
                    "newer" => "is newer than this Dutchman knows",
                    "too-old" => "is too old to open Dutchman's designs",
                    "older-history" => "A newer SolidCraft keeps more of their history.",
                    "none-found" => "SolidCraft not found. Install SolidCraft",
                    _ => "SolidCraft 0.27.0 is installed.",
                })
                .is_some());
            preview
                .render()
                .unwrap()
                .save(dir.join(format!("{name}-solidcraft-{case}.png")))
                .unwrap();
        }
        harness.get_by_label("Update to 0.2.6").click();
        harness.run();
        harness
            .render()
            .unwrap()
            .save(dir.join(format!("{name}-update-window.png")))
            .unwrap();
        harness.state_mut().updater.open = false;
        harness.run();
        harness
            .render()
            .unwrap()
            .save(dir.join(format!("{name}-empty.png")))
            .unwrap();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(760.0, 1040.0))
            .build_eframe(|cc| App::new(cc));
        harness.ctx.set_theme(theme);
        harness.state_mut().add_inputs(inputs.clone());
        harness
            .state_mut()
            .set_out_dir(work.path().join(format!("Converted-{name}")));
        harness.state_mut().set_verify(false);
        convert(&mut harness);
        // Scrolled to the results, then with the first file's losses open.
        harness.get_by_label("RESULTS").scroll_to_me();
        harness.run();
        harness
            .render()
            .unwrap()
            .save(dir.join(format!("{name}-results.png")))
            .unwrap();
        harness
            .query_all_by_label_contains("Losses:")
            .next()
            .unwrap()
            .click();
        harness.run();
        harness
            .render()
            .unwrap()
            .save(dir.join(format!("{name}-losses.png")))
            .unwrap();
    }
}

#[test]
fn dropped_files_and_results_show_format_status_and_one_invitation() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("creo.prt");
    let second = dir.path().join("nx.prt");
    std::fs::write(&first, b"#UGC:2").unwrap();
    std::fs::write(&second, b"SPLMSSTR").unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 1400.0))
        .build_eframe(|cc| App::new(cc));
    harness.state_mut().add_inputs([first, second]);
    harness.state_mut().set_out_dir(dir.path().join("out"));
    harness.run();
    for program in ["Creo", "NX"] {
        assert!(harness
            .query_by_label(&format!("{program} .prt: opens, no geometry yet  L1"))
            .is_some());
    }
    convert(&mut harness);
    assert!(harness
        .state()
        .rows()
        .iter()
        .all(|r| r.status == Status::Failed));
    assert_eq!(
        harness
            .query_all_by_label_contains("Want to help Dutchman read Creo, NX better?")
            .count(),
        1
    );
    assert!(harness
        .query_by_label(dutchman::json::FIXTURES_URL)
        .is_some());
    assert!(harness
        .query_by_label(dutchman::json::DISCORD_URL)
        .is_some());
}

#[test]
fn solidcraft_status_panel_waits_for_a_click_and_shares_version_notices() {
    for (found, expected) in [
        (Some("0.28.0"), "is newer than this Dutchman knows"),
        (Some("0.18.1"), "is too old to open Dutchman's designs"),
        (
            Some("0.24.0"),
            "A newer SolidCraft keeps more of their history.",
        ),
        (Some("0.27.0"), "SolidCraft 0.27.0 is installed."),
        (None, "SolidCraft not found. Install SolidCraft"),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1120.0, 680.0))
            .build_eframe(|cc| App::new(cc));
        harness.state_mut().preview_solidcraft(found);
        harness.run();
        assert!(harness.query_by_label("SolidCraft version").is_none());
        let label = found.map_or_else(
            || "SolidCraft not found".into(),
            |v| format!("SolidCraft {v}"),
        );
        harness.get_by_label(&label).click();
        harness.run();
        assert!(harness.query_by_label_contains(expected).is_some());
        if found.is_none() || found == Some("0.18.1") {
            assert!(harness.query_by_label("Download SolidCraft").is_some());
        }
    }
}

#[test]
fn update_pill_adapts_to_width_and_waits_for_a_click() {
    for (width, label) in [(760.0, "Update"), (1000.0, "Update to 0.2.6")] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 680.0))
            .build_eframe(|cc| App::new(cc));
        harness
            .state_mut()
            .updater
            .preview(dutchman_gui::update::Release {
                tag: "v0.2.6".into(),
                notes: "Release notes".into(),
                download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
            });
        harness.run();
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_none());
        harness.get_by_label(label).click();
        harness.run();
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_some());
        harness.get_by_label("Skip this version").click();
        harness.run();
        assert!(harness.query_by_label(label).is_none());
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_none());
    }
}
