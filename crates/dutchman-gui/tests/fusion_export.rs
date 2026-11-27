//! "Export from Fusion" driven as a person would: the window opened, designs listed and
//! picked, an export followed to its end, and the finished folder converted. A fake exporter
//! add-in answers through files, exactly as the job protocol describes.
//!
//! `DUTCHMAN_GUI_SCREENSHOTS=<dir> cargo test -p dutchman-gui --features test-support
//! --test fusion_export -- --ignored fusion_export_screenshots` renders the screens in
//! English and, with long German names from the fake account (the app has no translations
//! yet), in Day and Night.
#![cfg(feature = "test-support")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use dutchman_gui::fusion_export::fake::{sample_inventory, Ending, FakeAddin, Service, FEATURES};
use dutchman_gui::fusion_export::{Env, Stage};
use dutchman_gui::App;
use eframe::egui::{self, ThemePreference};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn test_app(cc: &eframe::CreationContext<'_>) -> App {
    dutchman_gui::worker::convert_in_process_for_tests();
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let folder = FOLDER.get_or_init(|| {
        std::env::temp_dir().join(format!(
            "dutchman-gui-fx-diagnostics-{}",
            std::process::id()
        ))
    });
    dutchman_gui::set_diagnostics_folder(folder.clone());
    App::new(cc)
}

struct Fixture {
    _dir: tempfile::TempDir,
    addin: FakeAddin,
    service: Service,
    harness: Harness<'static, App>,
}

fn fixture(german: bool, ending: Ending, theme: ThemePreference) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let addin = FakeAddin::new(&dir.path().join(".cad-export"));
    addin.announce(FEATURES, None);
    let service = Service::spawn(
        addin.clone(),
        sample_inventory(german),
        ending,
        Duration::from_millis(20),
    );
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    harness.ctx.set_theme(theme);
    harness
        .state_mut()
        .preview_fusion_export(Env::for_test(dir.path()));
    harness.run_steps(8);
    Fixture {
        _dir: dir,
        addin,
        service,
        harness,
    }
}

fn until(
    h: &mut Harness<'static, App>,
    what: &str,
    mut ready: impl FnMut(&mut Harness<'static, App>) -> bool,
) {
    let started = Instant::now();
    while !ready(h) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    h.run_steps(3);
}

fn open_picker(f: &mut Fixture) {
    f.harness.get_by_label("Export from Fusion…").click();
    f.harness.run_steps(3);
    until(&mut f.harness, "the picker", |h| {
        h.state().fusion_export_stage() == Stage::Picking
    });
}

#[test]
fn the_entry_is_only_offered_where_fusion_runs_or_in_a_test() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    h.run_steps(8);
    assert_eq!(
        h.query_by_label("Export from Fusion…").is_some(),
        cfg!(any(windows, target_os = "macos"))
    );
    h.state_mut()
        .preview_fusion_export(Env::for_test(dir.path()));
    h.run_steps(4);
    assert!(h.query_by_label("Export from Fusion…").is_some());
}

#[test]
fn without_the_exporter_the_window_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    h.state_mut()
        .preview_fusion_export(Env::for_test(dir.path()));
    h.run_steps(4);
    h.get_by_label("Export from Fusion…").click();
    h.run_steps(5);
    assert_eq!(h.state().fusion_export_stage(), Stage::NoAgent);
    assert!(h
        .query_by_label_contains("Waiting for the Fusion exporter")
        .is_some());
}

#[test]
fn picking_designs_exporting_them_and_converting_the_folder() {
    let mut f = fixture(false, Ending::done(), ThemePreference::Light);
    open_picker(&mut f);
    f.harness.get_by_label("Gearbox 7 (3)").click();
    f.harness.run_steps(3);
    f.harness.get_by_label("Housings (2)").click();
    f.harness.run_steps(3);
    f.harness.get_by_label("Housing top").click();
    f.harness.run_steps(3);
    f.harness.get_by_label("Export 1 design").click();
    f.harness.run_steps(2);
    // The finished export becomes an input of the normal conversion.
    until(&mut f.harness, "the export folder as an input", |h| {
        !h.state().inputs_for_test().is_empty()
    });
    let input = f.harness.state().inputs_for_test()[0].clone();
    assert!(
        input.starts_with(f._dir.path().join("Dutchman Fusion exports/exports")),
        "{input:?}"
    );
    assert_eq!(f.service.exports_seen.load(Ordering::SeqCst), 1);
    assert!(std::fs::read_dir(f.addin.jobs_dir())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn the_search_narrows_the_designs() {
    let mut f = fixture(false, Ending::done(), ThemePreference::Light);
    open_picker(&mut f);
    f.harness.get_by_label("Search").click();
    f.harness.get_by_label("Search").type_text("seal");
    f.harness.run_steps(4);
    assert!(f.harness.query_by_label("Seal holder").is_some());
    assert!(f.harness.query_by_label("Gear set").is_none());
    assert!(f
        .harness
        .query_by_label_contains("1 of 4 designs shown")
        .is_some());
}

#[test]
fn a_busy_folder_is_said_in_plain_words_with_try_again() {
    let mut f = fixture(
        false,
        Ending::failed_with("folder_busy"),
        ThemePreference::Light,
    );
    open_picker(&mut f);
    f.harness.get_by_label("Select all shown").click();
    f.harness.run_steps(2);
    f.harness.get_by_label("Export 4 designs").click();
    until(&mut f.harness, "the failure", |h| {
        h.state().fusion_export_stage() == Stage::Finished
    });
    assert!(f
        .harness
        .query_by_label_contains("Another export is using this export folder")
        .is_some());
    assert!(f.harness.query_by_label("Try again").is_some());
}

#[test]
fn failed_designs_are_counted_and_try_again_is_offered() {
    let mut ending = Ending::done();
    ending.failed = 2;
    let mut f = fixture(false, ending, ThemePreference::Light);
    open_picker(&mut f);
    f.harness.get_by_label("Select all shown").click();
    f.harness.run_steps(2);
    f.harness.get_by_label("Export 4 designs").click();
    until(&mut f.harness, "the end", |h| {
        h.state().fusion_export_stage() == Stage::Finished
    });
    assert!(f
        .harness
        .query_by_label_contains("2 of your designs failed to export; 3 exported.")
        .is_some());
    assert!(f.harness.query_by_label("Try again").is_some());
    assert!(f.harness.query_by_label("Convert what exported").is_some());
}

#[test]
fn stop_cancels_and_offers_what_was_exported() {
    let mut f = fixture(false, Ending::done(), ThemePreference::Light);
    f.service.hold_at.store(2, Ordering::SeqCst);
    f.service.hold.store(true, Ordering::SeqCst);
    open_picker(&mut f);
    f.harness.get_by_label("Select all shown").click();
    f.harness.run_steps(2);
    f.harness.get_by_label("Export 4 designs").click();
    until(&mut f.harness, "progress", |h| {
        h.query_by_label_contains("Exporting design 2 of 5")
            .is_some()
    });
    f.harness.get_by_label("Stop").click();
    until(&mut f.harness, "the stop", |h| {
        h.state().fusion_export_stage() == Stage::Finished
    });
    assert!(f
        .harness
        .query_by_label_contains("The export was stopped")
        .is_some());
    assert!(f.harness.query_by_label("Convert what exported").is_some());
}

fn save(h: &mut Harness<'static, App>, dir: &Path, file: &str) {
    let image = h.render().unwrap();
    let (width, height) = image.dimensions();
    assert!(
        height <= width,
        "screenshot {file} is {width}x{height}: render at a landscape size such as 1280x800"
    );
    image.save(dir.join(file)).unwrap();
}

#[test]
#[ignore = "renders real GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn fusion_export_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    for (language, german) in [("english", false), ("german", true)] {
        let (project, folder, design) = if german {
            (
                "Getriebegehäuse Baureihe 7 (3)",
                "Gehäuseteile für Straßenfahrzeuge (2)",
                "Lagerdeckel mit Befestigungsflansch Rückseite",
            )
        } else {
            ("Gearbox 7 (3)", "Housings (2)", "Bearing cap")
        };
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            // The picker, with a project and folder opened and two designs ticked.
            let mut f = fixture(german, Ending::done(), theme);
            open_picker(&mut f);
            f.harness.get_by_label(project).click();
            f.harness.run_steps(3);
            f.harness.get_by_label(folder).click();
            f.harness.run_steps(3);
            f.harness.get_by_label(design).click();
            f.harness.run_steps(3);
            save(
                &mut f.harness,
                &dir,
                &format!("fusion-picker-{language}-{name}.png"),
            );

            // Progress, held part way.
            let mut f = fixture(german, Ending::done(), theme);
            f.service.hold_at.store(2, Ordering::SeqCst);
            f.service.hold.store(true, Ordering::SeqCst);
            open_picker(&mut f);
            f.harness.get_by_label("Select all shown").click();
            f.harness.run_steps(2);
            f.harness.get_by_label("Export 4 designs").click();
            until(&mut f.harness, "progress", |h| {
                h.query_by_label_contains("Exporting design 2 of 5")
                    .is_some()
            });
            save(
                &mut f.harness,
                &dir,
                &format!("fusion-progress-{language}-{name}.png"),
            );
            f.service.hold.store(false, Ordering::SeqCst);

            // Done, with a failed design.
            let mut ending = Ending::done();
            ending.failed = 1;
            let mut f = fixture(german, ending, theme);
            open_picker(&mut f);
            f.harness.get_by_label("Select all shown").click();
            f.harness.run_steps(2);
            f.harness.get_by_label("Export 4 designs").click();
            until(&mut f.harness, "the end", |h| {
                h.state().fusion_export_stage() == Stage::Finished
            });
            save(
                &mut f.harness,
                &dir,
                &format!("fusion-done-failed-{language}-{name}.png"),
            );

            // Another export holds the folder.
            let mut f = fixture(german, Ending::failed_with("folder_busy"), theme);
            open_picker(&mut f);
            f.harness.get_by_label("Select all shown").click();
            f.harness.run_steps(2);
            f.harness.get_by_label("Export 4 designs").click();
            until(&mut f.harness, "the failure", |h| {
                h.state().fusion_export_stage() == Stage::Finished
            });
            save(
                &mut f.harness,
                &dir,
                &format!("fusion-folder-busy-{language}-{name}.png"),
            );
        }
    }
}
