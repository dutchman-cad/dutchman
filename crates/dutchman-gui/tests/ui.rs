//! The app driven as a person would: designs added, an output folder set, Convert pressed, and
//! the results read back from the window.
//!
//! `screenshots` renders the window in both themes; it needs a GPU, so it is ignored by default:
//! `DUTCHMAN_GUI_SCREENSHOTS=<dir> cargo test -p dutchman-gui --features test-support
//! --test ui -- --ignored screenshots`. Linux isolates diagnostics with XDG_STATE_HOME.

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
    harness.get_by_label("Convert").click_accesskit();
    harness.step();
    wait_for_conversion(harness);
}

#[test]
fn the_window_offers_the_file_problem_form_and_names_its_version() {
    let mut harness = harness();
    harness.run();
    let report = harness.get_by_label("Report a problem").rect();
    let night = harness.get_by_label("Night").rect();
    assert!((report.center().y - night.center().y).abs() < 1.0);
    assert!(report.left() > night.right());
    assert_eq!(
        dutchman_gui::window_title(),
        format!("Dutchman Convert {}", dutchman::BUILD_VERSION)
    );
    let name = harness.get_by_label("Dutchman Convert").rect();
    let version = harness
        .get_by_label(&format!("v{}", dutchman::BUILD_VERSION))
        .rect();
    assert!(version.left() >= name.right());
    assert!(version.height() < name.height());
    assert_eq!(
        dutchman_gui::REPORT_PROBLEM_URL,
        "https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml"
    );
}

#[test]
fn the_top_row_names_the_app_and_its_version_at_the_narrowest_width() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(520.0, 420.0))
        .build_eframe(|cc| App::new(cc));
    harness.run();
    let name = harness.get_by_label("Dutchman Convert").rect();
    let version = harness
        .get_by_label(&format!("v{}", dutchman::BUILD_VERSION))
        .rect();
    let report = harness.get_by_label("Report a problem").rect();
    assert!(version.left() >= name.right());
    assert!(version.bottom() <= 80.0);
    assert!(report.left() >= 0.0 && report.right() <= 520.0);
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
    assert!(out.path().join("generated_unit_cube.sc").is_file());
    assert!(out.path().join("synthetic_cube.sc").is_file());
    assert!(out.path().join("solidworks_cube.sc").is_file());
    assert!(harness.query_by_label("3 converted, 0 failed").is_some());
    assert_eq!(harness.query_all_by_label("Open in SolidCraft").count(), 3);
}

#[test]
fn the_format_list_shows_headlines_beside_reader_status() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 1800.0))
        .build_eframe(|cc| App::new(cc));
    harness.run();
    harness.get_by_label("Formats").click_accesskit();
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
    assert!(!dir.path().join("out/broken.sc").exists());

    // The failed row says to report it, and its button opens the form filled in, without
    // the file's name.
    harness
        .get_by_label("Report this on GitHub")
        .click_accesskit();
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
    // Environment changes happen only in a child process, never alongside other tests.
    // In particular, never inspect or write the owner's diagnostic logs.
    if std::env::var_os("DUTCHMAN_GUI_SCREENSHOT_CHILD").is_none() {
        let state = tempfile::tempdir().unwrap();
        if !cfg!(target_os = "linux") {
            panic!("diagnostics isolation uses XDG_STATE_HOME on Linux");
        }
        if !cfg!(feature = "test-support") {
            panic!("enable --features test-support for the memory dialog");
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "screenshots", "--nocapture"])
            .env("DUTCHMAN_GUI_SCREENSHOT_CHILD", "1")
            .env("XDG_STATE_HOME", state.path())
            .status()
            .unwrap();
        assert!(status.success(), "screenshot child failed: {status}");
        return;
    }
    let work = tempfile::tempdir().unwrap();
    let designs = work.path().join("Designs");
    std::fs::create_dir_all(&designs).unwrap();
    let input = designs.join("generated_unit_cube.f3d");
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let failed_input = designs.join("container_metadata_only.f3d");
    std::fs::copy(fixture("container_metadata_only.f3d"), &failed_input).unwrap();
    let mut index = format!("# Dutchman desktop UI review\n\nBuilt from Dutchman {}. 1100 × 800 logical pixels, one PNG per state and theme. Day = light; Night = dark.\n\n", env!("CARGO_PKG_VERSION"));
    for (theme, name) in [
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ] {
        let mut harness = screenshot_harness(theme);
        harness.state_mut().updater.preview(dutchman_gui::update::Release {
            tag: "v0.2.6".into(),
            notes: "Dutchman 0.2.6\n\nImproved desktop conversion progress and clearer reports.\nUpdated native format readers.".into(),
            download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
        });
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "00-update-pill",
            name,
            "A newer release is available: the Update pill in the header.",
        );
        for (found, case, update, width) in [
            (Some("0.28.9"), "current", false, 1120.0),
            (Some("0.29.0"), "newer", false, 1120.0),
            (Some("0.29.0"), "newer-update", true, 1120.0),
            (Some("0.17.9"), "too-old", false, 1120.0),
            (Some("0.24.0"), "older-history", false, 1120.0),
            (None, "none-found", false, 1120.0),
            (Some("0.29.0"), "narrow", true, 760.0),
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
                found
                    .and_then(dutchman_solidcraft::solidcraft_minor)
                    .unwrap_or_else(|| "not found".into())
            } else {
                found.map_or_else(
                    || "SolidCraft not found".into(),
                    |v| {
                        format!(
                            "SolidCraft {}",
                            dutchman_solidcraft::solidcraft_minor(v).unwrap()
                        )
                    },
                )
            };
            preview.get_by_label(&label).click_accesskit();
            preview.run();
            assert!(preview
                .query_by_label_contains(match case {
                    "newer-update" | "narrow" => "Update Dutchman",
                    "newer" => "is newer than this Dutchman knows",
                    "too-old" => "is too old to open Dutchman's designs",
                    "older-history" => "A newer SolidCraft keeps more of their history.",
                    "none-found" => "SolidCraft not found. Install SolidCraft",
                    _ => "SolidCraft 0.28 is installed.",
                })
                .is_some());
            let file = format!("00-solidcraft-{case}-{name}.png");
            preview.render().unwrap().save(dir.join(&file)).unwrap();
            index.push_str(&format!(
                "- [{file}]({file}) — SolidCraft version pill and panel: {case} ({width} px wide).\n"
            ));
        }
        harness.get_by_label("Update to 0.2.6").click_accesskit();
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "00-update-window",
            name,
            "The update window under the pill.",
        );
        for (installed, state, description) in [
            (
                false,
                "running",
                "Updating with Homebrew: progress and the output tail.",
            ),
            (true, "installed", "Homebrew finished: Restart Dutchman."),
        ] {
            harness.state_mut().updater.preview_homebrew(installed);
            harness.run_steps(4);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("00-update-homebrew-{state}"),
                name,
                description,
            );
        }
        harness.state_mut().updater.preview_scoop();
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "00-update-scoop-confirm",
            name,
            "Scoop: confirm closing Dutchman to update it.",
        );
        harness.state_mut().updater.open = false;
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "01-start",
            name,
            "Empty start window.",
        );
        harness.state_mut().add_inputs([input.clone()]);
        harness
            .state_mut()
            .set_out_dir(work.path().join(format!("Converted-{name}")));
        harness.state_mut().set_verify(false);
        wait_for_estimate(&mut harness);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "02-file-estimate",
            name,
            "Repository cube chosen, with its live estimate and output folder.",
        );
        for (number, previous, preset) in [
            ("03", "Standard", "Fast"),
            ("04", "Fast", "Standard"),
            ("05", "Standard", "Full"),
        ] {
            harness.get_by_value(previous).click_accesskit();
            harness.run();
            harness.get_by_label(preset).click_accesskit();
            wait_for_estimate(&mut harness);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("{number}-preset-{}", preset.to_lowercase()),
                name,
                &format!("{preset} preset selected, with refreshed estimate."),
            );
        }
        harness.get_by_label("Advanced").click_accesskit();
        harness.run();
        harness.get_by_label("Sketches").click_accesskit();
        wait_for_estimate(&mut harness);
        assert!(harness.query_by_value("Custom (based on Full)").is_some());
        shot(
            &mut harness,
            &dir,
            &mut index,
            "06-advanced-custom",
            name,
            "Advanced open; Sketches unchecked; Custom based on Full.",
        );
        // Collapse before conversion so the ordinary 800-pixel window shows more results.
        harness.get_by_label("Advanced").click_accesskit();
        harness.run();
        #[cfg(feature = "test-support")]
        harness.state_mut().show_memory_check_for_test(
            input.clone(),
            dutchman::choices::MemoryCheck::TooBig {
                estimate: dutchman::choices::Estimate {
                    peak_bytes: 8_000_000_000,
                    seconds: 120,
                    seconds_exact: 120.0,
                    band: 0.3,
                    memory_band: 0.3,
                    time_band: None,
                },
                available: 4_000_000_000,
                lighter: Some(dutchman::choices::Preset::Fast),
            },
        );
        harness.run();
        harness.get_by_label("This conversion may need more memory");
        shot(&mut harness, &dir, &mut index, "07-memory-dialog", name, "Injected TooBig memory decision: 8 GB estimated, 4 GB free, Use Fast / Convert anyway / Cancel.");
        harness.get_by_label("Cancel").click_accesskit();
        harness.run();

        harness.get_by_label("Convert").click_accesskit();
        // Do not settle frames here: the tiny cube may finish before the next frame.
        harness.step();
        // The click takes effect after the action bar is drawn. Paint its updated state.
        harness.step();
        let running = if harness.query_by_label("Stop").is_some() {
            "First frame after Convert for a real cube conversion; memory preflight / Stop is visible. The tiny fixture completes too quickly for a reliable mid-run progress frame."
        } else {
            "First frame after Convert for a real cube conversion; the tiny fixture completed before a running frame could be rendered."
        };
        shot(&mut harness, &dir, &mut index, "08-running", name, running);
        wait_for_conversion(&mut harness);
        assert_eq!(harness.state().rows().len(), 1);
        assert_eq!(harness.state().rows()[0].status, Status::Converted);
        harness.get_by_label("RESULTS").scroll_to_me();
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "09-done",
            name,
            "One successful conversion, report card and summary; scrolled to results.",
        );

        let mut harness = screenshot_harness(theme);
        harness.state_mut().add_inputs([failed_input.clone()]);
        harness
            .state_mut()
            .set_out_dir(work.path().join(format!("Failed-{name}")));
        harness.state_mut().set_verify(false);
        convert(&mut harness);
        assert_eq!(harness.state().rows()[0].status, Status::Failed);
        harness.get_by_label("RESULTS").scroll_to_me();
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "10-failed",
            name,
            "Repository metadata-only Fusion fixture fails conversion; report card explains why.",
        );

        let folder = dutchman::diagnostics::default_folder().unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let unfinished = folder.join("2026-10-29_201500.jsonl");
        std::fs::write(&unfinished, "{\"type\":\"start\",\"file\":\"f1\"}\n").unwrap();
        let mut harness = screenshot_harness(theme);
        harness.state_mut().add_inputs([input.clone()]);
        harness
            .state_mut()
            .set_out_dir(work.path().join(format!("Converted-{name}")));
        harness.run();
        harness.get_by_label("The last run didn't finish. Its diagnostic log is here:");
        shot(
            &mut harness,
            &dir,
            &mut index,
            "11-unfinished-log",
            name,
            "Unfinished-run banner detected from an isolated temporary default diagnostic log.",
        );
        harness
            .get_by_label("Save a diagnostic log (no names or contents from your file)")
            .scroll_to_me();
        harness.run();
        harness
            .get_by_label("Save a diagnostic log (no names or contents from your file)")
            .click_accesskit();
        harness.run();
        harness.get_by_label("Open diagnostics folder");
        shot(&mut harness, &dir, &mut index, "12-diagnostics", name, "Diagnostics checkbox on and Open diagnostics folder button; scrolled to show the controls.");
        harness
            .get_by_label("The last run didn't finish. Its diagnostic log is here:")
            .scroll_to_me();
        harness.run();
        harness.get_by_label("Dismiss").click_accesskit();
        harness.run();
        std::fs::remove_file(unfinished).unwrap();
        harness.get_by_label("OUTPUT FOLDER").scroll_to_me();
        harness.run();
        shot(&mut harness, &dir, &mut index, "13-settings", name, "Output folder, SolidCraft output and target, history checking, assembly project folder, and diagnostics settings; scrolled to the output section.");
        harness.get_by_label("Formats").click_accesskit();
        harness.run();
        harness.get_by_label("Fusion archive (.f3d)");
        shot(
            &mut harness,
            &dir,
            &mut index,
            "14-formats",
            name,
            "Formats menu open with reader status and capability levels.",
        );
    }
    index.push_str("\nMemory dialog uses the opt-in `test-support` hook; no oversized CAD file is allocated. All conversions use repository fixtures copied into a temporary folder. Native file/folder pickers and external folder/app launches are not invoked. Diagnostics are read from a temporary XDG_STATE_HOME in a child test process.\n");
    std::fs::write(dir.join("INDEX.md"), index).unwrap();
}

fn screenshot_harness(theme: ThemePreference) -> Harness<'static, App> {
    let harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .build_eframe(|cc| App::new(cc));
    harness.ctx.set_theme(theme);
    harness
}

fn shot(
    harness: &mut Harness<'static, App>,
    dir: &Path,
    index: &mut String,
    state: &str,
    theme: &str,
    description: &str,
) {
    let file = format!("{state}-{theme}.png");
    let image = harness.render().unwrap();
    assert_eq!(image.dimensions(), (1100, 800));
    use image::ImageEncoder;
    let output = std::fs::File::create(dir.join(&file)).unwrap();
    image::codecs::png::PngEncoder::new_with_quality(
        output,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(image.as_raw(), 1100, 800, image::ExtendedColorType::Rgba8)
    .unwrap();
    index.push_str(&format!("- [{file}]({file}) — {description}\n"));
}

fn wait_for_estimate(harness: &mut Harness<'static, App>) {
    harness.run();
    let started = Instant::now();
    while harness.query_all_by_label_contains("GB, ").count() == 0 {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "estimate did not arrive"
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.step();
    }
    harness.run();
}

fn wait_for_conversion(harness: &mut Harness<'static, App>) {
    let started = Instant::now();
    while harness.state().running() || harness.state().rows().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "conversion did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.step();
    }
    harness.run();
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
        (Some("0.29.0"), "is newer than this Dutchman knows"),
        (Some("0.17.9"), "is too old to open Dutchman's designs"),
        (
            Some("0.24.0"),
            "A newer SolidCraft keeps more of their history.",
        ),
        (Some("0.28.0"), "SolidCraft 0.28 is installed."),
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
            |v| {
                format!(
                    "SolidCraft {}",
                    dutchman_solidcraft::solidcraft_minor(v).unwrap()
                )
            },
        );
        harness.get_by_label(&label).click_accesskit();
        harness.run();
        assert!(harness.query_by_label_contains(expected).is_some());
        if found.is_none() || found == Some("0.17.9") {
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
        harness.get_by_label(label).click_accesskit();
        harness.run();
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_some());
        harness.get_by_label("Skip this version").click_accesskit();
        harness.run();
        assert!(harness.query_by_label(label).is_none());
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_none());
    }
}

#[test]
fn homebrew_job_survives_window_close_and_offers_restart_on_success() {
    let mut harness = harness();
    harness
        .state_mut()
        .updater
        .preview(dutchman_gui::update::Release {
            tag: "v0.2.6".into(),
            notes: "Release notes".into(),
            download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
        });
    harness.state_mut().updater.preview_homebrew(false);
    harness.run_steps(3);
    assert!(harness.query_by_label("Updating Dutchman…").is_some());
    harness.state_mut().updater.open = false;
    harness.run_steps(3);
    harness.get_by_label("Update").click_accesskit();
    harness.run_steps(3);
    assert!(harness.query_by_label("Updating Dutchman…").is_some());
    assert!(harness.query_by_label("Restart Dutchman").is_none());
    // While the update runs, offering the same command again would only invite running it twice.
    assert!(harness.query_by_label("Copy command").is_none());
    harness.state_mut().updater.preview_homebrew(true);
    harness.run_steps(3);
    assert!(harness
        .query_by_label("Dutchman 0.2.6 is installed")
        .is_some());
    assert!(harness.query_by_label("Restart Dutchman").is_some());
    assert!(harness.query_by_label("Update output").is_some());
    // After success only the result and Restart apply.
    for gone in ["Copy command", "Skip this version", "Remind me later"] {
        assert!(
            harness.query_by_label(gone).is_none(),
            "{gone} after success"
        );
    }
}

#[test]
fn scoop_confirmation_can_be_cancelled_without_starting_a_process() {
    let mut harness = harness();
    harness
        .state_mut()
        .updater
        .preview(dutchman_gui::update::Release {
            tag: "v0.2.6".into(),
            notes: "Release notes".into(),
            download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
        });
    harness.state_mut().updater.preview_scoop();
    harness.run();
    assert!(harness
        .query_by_label("Dutchman closes, Scoop updates it, and Dutchman opens again.")
        .is_some());
    assert!(harness.query_by_label("Update and restart").is_some());
    harness.get_by_label("Cancel").click_accesskit();
    harness.run();
    assert!(harness.query_by_label("Update and restart").is_none());
    harness.get_by_label("Update with Scoop").click_accesskit();
    harness.run();
    assert!(harness.query_by_label("Update and restart").is_some());
}

/// Public documentation images: real CC0 conversion, isolated paths, and both themes.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "renders README images with a GPU; set DUTCHMAN_GUI_SCREENSHOTS"]
fn readme_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    if std::env::var_os("DUTCHMAN_GUI_SCREENSHOT_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let designs = home.path().join("~/Designs");
        std::fs::create_dir_all(&designs).unwrap();
        std::fs::copy(
            fixture("design_history.FCStd"),
            designs.join("Bracket.FCStd"),
        )
        .unwrap();
        let tools = home.path().join("~/Applications/SolidCraft");
        std::fs::create_dir_all(&tools).unwrap();
        // Preserve the real check while keeping the machine's installation path private.
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            std::env::var_os("SOLIDCRAFT_CLI").unwrap(),
            tools.join("solidcraft-cli"),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "readme_screenshots", "--nocapture"])
            .current_dir(home.path())
            .env("DUTCHMAN_GUI_SCREENSHOT_CHILD", "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("SOLIDCRAFT_CLI", "~/Applications/SolidCraft/solidcraft-cli")
            .status()
            .unwrap();
        assert!(status.success(), "README screenshot child failed: {status}");
        return;
    }
    let mut index = String::new();
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        let mut harness = screenshot_harness(theme);
        harness.run();
        if name == "day" {
            shot(
                &mut harness,
                &dir,
                &mut index,
                "readme-drop",
                name,
                "Drop designs here.",
            );
        }
        harness
            .state_mut()
            .add_inputs([PathBuf::from("~/Designs/Bracket.FCStd")]);
        harness
            .state_mut()
            .set_out_dir(PathBuf::from("~/Designs/Converted"));
        wait_for_estimate(&mut harness);
        if name == "day" {
            shot(
                &mut harness,
                &dir,
                &mut index,
                "readme-preset",
                name,
                "Standard preset and live estimate.",
            );
        }
        convert(&mut harness);
        let row = &harness.state().rows()[0];
        assert_eq!(row.status, Status::Converted);
        let report = row.report.as_ref().unwrap();
        assert!(
            report["history"]["mapped"].as_u64().unwrap_or(0) > 0,
            "{report}"
        );
        harness.get_by_label("RESULTS").scroll_to_me();
        harness.run();
        shot(
            &mut harness,
            &dir,
            &mut index,
            "readme-result",
            name,
            "CC0 bracket with mapped feature history.",
        );
        std::fs::remove_dir_all("~/Designs/Converted").unwrap();
    }
}
