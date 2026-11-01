//! The app driven as a person would: designs added, an output folder set, Convert pressed, and
//! the results read back from the window.
//!
//! `screenshots` renders the window in both themes; it needs a GPU, so it is ignored by default:
//! `DUTCHMAN_GUI_SCREENSHOTS=<dir> cargo test -p dutchman-gui --features test-support
//! --test ui -- --ignored screenshots`. Linux isolates diagnostics with XDG_STATE_HOME.
//!
//! `--features test-support --test ui consent_e2e` drives consent against loopback 201/503
//! stubs. With `DUTCHMAN_GUI_SCREENSHOTS=<dir>`, it also renders day-theme evidence there
//! and eleven walkthrough frames in the sibling `walkthrough` directory. The ignored
//! `consent_e2e::consent_e2e_live` test requires `DUTCHMAN_E2E_ENDPOINT` and sends only a
//! synthetic fixture summary, marked with signature `e2e-test-desktop-consent-summary`.

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
    harness.run_steps(8);
    harness.get_by_label("Convert").click_accesskit();
    harness.step();
    wait_for_conversion(harness);
}

#[test]
fn the_window_offers_the_file_problem_form_and_names_its_version() {
    let mut harness = harness();
    harness.run_steps(8);
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
    harness.run_steps(8);
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
    harness.run_steps(8);
    harness.get_by_label("Formats").click_accesskit();
    harness.run_steps(8);
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
        .get_by_label("Report anonymously on GitHub")
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

/// A failed run of a file named `name` holding `bytes` bytes, in a window `width` wide. What
/// the file manager would show goes to `SHOWN`.
fn failed_card(name: &str, bytes: usize, width: f32) -> (Harness<'static, App>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join(name);
    std::fs::write(&broken, vec![b'x'; bytes]).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, 1100.0))
        .build_eframe(|cc| App::new(cc));
    harness.state_mut().set_file_manager(|path| {
        SHOWN.lock().unwrap().push(path.to_path_buf());
        Ok(())
    });
    harness.state_mut().add_inputs([broken]);
    harness.state_mut().set_out_dir(dir.path().join("out"));
    harness
        .state_mut()
        .set_diagnostic_log(dir.path().join("log.jsonl"));
    convert(&mut harness);
    assert_eq!(harness.state().rows()[0].status, Status::Failed);
    (harness, dir)
}

static SHOWN: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Presses the DM button: what it copied and opened.
fn press_dm(harness: &mut Harness<'static, App>) -> (String, String) {
    harness.get_by_label("Copy and open DM").click_accesskit();
    harness.step();
    let commands = &harness.output().platform_output.commands;
    let copied = commands.iter().find_map(|command| match command {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let opened = commands.iter().find_map(|command| match command {
        egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
        _ => None,
    });
    (copied.unwrap(), opened.unwrap())
}

#[test]
fn a_small_failed_file_is_dragged_into_the_dm_with_its_folder_open() {
    let (mut harness, dir) = failed_card("small-broken.f3d", 2048, 760.0);
    assert!(harness
        .query_by_label(
            "If this file opens in the app that made it, it's a bug in Dutchman. Help us fix it:"
        )
        .is_some());
    assert!(harness
        .query_by_label("Already in? Skip this step.")
        .is_some());
    assert!(harness
        .query_by_label("Drag the file in (its folder is open).")
        .is_some());
    assert!(harness.query_by_label("Have a STEP export?").is_some());
    let (message, opened) = press_dm(&mut harness);
    assert!(opened.starts_with("https://discord.com/users/"), "{opened}");
    assert!(message.contains("Size: 0.1 MB."), "{message}");
    assert!(message.ends_with("STEP: unknown."), "{message}");
    assert!(!message.contains("broken"), "{message}");
    assert!(!message.contains('\n'), "{message}");
    assert!(SHOWN
        .lock()
        .unwrap()
        .contains(&dir.path().join("small-broken.f3d")));

    // The card's id is the log's: the same run, the same salt.
    let log = std::fs::read_to_string(dir.path().join("log.jsonl")).unwrap();
    let first: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    let id = first["file"].as_str().unwrap();
    assert!(message.contains(&format!("Report id: {id}.")), "{message}");
}

#[test]
fn a_large_failed_file_takes_the_private_link_and_no_folder_opens() {
    let (mut harness, dir) = failed_card("large-broken.f3d", 11 << 20, 760.0);
    assert!(harness
        .query_by_label_contains("Dutchy replies with a private upload link (up to 300 MB")
        .is_some());
    assert!(harness
        .query_by_label_contains("Drag the file in")
        .is_none());
    let (message, _) = press_dm(&mut harness);
    assert!(message.contains("Size: 11 MB."), "{message}");
    assert!(!SHOWN
        .lock()
        .unwrap()
        .contains(&dir.path().join("large-broken.f3d")));
}

#[test]
fn the_step_cards_stand_side_by_side_and_wrap_when_narrow() {
    let (harness, _dir) = failed_card("wide.f3d", 10, 1000.0);
    let join = harness.get_by_label("Join our Discord server").rect();
    let dm = harness.get_by_label("DM Dutchy").rect();
    assert!((join.top() - dm.top()).abs() < 1.0 && join.right() < dm.left());
    let (harness, _dir) = failed_card("narrow.f3d", 10, 520.0);
    let join = harness.get_by_label("Join our Discord server").rect();
    let step = harness.get_by_label("Have a STEP export?").rect();
    assert!(step.top() > join.bottom());
}

#[test]
fn a_step_file_is_not_asked_for_a_step_export() {
    let (mut harness, _dir) = failed_card("broken.step", 2048, 760.0);
    assert!(harness.query_by_label("Have a STEP export?").is_none());
    let (message, _) = press_dm(&mut harness);
    assert!(message.ends_with("STEP: yes."), "{message}");
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
    harness.run_steps(8);
    let row = &harness.state().rows()[0];
    assert_eq!(row.status, Status::Converted);
    assert_eq!(row.job.as_ref().unwrap().search.also, [parts]);
    assert!(harness.query_by_label("1 converted, 0 failed").is_some());
}

#[test]
fn adding_a_design_selects_an_output_folder() {
    let mut harness = harness();
    harness.run_steps(8);
    assert!(harness.query_by_label("Add designs to convert.").is_some());
    harness
        .state_mut()
        .add_inputs([fixture("synthetic_cube.ipt")]);
    harness.run_steps(8);
    assert!(harness.query_by_label("Choose an output folder.").is_none());
    assert!(harness.query_by_label("Convert").is_some());
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
        for (case, size, description) in [
            (
                "small",
                4 << 20,
                "A failed file small enough to drag into the DM.",
            ),
            (
                "large",
                40 << 20,
                "A failed file over Discord's limit: the private link.",
            ),
        ] {
            let mut harness = screenshot_harness(theme);
            harness.state_mut().add_inputs([failed_input.clone()]);
            harness
                .state_mut()
                .set_out_dir(work.path().join(format!("card-{case}-{name}")));
            convert(&mut harness);
            harness.state_mut().preview_size(0, size);
            harness.run_steps(8);
            harness
                .get_by_label("Join our Discord server")
                .scroll_to_me();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("05-send-privately-{case}"),
                name,
                description,
            );
        }
    }
    for (theme, name) in [
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ] {
        let mut harness = screenshot_harness(theme);
        harness.state_mut().updater.preview(dutchman_gui::update::Release {
            fixes_reports: Vec::new(),
            tag: "v0.2.6".into(),
            notes: "Dutchman 0.2.6\n\nImproved desktop conversion progress and clearer reports.\nUpdated native format readers.".into(),
            download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
        });
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "00-update-pill",
            name,
            "A newer release is available: the Update pill in the header.",
        );
        for (found, case, update, width) in [
            (Some("0.29.9"), "current", false, 1120.0),
            (Some("0.30.0"), "newer", false, 1120.0),
            (Some("0.30.0"), "newer-update", true, 1120.0),
            (Some("0.17.9"), "too-old", false, 1120.0),
            (Some("0.24.0"), "older-history", false, 1120.0),
            (None, "none-found", false, 1120.0),
            (Some("0.30.0"), "narrow", true, 760.0),
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
                        fixes_reports: Vec::new(),
                        tag: "v0.2.6".into(),
                        notes: "Release notes".into(),
                        download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6"
                            .into(),
                    });
            }
            preview.run_steps(8);
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
            preview.run_steps(8);
            assert!(preview
                .query_by_label_contains(match case {
                    "newer-update" | "narrow" => "Update Dutchman",
                    "newer" => "is newer than this Dutchman knows",
                    "too-old" => "is too old to open Dutchman's designs",
                    "older-history" => "A newer SolidCraft keeps more of their history.",
                    "none-found" => "SolidCraft not found. Install SolidCraft",
                    _ => "SolidCraft 0.29 is installed.",
                })
                .is_some());
            let file = format!("00-solidcraft-{case}-{name}.png");
            preview.render().unwrap().save(dir.join(&file)).unwrap();
            index.push_str(&format!(
                "- [{file}]({file}) — SolidCraft version pill and panel: {case} ({width} px wide).\n"
            ));
        }
        harness.get_by_label("Update to 0.2.6").click_accesskit();
        harness.run_steps(8);
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
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "00-update-scoop-confirm",
            name,
            "Scoop: confirm closing Dutchman to update it.",
        );
        harness.state_mut().updater.open = false;
        harness.run_steps(8);
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
            harness.run_steps(8);
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
        harness.run_steps(8);
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
        harness.run_steps(8);
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
        harness.run_steps(8);
        harness.get_by_label("This conversion may need more memory");
        shot(&mut harness, &dir, &mut index, "07-memory-dialog", name, "Injected TooBig memory decision: 8 GB estimated, 4 GB free, Use Fast / Convert anyway / Cancel.");
        harness.get_by_label("Cancel").click_accesskit();
        harness.run_steps(8);

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
        harness.run_steps(8);
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
        harness.run_steps(8);
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
        harness.run_steps(8);
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
        harness.run_steps(8);
        harness
            .get_by_label("Save a diagnostic log (no names or contents from your file)")
            .click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Open diagnostics folder");
        shot(&mut harness, &dir, &mut index, "12-diagnostics", name, "Diagnostics checkbox on and Open diagnostics folder button; scrolled to show the controls.");
        harness
            .get_by_label("The last run didn't finish. Its diagnostic log is here:")
            .scroll_to_me();
        harness.run_steps(8);
        harness.get_by_label("Dismiss").click_accesskit();
        harness.run_steps(8);
        std::fs::remove_file(unfinished).unwrap();
        harness.get_by_label("OUTPUT FOLDER").scroll_to_me();
        harness.run_steps(8);
        shot(&mut harness, &dir, &mut index, "13-settings", name, "Output folder, SolidCraft output and target, history checking, assembly project folder, and diagnostics settings; scrolled to the output section.");
        harness.get_by_label("Formats").click_accesskit();
        harness.run_steps(8);
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
    harness.run_steps(8);
    let started = Instant::now();
    while harness.query_all_by_label_contains("GB, ").count() == 0 {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "estimate did not arrive"
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.step();
    }
    harness.run_steps(8);
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
    harness.run_steps(8);
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
    harness.run_steps(8);
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
        (Some("0.30.0"), "is newer than this Dutchman knows"),
        (Some("0.17.9"), "is too old to open Dutchman's designs"),
        (
            Some("0.24.0"),
            "A newer SolidCraft keeps more of their history.",
        ),
        (Some("0.29.0"), "SolidCraft 0.29 is installed."),
        (None, "SolidCraft not found. Install SolidCraft"),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1120.0, 680.0))
            .build_eframe(|cc| App::new(cc));
        harness.state_mut().preview_solidcraft(found);
        harness.run_steps(8);
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
        harness.run_steps(8);
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
                fixes_reports: Vec::new(),
                tag: "v0.2.6".into(),
                notes: "Release notes".into(),
                download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
            });
        harness.run_steps(8);
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_none());
        harness.get_by_label(label).click_accesskit();
        harness.run_steps(8);
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_some());
        harness.get_by_label("Skip this version").click_accesskit();
        harness.run_steps(8);
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
            fixes_reports: Vec::new(),
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
            fixes_reports: Vec::new(),
            tag: "v0.2.6".into(),
            notes: "Release notes".into(),
            download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
        });
    harness.state_mut().updater.preview_scoop();
    harness.run_steps(8);
    assert!(harness
        .query_by_label("Dutchman closes, Scoop updates it, and Dutchman opens again.")
        .is_some());
    assert!(harness.query_by_label("Update and restart").is_some());
    harness.get_by_label("Cancel").click_accesskit();
    harness.run_steps(8);
    assert!(harness.query_by_label("Update and restart").is_none());
    harness.get_by_label("Update with Scoop").click_accesskit();
    harness.run_steps(8);
    assert!(harness.query_by_label("Update and restart").is_some());
}

/// Public documentation images: real CC0 conversion, isolated paths, and both themes.
#[cfg(all(target_os = "linux", feature = "freecad"))]
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
        harness.run_steps(8);
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
        harness.run_steps(8);
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

#[test]
fn solidworks_folder_shows_the_plan_and_hides_inventor_settings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("cube.sldprt"), b"synthetic").unwrap();
    std::fs::write(dir.path().join("main.sldasm"), b"synthetic").unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 1200.0))
        .build_eframe(|cc| App::new(cc));
    harness.state_mut().add_inputs([dir.path().to_path_buf()]);
    let started = Instant::now();
    loop {
        harness.run_steps(2);
        if harness
            .query_all_by_label_contains("SolidWorks assemblies not read yet")
            .count()
            > 0
        {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        harness
            .query_all_by_label_contains("Inventor project folder:")
            .count(),
        0
    );
    assert!(harness.query_by_label("Choose an output folder.").is_none());
}

#[test]
#[ignore = "renders a supplied project zip with a GPU; set DUTCHMAN_GUI_SCREENSHOTS and DUTCHMAN_PROJECT_ZIP"]
fn project_zip_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    let input = PathBuf::from(std::env::var_os("DUTCHMAN_PROJECT_ZIP").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut harness = screenshot_harness(ThemePreference::Light);
    let expected_plan =
        dutchman::batch::plan(std::slice::from_ref(&input), Path::new("")).description;
    harness.state_mut().add_inputs([input]);
    harness.state_mut().set_verify(false);
    let started = Instant::now();
    loop {
        harness.run_steps(2);
        if harness.query_by_label(&expected_plan).is_some() {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut index = String::new();
    shot(
        &mut harness,
        &dir,
        &mut index,
        "project-added",
        "day",
        "Project plan before conversion.",
    );
    harness.state_mut().set_out_dir(dir.join("converted"));
    harness.run_steps(2);
    harness.get_by_label("Convert").click_accesskit();
    harness.step();
    let started = Instant::now();
    let mut reported = None;
    while harness.state().running() || harness.state().rows().is_empty() {
        if harness.query_by_label("Use Fast").is_some() {
            eprintln!("Using Fast for the project memory check");
            harness.get_by_label("Use Fast").click_accesskit();
            harness.step();
        }
        assert!(
            harness.query_by_label("Convert anyway").is_none(),
            "project exceeds the available memory for this screenshot run"
        );
        let rows = harness.state().rows();
        let completed = rows
            .iter()
            .filter(|row| matches!(row.status, Status::Converted | Status::Failed))
            .count();
        let counts = (completed, rows.len());
        if reported != Some(counts) {
            eprintln!("Project conversion: {completed} of {} finished", rows.len());
            reported = Some(counts);
        }
        assert!(
            started.elapsed() < Duration::from_secs(3600),
            "project conversion did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.run_steps(2);
    }
    harness.get_by_label("RESULTS").scroll_to_me();
    harness.run_steps(8);
    shot(
        &mut harness,
        &dir,
        &mut index,
        "project-converted",
        "day",
        "Project conversion results.",
    );
    std::fs::write(dir.join("project-screenshots.md"), index).unwrap();
}

#[cfg(feature = "test-support")]
mod consent_e2e {
    use super::*;
    use egui_kittest::kittest::NodeT;
    use serde_json::{json, Value};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{channel, Receiver, Sender};

    const SENT: &str = "Sent. The report was received.";

    struct Stub {
        endpoint: String,
        requests: Receiver<Value>,
        release: Sender<()>,
        thread: std::thread::JoinHandle<()>,
    }

    impl Stub {
        fn new(status: u16) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/api/reports", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let (tx, requests) = channel();
            let (release, gate) = channel();
            let thread = std::thread::spawn(move || {
                for attempt in 0..if status == 503 { 2 } else { 1 } {
                    let started = Instant::now();
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(started.elapsed() < Duration::from_secs(30));
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            Err(e) => panic!("{e}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let (header_end, size) = loop {
                        let mut chunk = [0; 4096];
                        let n = stream.read(&mut chunk).unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&chunk[..n]);
                        assert!(bytes.len() < 32 * 1024);
                        if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                            let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                            assert!(headers.starts_with("POST /api/reports HTTP/1.1"));
                            let size = headers
                                .lines()
                                .find_map(|line| {
                                    let (key, value) = line.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap();
                            break (end + 4, size);
                        }
                    };
                    while bytes.len() < header_end + size {
                        let mut chunk = [0; 4096];
                        let n = stream.read(&mut chunk).unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&chunk[..n]);
                        assert!(bytes.len() < 32 * 1024);
                    }
                    let body = std::str::from_utf8(&bytes[header_end..]).unwrap();
                    assert!(body.contains("name=\"report\""));
                    assert!(!body.contains("name=\"bundle\""));
                    assert!(!body.contains("container_metadata_only"));
                    let metadata = body
                        .split_once("\r\n\r\n")
                        .unwrap()
                        .1
                        .split("\r\n--")
                        .next()
                        .unwrap();
                    let report: Value = serde_json::from_str(metadata).unwrap();
                    tx.send(report.clone()).unwrap();
                    if attempt == 0 {
                        gate.recv_timeout(Duration::from_secs(20)).unwrap();
                    }
                    let receipt = if status == 201 {
                        json!({"report_id": report["report_id"], "received": 1793534400u64})
                    } else {
                        json!({"error": "Storage full"})
                    }
                    .to_string();
                    write!(stream, "HTTP/1.1 {status} Stub\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{receipt}", receipt.len()).unwrap();
                }
            });
            Self {
                endpoint,
                requests,
                release,
                thread,
            }
        }
    }

    fn review(
        endpoint: String,
        signature: Option<String>,
        capture_walkthrough: bool,
    ) -> (Harness<'static, App>, tempfile::TempDir, PathBuf) {
        let out = tempfile::tempdir().unwrap();
        let mut harness = screenshot_harness(ThemePreference::Light);
        harness
            .state_mut()
            .set_report_transport_for_test(endpoint, signature);
        let input = out.path().join("container_metadata_only.f3d");
        std::fs::copy(fixture("container_metadata_only.f3d"), &input).unwrap();
        harness.state_mut().add_inputs([input]);
        harness.state_mut().set_out_dir(out.path().to_path_buf());
        harness.state_mut().set_verify(false);
        harness
            .state_mut()
            .set_diagnostic_log(out.path().join("diagnostics.jsonl"));
        convert(&mut harness);
        assert_eq!(harness.state().rows()[0].status, Status::Failed);
        harness.get_by_label("RESULTS").scroll_to_me();
        harness.run_steps(8);
        assert!(harness.query_by_label("0 converted, 1 failed").is_some());
        let diagnose = harness.get_by_label("Diagnose this file").rect();
        assert!(diagnose.top() > 70.0 && diagnose.bottom() < 730.0);
        if capture_walkthrough {
            walkthrough(&mut harness, "01-failed-card");
        }
        harness
            .get_by_label("Review and send report")
            .click_accesskit();
        harness.run_steps(8);
        harness
            .get_by_label("Send the whole file privately")
            .click_accesskit();
        harness.run_steps(8);
        assert!(harness.query_by_label("Send privately").is_some());
        if capture_walkthrough {
            walkthrough(&mut harness, "02-private-send-card");
        }
        // The private card and the row both offer review; select the window's button.
        harness
            .get_by_label("Send privately")
            .get_by_label("Review and send report")
            .click_accesskit();
        harness.run_steps(8);
        assert!(harness.query_by_label("Send a problem report").is_some());
        assert!(harness.query_by_label(SENT).is_none());
        assert!(harness.query_by_label("Send the bundle").is_none());
        assert!(harness.query_by_label("Retry").is_none());
        assert!(harness.query_by_label_contains("JSON").is_none());
        assert!(harness.query_by_label_contains("claim hash").is_none());
        assert_eq!(
            harness
                .get_by_label("You may use this report in a public test case")
                .accesskit_node()
                .toggled(),
            Some(egui::accesskit::Toggled::False)
        );
        assert!(!harness.get_by_label("Send").accesskit_node().is_disabled());
        let folder = std::fs::read_dir(out.path().join("reports"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(folder.join("prepared.json").is_file());
        assert!(!folder.join("submission.json").exists());
        harness
            .get_by_label("Anonymous summary only")
            .click_accesskit();
        harness.run_steps(3);
        assert!(!harness.get_by_label("Send").accesskit_node().is_disabled());
        if capture_walkthrough {
            walkthrough(&mut harness, "03-consent-screen");
        }
        (harness, out, folder)
    }

    fn walkthrough(harness: &mut Harness<'static, App>, name: &str) {
        if let Some(dir) = std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS") {
            let dir = PathBuf::from(dir).parent().unwrap().join("walkthrough");
            std::fs::create_dir_all(&dir).unwrap();
            let image = harness.render().unwrap();
            assert_eq!(image.dimensions(), (1100, 800));
            image.save(dir.join(format!("{name}.png"))).unwrap();
        }
    }

    fn snapshot(harness: &mut Harness<'static, App>, name: &str) {
        if let Some(dir) = std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            shot(
                harness,
                &dir,
                &mut String::new(),
                name,
                "day",
                "Consent E2E",
            );
        }
    }

    fn wait_for(harness: &mut Harness<'static, App>, label: &str) {
        let started = Instant::now();
        loop {
            harness.run_steps(2);
            if harness.query_all_by_label_contains(label).count() > 0 {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "waiting for {label}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        harness.run_steps(8);
    }

    fn local_case(status: u16) {
        let stub = Stub::new(status);
        let (mut harness, _out, folder) = review(stub.endpoint, None, status == 201);
        assert!(stub.requests.try_recv().is_err());
        snapshot(&mut harness, &format!("consent-{status}-before"));
        harness.get_by_label("Send").click_accesskit();
        harness.run_steps(3);
        let report = stub.requests.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            report["consent"],
            json!({"choice": "summary", "text_version": 1, "public_test": false})
        );
        assert_eq!(report["source_format"], "f3d");
        assert_eq!(report["source_app"], "Fusion");
        assert_eq!(report["dutchman_version"], dutchman::BUILD_VERSION);
        assert_eq!(report["claim_hash"].as_str().unwrap().len(), 64);
        assert_eq!(
            serde_json::from_slice::<Value>(
                &std::fs::read(folder.join("submission.json")).unwrap()
            )
            .unwrap(),
            report
        );
        assert!(harness.query_by_label("Sending…").is_some());
        assert!(harness.query_by_label(SENT).is_none());
        assert!(!folder.join("receipt.json").exists());
        assert!(harness.get_by_label("Send").accesskit_node().is_disabled());
        if status == 201 {
            walkthrough(&mut harness, "04-sending");
        }
        stub.release.send(()).unwrap();
        if status == 201 {
            // Observe every frame: the UI must never say sent before the durable receipt.
            let started = Instant::now();
            loop {
                harness.step();
                if harness.query_by_label(SENT).is_some() {
                    assert!(folder.join("receipt.json").is_file());
                    break;
                }
                assert!(started.elapsed() < Duration::from_secs(20));
                std::thread::sleep(Duration::from_millis(10));
            }
            let receipt: Value =
                serde_json::from_slice(&std::fs::read(folder.join("receipt.json")).unwrap())
                    .unwrap();
            assert_eq!(
                receipt,
                json!({"report_id": report["report_id"], "received": 1793534400u64})
            );
            harness.run_steps(8);
            assert!(harness.get_by_label("Send").accesskit_node().is_disabled());
            assert!(harness.query_by_label("Retry").is_none());
        } else {
            wait_for(&mut harness, "It did not go through.");
            assert!(harness.query_by_label("Send").is_none());
            assert_eq!(
                stub.requests.recv_timeout(Duration::from_secs(5)).unwrap(),
                report
            );
            assert!(!folder.join("receipt.json").exists());
            assert!(folder.join("prepared.json").is_file());
            assert!(folder.join("submission.json").is_file());
            assert!(harness.query_by_label(SENT).is_none());
            assert!(!harness.get_by_label("Retry").accesskit_node().is_disabled());
            assert!(harness
                .query_by_label("Send the whole file privately")
                .is_some());
            assert!(harness
                .query_by_label("Open GitHub with anonymous summary only")
                .is_some());
        }
        snapshot(&mut harness, &format!("consent-{status}-after"));
        stub.thread.join().unwrap();
        if status == 201 {
            walkthrough(&mut harness, "05-sent-receipt");
            walkthrough_extras(&mut harness, &report);
        } else {
            walkthrough(&mut harness, "06-send-failed-fallbacks");
        }
    }

    fn walkthrough_extras(harness: &mut Harness<'static, App>, report: &Value) {
        harness
            .get_by_label("Open GitHub with anonymous summary only")
            .click_accesskit();
        harness.step();
        let url = harness
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| {
                if let egui::OutputCommand::OpenUrl(open) = command {
                    Some(open.url.clone())
                } else {
                    None
                }
            })
            .unwrap();
        assert!(url.starts_with(dutchman_gui::REPORT_PROBLEM_URL));
        assert!(url.len() < 2000);
        harness.run_steps(8);
        assert!(harness
            .query_by_label(dutchman::present::REPORT_COPIED)
            .is_some());
        walkthrough(harness, "07-github-short-link");
        harness.get_by_label("Diagnose this file").click_accesskit();
        harness.run_steps(8);
        assert!(harness
            .query_by_label("Works locally. Keep the original and local-key.json private.")
            .is_some());
        assert!(harness
            .query_by_label(dutchman::present::REPORT_COPIED)
            .is_none());
        walkthrough(harness, "08-diagnose-wizard");
        let started = Instant::now();
        while harness.get_by_label("Close").accesskit_node().is_disabled() {
            assert!(started.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(20));
            harness.run_steps(2);
        }
        harness.get_by_label("Close").click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Add matching STEP").click_accesskit();
        harness.run_steps(8);
        assert!(harness.query_by_label("Part to check").is_some());
        walkthrough(harness, "09-add-matching-step");
        harness.get_by_label("Back to conversion").click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Check a folder").scroll_to_me();
        harness.run_steps(8);
        harness.get_by_label("Check a folder").click_accesskit();
        harness.run_steps(8);
        assert!(harness.query_by_label("Choose folder…").is_some());
        harness.get_by_label(dutchman_gui::APP_NAME);
        walkthrough(harness, "10-check-a-folder");
        harness.get_by_label("Back to conversion").click_accesskit();
        harness.run_steps(8);
        harness
            .state_mut()
            .updater
            .preview(dutchman_gui::update::Release {
                fixes_reports: vec![dutchman::report_send::hash(
                    report["report_id"].as_str().unwrap().as_bytes(),
                )],
                tag: "v99.0.0".into(),
                notes: "Synthetic release for desktop report testing.".into(),
                download: "https://github.com/dutchman-cad/dutchman/releases/tag/v99.0.0".into(),
            });
        harness.run_steps(8);
        harness.get_by_label("Update to 99.0.0").click_accesskit();
        harness.run_steps(8);
        assert!(harness
            .query_by_label_contains("This update fixes a problem you reported")
            .is_some());
        walkthrough(harness, "11-update-fixes-your-report");
    }

    #[test]
    fn consent_e2e_201_receipt() {
        local_case(201);
    }

    #[test]
    fn consent_e2e_503_keeps_report_and_fallbacks() {
        local_case(503);
    }

    #[test]
    #[ignore = "sends a synthetic summary to DUTCHMAN_E2E_ENDPOINT after UI consent"]
    fn consent_e2e_live() {
        let endpoint = std::env::var("DUTCHMAN_E2E_ENDPOINT").unwrap();
        let (mut harness, _out, folder) = review(
            endpoint,
            Some("e2e-test-desktop-consent-summary".into()),
            false,
        );
        snapshot(&mut harness, "consent-live-before");
        harness.get_by_label("Send").click_accesskit();
        harness.step();
        wait_for(&mut harness, SENT);
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(folder.join("receipt.json")).unwrap()).unwrap();
        assert!(receipt["received"].as_u64().is_some());
        let saved: Value =
            serde_json::from_slice(&std::fs::read(folder.join("submission.json")).unwrap())
                .unwrap();
        assert_eq!(saved["signature"], "e2e-test-desktop-consent-summary");
        assert_eq!(receipt["report_id"], saved["report_id"]);
        eprintln!("Live receipt: {receipt}");
        snapshot(&mut harness, "consent-live-after");
    }
}
