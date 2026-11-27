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

/// An app whose run logs, and the unfinished-run notice, use a folder of this test process,
/// never the real diagnostics folder (CI runs these tests without the test-support feature).
fn test_app(cc: &eframe::CreationContext<'_>) -> App {
    dutchman_gui::worker::convert_in_process_for_tests();
    static FOLDER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    let folder = FOLDER.get_or_init(|| {
        std::env::temp_dir().join(format!(
            "dutchman-gui-ui-diagnostics-{}",
            std::process::id()
        ))
    });
    dutchman_gui::set_diagnostics_folder(folder.clone());
    let mut app = App::new(cc);
    // Windows and macOS offer "Export from Fusion…"; the layout tests see it on Linux too.
    app.offer_everywhere_for_tests();
    app
}
use eframe::egui::{self, ThemePreference};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

#[cfg(feature = "test-support")]
#[path = "support/losses.rs"]
mod loss_fixture;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

fn harness() -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(760.0, 680.0))
        .build_eframe(|cc| test_app(cc))
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

fn header_harness(width: f32, theme: ThemePreference, update: bool) -> Harness<'static, App> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(width, 680.0))
        .build_eframe(|cc| test_app(cc));
    h.ctx.set_theme(theme);
    h.state_mut().preview_solidcraft(Some("0.50.9"));
    if update {
        h.state_mut()
            .updater
            .preview(dutchman_gui::update::Release {
                fixes_reports: Vec::new(),
                tag: "v0.2.6".into(),
                notes: "Release notes".into(),
                download: "https://github.com/dutchman-cad/dutchman/releases/tag/v0.2.6".into(),
            });
    }
    h.run_steps(8);
    h
}

fn assert_header_fits(h: &Harness<'_, App>, width: f32, update: bool, theme: &str) {
    let name = h.get_by_label("Dutchman Convert").rect();
    let version = h
        .get_by_label(&format!("v{}", dutchman::BUILD_VERSION))
        .rect();
    assert!(version.left() >= name.right());
    let mut title = name.union(version);
    if let Some(subtitle) = h.query_by_label("Converts native CAD designs into SolidCraft designs.")
    {
        title = title.union(subtitle.rect());
    }
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 680.0));
    assert!(window.contains_rect(title));
    let narrow = width - 64.0 < 850.0;
    let mut labels = vec!["Settings", "Report a problem"];
    let found = h
        .query_by_label(if narrow { "0.50" } else { "SolidCraft 0.50" })
        .is_some();
    labels.push(match (narrow, found) {
        (true, true) => "0.50",
        (false, true) => "SolidCraft 0.50",
        (true, false) => "not found",
        (false, false) => "SolidCraft not found",
    });
    if narrow {
        labels.push(theme);
    } else {
        labels.extend(["Night", "Day", "System"]);
    }
    if update {
        labels.push(if narrow { "Update" } else { "Update to 0.2.6" });
    }
    let mut rects = Vec::new();
    for label in labels {
        let rect = h.get_by_label(label).rect();
        assert!(
            !rect.intersects(title),
            "{width}: {label} {rect:?} overlaps title {title:?}"
        );
        assert!(
            window.contains_rect(rect),
            "{width}: {label} {rect:?} outside window"
        );
        for other in &rects {
            assert!(
                !rect.intersects(*other),
                "{width}: {label} overlaps another control"
            );
        }
        rects.push(rect);
    }
}

#[test]
fn header_controls_fit_without_overlapping_the_title() {
    for width in [
        520.0, 600.0, 760.0, 913.0, 914.0, 1000.0, 1100.0, 1180.0, 1263.0, 1264.0, 1400.0,
    ] {
        for update in [false, true] {
            for (theme, label) in [
                (ThemePreference::Light, "Day"),
                (ThemePreference::Dark, "Night"),
                (ThemePreference::System, "System"),
            ] {
                let mut h = header_harness(width, theme, update);
                assert_header_fits(&h, width, update, label);
                h.state_mut().preview_solidcraft(None);
                h.run_steps(8);
                assert_header_fits(&h, width, update, label);
            }
        }
    }
}

#[test]
#[ignore = "renders real GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn header_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, label) in [
        (ThemePreference::Light, "Day"),
        (ThemePreference::Dark, "Night"),
    ] {
        for width in [520.0, 760.0] {
            let mut h = header_harness(width, theme, true);
            assert_header_fits(&h, width, true, label);
            h.render()
                .unwrap()
                .save(dir.join(format!("header-{width}-{label}.png")))
                .unwrap();
        }
    }
}

/// The baseline of a one-line label: its bottom edge, less the font's descent below the baseline.
fn baseline_of(h: &Harness<'_, App>, label: &str, style: egui::TextStyle) -> f32 {
    let rect = h.get_by_label(label).rect();
    let font = style.resolve(&h.ctx.style());
    let below = h.ctx.fonts_mut(|fonts| {
        let galley = fonts.layout_no_wrap(label.to_owned(), font, egui::Color32::WHITE);
        let row = &galley.rows[0];
        row.size.y - row.glyphs[0].pos.y
    });
    rect.bottom() - below
}

#[test]
fn the_version_sits_on_the_titles_baseline_in_one_line() {
    let version = dutchman_gui::version_label();
    for (width, title_style) in [
        (1400.0, dutchman_gui::theme::title()),
        (1100.0, egui::TextStyle::Heading),
        (520.0, egui::TextStyle::Heading),
    ] {
        for theme in [ThemePreference::Light, ThemePreference::Dark] {
            let h = header_harness(width, theme, false);
            let title = baseline_of(&h, "Dutchman Convert", title_style.clone());
            let small = baseline_of(&h, &version, egui::TextStyle::Small);
            assert!(
                (title - small).abs() <= 1.0,
                "{width}: title baseline {title}, version baseline {small}"
            );
            let name = h.get_by_label("Dutchman Convert").rect();
            let tag = h.get_by_label(&version).rect();
            assert!(tag.left() >= name.right() && tag.height() < name.height());
        }
    }
}

/// The main window's header at 1280x800: the version on the title's line, in English and with a
/// long German subtitle (the app has no German text yet), Day and Night.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the header; set DUTCHMAN_GUI_SCREENSHOTS"]
fn header_version_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (language, subtitle) in [
        ("english", None),
        (
            "german",
            Some("Wandelt native CAD-Konstruktionen in SolidCraft-Konstruktionen um.".to_owned()),
        ),
    ] {
        *dutchman_gui::SUBTITLE_FOR_TEST.lock().unwrap() = subtitle;
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1280.0, 800.0))
                .build_eframe(|cc| test_app(cc));
            harness.ctx.set_theme(theme);
            harness.state_mut().preview_solidcraft(Some("0.50.0"));
            harness.run_steps(8);
            let title = baseline_of(&harness, "Dutchman Convert", dutchman_gui::theme::title());
            let small = baseline_of(
                &harness,
                &dutchman_gui::version_label(),
                egui::TextStyle::Small,
            );
            assert!((title - small).abs() <= 1.0, "{language} {name}");
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("header-version-{language}"),
                name,
                "The version is on the title's line, on its baseline.",
            );
        }
    }
    *dutchman_gui::SUBTITLE_FOR_TEST.lock().unwrap() = None;
    std::fs::write(dir.join("header-version.md"), index).unwrap();
}

/// The Export from Fusion window's first half at 1280x800, Day and Night. No name a person
/// chose appears in these screens (only the exporter's own text and version numbers), so there
/// is no long-German-name set; the app has no translations.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn fusion_install_screenshots() {
    use dutchman_gui::fusion_export::agent::{Agent, Presence};
    use dutchman_gui::fusion_export::install::Step;
    use dutchman_gui::fusion_export::installer::Install;
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    let agent = || Agent {
        jobs_dir: "/Users/someone/.cad-export/jobs".into(),
        features: vec!["export".into(), "cancel".into()],
        version: "0.4.0".into(),
        busy: None,
        job: None,
        fusion_version: Some("2.0.21508".into()),
        pid: Some(41237),
        updated: None,
    };
    let cases = [
        (
            "fusion-1-not-found",
            Presence::Absent,
            Install::Idle,
            "The exporter was not found: one button installs it.",
        ),
        (
            "fusion-2-installing",
            Presence::Absent,
            Install::Running(Step::Downloading),
            "Installing: downloading the pinned release.",
        ),
        (
            "fusion-3-switch-on",
            Presence::Absent,
            Install::Done("0.4.0".into()),
            "Installed: the steps to switch it on once in Fusion, while Dutchman keeps waiting.",
        ),
        (
            "fusion-4-running",
            Presence::Running(agent()),
            Install::Idle,
            "The exporter is running in Fusion.",
        ),
        (
            "fusion-5-busy",
            Presence::Unresponsive(agent()),
            Install::Idle,
            "Fusion is busy: the heartbeat is stale but Fusion is still running.",
        ),
    ];
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        for (state, presence, step, description) in &cases {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1280.0, 800.0))
                .build_eframe(|cc| test_app(cc));
            harness.ctx.set_theme(theme);
            harness.state_mut().preview_solidcraft(Some("0.47.0"));
            harness
                .state_mut()
                .preview_fusion(true, presence.clone(), None, step.clone());
            harness.run_steps(8);
            shot(&mut harness, &dir, &mut index, state, name, description);
        }
    }
    std::fs::write(dir.join("fusion-install.md"), index).unwrap();
}

#[test]
fn designs_convert_from_the_window() {
    let out = tempfile::tempdir().unwrap();
    // Keep Convert and all three results visible without scrolling.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 1280.0))
        .build_eframe(|cc| test_app(cc));
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
fn an_existing_file_in_the_output_folder_is_kept_and_the_new_design_is_numbered() {
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("synthetic_cube.sc"), b"MY OWN DESIGN").unwrap();
    std::fs::write(
        out.path().join("synthetic_cube.report.json"),
        b"MY OWN REPORT",
    )
    .unwrap();
    let mut harness = harness();
    harness
        .state_mut()
        .add_inputs([fixture("synthetic_cube.ipt")]);
    harness.state_mut().set_out_dir(out.path().to_path_buf());
    harness.state_mut().set_verify(false);
    convert(&mut harness);
    assert_eq!(harness.state().rows()[0].status, Status::Converted);
    assert_eq!(
        std::fs::read(out.path().join("synthetic_cube.sc")).unwrap(),
        b"MY OWN DESIGN"
    );
    assert_eq!(
        std::fs::read(out.path().join("synthetic_cube.report.json")).unwrap(),
        b"MY OWN REPORT"
    );
    assert!(out.path().join("synthetic_cube (2).sc").is_file());
    assert!(out.path().join("synthetic_cube (2).report.json").is_file());
}

/// Replacing is asked once, and the question names only the files Dutchman did not make from
/// the files being converted. "Replace" replaces them; "Cancel" keeps them and numbers.
#[test]
fn replacing_asks_once_and_lists_only_files_dutchman_did_not_make() {
    std::env::set_var("DUTCHMAN_NO_TRASH", "1");
    for replace in [true, false] {
        let out = tempfile::tempdir().unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 1100.0))
            .build_eframe(|cc| test_app(cc));
        harness.state_mut().add_inputs([
            fixture("generated_unit_cube.f3d"),
            fixture("synthetic_cube.ipt"),
        ]);
        harness.state_mut().set_out_dir(out.path().to_path_buf());
        harness.state_mut().set_verify(false);
        convert(&mut harness);
        // One of the two is now a design the person made themselves.
        std::fs::write(out.path().join("synthetic_cube.sc"), b"MY OWN DESIGN").unwrap();
        // The results fold the output choices away; "Change…" opens them again.
        harness.get_by_label("Change…").click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Advanced").click_accesskit();
        harness.run_steps(8);
        harness
            .get_by_label("Replace existing files")
            .click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Convert").click_accesskit();
        harness.run_steps(8);
        assert!(harness
            .query_by_label("Replace 1 file you made yourself?")
            .is_some());
        assert!(harness.query_by_label("synthetic_cube.sc").is_some());
        assert!(harness.query_by_label("generated_unit_cube.sc").is_none());
        harness
            .get_by_label(if replace { "Replace" } else { "Cancel" })
            .click_accesskit();
        harness.step();
        wait_for_conversion(&mut harness);
        assert!(harness
            .query_by_label("Replace 1 file you made yourself?")
            .is_none());
        let mine = std::fs::read(out.path().join("synthetic_cube.sc")).unwrap();
        if replace {
            assert_ne!(mine, b"MY OWN DESIGN");
            assert!(!out.path().join("synthetic_cube (2).sc").exists());
        } else {
            assert_eq!(mine, b"MY OWN DESIGN");
            assert!(out.path().join("synthetic_cube (2).sc").is_file());
        }
        // The design made from its own file is replaced in place, never numbered.
        assert!(!out.path().join("generated_unit_cube (2).sc").exists());
    }
}

#[test]
fn the_format_list_shows_headlines_beside_reader_status() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 1800.0))
        .build_eframe(|cc| test_app(cc));
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

    // The panel asks about it at once, Discord first; its GitHub link, the backup, opens the
    // form filled in, without the file's name.
    harness.get_by_label(dutchman_gui::report_panel::QUESTION);
    harness.get_by_label("Copy and open DM");
    harness
        .query_all_by_label("File a GitHub issue ↗")
        .next()
        .unwrap()
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
        .build_eframe(|cc| test_app(cc));
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

/// Opens the report panel's folded "More": the anonymous review, the AI prompt and Diagnose.
const CLOUD: &str = dutchman_gui::report_panel::CLOUD_REPORT_BUTTON;

fn open_more(harness: &mut Harness<'static, App>) {
    harness
        .get_by_label(dutchman_gui::report_panel::MORE)
        .click_accesskit();
    harness.run_steps(8);
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
    let copied = copied.unwrap();
    assert!(copied.contains("Run log: log.jsonl."));
    assert!(copied.contains("no names or contents from the design"));
    (copied, opened.unwrap())
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
    assert!(harness.query_by_label("Please add a STEP export").is_some());
    let (message, opened) = press_dm(&mut harness);
    assert!(opened.starts_with("https://discord.com/users/"), "{opened}");
    // One line of details for Dutchy, then a line for the person's own words.
    let lines: Vec<&str> = message.lines().collect();
    assert_eq!(lines.len(), 2, "{message}");
    assert!(lines[0].contains("Size: 0.1 MB."), "{message}");
    assert!(lines[0].contains("STEP: unknown."), "{message}");
    assert!(!message.contains("broken"), "{message}");
    // The step it came from says what to do next.
    harness.run_steps(4);
    harness.get_by_label(dutchman_gui::report_panel::DM_DONE_DRAG);
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
    let join = harness.get_by_label("Join our Discord").rect();
    let dm = harness.get_by_label("DM Dutchy").rect();
    assert!((join.top() - dm.top()).abs() < 1.0 && join.right() < dm.left());
    let (harness, _dir) = failed_card("narrow.f3d", 10, 520.0);
    let join = harness.get_by_label("Join our Discord").rect();
    let step = harness.get_by_label("Please add a STEP export").rect();
    assert!(step.top() > join.bottom());
}

#[test]
fn a_step_file_is_not_asked_for_a_step_export() {
    let (mut harness, _dir) = failed_card("broken.step", 2048, 760.0);
    assert!(harness.query_by_label("Please add a STEP export").is_none());
    let (message, _) = press_dm(&mut harness);
    assert!(message.contains("STEP: yes."), "{message}");
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

/// "Convert again" replaces the file this window wrote only while it is unchanged: a file the
/// person replaced in between is never overwritten, and the new design gets a free name.
#[test]
fn converting_again_never_overwrites_a_file_changed_since_it_was_written() {
    let out = tempfile::tempdir().unwrap();
    let mut harness = harness();
    harness
        .state_mut()
        .add_inputs([fixture("synthetic_cube.ipt")]);
    harness.state_mut().set_out_dir(out.path().to_path_buf());
    harness.state_mut().set_verify(false);
    convert(&mut harness);
    let design = out.path().join("synthetic_cube.sc");
    std::fs::write(&design, b"THE PERSON'S OWN DESIGN, SAVED OVER IT").unwrap();

    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .search_for_row(0, out.path().join("Parts"), &ctx);
    let started = Instant::now();
    while harness.state().running() {
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(20));
        harness.step();
    }
    harness.run_steps(8);
    assert_eq!(harness.state().rows()[0].status, Status::Converted);
    assert_eq!(
        std::fs::read(&design).unwrap(),
        b"THE PERSON'S OWN DESIGN, SAVED OVER IT"
    );
    assert!(out.path().join("synthetic_cube (2).sc").is_file());
}

/// The "Replace existing files" setting and the question before it replaces a file the person made,
/// at 1280x800, Day and Night. The app has no German text yet: the German set shows a long German
/// design name.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn replace_existing_files_screenshots() {
    // Settings and paths are neutral, so the images show nothing of this machine.
    let demo = PathBuf::from("/tmp/demo");
    if std::env::var_os("DUTCHMAN_REPLACE_SHOTS_CHILD").is_none() {
        std::fs::create_dir_all(&demo).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "replace_existing_files_screenshots"])
            .env("DUTCHMAN_REPLACE_SHOTS_CHILD", "1")
            .env("HOME", &demo)
            .env("LOCALAPPDATA", &demo)
            .env("XDG_CONFIG_HOME", demo.join("config"))
            .env("XDG_CACHE_HOME", demo.join("cache"))
            .env("XDG_DATA_HOME", demo.join("data"))
            .env("XDG_STATE_HOME", demo.join("state"))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("DUTCHMAN_NO_TRASH", "1");
    let mut index = String::new();
    for (set, design) in [
        ("en", "Toob v2"),
        (
            "de",
            "Gehäuse-Rückwand für das Mehrzweck-Fräsgerät, Fassung 2",
        ),
    ] {
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            let work = tempfile::tempdir().unwrap();
            let input = work.path().join(format!("{design}.ipt"));
            std::fs::copy(fixture("synthetic_cube.ipt"), &input).unwrap();
            let out = demo.join("out");
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join(format!("{design}.sc")), b"MY OWN DESIGN").unwrap();
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1280.0, 800.0))
                .build_eframe(|cc| test_app(cc));
            harness.ctx.set_theme(theme);
            harness.state_mut().add_inputs([input]);
            harness.state_mut().set_out_dir(out);
            harness.state_mut().set_verify(false);
            harness.state_mut().set_checker_for_test(true);
            harness.state_mut().preview_solidcraft(Some("0.50.0"));
            harness.run_steps(8);
            harness.get_by_label("Advanced").click_accesskit();
            harness.run_steps(8);
            harness
                .get_by_label("Replace existing files")
                .scroll_to_me();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("replace-setting-{set}"),
                name,
                "Advanced: Replace existing files, off by default",
            );
            harness
                .get_by_label("Replace existing files")
                .click_accesskit();
            harness.run_steps(8);
            harness.get_by_label("Convert").click_accesskit();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("replace-confirmation-{set}"),
                name,
                "One question names the file Dutchman did not make; Cancel keeps it",
            );
        }
    }
    std::fs::write(dir.join("replace-existing-files.md"), index).unwrap();
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
            panic!("enable --features test-support for the screenshot run");
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
    let mut index = format!("# Dutchman desktop UI review\n\nBuilt from Dutchman {}. Usually 1100 × 800 logical pixels; long-loss frames use 520 or 1000 × 900. One PNG per state and theme. Day = light; Night = dark.\n\n", env!("CARGO_PKG_VERSION"));
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
            harness.get_by_label("Join our Discord").scroll_to_me();
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
        #[cfg(feature = "test-support")]
        for width in [520.0, 1000.0] {
            let mut harness = screenshot_harness(theme);
            harness.set_size(egui::vec2(width, 900.0));
            harness.state_mut().set_checker_for_test(true);
            harness
                .state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: PathBuf::from("synthetic.f3d"),
                    output: Some(PathBuf::from("converted/synthetic.sc")),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: None,
                    report: Some(loss_fixture::report()),
                });
            harness.run_steps(8);
            harness.get_by_label_contains("Losses:").scroll_to_me();
            harness.run_steps(8);
            harness.get_by_label_contains("Losses:").click_accesskit();
            harness.run_steps(8);
            harness.get_by_label("Save the full log…").scroll_to_me();
            harness.run_steps(8);
            assert!(harness.query_by_label_contains("/home/").is_none());
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("15-long-losses-{width}"),
                name,
                "Expanded synthetic losses with long native ids and an oversized unbroken token.",
            );
        }
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
            (Some("0.50.9"), "current", false, 1120.0),
            (Some("0.51.0"), "newer", false, 1120.0),
            (Some("0.51.0"), "newer-update", true, 1120.0),
            (Some("0.17.9"), "too-old", false, 1120.0),
            (Some("0.24.0"), "older-history", false, 1120.0),
            (None, "none-found", false, 1120.0),
            (Some("0.51.0"), "narrow", true, 760.0),
        ] {
            let mut preview = Harness::builder()
                .with_size(egui::vec2(width, 680.0))
                .build_eframe(|cc| test_app(cc));
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
                    _ => "SolidCraft 0.50 is installed.",
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
        harness.get_by_label("Convert").click_accesskit();
        // Do not settle frames here: the tiny cube may finish before the next frame.
        harness.step();
        // The click takes effect after the action bar is drawn. Paint its updated state.
        harness.step();
        let running = if harness.query_by_label("Stop").is_some() {
            "First frame after Convert for a real cube conversion; Stop is visible. The tiny fixture completes too quickly for a reliable mid-run progress frame."
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
            .get_by_label("Open diagnostics folder")
            .scroll_to_me();
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "12-diagnostics",
            name,
            "Open diagnostics folder button; scrolled to show the controls.",
        );
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
    index.push_str("\nAll conversions use repository fixtures copied into a temporary folder. Native file/folder pickers and external folder/app launches are not invoked. Diagnostics are read from a temporary XDG_STATE_HOME in a child test process.\n");
    std::fs::write(dir.join("INDEX.md"), index).unwrap();
}

fn screenshot_harness(theme: ThemePreference) -> Harness<'static, App> {
    let harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .build_eframe(|cc| test_app(cc));
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
    let (width, height) = image.dimensions();
    // People use a square or wider window, never a tall narrow one.
    assert!(
        height <= width,
        "screenshot {file} is {width}x{height}: render at a landscape size such as 1280x800"
    );
    use image::ImageEncoder;
    let output = std::fs::File::create(dir.join(&file)).unwrap();
    image::codecs::png::PngEncoder::new_with_quality(
        output,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(
        image.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    index.push_str(&format!("- [{file}]({file}) — {description}\n"));
}

fn wait_for_estimate(harness: &mut Harness<'static, App>) {
    harness.run_steps(8);
    let started = Instant::now();
    while harness
        .query_all_by_label_contains("GB of memory and ")
        .count()
        == 0
        && harness.query_all_by_label_contains("GB, ").count() == 0
        && harness
            .query_all_by_label_contains("GB of memory at peak and ")
            .count()
            == 0
    {
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
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().add_inputs([first, second]);
    harness.state_mut().set_out_dir(dir.path().join("out"));
    harness.run_steps(8);
    assert!(harness
        .query_by_label("Creo .prt: reads geometry")
        .is_some());
    assert!(harness.query_by_label("NX .prt: reads geometry").is_some());
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

/// Opening a missing-CLI preview retries discovery. Isolate its environment without
/// changing the process-wide environment of the other, concurrently running tests.
fn isolate_missing_cli_preview(test: &str) -> bool {
    const ISOLATED: &str = "DUTCHMAN_GUI_MISSING_CLI_PREVIEW";
    if std::env::var(ISOLATED).as_deref() == Ok(test) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env(ISOLATED, test)
        .env_remove("SOLIDCRAFT_CLI")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[test]
fn solidcraft_and_onshape_panels_keep_their_own_state() {
    for width in [760.0, 1120.0] {
        for connected in [false, true] {
            let mut h = Harness::builder()
                .with_size(egui::vec2(width, 900.0))
                .build_eframe(|cc| test_app(cc));
            h.state_mut().preview_solidcraft(None);
            h.state_mut().preview_solidcraft_app(false);
            h.state_mut()
                .preview_onshape(connected.then_some("Test designer"));
            h.state_mut()
                .set_clipboard_for_test(|| (vec![], String::new()));
            h.run_steps(8);
            let status = if width < 920.0 {
                "not found"
            } else {
                "SolidCraft not found"
            };
            h.get_by_label(status).click_accesskit();
            h.run_steps(8);
            h.get_by_label_contains("SolidCraft not found. Install SolidCraft");
            assert!(h.query_by_label("Your Onshape API keys").is_none());
            h.get_by_label(status).click_accesskit();
            h.run_steps(8);
            assert!(h.query_by_label("SolidCraft version").is_none());

            h.get_by_label(if connected {
                "Onshape connected ✓"
            } else {
                "Connect Onshape"
            })
            .click_accesskit();
            h.run_steps(8);
            if connected {
                h.get_by_label("Connected to Onshape as Test designer");
            } else {
                h.get_by_label("Your Onshape API keys");
            }
            assert!(h.query_by_label("SolidCraft version").is_none());
            h.get_by_label(status).click();
            h.run_steps(8);
            h.get_by_label_contains("SolidCraft not found. Install SolidCraft");
        }
    }
}

#[test]
fn solidcraft_status_panel_waits_for_a_click_and_shares_version_notices() {
    if isolate_missing_cli_preview(
        "solidcraft_status_panel_waits_for_a_click_and_shares_version_notices",
    ) {
        return;
    }
    for (found, expected) in [
        (Some("0.51.0"), "is newer than this Dutchman knows"),
        (Some("0.17.9"), "is too old to open Dutchman's designs"),
        (
            Some("0.24.0"),
            "A newer SolidCraft keeps more of their history.",
        ),
        (Some("0.50.0"), "SolidCraft 0.50 is installed."),
        (None, "SolidCraft not found. Install SolidCraft"),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1120.0, 680.0))
            .build_eframe(|cc| test_app(cc));
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

#[cfg(unix)]
#[test]
fn solidcraft_panel_takes_a_chosen_cli_and_explains_the_app_alone() {
    if isolate_missing_cli_preview("solidcraft_panel_takes_a_chosen_cli_and_explains_the_app_alone")
    {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let script = |path: &std::path::Path, line: &str| {
        std::fs::write(path, format!("#!/bin/sh\necho '{line}'\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("solidcraft");
    script(&app, "solidcraft 0.30.0");
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1120.0, 680.0))
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().preview_solidcraft(None);
    harness.state_mut().preview_solidcraft_app(true);
    harness.run_steps(8);
    harness
        .get_by_label("SolidCraft not found")
        .click_accesskit();
    harness.run_steps(8);
    assert!(harness
        .query_by_label_contains("Found the SolidCraft app, but not solidcraft-cli")
        .is_some());
    assert!(harness.query_by_label("Choose solidcraft-cli…").is_some());
    assert!(harness.query_by_label("Find automatically").is_none());

    harness.state_mut().choose_solidcraft_cli(&app);
    harness.run_steps(8);
    assert!(harness
        .query_by_label_contains("no solidcraft-cli beside it")
        .is_some());
    assert!(harness
        .query_by_label_contains("Found the SolidCraft app")
        .is_none());

    script(&dir.path().join("solidcraft-cli"), "solidcraft-cli 0.50.0");
    harness.state_mut().choose_solidcraft_cli(&app);
    harness.run_steps(8);
    assert!(harness.query_by_label("SolidCraft 0.50").is_some());
    assert!(harness.query_by_label("Find automatically").is_some());
    assert!(harness
        .query_by_label_contains("the portable ZIP and the MSI")
        .is_none());
}

#[cfg(unix)]
#[test]
fn solidcraft_panel_refuses_a_chosen_cli_older_than_the_oldest_supported() {
    if isolate_missing_cli_preview(
        "solidcraft_panel_refuses_a_chosen_cli_older_than_the_oldest_supported",
    ) {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("solidcraft-cli");
    std::fs::write(&cli, "#!/bin/sh\necho 'solidcraft-cli 0.17.0'\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1120.0, 680.0))
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().preview_solidcraft(None);
    harness.run_steps(8);
    harness
        .get_by_label("SolidCraft not found")
        .click_accesskit();
    harness.run_steps(8);
    harness.state_mut().choose_solidcraft_cli(&cli);
    harness.run_steps(8);
    assert!(harness
        .query_by_label_contains("older than SolidCraft 0.18 patch 2")
        .is_some());
    assert!(harness.query_by_label("Find automatically").is_none());
}

#[test]
fn update_pill_adapts_to_width_and_waits_for_a_click() {
    for (width, label) in [(760.0, "Update"), (1000.0, "Update to 0.2.6")] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 680.0))
            .build_eframe(|cc| test_app(cc));
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
fn update_window_stays_clear_of_the_header_controls() {
    for width in [520.0, 900.0, 1440.0] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 900.0))
            .build_eframe(|cc| test_app(cc));
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
        let pill = if width < 920.0 {
            "Update"
        } else {
            "Update to 0.2.6"
        };
        harness.get_by_label(pill).click_accesskit();
        harness.run_steps(8);
        assert!(harness
            .query_by_label("Dutchman 0.2.6 is available")
            .is_some());
        let window = harness
            .ctx
            .memory(|m| m.area_rect(egui::Id::new("Dutchman update")))
            .expect("the update window is open");
        let mut checked = 0;
        for label in [
            "Settings",
            "Report a problem",
            "System",
            "Day",
            "Night",
            "SolidCraft",
            "not found",
            pill,
        ] {
            for node in harness.query_all_by_label_contains(label) {
                let rect = node.rect();
                if rect.height() > 80.0 {
                    continue;
                }
                checked += 1;
                assert!(
                    !window.intersects(rect),
                    "{width}: window {window:?} covers {label:?} at {rect:?}"
                );
            }
        }
        assert!(
            checked >= 4,
            "{width}: only {checked} header controls found"
        );
    }
}

#[test]
fn update_notes_open_at_the_top_each_time() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 760.0))
        .build_eframe(|cc| test_app(cc));
    let long: String = (1..=60)
        .map(|n| format!("Line number {n} of the notes.\n\n"))
        .collect();
    for tag in ["v0.2.6", "v0.2.7", "v0.2.7"] {
        harness
            .state_mut()
            .updater
            .preview(dutchman_gui::update::Release {
                fixes_reports: Vec::new(),
                tag: tag.into(),
                notes: long.clone(),
                download: format!("https://github.com/dutchman-cad/dutchman/releases/tag/{tag}"),
            });
        harness.state_mut().updater.open = true;
        harness.run_steps(8);
        harness
            .get_by_label_contains("Line number 40 of")
            .scroll_to_me();
        harness.run_steps(8);
        // Closing and opening again, or opening another version, starts at the top.
        harness.state_mut().updater.open = false;
        harness.run_steps(4);
    }
    harness.state_mut().updater.open = true;
    harness.run_steps(8);
    let top = harness.get_by_label_contains("Line number 1 of").rect();
    let window = harness
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("Dutchman update")))
        .unwrap();
    assert!(
        window.contains_rect(top),
        "first line {top:?} not in {window:?}"
    );
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
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dutchman-core/tests/fixtures");
    for (source, name) in [
        ("solidworks_cube.sldprt", "solidworks_cube.sldprt"),
        ("placed_cubes.sldasm", "main.SLDASM"),
    ] {
        std::fs::copy(fixtures.join(source), dir.path().join(name)).unwrap();
    }
    std::fs::write(dir.path().join("~$main.SLDASM"), b"lock").unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 1200.0))
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().add_inputs([dir.path().to_path_buf()]);
    let started = Instant::now();
    loop {
        harness.run_steps(2);
        if harness
            .query_all_by_label_contains(
                "1 assembly, 1 part, 0 STEP files; converting assemblies with their parts",
            )
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
    harness
        .state_mut()
        .set_out_dir(dir.path().join("converted"));
    harness.state_mut().set_verify(false);
    convert(&mut harness);
    assert_eq!(harness.state().rows().len(), 1);
    let row = &harness.state().rows()[0];
    assert_eq!(row.status, Status::Converted);
    assert_eq!(row.input.file_name().unwrap(), "main.SLDASM");
    let report = row.report.as_ref().unwrap();
    assert_eq!(report["completeness"]["left_out"], serde_json::json!([]));
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
                    let mut stream =
                        test_support::accept_blocking(&listener, started + Duration::from_secs(30))
                            .unwrap();
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
        // A log path as long as a Windows temp folder's, so every platform checks that a
        // long "Run log:" line keeps to one line and leaves the buttons beside it alone.
        let logs = [
            "Users",
            "RUNNERADMIN",
            "AppData",
            "Local",
            "Temp",
            ".tmpAbCdEf",
        ]
        .iter()
        .fold(out.path().to_path_buf(), |dir, part| dir.join(part));
        std::fs::create_dir_all(&logs).unwrap();
        harness
            .state_mut()
            .set_diagnostic_log(logs.join("diagnostics.jsonl"));
        convert(&mut harness);
        assert_eq!(harness.state().rows()[0].status, Status::Failed);
        harness.get_by_label("RESULTS").scroll_to_me();
        harness.run_steps(8);
        assert!(harness.query_by_label("0 converted, 1 failed").is_some());
        open_more(&mut harness);
        harness.get_by_label("Diagnose this file").scroll_to_me();
        harness.run_steps(8);
        let diagnose = harness.get_by_label("Diagnose this file").rect();
        let footer = harness.get_by_label("Convert").rect().top();
        assert!(
            diagnose.top() > 70.0 && diagnose.bottom() <= footer,
            "{diagnose:?} runs under the footer at {footer}"
        );
        let show = harness.get_by_label("Show this run's log").rect();
        assert!(
            show.height() < 40.0,
            "Show this run's log is squeezed: {show:?}"
        );
        if capture_walkthrough {
            walkthrough(&mut harness, "01-failed-card");
        }
        harness.get_by_label(CLOUD).click_accesskit();
        harness.run_steps(8);
        harness
            .get_by_label("Send the whole file privately")
            .click_accesskit();
        harness.run_steps(8);
        // Back to the panel's steps: no other window opens.
        assert!(harness
            .query_by_label(dutchman_gui::report_consent::REVIEW_TITLE)
            .is_none());
        harness.get_by_label("Copy and open DM");
        if capture_walkthrough {
            walkthrough(&mut harness, "02-private-send-card");
        }
        harness.get_by_label(CLOUD).click_accesskit();
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

    /// Reports a converted result as `losses` describes it, through the card and the consent
    /// screen to a stub endpoint; returns what the endpoint received.
    fn send_converted(report: Value) -> Value {
        let stub = Stub::new(201);
        let out = tempfile::tempdir().unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| test_app(cc));
        harness
            .state_mut()
            .set_report_transport_for_test(stub.endpoint.clone(), None);
        harness.state_mut().set_out_dir(out.path().to_path_buf());
        let input = out.path().join("Bracket.f3d");
        let mut report = report;
        report["input"] = json!({"path": input, "format": "fusion_archive"});
        harness
            .state_mut()
            .preview_result_for_test(dutchman_gui::app::Row {
                input: input.clone(),
                output: Some(out.path().join("Bracket.sc")),
                status: Status::Converted,
                progress: None,
                report: Some(report),
                job: None,
                size: Some(100),
            });
        harness.run_steps(8);
        open_more(&mut harness);
        harness.get_by_label(CLOUD).click_accesskit();
        harness.run_steps(8);
        harness
            .get_by_label("Anonymous summary only")
            .click_accesskit();
        harness.run_steps(3);
        harness.get_by_label("Send").click_accesskit();
        harness.run_steps(3);
        let sent = stub.requests.recv_timeout(Duration::from_secs(10)).unwrap();
        stub.release.send(()).unwrap();
        wait_for(&mut harness, SENT);
        stub.thread.join().unwrap();
        sent
    }

    #[test]
    fn a_converted_result_with_any_loss_can_be_reported() {
        let cases = [
            // Only a note that history was not kept (Info).
            json!({"ok": true, "mode": "import", "losses": [{"level": "Info", "subject": "history",
                "message": "The original geometry is kept alongside the resolved sketches."}]}),
            // Partial history: some features kept, some not.
            json!({"ok": true, "mode": "partial_history", "history": {"mapped": 2, "total": 9},
                "losses": [{"level": "Info", "subject": "history", "message": "7 features were not kept."}]}),
            // Features blocked or flattened.
            json!({"ok": true, "mode": "history", "verification": {"status": "flattened", "reason": "fillet is not mapped yet"},
                "losses": [{"level": "Dropped", "subject": "feature", "message": "Fillet cannot be mapped yet"}]}),
            json!({"ok": true, "mode": "history", "losses": [{"level": "Degraded", "subject": "feature", "message": "edge selection cannot resolve"}]}),
            // Parts unplaced.
            json!({"ok": true, "mode": "import", "completeness": {"placed": 2, "placed_empty": 1, "placements": 3, "left_out": []}}),
            // Colours dropped.
            json!({"ok": true, "mode": "import", "losses": [{"level": "Dropped", "subject": "appearance", "message": "Color omitted"}]}),
        ];
        for report in cases {
            let sent = send_converted(report.clone());
            assert_eq!(sent["signature"], "converted-with-problem", "{report}");
            assert_eq!(sent["source_format"], "f3d");
            assert_eq!(sent["consent"]["choice"], "summary");
            assert!(!sent.to_string().contains("Bracket"));
        }
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
                .query_all_by_label("File a GitHub issue ↗")
                .next()
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
            .query_all_by_label("File a GitHub issue ↗")
            .next()
            .unwrap()
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

/// A synthetic design whose estimate is far above the free memory given.
#[cfg(feature = "test-support")]
fn memory_harness(free: u64) -> (Harness<'static, App>, PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("Synthetic assembly.f3d");
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let sizes = dutchman::choices::read_sizes(&input).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(760.0, 900.0))
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().add_inputs([input.clone()]);
    harness.state_mut().set_out_dir(dir.path().join("out"));
    harness.state_mut().set_verify(false);
    harness
        .state_mut()
        .set_sizes_for_test(input.clone(), sizes, free);
    harness.run_steps(8);
    (harness, input, dir)
}

#[test]
#[cfg(feature = "test-support")]
fn an_estimate_above_free_memory_starts_converting_at_once() {
    let (mut h, _input, dir) = memory_harness(1);
    h.run_steps(8);
    h.get_by_label("Convert").click_accesskit();
    h.step();
    assert!(h.state().running());
    assert!(h
        .query_by_label("This conversion may need more memory")
        .is_none());
    assert!(h.query_by_label("Convert with these choices").is_none());
    wait_for_conversion(&mut h);
    assert_eq!(h.state().rows().len(), 1);
    assert_eq!(h.state().rows()[0].status, Status::Converted);
    assert!(dir.path().join("out").exists());
}

#[test]
#[cfg(feature = "test-support")]
fn memory_presets_expose_help_to_focus_and_accessibility() {
    use egui_kittest::kittest::NodeT;
    let (mut h, _, _dir) = memory_harness(1);
    let selector = h
        .query_all_by_value("Standard")
        .find(|node| !node.accesskit_node().is_disabled())
        .unwrap();
    let description = selector.accesskit_node().description().unwrap().to_owned();
    assert!(description.contains("leaves history out"));
    selector.focus();
    h.run_steps(4);
    h.get_by_label(&description);
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    for (label, text) in [
        ("Fast", "Shape only"),
        ("Standard", "leaves history out"),
        ("Full", "keeps history regardless"),
    ] {
        let entry = h
            .query_all_by_label(label)
            .find(|node| !node.accesskit_node().is_disabled())
            .unwrap();
        assert!(entry.accesskit_node().description().unwrap().contains(text));
        let description = entry.accesskit_node().description().unwrap().to_owned();
        entry.focus();
        h.run_steps(4);
        h.get_by_label(&description);
    }
}

#[test]
#[cfg(feature = "test-support")]
fn choices_reach_the_conversion() {
    let (mut h, input, dir) = memory_harness(1);
    h.get_by_label("Advanced").click_accesskit();
    h.run_steps(8);
    for label in ["Sketches", "Joints", "Feature history"] {
        h.get_by_label(label).click_accesskit();
        h.run_steps(4);
    }
    convert(&mut h);
    let report = h.state().rows().first().unwrap().report.as_ref().unwrap();
    for choice in ["history", "sketches", "joints"] {
        assert_eq!(
            report.pointer(&format!("/choices/{choice}")),
            Some(&serde_json::json!(false))
        );
    }
    let choices = dutchman::choices::Choices {
        history: false,
        sketches: false,
        joints: false,
        history_within_budget: true,
    };
    let unchecked = dutchman::choices::estimate(&input, &choices).unwrap();
    assert_eq!(
        report.pointer("/estimate/peak_bytes"),
        Some(&serde_json::json!(unchecked.peak_bytes))
    );
    assert!(dir.path().join("out").exists());
}

#[test]
fn run_log_buttons_share_the_batch_log_and_review_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = screenshot_harness(ThemePreference::Light);
    let log = dir
        .path()
        .join("2026-11-02_163710_dutchman-0.2.6_failed.jsonl");
    for name in ["first.f3d", "second.f3d"] {
        let input = dir.path().join(name);
        std::fs::write(&input, "synthetic refusal").unwrap();
        harness.state_mut().add_inputs([input]);
    }
    harness.state_mut().set_diagnostic_log(log.clone());
    harness
        .state_mut()
        .set_out_dir(dir.path().join("converted"));
    convert(&mut harness);
    harness.get_by_label("RESULTS").scroll_to_me();
    harness.run_steps(8);
    assert_eq!(harness.query_all_by_label("Show this run's log").count(), 2);
    assert_eq!(harness.query_all_by_label("Copy log to…").count(), 2);
    assert_eq!(
        harness
            .query_all_by_label(&format!(
                "Run log: {}",
                dutchman_gui::platform::display_path(&log)
            ))
            .count(),
        2
    );
    assert!(log.is_file()); // Explicit paths remain exactly as supplied, even result-like names.
    open_more(&mut harness);
    harness.get_by_label(CLOUD).click_accesskit();
    harness.run_steps(8);
    // The review, in the panel, names the same log as the two cards.
    harness.get_by_label(dutchman_gui::report_consent::REVIEW_TITLE);
    assert_eq!(
        harness
            .query_all_by_label(&format!(
                "Run log: {}",
                dutchman_gui::platform::display_path(&log)
            ))
            .count(),
        3
    );
    harness.get_by_label("This run's log contains no names or contents from the design. It is kept locally; Send uploads only the summary or bundle reviewed below.");
}

#[test]
#[ignore = "renders the failed run-log card with a GPU"]
fn run_log_screenshots() {
    // Isolate recovery notices too, so screenshots never display the owner's log paths.
    if std::env::var_os("DUTCHMAN_RUNLOG_SCREENSHOT_CHILD").is_none() {
        let state = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "run_log_screenshots"])
            .env("DUTCHMAN_RUNLOG_SCREENSHOT_CHILD", "1")
            .env("HOME", state.path())
            .env("LOCALAPPDATA", state.path())
            .env("XDG_STATE_HOME", state.path())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("synthetic-part.f3d");
    std::fs::copy(fixture("container_metadata_only.f3d"), &input).unwrap();
    let mut index = String::new();
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        let mut harness = screenshot_harness(theme);
        harness.state_mut().add_inputs([input.clone()]);
        harness.state_mut().set_out_dir(work.path().join(name));
        harness.state_mut().set_verify(false);
        harness.state_mut().set_diagnostic_log(
            work.path()
                .join("2026-11-02_163710_dutchman-0.2.6_failed.jsonl"),
        );
        convert(&mut harness);
        assert_eq!(harness.state().rows()[0].status, Status::Failed);
        harness
            .query_all_by_label(dutchman::present::REPORT_BUTTON)
            .next()
            .unwrap()
            .scroll_to_me();
        harness.run_steps(8);
        for label in [
            "Show this run's log",
            "Copy log to…",
            dutchman::present::REPORT_BUTTON,
        ] {
            let rect = harness.get_by_label(label).rect();
            assert!(
                rect.left() >= 0.0
                    && rect.right() < 1100.0
                    && rect.top() > 60.0
                    && rect.bottom() < 730.0,
                "{label}: {rect:?}"
            );
        }
        shot(
            &mut harness,
            &dir,
            &mut index,
            "run-log-failed",
            name,
            "Failed synthetic conversion with its own log selected or copied.",
        );
    }
    std::fs::write(dir.join("run-logs.md"), index).unwrap();
}

fn ai_result_card_at(
    theme: ThemePreference,
    width: f32,
) -> (Harness<'static, App>, tempfile::TempDir) {
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("synthetic-part.f3d");
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, 800.0))
        .build_eframe(|cc| test_app(cc));
    harness.ctx.set_theme(theme);
    harness.state_mut().add_inputs([input]);
    harness
        .state_mut()
        .set_out_dir(work.path().join("converted"));
    harness.state_mut().set_verify(false);
    // No diagnostic log is chosen: the run still keeps one in the diagnostics folder.
    convert(&mut harness);
    assert_eq!(harness.state().rows()[0].status, Status::Converted);
    open_more(&mut harness);
    harness
        .get_by_label("Ask your AI about this file")
        .scroll_to_me();
    harness.run_steps(8);
    (harness, work)
}

fn ai_result_card(theme: ThemePreference) -> (Harness<'static, App>, tempfile::TempDir) {
    ai_result_card_at(theme, 1100.0)
}

const COPIED: &str = "Copied. Paste it into your AI assistant.";

fn copied_texts(harness: &Harness<'static, App>) -> Vec<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Clicks `label` and returns what the click copied.
fn copy_with(harness: &mut Harness<'static, App>, label: &str) -> String {
    harness.get_by_label(label).click_accesskit();
    harness.step();
    let mut copied = copied_texts(harness);
    assert_eq!(copied.len(), 1, "{label}: {copied:?}");
    assert!(!harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_))));
    harness.run_steps(8);
    harness.get_by_label(COPIED);
    copied.remove(0)
}

fn copy_ai_prompt(harness: &mut Harness<'static, App>) -> String {
    copy_with(harness, "Ask your AI about this file")
}

/// The text of the prompt box under the button.
#[cfg(feature = "test-support")]
fn shown_prompt(harness: &Harness<'static, App>) -> String {
    harness
        .get_by_role(egui::accesskit::Role::MultilineTextInput)
        .value()
        .unwrap()
}

#[cfg(feature = "test-support")]
#[test]
fn ask_ai_copies_a_short_prompt_naming_files_that_exist() {
    let (mut harness, _work) = ai_result_card(ThemePreference::Light);
    assert!(harness.query_by_label(COPIED).is_none());
    assert!(harness
        .query_by_role(egui::accesskit::Role::MultilineTextInput)
        .is_none());
    let prompt = copy_ai_prompt(&mut harness);
    assert!(!prompt.contains('\n'));
    let row = &harness.state().rows()[0];
    let output = row.output.as_ref().unwrap();
    let report = output.with_extension("report.json");
    // The run's log is in the diagnostics folder (isolated under test), not beside the design.
    let folder = harness.state().diagnostics_folder_for_test().unwrap();
    let log = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .expect("a log in the diagnostics folder");
    assert!(log.starts_with(folder));
    assert!(std::fs::read_dir(output.parent().unwrap())
        .unwrap()
        .all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|ext| ext != "jsonl")));
    for path in [&row.input, output, &report, &log] {
        assert!(path.exists(), "{path:?}");
        assert!(prompt.contains(&path.display().to_string()), "{prompt}");
    }
    // The guide is beside the app when installed, else on GitHub.
    let guide = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("AGENTS.md");
    if guide.is_file() {
        assert!(prompt.contains(&guide.display().to_string()));
    } else {
        assert!(prompt.contains(&dutchman_gui::app::agents_url(dutchman::BUILD_VERSION)));
    }
    assert!(prompt.contains("Don't send any files anywhere without asking me"));
    assert!(
        prompt.contains("`dutchman investigate` on the original file"),
        "{prompt}"
    );
    // Short apart from the paths it names (these are long temporary ones).
    let paths: usize = [&row.input, output, &report, &log]
        .iter()
        .map(|path| path.display().to_string().len())
        .sum();
    assert!(
        prompt.len() - paths < 600,
        "{} bytes: {prompt}",
        prompt.len()
    );
    assert!(prompt.matches(". ").count() <= 6, "{prompt}");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(Some(&saved), row.report.as_ref());
    // The same text is shown under the button, and Copy copies it again.
    assert_eq!(shown_prompt(&harness), prompt);
    assert_eq!(copy_with(&mut harness, "Copy"), prompt);
    assert_eq!(shown_prompt(&harness), prompt);
    harness.get_by_label("Close").click_accesskit();
    harness.run_steps(4);
    assert!(harness
        .query_by_role(egui::accesskit::Role::MultilineTextInput)
        .is_none());
}

#[test]
fn ask_ai_copies_every_time_and_after_a_new_conversion() {
    let (mut harness, _work) = ai_result_card(ThemePreference::Light);
    let first = copy_ai_prompt(&mut harness);
    // Copied goes away after a short while and the button is as before.
    std::thread::sleep(Duration::from_millis(2200));
    harness.run_steps(4);
    assert!(harness.query_by_label(COPIED).is_none());
    assert_eq!(copy_ai_prompt(&mut harness), first);
    // Straight away again: it copies again, without waiting.
    harness
        .get_by_label("Ask your AI about this file")
        .click_accesskit();
    harness.step();
    assert_eq!(copied_texts(&harness), std::slice::from_ref(&first));
    harness.run_steps(8);
    // Converting again replaces the card: no stale prompt or Copied, and the button works.
    harness.get_by_label("Convert").click_accesskit();
    harness.step();
    wait_for_conversion(&mut harness);
    harness.run_steps(8);
    assert!(harness.query_by_label(COPIED).is_none());
    assert!(harness
        .query_by_role(egui::accesskit::Role::MultilineTextInput)
        .is_none());
    let again = copy_ai_prompt(&mut harness);
    assert!(again.starts_with("Help me with a Dutchman conversion."));
}

#[cfg(feature = "test-support")]
#[test]
fn every_result_has_the_report_panel_and_a_button_for_it() {
    use dutchman_gui::app::REPORT_THIS_FILE;
    use dutchman_gui::report_panel::QUESTION;
    for width in [1100.0, 520.0] {
        // With losses: the cube.
        let (mut harness, work) = ai_result_card_at(ThemePreference::Light, width);
        harness.get_by_label(QUESTION);
        let button = harness.get_by_label(REPORT_THIS_FILE).rect();
        assert!(
            button.left() >= 0.0 && button.right() <= width,
            "{button:?}"
        );
        let ask = harness.get_by_label("Ask your AI about this file").rect();
        assert!(ask.left() >= 0.0 && ask.right() <= width, "{ask:?}");
        harness.get_by_label(REPORT_THIS_FILE).click_accesskit();
        harness.run_steps(8);
        for label in ["Copy and open DM", CLOUD] {
            let rect = harness.query_all_by_label(label).next().unwrap().rect();
            assert!(
                rect.left() >= 0.0 && rect.right() <= width,
                "{label}: {rect:?}"
            );
        }
        let _ = work;
    }
    // A clean result is never taken for one without problems: it has the same button, and the
    // panel asks about it.
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("Clean.f3d");
    harness
        .state_mut()
        .preview_result_for_test(dutchman_gui::app::Row {
            input: input.clone(),
            output: Some(dir.path().join("Clean.sc")),
            status: Status::Converted,
            progress: None,
            report: Some(serde_json::json!({"ok": true, "mode": "history",
            "input": {"path": input, "format": "fusion_archive"}, "losses": []})),
            job: None,
            size: Some(100),
        });
    harness.run_steps(8);
    harness.get_by_label(QUESTION);
    harness.get_by_label("About Clean.f3d");
    harness.get_by_label(REPORT_THIS_FILE);
    harness.get_by_label("Copy and open DM");
    assert!(harness
        .query_by_label("Ask your AI about this file")
        .is_none());
    open_more(&mut harness);
    harness.get_by_label("Ask your AI about this file");
}

/// The review under More opens locally without starting any network request.
#[cfg(feature = "test-support")]
#[test]
fn anonymous_report_only_opens_review_and_sends_nothing() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let (mut h, dir) = failed_card("private-review.f3d", 100, 1600.0);
    h.state_mut().set_report_transport_for_test(
        format!("http://{}/api/reports", listener.local_addr().unwrap()),
        None,
    );
    assert_eq!(h.query_all_by_label("File a GitHub issue ↗").count(), 1);
    let github = h.get_by_label("File a GitHub issue ↗").rect();
    let dm = h.get_by_label("Copy and open DM").rect();
    assert!(github.top() > dm.bottom());
    // The cloud report is the last route: after the GitHub link, on its row or below it.
    let cloud = h.get_by_label(CLOUD).rect();
    assert!(
        github.right() <= cloud.left() || github.bottom() <= cloud.top(),
        "{github:?} {cloud:?}"
    );
    assert!(h.query_by_label("No Discord?").is_none());
    h.get_by_label(CLOUD).click_accesskit();
    h.run_steps(12);
    h.get_by_label(dutchman_gui::report_consent::REVIEW_TITLE);
    h.get_by_label("Send");
    assert_eq!(
        test_support::accept_blocking(&listener, std::time::Instant::now())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::TimedOut
    );
    assert!(!h.state().running());
    assert!(!dir.path().join("submission.json").exists());
    assert!(h.query_by_label("Sent. The report was received.").is_none());
}

/// The Stop button reaches production Run supervision and kills a converter's child too.
#[cfg(all(feature = "test-support", target_os = "linux"))]
#[test]
fn gui_stop_terminates_the_isolated_converter_and_its_children_promptly() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("sleeping-converter");
    let pids = dir.path().join("pids");
    // The stub receives the production worker arguments; its own child inherits the pipes.
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nsleep 120 &\necho \"$$ $!\" > '{}'\nwait\n",
            pids.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    h.state_mut()
        .add_inputs([fixture("generated_unit_cube.f3d")]);
    h.state_mut().set_out_dir(dir.path().join("converted"));
    h.state_mut().set_verify(false);
    let ctx = h.ctx.clone();
    h.state_mut().start_isolated_for_test(program, &ctx);
    let started = Instant::now();
    while !pids.is_file() && started.elapsed() < Duration::from_secs(10) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let pids = std::fs::read_to_string(&pids).unwrap();
    h.run_steps(8);
    let stopped = Instant::now();
    h.get_by_label("Stop").click_accesskit();
    h.step();
    while h.state().running() && stopped.elapsed() < Duration::from_secs(2) {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let elapsed = stopped.elapsed();
    eprintln!("GUI Stop ended isolated converter and child after {elapsed:?}");
    assert!(!h.state().running(), "Stop took {elapsed:?}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert_eq!(h.state().rows()[0].status, Status::NotStarted);
    for pid in pids.split_whitespace() {
        if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
            assert!(
                status
                    .lines()
                    .any(|line| line.starts_with("State:") && line.contains("Z (zombie)")),
                "process {pid} survived Stop: {status}"
            );
        }
    }
}

/// Finishing a run makes room for reporting; both summary actions restore every setting.
#[test]
fn output_settings_collapse_after_conversion_and_reopen() {
    for size in [[1280.0, 800.0], [1600.0, 1000.0]] {
        let work = tempfile::tempdir().unwrap();
        let mut h = Harness::builder()
            .with_size(egui::vec2(size[0], size[1]))
            .build_eframe(|cc| test_app(cc));
        h.state_mut()
            .add_inputs([fixture("generated_unit_cube.f3d")]);
        let out = work.path().join("converted");
        h.state_mut().set_out_dir(out.clone());
        h.state_mut().set_verify(false);
        h.run_steps(8);
        h.get_by_label("Output format");
        convert(&mut h);
        assert!(h.query_by_label("Output format").is_none());
        let footer = h.get_by_label("Convert").rect().top();
        for label in [
            dutchman_gui::report_panel::QUESTION,
            "Join our Discord",
            dutchman::present::SEND_JOIN_BUTTON,
            dutchman::present::SEND_JOIN_NOTE,
        ] {
            let rect = h.get_by_label(label).rect();
            assert!(
                rect.top() >= 80.0 && rect.bottom() < footer,
                "{size:?}: {label}: {rect:?}, footer {footer}"
            );
        }
        if size[1] == 1000.0 {
            assert!(h.get_by_label("More").rect().bottom() < footer);
        }
        h.get_by_label("Change…").click_accesskit();
        h.run_steps(8);
        h.get_by_label("Output format");
        h.get_by_label("Choose…");
        h.get_by_label("Open diagnostics folder");
        h.get_by_label("Clear").click_accesskit();
        h.run_steps(8);
        assert!(h.state().rows().is_empty());
        h.get_by_label("Output format");
        assert!(h.query_by_label("Change…").is_none());
        h.state_mut()
            .add_inputs([fixture("generated_unit_cube.f3d")]);
        h.run_steps(8);
        convert(&mut h);
        assert!(h.query_by_label("Output format").is_none());
        h.get_by_label(&out.display().to_string()).click_accesskit();
        h.run_steps(8);
        h.get_by_label("Output format");
    }
}

/// The report panel shares the left column's scroll at short desktop sizes. The header
/// must bring its actions into view even when output settings initially push them below it.
#[test]
fn the_header_reveals_report_steps_in_a_short_window() {
    for width in [1100.0, 760.0, 520.0] {
        let mut h = Harness::builder()
            .with_size(egui::vec2(width, 680.0))
            .build_eframe(|cc| test_app(cc));
        h.run_steps(8);
        h.get_by_label("Report a problem").click_accesskit();
        h.run_steps(12);
        let heading = h.get_by_label(dutchman_gui::report_panel::QUESTION).rect();
        let dm = h.get_by_label("Copy and open DM").rect();
        for rect in [heading, dm] {
            assert!(
                rect.left() >= 0.0 && rect.right() <= width,
                "{width}: {rect:?}"
            );
            assert!(
                rect.top() >= 80.0 && rect.bottom() < 620.0,
                "{width}: {rect:?}"
            );
        }
        assert!(h
            .output()
            .platform_output
            .commands
            .iter()
            .all(|command| { !matches!(command, egui::OutputCommand::OpenUrl(_)) }));
    }
}

#[test]
fn ask_ai_prompt_fits_a_narrow_window() {
    let (mut harness, _work) = ai_result_card_at(ThemePreference::Light, 520.0);
    copy_ai_prompt(&mut harness);
    harness.get_by_label("Close").scroll_to_me();
    harness.run_steps(8);
    for label in ["Copy", "Close", "Ask your AI about this file"] {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.left() >= 0.0 && rect.right() <= 520.0,
            "{label}: {rect:?}"
        );
    }
    let prompt = harness
        .get_by_role(egui::accesskit::Role::MultilineTextInput)
        .rect();
    assert!(
        prompt.left() >= 0.0 && prompt.right() <= 520.0,
        "{prompt:?}"
    );
}

#[test]
#[ignore = "renders the AI result card in Day and Night with a GPU"]
fn ask_ai_screenshots() {
    // Keep paths private and out of the pictures: work in a fixed folder under /tmp.
    let root = PathBuf::from("/tmp/dutchman-shots/ask-ai");
    if std::env::var_os("DUTCHMAN_AI_SCREENSHOT_CHILD").is_none() {
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("state")).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "ask_ai_screenshots", "--nocapture"])
            .env("DUTCHMAN_AI_SCREENSHOT_CHILD", "1")
            .env("XDG_STATE_HOME", root.join("state"))
            .env("LOCALAPPDATA", root.join("state"))
            .env("TMPDIR", &root)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        let (mut harness, _work) = ai_result_card(theme);
        // Show neutral checker metadata rather than the machine's installed CLI path.
        #[cfg(feature = "test-support")]
        harness.state_mut().set_checker_for_test(true);
        harness.run_steps(8);
        let button = harness.get_by_label("Ask your AI about this file").rect();
        assert!(button.top() > 60.0 && button.bottom() < 800.0, "{button:?}");
        shot(
            &mut harness,
            &dir,
            &mut index,
            "ask-ai-before",
            name,
            "Converted synthetic file with losses: the button before any click.",
        );
        let prompt = copy_ai_prompt(&mut harness);
        std::fs::write(dir.join(format!("copied-prompt-{name}.txt")), &prompt).unwrap();
        if name == "day" {
            std::fs::write(dir.join("copied-prompt.txt"), &prompt).unwrap();
        }
        assert_eq!(harness.query_all_by_label_contains("/home/").count(), 0);
        harness.get_by_label("Close").scroll_to_me();
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "ask-ai-copied",
            name,
            "Right after the click: Copied, and the prompt open under the button.",
        );
        std::thread::sleep(Duration::from_millis(2200));
        harness.run_steps(8);
        assert!(harness.query_by_label(COPIED).is_none());
        shot(
            &mut harness,
            &dir,
            &mut index,
            "ask-ai-after",
            name,
            "Two seconds later: Copied gone, the prompt still open.",
        );
    }
    std::fs::write(dir.join("ask-ai.md"), index).unwrap();
}

#[test]
#[ignore = "renders real GPU screenshots"]
fn converted_report_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    if std::env::var_os("DUTCHMAN_GUI_SCREENSHOT_CHILD").is_none() {
        let state = tempfile::tempdir().unwrap();
        if !cfg!(target_os = "linux") {
            panic!("diagnostics isolation uses XDG_STATE_HOME on Linux");
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "converted_report_screenshots",
                "--nocapture",
            ])
            .env("DUTCHMAN_GUI_SCREENSHOT_CHILD", "1")
            .env("XDG_STATE_HOME", state.path())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let work = tempfile::tempdir().unwrap();
    // Use the real checker through a temporary alias so its displayed path has no home name.
    let screenshot_cli = work.path().join("solidcraft-cli");
    #[cfg(unix)]
    if let Ok(cli) = dutchman_solidcraft::usable_cli(&dutchman_solidcraft::Verification::Auto) {
        std::os::unix::fs::symlink(cli, &screenshot_cli).unwrap();
    }
    let input = work.path().join("Synthetic cube.f3d");
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let mut index =
        String::from("# Converted file reporting\n\nSynthetic cube, report section expanded.\n\n");
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        for (width, label) in [(1100.0, "wide"), (520.0, "520")] {
            let mut h = Harness::builder()
                .with_size(egui::vec2(width, 1100.0))
                .build_eframe(|cc| test_app(cc));
            h.ctx.set_theme(theme);
            h.state_mut()
                .set_solidcraft_cli(Some(screenshot_cli.clone()));
            h.state_mut().set_verify(false);
            h.state_mut().add_inputs([input.clone()]);
            h.state_mut()
                .set_out_dir(work.path().join(format!("converted-{name}-{label}")));
            convert(&mut h);
            assert_eq!(h.state().rows()[0].status, Status::Converted);
            assert!(h.state().rows()[0].size.is_some_and(|bytes| bytes > 0));
            h.get_by_label("Report this file…").scroll_to_me();
            h.run_steps(8);
            shot(
                &mut h,
                &dir,
                &mut index,
                &format!("converted-button-{label}"),
                name,
                "Converted synthetic file with losses: the Report this file button in the card's row.",
            );
            h.get_by_label("Report this file…").click_accesskit();
            h.run_steps(8);
            h.query_all_by_label(dutchman::present::REPORT_BUTTON)
                .next()
                .unwrap()
                .scroll_to_me();
            h.run_steps(8);
            h.get_by_label(dutchman::present::SEND_DM_BUTTON);
            h.get_by_label("Drag the file in (its folder is open).");
            assert!(h.query_by_label("Diagnose this file").is_none());
            assert_eq!(h.query_all_by_label_contains("/home/").count(), 0);
            let word = dutchman::present::FileView::from_report(
                h.state().rows()[0].report.as_ref().unwrap(),
            )
            .status_word;
            assert!(h.get_by_label(word).rect().top() > 70.0);
            assert!(
                h.query_all_by_label(dutchman::present::REPORT_BUTTON)
                    .next()
                    .unwrap()
                    .rect()
                    .bottom()
                    < 1030.0
            );
            shot(
                &mut h,
                &dir,
                &mut index,
                &format!("converted-report-{label}"),
                name,
                "Converted synthetic file with report review and private-send steps expanded.",
            );
        }
    }
    std::fs::write(dir.join("index.md"), index).unwrap();
}

/// The plain notes on a card with a Degraded note and an "unverified" history, Details closed and
/// open, Day and Night, with neutral paths. Needs a GPU renderer; set DUTCHMAN_GUI_SCREENSHOTS.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the plain notes; set DUTCHMAN_GUI_SCREENSHOTS"]
fn plain_notes_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (language, titles) in [
        ("english", None),
        (
            "german",
            Some([
                "Unserem Discord-Server beitreten".to_owned(),
                "Dutchy eine Direktnachricht senden".to_owned(),
                "Bitte einen STEP-Export hinzufügen".to_owned(),
            ]),
        ),
    ] {
        *dutchman_gui::report_panel::TITLES_FOR_TEST.lock().unwrap() = titles;
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            let docs = PathBuf::from("/Users/demo/Documents");
            let mut harness = screenshot_harness(theme);
            harness.set_size(egui::vec2(1280.0, 800.0));
            harness.state_mut().set_checker_for_test(true);
            harness.state_mut().preview_solidcraft(Some("0.46.2"));
            harness.state_mut().set_out_dir(docs.join("Converted"));
            let mut report = developer_notes_report();
            report["mode"] = serde_json::json!("history");
            harness
                .state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: docs.join("Cube.ipt"),
                    output: Some(docs.join("Converted").join("Cube.sc")),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: None,
                    report: Some(report),
                });
            harness.run_steps(8);
            harness.get_by_label_contains("Losses:").click_accesskit();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("plain-notes-{language}"),
                name,
                "Plain notes, Details closed.",
            );
            harness.get_by_label("Details").click_accesskit();
            harness.run_steps(8);
            // The results column scrolls; bring its last button into view so the whole list shows.
            harness
                .get_by_label_contains("Report this file")
                .scroll_to_me();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("plain-notes-details-{language}"),
                name,
                "Plain notes, Details open.",
            );
        }
    }
    *dutchman_gui::report_panel::TITLES_FOR_TEST.lock().unwrap() = None;
    std::fs::write(dir.join("plain-notes.md"), index).unwrap();
}

/// A synthetic Fusion tool library: `kinds` are the Fusion tool types, one tool each.
#[cfg(feature = "test-support")]
fn synthetic_tool_library(kinds: &[&str]) -> String {
    let tools: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            serde_json::json!({
                "BMC": "carbide", "description": format!("Tool {}", i + 1), "type": kind,
                "unit": "millimeters",
                "geometry": {"DC": 6.0, "LCF": 12.0, "LB": 40.0, "SFDM": 6.0, "NOF": 2},
                "post-process": {"number": i + 1},
                "start-values": {"presets": [{"n": 9000.0, "v_f": 1500.0,
                    "material": {"category": "all"}}]}
            })
        })
        .collect();
    serde_json::json!({"version": 22, "data": tools}).to_string()
}

/// A synthetic library converted for real in a temporary folder, as the row a finished card shows,
/// with neutral paths. Returns the row, its input path and the Designs summary the real planner
/// makes for the library.
#[cfg(feature = "test-support")]
fn converted_tool_library(kinds: &[&str]) -> (dutchman_gui::app::Row, PathBuf, String) {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("Shop tools.json");
    std::fs::write(&source, synthetic_tool_library(kinds)).unwrap();
    let made = work.path().join("Shop tools.camtools");
    let summary = dutchman::batch::plan(&[source.clone()], work.path()).description;
    let mut report = dutchman::tool_library::convert(&source, &made, true);
    let docs = PathBuf::from("/Users/demo/Documents");
    let input = docs.join("Shop tools.json");
    let output = docs.join("Converted").join("Shop tools.camtools");
    report["input"]["path"] = serde_json::json!(input);
    let ok = report["ok"] == true;
    if ok {
        report["output"]["path"] = serde_json::json!(output);
    }
    let row = dutchman_gui::app::Row {
        input: input.clone(),
        output: ok.then_some(output),
        status: if ok {
            Status::Converted
        } else {
            Status::Failed
        },
        progress: None,
        job: None,
        size: None,
        report: Some(report),
    };
    (row, input, summary)
}

/// A converted Fusion tool library is shown as one, not as a design.
#[cfg(feature = "test-support")]
#[test]
fn a_tool_library_card_uses_tool_wording_and_no_design_buttons() {
    for (kinds, word, body) in [
        (
            &["flat end mill", "ball end mill", "reamer"][..],
            "Converted, some tools left out",
            "2 of 3 tools written to CamCraft's tool library",
        ),
        (
            &["flat end mill", "ball end mill"][..],
            "Converted",
            "2 of 2 tools written to CamCraft's tool library",
        ),
    ] {
        let (row, input, summary) = converted_tool_library(kinds);
        let mut harness = harness();
        harness.state_mut().add_inputs([input]);
        harness.state_mut().preview_result_for_test(row);
        harness.run_steps(16);
        assert_eq!(summary, "1 tool library");
        harness.state_mut().set_project_summary_for_test(&summary);
        harness.run_steps(8);
        harness.get_by_label(word);
        harness.get_by_label(body);
        harness.get_by_label("Show in folder");
        harness.get_by_label_contains("Fusion tool library (.json)");
        harness.get_by_label_contains("1 tool library");
        harness.get_by_label(dutchman::present::SEND_TOOLS_INTRO);
        harness.get_by_label("Please add the same library as .tools");
        harness.get_by_label(dutchman::present::SEND_TOOLS_STEP_NOTE);
        // The Designs list shows the file's name; its full path is in the tooltip.
        assert!(harness
            .query_by_label_contains("/Users/demo/Documents/Shop tools.json")
            .is_none());
        assert!(
            harness
                .query_all_by_label_contains("Shop tools.json")
                .count()
                >= 1
        );
        assert!(harness.query_by_label_contains("for the design").is_none());
        for absent in [
            "Open in SolidCraft",
            "Add matching STEP",
            "Converted, no history",
            dutchman::present::SEND_CONVERTED_INTRO,
            dutchman::present::SEND_STEP_TITLE,
        ] {
            assert!(
                harness.query_by_label(absent).is_none(),
                "{absent} is shown for a tool library"
            );
        }
        assert!(harness
            .query_by_label_contains("Geometry without history")
            .is_none());
    }
}

/// A Fusion tool library converted to CamCraft's `.camtools`: converted with a tool that cannot
/// be written, and one where no tool can be written. Synthetic libraries, built here.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the tool library flow; set DUTCHMAN_GUI_SCREENSHOTS"]
fn tool_library_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (language, titles) in [
        ("english", None),
        (
            "german",
            Some([
                "Unserem Discord-Server beitreten".to_owned(),
                "Dutchy eine Direktnachricht senden".to_owned(),
                "Bitte dieselbe Werkzeugbibliothek als .tools hinzufügen".to_owned(),
            ]),
        ),
    ] {
        *dutchman_gui::report_panel::TITLES_FOR_TEST.lock().unwrap() = titles;
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            for (case, kinds) in [
                (
                    "tool-library-partial",
                    &["flat end mill", "ball end mill", "reamer"][..],
                ),
                ("tool-library-none", &["reamer", "reamer"][..]),
            ] {
                let docs = PathBuf::from("/Users/demo/Documents");
                let (row, input, summary) = converted_tool_library(kinds);
                let mut harness = screenshot_harness(theme);
                harness.set_size(egui::vec2(1280.0, 800.0));
                harness.state_mut().set_checker_for_test(true);
                harness.state_mut().preview_solidcraft(Some("0.50.0"));
                harness.state_mut().add_inputs([input.clone()]);
                harness.state_mut().set_out_dir(docs.join("Converted"));
                harness.state_mut().set_project_summary_for_test(&summary);
                harness.run_steps(8);
                harness.state_mut().preview_result_for_test(row);
                harness.state_mut().set_project_summary_for_test(&summary);
                harness.run_steps(16);
                shot(
                    &mut harness,
                    &dir,
                    &mut index,
                    &format!("{case}-{language}"),
                    name,
                    "The result after converting the library.",
                );
            }
        }
    }
    *dutchman_gui::report_panel::TITLES_FOR_TEST.lock().unwrap() = None;
    std::fs::write(dir.join("tool-library.md"), index).unwrap();
}

#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of a card with unchecked extrude regions"]
fn unchecked_regions_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let extrude = |unchecked: bool| {
        let mut row = serde_json::json!({"name": "Extrude", "kind": "extrude", "status": "mapped"});
        if unchecked {
            row["profile_unverified"] = serde_json::Value::Bool(true);
        }
        row
    };
    let report = serde_json::json!({
        "ok": true,
        "mode": "partial_history",
        "verification": {"status": "partial", "reason": "history stopped at feature \"Fillet 1\""},
        "history": {"mapped": 5, "total": 7, "rebuilt": 5, "needing": 7, "features": [
            {"name": "Sketch 1", "kind": "sketch", "status": "mapped"},
            extrude(true), extrude(true), extrude(false),
            {"name": "Sketch 2", "kind": "sketch", "status": "mapped"},
            {"name": "Fillet 1", "kind": "fillet", "status": "blocked"},
            {"name": "Chamfer 1", "kind": "chamfer", "status": "blocked"}
        ]},
        "losses": []
    });
    let mut index = String::from("# Unchecked extrude regions on the card\n\n");
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        for width in [520.0, 1000.0] {
            let mut harness = screenshot_harness(theme);
            harness.set_size(egui::vec2(width, 700.0));
            harness.state_mut().set_checker_for_test(true);
            harness
                .state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: PathBuf::from("synthetic.f3d"),
                    output: Some(PathBuf::from("converted/synthetic.sc")),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: None,
                    report: Some(report.clone()),
                });
            harness.run_steps(8);
            harness
                .get_by_label_contains("may use the wrong sketch region")
                .scroll_to_me();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("unchecked-regions-{width}"),
                name,
                "Synthetic partial history with two rebuilt extrudes whose regions couldn't be checked.",
            );
        }
    }
    std::fs::write(dir.join("index.md"), index).unwrap();
}

#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of a card whose rebuilt features don't match the source"]
fn body_mismatch_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let why = "the retained features give 0 bodies where the source has 1 body at the same point";
    let report = serde_json::json!({
        "ok": true,
        "mode": "partial_history",
        "verification": {"status": "partial", "reason": format!(
            "history stopped at feature \"Chamfer 1\"; retained features replay in SolidCraft, but {why}"
        )},
        "history": {"mapped": 5, "total": 7, "rebuilt": 5, "needing": 7, "features": [
            {"name": "Sketch 1", "kind": "sketch", "status": "mapped"},
            {"name": "Extrude 1", "kind": "extrude", "status": "mapped"},
            {"name": "Extrude 2", "kind": "extrude", "status": "mapped"},
            {"name": "Sketch 2", "kind": "sketch", "status": "mapped"},
            {"name": "Extrude 3", "kind": "extrude", "status": "mapped"},
            {"name": "Chamfer 1", "kind": "chamfer", "status": "blocked"},
            {"name": "Fillet 1", "kind": "fillet", "status": "blocked"}
        ]},
        "losses": [{"code": "history/body-mismatch", "level": "Degraded",
            "subject": "synthetic.f3d", "message": why}]
    });
    let mut index = String::from("# Rebuilt features that don't match the source, on the card\n\n");
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        for width in [520.0, 1000.0] {
            let mut harness = screenshot_harness(theme);
            harness.set_size(egui::vec2(width, 700.0));
            harness.state_mut().set_checker_for_test(true);
            harness
                .state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: PathBuf::from("synthetic.f3d"),
                    output: Some(PathBuf::from("converted/synthetic.sc")),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: None,
                    report: Some(report.clone()),
                });
            harness.run_steps(8);
            harness
                .get_by_label_contains("don't match the original")
                .scroll_to_me();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("body-mismatch-{width}"),
                name,
                "Synthetic partial history whose rebuilt features give no body where the source has one.",
            );
        }
    }
    std::fs::write(dir.join("index.md"), index).unwrap();
}

#[test]
#[ignore = "renders a GPU screenshot of the Part positions choices"]
fn part_positions_screenshot() {
    // Isolate settings and paths so the image never shows the owner's home folder.
    if std::env::var_os("DUTCHMAN_PART_POSITIONS_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "part_positions_screenshot"])
            .env("DUTCHMAN_PART_POSITIONS_CHILD", "1")
            .env("HOME", home.path())
            .env("LOCALAPPDATA", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("synthetic-part.f3d");
    std::fs::copy(fixture("container_metadata_only.f3d"), &input).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    harness.ctx.set_theme(ThemePreference::Light);
    harness.state_mut().add_inputs([input]);
    harness
        .state_mut()
        .set_out_dir(work.path().join("converted"));
    wait_for_estimate(&mut harness);
    harness.get_by_label("Advanced").click_accesskit();
    harness.run_steps(8);
    harness.get_by_label("Part positions").click_accesskit();
    harness.run_steps(8);
    harness.get_by_label("As Fusion last showed them");
    harness.get_by_label("As first placed");
    let mut index = String::new();
    shot(
        &mut harness,
        &dir,
        &mut index,
        "part-positions",
        "day",
        "Advanced open with the Part positions choices listed.",
    );
}

#[test]
fn part_positions_last_shown_names_the_program() {
    let cases: [(&[&str], &str); 5] = [
        (&["a.f3d", "b.f3d"], "As Fusion last showed them"),
        (&["a.ipt", "b.iam"], "As Inventor last showed them"),
        (&["a.sldprt"], "As SolidWorks last showed them"),
        (&["a.f3d", "b.ipt"], "As last shown in the original program"),
        (&[], "As last shown in the original program"),
    ];
    for (files, expected) in cases {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 800.0))
            .build_eframe(|cc| test_app(cc));
        harness
            .state_mut()
            .add_inputs(files.iter().map(PathBuf::from));
        harness.run_steps(8);
        harness.get_by_label("Advanced").click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Part positions").click_accesskit();
        harness.run_steps(8);
        harness.get_by_label(expected);
        assert_eq!(
            harness
                .query_all_by_label_contains("last showed them")
                .count()
                + harness
                    .query_all_by_label_contains("last shown in the original")
                    .count(),
            1,
            "{files:?}"
        );
    }
}

/// A window of the default size for a monitor of `monitor` points, with one converted design
/// as the only result, and the Advanced choices open.
fn fit_harness(size: [f32; 2], theme: ThemePreference, work: &Path) -> Harness<'static, App> {
    fit_harness_for(size, theme, work, "generated_unit_cube.f3d")
}

/// [`fit_harness`] with a file that cannot convert: its failed card is the one result.
fn failed_fit_harness(
    size: [f32; 2],
    theme: ThemePreference,
    work: &Path,
) -> Harness<'static, App> {
    let h = fit_harness_for(size, theme, work, "container_metadata_only.f3d");
    assert_eq!(h.state().rows()[0].status, Status::Failed);
    h
}

/// A failed card fits the window at once: its title, log and the report button sit between
/// the RESULTS heading and the Convert row, with nothing scrolling.
fn assert_failed_card_in_view(h: &Harness<'_, App>, name: impl std::fmt::Debug) {
    assert!(!h.state().page_scrolls(), "{name:?}: the page scrolls");
    let heading = h.get_by_label("RESULTS").rect();
    let convert_row = h.get_by_label("Convert").rect().top();
    for label in [
        "Failed",
        "Show this run's log",
        dutchman_gui::app::REPORT_THIS_FILE,
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(
            rect.top() >= heading.bottom() && rect.bottom() <= convert_row,
            "{name:?}: {label} {rect:?} out of view between {heading:?} and {convert_row}"
        );
    }
}

fn fit_harness_for(
    size: [f32; 2],
    theme: ThemePreference,
    work: &Path,
    name: &str,
) -> Harness<'static, App> {
    let input = work.join(name);
    std::fs::copy(fixture(name), &input).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_eframe(|cc| test_app(cc));
    harness.ctx.set_theme(theme);
    harness.state_mut().preview_solidcraft(Some("0.50.9"));
    // A fixed checker, so no path of the machine running the test is shown.
    #[cfg(feature = "test-support")]
    harness.state_mut().set_checker_for_test(true);
    harness.state_mut().add_inputs([input]);
    harness.state_mut().set_out_dir(work.join("converted"));
    harness.state_mut().set_verify(false);
    harness.run_steps(8);
    harness.get_by_label("Advanced").click_accesskit();
    harness.run_steps(8);
    convert(&mut harness);
    harness.get_by_label("Change…").click_accesskit();
    harness.run_steps(8);
    harness
}

/// Every control and heading of the main screen sits inside the window, above the Convert row.
fn assert_main_screen_in_view(h: &mut Harness<'_, App>, size: [f32; 2]) {
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
    let convert_row = h.get_by_label("Convert").rect().top();
    for label in [
        "DESIGNS",
        "Add files…",
        "OUTPUT FOLDER",
        "Preset",
        "Part positions",
        "Feature history",
        "Joints",
        "Output format",
        "Replace existing files",
        "Open diagnostics folder",
        "RESULTS",
        "Open in SolidCraft",
    ] {
        h.get_by_label(label).scroll_to_me();
        h.run_steps(12);
        let rect = h.get_by_label(label).rect();
        assert!(
            window.contains_rect(rect),
            "{size:?}: {label} {rect:?} outside the window"
        );
        assert!(
            rect.bottom() <= convert_row,
            "{size:?}: {label} {rect:?} runs under the Convert row at {convert_row}"
        );
    }
    // The report panel is part of the one scrolling left column: scrolled to, it is in view.
    h.get_by_label(dutchman_gui::report_panel::QUESTION)
        .scroll_to_me();
    h.run_steps(12);
    let question = h.get_by_label(dutchman_gui::report_panel::QUESTION).rect();
    assert!(window.contains_rect(question) && question.bottom() <= convert_row);
}

#[test]
fn the_default_size_fits_the_main_screen_without_scrolling() {
    for monitor in [[1440.0, 900.0], [1920.0, 1080.0]] {
        let size = dutchman_gui::fit_size(monitor);
        assert!(size[0] <= monitor[0] * 0.9 && size[1] <= monitor[1] * 0.9);
        assert!(size[0] >= dutchman_gui::WIDE_BREAKPOINT);
        let work = tempfile::tempdir().unwrap();
        let mut h = fit_harness(size, ThemePreference::Light, work.path());
        assert_eq!(h.state().rows().len(), 1);
        assert!(
            !h.state().page_scrolls(),
            "{monitor:?}: the page scrolls at {size:?}"
        );
        assert_main_screen_in_view(&mut h, size);
        // After a run that didn't finish, its notice still fits: nothing scrolls or moves out.
        let mut h = h;
        h.state_mut()
            .preview_unfinished_run(work.path().join("2026-11-05_010203_dutchman_running.jsonl"));
        h.run_steps(4);
        assert!(h.get_by_label("The last run didn't finish.").rect().width() > 0.0);
        assert!(
            !h.state().page_scrolls(),
            "{monitor:?}: the unfinished-run notice makes the page scroll at {size:?}"
        );
        assert_main_screen_in_view(&mut h, size);
        // Designs and the output choices sit beside the results, not above them.
        let designs = h.get_by_label("DESIGNS").rect();
        let results = h.get_by_label("RESULTS").rect();
        assert!(
            results.left() > designs.right(),
            "{monitor:?}: not side by side"
        );
        // The report panel's question is in the window under the choices, beside the results.
        let question = h.get_by_label(dutchman_gui::report_panel::QUESTION).rect();
        let convert_row = h.get_by_label("Convert").rect().top();
        assert!(
            question.top() >= h.get_by_label("Open diagnostics folder").rect().bottom()
                && question.bottom() <= convert_row
                && question.right() < results.left(),
            "{monitor:?}: the question {question:?} is not under the choices"
        );
        // Opening the AI prompt scrolls the left column to its controls. Results stay put,
        // and the prompt keeps its own text to a few visible lines.
        let mut h = h;
        open_more(&mut h);
        h.get_by_label("Ask your AI about this file")
            .click_accesskit();
        h.run_steps(8);
        assert!(
            !h.state().page_scrolls(),
            "{monitor:?}: the open prompt makes the page scroll at {size:?}"
        );
        let heading = h.get_by_label("RESULTS").rect();
        let top = h.get_by_label("Open in SolidCraft").rect();
        assert!(
            top.top() >= heading.bottom(),
            "{monitor:?}: the card's top {top:?} scrolled under RESULTS {heading:?}"
        );
        for label in ["Copy", "Close"] {
            let rect = h.get_by_label(label).rect();
            assert!(
                rect.bottom() <= convert_row,
                "{monitor:?}: {label} {rect:?} below the Convert row at {convert_row}"
            );
        }
        // The box's own rect is its whole text; what shows is the space between the button
        // and the Copy row under it: "Copied" and about five lines (uncapped, the whole
        // prompt took about 400 px).
        let ask = h.get_by_label("Ask your AI about this file").rect();
        let copy = h.get_by_label("Copy").rect();
        assert!(
            copy.top() - ask.bottom() < 220.0,
            "{monitor:?}: the prompt takes {} px",
            copy.top() - ask.bottom()
        );
        // A failed file's card fits too, and the panel offers the Discord steps beside it.
        let work = tempfile::tempdir().unwrap();
        let h = failed_fit_harness(size, ThemePreference::Light, work.path());
        assert_failed_card_in_view(&h, monitor);
        h.get_by_label("Open invite");
    }
}

/// People use a square or wider window, so the window never opens taller than wide, on any
/// landscape monitor and at the smallest size the app allows.
#[test]
fn the_default_window_is_never_taller_than_wide() {
    let fit = dutchman_gui::FIT_SIZE;
    assert!(fit[1] <= fit[0], "the fitted size {fit:?} is tall");
    for monitor in [
        [1280.0, 720.0],
        [1366.0, 768.0],
        [1440.0, 900.0],
        [1920.0, 1080.0],
        [2560.0, 1440.0],
        [1024.0, 1024.0],
    ] {
        let size = dutchman_gui::fit_size(monitor);
        assert!(size[1] <= size[0], "{size:?} on {monitor:?} is tall");
    }
    // The smallest window the app allows is landscape too.
    let smallest = dutchman_gui::fit_size([100.0, 100.0]);
    assert!(smallest[1] <= smallest[0], "{smallest:?} is tall");
}

#[test]
fn the_default_size_is_centred_and_capped_at_the_monitor() {
    assert_eq!(
        dutchman_gui::fit_size([1920.0, 1080.0]),
        dutchman_gui::FIT_SIZE
    );
    assert_eq!(
        dutchman_gui::fit_size([1440.0, 900.0]),
        dutchman_gui::FIT_SIZE
    );
    // A small monitor takes 90% of it, never less than the smallest usable window.
    assert_eq!(dutchman_gui::fit_size([1280.0, 720.0]), [1152.0, 648.0]);
    assert_eq!(dutchman_gui::fit_size([400.0, 300.0]), [520.0, 420.0]);
    assert_eq!(
        dutchman_gui::centred_position([1920.0, 1080.0], [1180.0, 800.0]),
        [370.0, 140.0]
    );
    assert_eq!(
        dutchman_gui::centred_position([400.0, 300.0], [520.0, 420.0]),
        [0.0, 0.0]
    );
}

#[test]
fn many_results_scroll_in_their_own_column_and_not_the_page() {
    let out = tempfile::tempdir().unwrap();
    let size = dutchman_gui::FIT_SIZE;
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_eframe(|cc| test_app(cc));
    // One folder of three designs gives three results from one entry in Designs.
    let folder = out.path().join("Designs");
    std::fs::create_dir_all(&folder).unwrap();
    for name in [
        "generated_unit_cube.f3d",
        "synthetic_cube.ipt",
        "solidworks_cube.sldprt",
    ] {
        std::fs::copy(fixture(name), folder.join(name)).unwrap();
    }
    h.state_mut().add_inputs([folder]);
    h.state_mut().set_out_dir(out.path().join("converted"));
    h.state_mut().set_verify(false);
    h.run_steps(8);
    h.get_by_label("Advanced").click_accesskit();
    h.run_steps(8);
    convert(&mut h);
    assert_eq!(h.state().rows().len(), 3);
    h.get_by_label("Change…").click_accesskit();
    h.run_steps(8);
    assert!(!h.state().page_scrolls());
    let convert_row = h.get_by_label("Convert").rect().top();
    h.get_by_label("Open diagnostics folder").scroll_to_me();
    h.run_steps(12);
    assert!(
        h.get_by_label("Open diagnostics folder").rect().bottom()
            <= h.get_by_label(dutchman_gui::report_panel::QUESTION)
                .rect()
                .top()
    );
    assert!(h.get_by_label("Open diagnostics folder").rect().bottom() <= convert_row);
    assert!(h.get_by_label("RESULTS").rect().bottom() <= convert_row);
}

#[test]
fn a_narrow_window_keeps_the_single_column() {
    let work = tempfile::tempdir().unwrap();
    // Tall enough for the whole page, report panel included, so nothing scrolls the headings away.
    let size = [520.0, 2400.0];
    let h = fit_harness(size, ThemePreference::Light, work.path());
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
    let designs = h.get_by_label("DESIGNS").rect();
    let output = h.get_by_label("OUTPUT FOLDER").rect();
    let results = h.get_by_label("RESULTS").rect();
    assert!(
        output.top() > designs.bottom(),
        "Output folder is below Designs"
    );
    assert!(
        results.top() > output.bottom(),
        "Results is below the options"
    );
    for label in [
        "DESIGNS",
        "Add files…",
        "OUTPUT FOLDER",
        "Preset",
        "RESULTS",
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(
            window.contains_rect(rect),
            "{label} {rect:?} outside the window"
        );
    }
    // The column keeps its width: nothing runs past the right edge.
    assert!(h.get_by_label("Open in SolidCraft").rect().right() <= size[0]);
}

#[test]
fn the_remove_button_does_not_overlap_the_row_text() {
    for width in [520.0, 1180.0] {
        let work = tempfile::tempdir().unwrap();
        let h = fit_harness([width, 1500.0], ThemePreference::Light, work.path());
        let remove = h.get_by_label("Remove").rect();
        let mut seen = 0;
        for node in h.get_all_by_role(egui::accesskit::Role::Label) {
            let rect = node.rect();
            // Text on the Remove button's own line.
            if rect.bottom() > remove.top()
                && rect.top() < remove.bottom()
                && rect.right() > 0.0
                && rect != remove
            {
                seen += 1;
                assert!(
                    rect.right() <= remove.left() + 0.5 || rect.left() >= remove.right() - 0.5,
                    "{width}: text {rect:?} overlaps Remove {remove:?}"
                );
            }
        }
        assert!(seen > 0, "{width}: found no text beside Remove");
    }
}

#[test]
fn a_result_title_has_its_own_line_above_one_row_of_buttons() {
    for size in [[1180.0, 800.0], [520.0, 1500.0]] {
        let work = tempfile::tempdir().unwrap();
        let h = fit_harness(size, ThemePreference::Light, work.path());
        let name = h
            .get_all_by_label("generated_unit_cube.f3d")
            .rfind(|node| node.rect().top() > 80.0)
            .expect("result title");
        let name = name.rect();
        // The whole name is laid out on one line: nothing is elided.
        assert!(name.width() > 150.0, "{size:?}: name {name:?}");
        let open = h.get_by_label("Open in SolidCraft").rect();
        let show = h.get_by_label("Show in folder").rect();
        let step = h.get_by_label("Add matching STEP").rect();
        for b in [open, show, step] {
            assert!(
                b.top() >= name.bottom() - 0.5,
                "{size:?}: {b:?} not below {name:?}"
            );
        }
        // One row, in order.
        assert!((open.center().y - show.center().y).abs() < 1.0);
        assert!(open.right() <= show.left());
        if size[0] > 1000.0 {
            assert!((open.center().y - step.center().y).abs() < 1.0);
            assert!(show.right() <= step.left());
        }
    }
}

/// In a wide window the left column (Designs, Output folder, the report panel) is not scrolled by
/// a conversion: it sits at the top before and after, with the first frames of the result too.
#[test]
fn the_wide_left_column_stays_at_the_top_through_a_conversion() {
    let work = tempfile::tempdir().unwrap();
    let input = work.path().join("cube.f3d");
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    #[cfg(feature = "test-support")]
    h.state_mut().set_checker_for_test(true);
    h.state_mut().add_inputs([input]);
    h.state_mut().set_out_dir(work.path().join("converted"));
    h.state_mut().set_verify(false);
    h.run_steps(8);
    let before = h.get_by_label("DESIGNS").rect().top();
    convert(&mut h);
    for _ in 0..4 {
        h.run_steps(4);
        assert_eq!(h.get_by_label("DESIGNS").rect().top(), before);
    }
}

#[test]
fn a_very_long_result_name_wraps_instead_of_eliding() {
    let work = tempfile::tempdir().unwrap();
    let long = "a_very_long_generated_unit_cube_design_name_".repeat(3) + ".f3d";
    let input = work.path().join(&long);
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(520.0, 1500.0))
        .build_eframe(|cc| test_app(cc));
    #[cfg(feature = "test-support")]
    h.state_mut().set_checker_for_test(true);
    h.state_mut().add_inputs([input]);
    h.state_mut().set_out_dir(work.path().join("converted"));
    h.state_mut().set_verify(false);
    h.run_steps(8);
    convert(&mut h);
    // A narrow window scrolls its one page to the Results after a run (the app does that on
    // purpose, as before the one-column change), so this test does not assume a scroll
    // position: every place the name is shown counts, the design list and the result title.
    let names: Vec<_> = h.get_all_by_label_contains("a_very_long").collect();
    assert!(names.len() >= 2, "the design list and the result title");
    for name in &names {
        assert!(name.rect().right() <= 520.0, "{:?}", name.rect());
    }
    // Wrapped onto several lines, so taller than one heading line.
    let tallest = names.iter().map(|n| n.rect().height()).fold(0.0, f32::max);
    assert!(tallest > 40.0, "tallest name {tallest}");
}

#[test]
#[ignore = "renders GPU screenshots of the default window size; set DUTCHMAN_GUI_SCREENSHOTS"]
fn fit_screenshots() {
    // Isolate settings and paths so no image shows the owner's home folder.
    if std::env::var_os("DUTCHMAN_FIT_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "fit_screenshots", "--nocapture"])
            .env("DUTCHMAN_FIT_CHILD", "1")
            .env("HOME", home.path())
            .env("LOCALAPPDATA", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success(), "screenshot child failed: {status}");
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let cases = [
        ("fit-1440x900", dutchman_gui::fit_size([1440.0, 900.0])),
        ("fit-1920x1080", dutchman_gui::fit_size([1920.0, 1080.0])),
        ("fit-520", [520.0, 1500.0]),
    ];
    for (theme, label) in [
        (ThemePreference::Light, "Day"),
        (ThemePreference::Dark, "Night"),
    ] {
        for (name, size) in cases {
            let work = tempfile::tempdir().unwrap();
            let mut h = fit_harness(size, theme, work.path());
            if size[0] >= dutchman_gui::WIDE_BREAKPOINT {
                assert!(!h.state().page_scrolls(), "{name}: the page scrolls");
            }
            h.render()
                .unwrap()
                .save(dir.join(format!("{name}-{label}.png")))
                .unwrap();
            if name == "fit-1440x900" {
                h.state_mut().preview_unfinished_run(
                    work.path().join("2026-11-05_010203_dutchman_running.jsonl"),
                );
                h.run_steps(4);
                assert!(
                    !h.state().page_scrolls(),
                    "{name}: the notice makes the page scroll"
                );
                h.render()
                    .unwrap()
                    .save(dir.join(format!("{name}-unfinished-{label}.png")))
                    .unwrap();
            }
            if size[0] >= dutchman_gui::WIDE_BREAKPOINT {
                let work = tempfile::tempdir().unwrap();
                let mut h = failed_fit_harness(size, theme, work.path());
                assert_failed_card_in_view(&h, name);
                h.render()
                    .unwrap()
                    .save(dir.join(format!("{name}-failed-{label}.png")))
                    .unwrap();
                open_more(&mut h);
                h.render()
                    .unwrap()
                    .save(dir.join(format!("{name}-failed-more-{label}.png")))
                    .unwrap();
            }
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders the SolidCraft version window with a GPU; set DUTCHMAN_GUI_SCREENSHOTS"]
fn solidcraft_window_screenshots() {
    // Isolate settings and paths so no image shows the owner's home folder.
    if std::env::var_os("DUTCHMAN_FIT_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "solidcraft_window_screenshots",
                "--nocapture",
            ])
            .env("DUTCHMAN_FIT_CHILD", "1")
            .env("HOME", home.path())
            .env("LOCALAPPDATA", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success(), "screenshot child failed: {status}");
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let size = dutchman_gui::fit_size([1440.0, 900.0]);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
    for found in [true, false] {
        let work = tempfile::tempdir().unwrap();
        let mut h = fit_harness(size, ThemePreference::Light, work.path());
        if !found {
            h.state_mut().preview_solidcraft(None);
            h.state_mut().preview_search_for_test(&[
                ("SOLIDCRAFT_CLI", "unset"),
                ("PATH", "not_found"),
                ("beside Dutchman", "not_found"),
                ("Program Files", "not_found"),
                ("Local app data", "not_found"),
                ("Windows registry", "not_found"),
            ]);
        }
        h.run_steps(8);
        h.get_by_label("SolidCraft version…").click_accesskit();
        h.run_steps(8);
        let window = h
            .ctx
            .memory(|m| m.area_rect(egui::Id::new("SolidCraft version")))
            .expect("the version window is open");
        assert!(
            viewport.contains_rect(window),
            "{window:?} outside {viewport:?}"
        );
        let texts: &[&str] = if found {
            &["SolidCraft 0.50 is installed."]
        } else {
            &[
                "SolidCraft not found.",
                "Looked in:\n• SOLIDCRAFT_CLI: not set",
            ]
        };
        for text in texts {
            let rect = h.get_by_label_contains(text).rect();
            assert!(
                window.contains_rect(rect),
                "{text}: {rect:?} outside the window {window:?}"
            );
        }
        let name = if found { "found" } else { "not-found" };
        h.render()
            .unwrap()
            .save(dir.join(format!("solidcraft-window-{name}-Day.png")))
            .unwrap();
    }
}

#[test]
#[ignore = "renders the update window with a GPU; set DUTCHMAN_GUI_SCREENSHOTS to a folder"]
fn update_dialog_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    // Settings, window size and diagnostics come from a temporary home, never the reader's.
    if std::env::var_os("DUTCHMAN_GUI_SCREENSHOT_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "update_dialog_screenshots",
                "--nocapture",
            ])
            .env("DUTCHMAN_GUI_SCREENSHOT_CHILD", "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success(), "screenshot child failed: {status}");
        return;
    }
    let feed = include_bytes!("../src/update/fixtures/releases.atom");
    let mut index = String::new();
    for state in ["checking", "available", "failed"] {
        let mut harness = screenshot_harness(ThemePreference::Light);
        harness.set_size(egui::vec2(900.0, 760.0));
        #[cfg(feature = "test-support")]
        harness.state_mut().set_checker_for_test(true);
        match state {
            "checking" => harness.state_mut().updater.preview_checking(),
            "available" => harness.state_mut().updater.preview_feed(feed),
            _ => harness
                .state_mut()
                .updater
                .preview_failed("dns error: failed to lookup address information"),
        }
        harness.run_steps(8);
        if state == "available" {
            // The notes open at their first line, as the reader sees them.
            let window = harness
                .ctx
                .memory(|m| m.area_rect(egui::Id::new("Dutchman update")))
                .unwrap();
            let first = harness
                .get_by_label_contains("Dutchman Convert 0.2.7 adds")
                .rect();
            assert!(
                window.contains_rect(first),
                "intro {first:?} not in {window:?}"
            );
        }
        match state {
            "checking" => assert!(harness
                .query_by_label_contains("Checking for updates")
                .is_some()),
            "available" => {
                assert!(harness
                    .query_by_label_contains("Dutchman 0.2.7 is available")
                    .is_some());
                assert!(harness.query_by_label_contains("What's new").is_some());
            }
            _ => assert!(harness.query_by_label("Retry").is_some()),
        }
        let homes: Vec<_> = harness
            .query_all_by_label_contains("/home/")
            .map(|n| {
                format!(
                    "{:?} {:?}",
                    egui_kittest::kittest::NodeT::accesskit_node(&n).value(),
                    egui_kittest::kittest::NodeT::accesskit_node(&n).role()
                )
            })
            .collect();
        assert!(homes.is_empty(), "{homes:?}");
        shot(
            &mut harness,
            &dir,
            &mut index,
            &format!("update-{state}"),
            "light",
            state,
        );
        if state == "available" {
            harness.get_by_label("What's new").scroll_to_me();
            harness.run_steps(8);
            harness
                .get_by_label_contains("Report problems")
                .scroll_to_me();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                "update-available-code",
                "light",
                "inline code",
            );
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders the update window with a GPU; set DUTCHMAN_GUI_SCREENSHOTS to a folder"]
fn update_unreachable_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    // Settings come from a temporary home, never the reader's. The panel shows no design name or
    // path, so there is no long German string to simulate: English Day and Night only.
    if std::env::var_os("DUTCHMAN_GUI_SCREENSHOT_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "update_unreachable_screenshots",
                "--nocapture",
            ])
            .env("DUTCHMAN_GUI_SCREENSHOT_CHILD", "1")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success(), "screenshot child failed: {status}");
        return;
    }
    // Offer the next minor release after the running build, so the panel never offers an older one.
    let mut running = env!("CARGO_PKG_VERSION").split('.');
    let major = running.next().unwrap();
    let minor: u64 = running.next().unwrap().parse().unwrap();
    let newer = format!("{major}.{}.0", minor + 1);
    let mut index = String::new();
    for (theme, name) in [
        (ThemePreference::Light, "Day"),
        (ThemePreference::Dark, "Night"),
    ] {
        let mut harness = screenshot_harness(theme);
        harness.set_size(egui::vec2(1280.0, 800.0));
        harness.state_mut().set_checker_for_test(true);
        harness
            .state_mut()
            .updater
            .preview(dutchman_gui::update::Release {
                fixes_reports: Vec::new(),
                tag: format!("v{newer}"),
                notes: format!("Dutchman {newer}\n\nA newer version of Dutchman Convert."),
                download: format!("https://github.com/dutchman-cad/dutchman/releases/tag/v{newer}"),
            });
        harness.state_mut().updater.preview_download_unreachable();
        harness.run_steps(8);
        assert!(harness
            .query_by_label_contains(&format!("Dutchman {newer} is available"))
            .is_some());
        assert!(harness.query_by_label("Try again").is_some());
        assert!(harness.query_by_label("Download manually").is_some());
        shot(
            &mut harness,
            &dir,
            &mut index,
            "update-unreachable",
            name,
            "download could not reach GitHub",
        );
    }
}

#[test]
fn the_guide_link_names_this_builds_version() {
    assert_eq!(
        dutchman_gui::app::agents_url("0.2.7"),
        "https://github.com/dutchman-cad/dutchman/blob/v0.2.7/assets/install/AGENTS.md"
    );
    assert_eq!(
        dutchman_gui::app::agents_url("0.2.8-nightly.abc1234"),
        "https://github.com/dutchman-cad/dutchman/blob/abc1234/assets/install/AGENTS.md"
    );
}

#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders running-step GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn running_step_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, label) in [
        (ThemePreference::Light, "Day"),
        (ThemePreference::Dark, "Night"),
    ] {
        for seconds in [0, 61, 120, 121] {
            let mut harness = screenshot_harness(theme);
            harness.state_mut().preview_solidcraft(Some("0.50.0"));
            harness.state_mut().set_checker_for_test(true);
            harness
                .state_mut()
                .preview_running_for_test(dutchman_gui::app::Row {
                    input: PathBuf::from("Assembly.iam"),
                    output: None,
                    status: Status::Converting,
                    report: None,
                    job: None,
                    size: None,
                    progress: Some(dutchman_core::ProgressEvent {
                        stage: dutchman_core::Stage::Map,
                        elapsed_ms: (seconds + 30) * 1000,
                        designs_read: 261,
                        designs_total: Some(264),
                        heartbeat: true,
                        running: Some(dutchman_core::RunningStep {
                            kind: dutchman_core::StepKind::CheckingHistory,
                            name: Some("Bracket".into()),
                            elapsed_ms: seconds * 1000,
                            usual_ms: dutchman_core::StepKind::Checking.usual_ms(),
                            limit_ms: Some(dutchman_solidcraft::DEFAULT_TIMEOUT.as_secs() * 1000),
                            consequence: Some(
                                dutchman_core::progress_words::history_limit(false, false).into(),
                            ),
                            feature: None,
                        }),
                    }),
                });
            harness.run_steps(8);
            let caption = harness
                .state()
                .rows()
                .first()
                .unwrap()
                .progress
                .as_ref()
                .unwrap()
                .running_line();
            assert!(harness.query_by_label(&caption).is_some());
            assert!(harness.query_by_label("261 of 264 designs read").is_some());
            assert!(harness.query_by_label("Stop").is_some());
            harness
                .render()
                .unwrap()
                .save(dir.join(format!("running-{seconds}s-{label}.png")))
                .unwrap();
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
fn folder_run_keeps_the_oldest_file_visible_below_the_counts() {
    for (theme, label, width) in [
        (ThemePreference::Light, "Day", 1100.0),
        (ThemePreference::Dark, "Night", 1100.0),
        (ThemePreference::Light, "Day", 520.0),
        (ThemePreference::Dark, "Night", 520.0),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(width, 800.0))
            .build_eframe(|cc| test_app(cc));
        harness.ctx.set_theme(theme);
        harness.state_mut().preview_solidcraft(Some("0.50.0"));
        harness.state_mut().set_checker_for_test(true);
        let rows = (0..1293)
            .map(|index| {
                let active = (1261..1264).contains(&index);
                dutchman_gui::app::Row {
                    input: PathBuf::from(if index == 1261 {
                        "Assembly.iam".into()
                    } else {
                        format!("Part-{index}.ipt")
                    }),
                    output: None,
                    status: if index < 1261 {
                        Status::Converted
                    } else if active {
                        Status::Converting
                    } else {
                        Status::Waiting
                    },
                    report: None,
                    job: None,
                    size: None,
                    progress: active.then(|| dutchman_core::ProgressEvent {
                        stage: dutchman_core::Stage::Map,
                        elapsed_ms: if index == 1261 { 151_000 } else { 30_000 },
                        designs_read: 261,
                        designs_total: Some(264),
                        heartbeat: true,
                        running: Some(dutchman_core::RunningStep {
                            kind: dutchman_core::StepKind::CheckingHistory,
                            name: Some("Bracket".into()),
                            elapsed_ms: 61_000,
                            usual_ms: 60_000,
                            limit_ms: Some(120_000),
                            consequence: Some(
                                dutchman_core::progress_words::history_limit(false, false).into(),
                            ),
                            feature: None,
                        }),
                    }),
                }
            })
            .collect();
        harness
            .state_mut()
            .preview_folder_running_for_test(PathBuf::from("Project"), rows);
        harness.run_steps(8);
        assert!(harness.query_by_label("1261 of 1293 done").is_some());
        assert!(harness.query_by_label("Assembly.iam: building history (2 other files running)\nStill checking Bracket (rebuilt history): 1 min 1 s").is_some());
        let line = harness.get_by_label("Assembly.iam: building history (2 other files running)\nStill checking Bracket (rebuilt history): 1 min 1 s").rect();
        let count = harness.get_by_label("1261 of 1293 done").rect();
        assert!(line.top() >= count.bottom());
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))
                .contains_rect(line)
        );
        if let Some(dir) = std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            harness
                .render()
                .unwrap()
                .save(dir.join(format!("folder-1261-of-1293-{width:.0}-{label}.png")))
                .unwrap();
        }
    }
}

mod paste_inputs {
    use super::*;
    use dutchman_gui::paste::{Response, Transport};
    use egui_kittest::kittest::NodeT;
    use std::sync::{Arc, Mutex};

    const LINK: &str = "https://cad.onshape.com/documents/aaaaaaaaaaaaaaaaaaaaaaaa/w/bbbbbbbbbbbbbbbbbbbbbbbb/e/cccccccccccccccccccccccc";

    fn window(connected: bool) -> Harness<'static, App> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1180.0, 1000.0))
            .build_eframe(|cc| test_app(cc));
        h.state_mut()
            .preview_onshape(connected.then_some("Test designer"));
        h.state_mut()
            .set_clipboard_for_test(|| (vec![], String::new()));
        h.run_steps(8);
        h
    }

    fn paste(h: &mut Harness<'static, App>, text: &str) {
        h.input_mut().events.push(egui::Event::Paste(text.into()));
        h.step();
    }

    #[test]
    fn onshape_button_names_both_states_and_connected_dialog_hides_keys() {
        let mut h = window(false);
        assert!(h.query_by_label("Connect Onshape").is_some());
        assert!(h.query_by_label("Onshape connected ✓").is_none());
        h.state_mut().preview_onshape(Some("Test designer"));
        h.run_steps(4);
        h.get_by_label("Onshape connected ✓").click_accesskit();
        h.run_steps(4);
        assert!(h
            .query_by_label("Connected to Onshape as Test designer")
            .is_some());
        assert!(h.query_by_label("Access key").is_none());
        assert!(h.query_by_label("Secret key").is_none());
        assert!(h.query_by_label("Use bearer token file").is_some());
        assert!(h.query_by_label("Change keys").is_some());
        assert!(h.query_by_label("Disconnect").is_some());
        h.get_by_label("Change keys").click_accesskit();
        h.run_steps(4);
        assert!(h.query_by_label("Your Onshape API keys").is_some());
    }

    /// The startup estimate reports free memory from a background thread. Its line appearing in
    /// the frame that handles a click must not lose the click.
    #[cfg(feature = "test-support")]
    #[test]
    fn a_late_free_memory_line_does_not_drop_a_click_below_it() {
        let mut h = window(true);
        let started = Instant::now();
        // Let the real startup estimate land first, so only the test changes the line below.
        while h.query_by_label_contains("of memory free").is_none()
            && started.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        let input = fixture("generated_unit_cube.f3d");
        let sizes = dutchman::choices::read_sizes(&input).unwrap();
        h.state_mut().set_sizes_for_test(input.clone(), sizes, 0);
        h.run_steps(2);
        assert!(h.query_by_label_contains("of memory free").is_none());
        h.get_by_label("Onshape connected ✓").click_accesskit();
        let sizes = dutchman::choices::read_sizes(&input).unwrap();
        h.state_mut()
            .set_sizes_for_test(input, sizes, 8_000_000_000);
        h.step();
        h.run_steps(3);
        assert!(h.query_by_label_contains("of memory free").is_some());
        assert!(h
            .query_by_label("Paste your Onshape Part Studio link")
            .is_some());
    }

    #[test]
    fn onshape_open_enables_only_for_a_document_link_and_keeps_text_paste_in_the_field() {
        let mut h = window(true);
        h.get_by_label("Onshape connected ✓").click_accesskit();
        h.run_steps(4);
        assert!(h.get_by_label("Open").accesskit_node().is_disabled());
        let field = h.get_by_label("Paste your Onshape Part Studio link");
        assert_eq!(field.accesskit_node().value().unwrap_or_default(), "");
        field.focus();
        h.step();
        paste(&mut h, "https://example.test/file.f3d");
        h.run_steps(3);
        assert!(h.get_by_label("Open").accesskit_node().is_disabled());
        assert!(h.query_by_label("Download file").is_none());
        // Replace the text through ordinary keyboard editing, preserving the focused field.
        h.get_by_label("Paste your Onshape Part Studio link")
            .click_accesskit();
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                command: true,
                ctrl: true,
                ..Default::default()
            },
        });
        h.step();
        paste(&mut h, LINK);
        h.run_steps(3);
        assert!(!h.get_by_label("Open").accesskit_node().is_disabled());
    }

    fn fetch(
        _link: String,
        folder: &Path,
        _progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        std::thread::sleep(Duration::from_millis(100));
        let out = folder.join("part.onshape-bundle");
        std::fs::copy(fixture("onshape-revolve.onshape-bundle"), &out)
            .map_err(|e| e.to_string())?;
        Ok(out)
    }

    #[derive(Default)]
    struct MockNetwork {
        methods: Mutex<Vec<String>>,
    }
    impl Transport for MockNetwork {
        fn request(&self, method: &str, url: &str) -> Result<Response, String> {
            assert!(url.starts_with("https://"));
            self.methods.lock().unwrap().push(method.into());
            let bytes = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
            Ok(Response {
                status: 200,
                length: Some(bytes.len() as u64),
                location: None,
                body: Box::new(std::io::Cursor::new(bytes)),
            })
        }
    }

    fn output_picker(h: &mut Harness<'static, App>, folder: &Path) {
        let folder = folder.to_owned();
        h.state_mut()
            .set_output_picker_for_test(move |sender, ctx| {
                sender
                    .send(dutchman_gui::worker::Message::Picked {
                        pick: dutchman_gui::worker::Pick::Output,
                        paths: vec![folder.clone()],
                    })
                    .unwrap();
                ctx.request_repaint();
            });
    }

    fn assert_bundle_survives_exit(h: Harness<'static, App>, output: &Path) {
        let row = &h.state().rows()[0];
        let bundle = row.input.clone();
        assert_eq!(bundle.parent(), Some(output));
        assert_eq!(row.output.as_ref().unwrap(), &bundle.with_extension("sc"));
        let expected = std::fs::read(fixture("onshape-revolve.onshape-bundle")).unwrap();
        assert_eq!(std::fs::read(&bundle).unwrap(), expected);
        drop(h);
        assert_eq!(std::fs::read(&bundle).unwrap(), expected);
    }

    #[test]
    fn pasted_onshape_link_adds_a_main_job_and_asks_for_an_output_folder() {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), fetch);
        output_picker(&mut h, output.path());
        paste(&mut h, LINK);
        h.step();
        assert!(h.state().running());
        assert_eq!(h.state().rows().len(), 1);
        assert_eq!(h.state().rows()[0].status, Status::Converting);
        assert!(h.query_by_label("Your Onshape API keys").is_none());
        wait_for_conversion(&mut h);
        assert_eq!(h.state().rows().len(), 1);
        assert!(h.state().rows()[0]
            .output
            .as_ref()
            .unwrap()
            .starts_with(output.path()));
        assert!(h.state().rows()[0].report.is_some());
        assert_bundle_survives_exit(h, output.path());
    }

    fn failing_fetch(
        _link: String,
        _folder: &Path,
        _progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        std::thread::sleep(Duration::from_millis(50));
        Err("The Onshape fetch stopped after 50 calls at the safety limit.".to_owned())
    }

    fn panicking_fetch(
        _link: String,
        _folder: &Path,
        _progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        panic!("synthetic fetch failure");
    }

    fn fetch_error_message(fetch: dutchman_gui::paste::Fetch) -> String {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), fetch);
        output_picker(&mut h, output.path());
        paste(&mut h, LINK);
        let started = Instant::now();
        while h.state().running() || h.state().rows().is_empty() {
            assert!(started.elapsed() < Duration::from_secs(20), "fetch hung");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(8);
        assert_eq!(h.state().rows().len(), 1);
        assert_eq!(h.state().rows()[0].status, Status::Failed);
        assert!(h.query_by_label_contains("Fetching from Onshape").is_none());
        let message = h.state().rows()[0].report.as_ref().unwrap()["error"]["message"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            h.query_all_by_label_contains(&message).next().is_some(),
            "{message}"
        );
        message
    }

    #[test]
    fn a_failed_onshape_fetch_ends_the_job_with_its_message() {
        assert_eq!(
            fetch_error_message(failing_fetch),
            "The Onshape fetch stopped after 50 calls at the safety limit."
        );
    }

    fn budget_then_ok_fetch(
        link: String,
        folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(App::budget_stop_message_for_test());
        }
        fetch(link, folder, progress)
    }

    /// Reports 12 of about 51 calls and holds the fetch open until the test has looked.
    fn counting_fetch(
        link: String,
        folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        progress.calls(12, Some(51));
        let started = Instant::now();
        while !COUNT_SEEN.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "caption never read"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        progress.calls(53, Some(51));
        fetch(link, folder, progress)
    }
    static COUNT_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    #[test]
    fn an_onshape_fetch_counts_its_calls_on_the_result_card() {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_out_dir(output.path().into());
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), counting_fetch);
        paste(&mut h, LINK);
        let started = Instant::now();
        while h
            .query_by_label("Fetching from Onshape: 12 of about 51 API calls…")
            .is_none()
        {
            assert!(started.elapsed() < Duration::from_secs(30), "no call count");
            std::thread::sleep(Duration::from_millis(10));
            h.step();
        }
        COUNT_SEEN.store(true, std::sync::atomic::Ordering::SeqCst);
        wait_for_conversion(&mut h);
        assert!(h.query_by_label_contains("API calls").is_none());
        assert!(h.state().rows()[0].report.is_some());
    }

    /// Holds the fetch at 12 of about 51 calls until the screenshot is taken.
    fn held_fetch(
        link: String,
        folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        progress.calls(12, Some(51));
        let started = Instant::now();
        while !SHOT_TAKEN.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(started.elapsed() < Duration::from_secs(60), "never shot");
            std::thread::sleep(Duration::from_millis(10));
        }
        fetch(link, folder, progress)
    }
    static SHOT_TAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    #[test]
    #[ignore = "renders real GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
    fn onshape_call_count_screenshots() {
        let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            SHOT_TAKEN.store(false, std::sync::atomic::Ordering::SeqCst);
            let output = tempfile::tempdir().unwrap();
            let mut h = window(true);
            h.ctx.set_theme(theme);
            h.state_mut().set_out_dir(output.path().into());
            h.state_mut().set_verify(false);
            h.state_mut()
                .set_link_transport_for_test(Arc::new(MockNetwork::default()), held_fetch);
            paste(&mut h, LINK);
            let started = Instant::now();
            while h
                .query_by_label("Fetching from Onshape: 12 of about 51 API calls…")
                .is_none()
            {
                assert!(started.elapsed() < Duration::from_secs(30), "no call count");
                std::thread::sleep(Duration::from_millis(10));
                h.step();
            }
            h.run_steps(3);
            h.render()
                .unwrap()
                .save(dir.join(format!("onshape-call-count-{name}.png")))
                .unwrap();
            SHOT_TAKEN.store(true, std::sync::atomic::Ordering::SeqCst);
            wait_for_conversion(&mut h);
        }
    }

    /// Names the design after 12 calls, then holds until released.
    fn named_fetch(
        link: String,
        folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        progress.calls(12, Some(51));
        progress.named("Bracket - Part Studio 1");
        let started = Instant::now();
        while !NAME_SEEN.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(started.elapsed() < Duration::from_secs(30), "never read");
            std::thread::sleep(Duration::from_millis(10));
        }
        fetch(link, folder, progress)
    }
    static NAME_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    #[test]
    fn the_row_goes_by_the_design_name_once_the_fetch_knows_it() {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_out_dir(output.path().into());
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), named_fetch);
        paste(&mut h, LINK);
        let started = Instant::now();
        while h
            .query_by_label("Bracket - Part Studio 1.onshape-bundle")
            .is_none()
        {
            assert!(started.elapsed() < Duration::from_secs(30), "no title");
            std::thread::sleep(Duration::from_millis(10));
            h.step();
        }
        assert_eq!(
            h.state().rows()[0].input,
            PathBuf::from("Bracket - Part Studio 1.onshape-bundle")
        );
        assert!(h
            .query_by_label("cccccccccccccccccccccccc.onshape-bundle")
            .is_none());
        NAME_SEEN.store(true, std::sync::atomic::Ordering::SeqCst);
        wait_for_conversion(&mut h);
    }

    /// The design name [`named_held_fetch`] reports.
    #[cfg(feature = "test-support")]
    static SHOT_TITLE: std::sync::Mutex<&str> = std::sync::Mutex::new("Bracket - Part Studio 1");

    #[cfg(feature = "test-support")]
    /// Names the design, then holds until the screenshot is taken.
    fn named_held_fetch(
        _link: String,
        _folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        progress.calls(12, Some(51));
        let title = *SHOT_TITLE.lock().unwrap();
        progress.named(title);
        let started = Instant::now();
        while !SHOT_TAKEN.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(started.elapsed() < Duration::from_secs(60), "never shot");
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("Fetch ended.".into())
    }

    #[cfg(feature = "test-support")]
    #[test]
    #[ignore = "renders real GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
    fn onshape_title_screenshots() {
        let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        // English, and a long simulated German design name (the app has no translations yet).
        for (language, title) in [
            ("english", "Bracket - Part Studio 1"),
            (
                "german",
                "Getriebegehäuse Rückseite mit Befestigungsflansch - Teilestudio Lagerdeckel 1",
            ),
        ] {
            *SHOT_TITLE.lock().unwrap() = title;
            for (theme, name) in [
                (ThemePreference::Light, "day"),
                (ThemePreference::Dark, "night"),
            ] {
                SHOT_TAKEN.store(false, std::sync::atomic::Ordering::SeqCst);
                let mut h = window(true);
                h.ctx.set_theme(theme);
                h.state_mut().set_checker_for_test(true);
                h.state_mut().preview_solidcraft(Some("0.50.0"));
                h.state_mut()
                    .set_out_dir(PathBuf::from("/Users/demo/Documents/Converted"));
                h.state_mut().set_verify(false);
                h.state_mut().set_link_transport_for_test(
                    Arc::new(MockNetwork::default()),
                    named_held_fetch,
                );
                paste(&mut h, LINK);
                let started = Instant::now();
                while h
                    .query_by_label(&format!("{title}.onshape-bundle"))
                    .is_none()
                {
                    assert!(started.elapsed() < Duration::from_secs(30), "no title");
                    std::thread::sleep(Duration::from_millis(10));
                    h.step();
                }
                h.run_steps(3);
                h.render()
                    .unwrap()
                    .save(dir.join(format!("onshape-title-fetching-{language}-{name}.png")))
                    .unwrap();
                SHOT_TAKEN.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// Reports 12 calls, then loops until the window's Stop button asks it to end.
    fn stoppable_fetch(
        _link: String,
        _folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        progress.calls(12, Some(51));
        let started = Instant::now();
        while !progress.stop_requested() {
            assert!(started.elapsed() < Duration::from_secs(30), "never stopped");
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(App::budget_stop_message_for_test())
    }

    #[test]
    fn stop_ends_an_onshape_fetch_and_offers_continue_fetching() {
        let mut h = window(true);
        let output = tempfile::tempdir().unwrap();
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), stoppable_fetch);
        output_picker(&mut h, output.path());
        paste(&mut h, LINK);
        let started = Instant::now();
        while h
            .query_by_label("Fetching from Onshape: 12 of about 51 API calls…")
            .is_none()
        {
            assert!(started.elapsed() < Duration::from_secs(20), "no call count");
            std::thread::sleep(Duration::from_millis(10));
            h.step();
        }
        h.get_by_label("Stop").click_accesskit();
        while h.query_by_label("Continue fetching").is_none() {
            assert!(started.elapsed() < Duration::from_secs(20), "no button");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(3);
        assert_eq!(h.state().rows()[0].status, Status::Stopped);
        assert!(h.query_by_label("Failed").is_none());
    }

    #[test]
    fn a_request_limit_stop_offers_continue_fetching_without_command_line_words() {
        let mut h = window(true);
        let output = tempfile::tempdir().unwrap();
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), budget_then_ok_fetch);
        output_picker(&mut h, output.path());
        paste(&mut h, LINK);
        let started = Instant::now();
        while h.query_by_label("Continue fetching").is_none() {
            assert!(started.elapsed() < Duration::from_secs(20), "no button");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(3);
        let report = h.state().rows()[0].report.as_ref().unwrap();
        let message = report["error"]["message"].as_str().unwrap().to_owned();
        assert!(!message.contains("--"), "{message}");
        // The numbers are stated, with a thousands separator.
        assert_eq!(
            message,
            "Stopped after 50 Onshape API calls. About 1,500 more are needed; Continue fetching uses them from your yearly allowance. What's fetched so far is kept."
        );
        // It is resumable, so it does not look like a failure.
        assert_eq!(h.state().rows()[0].status, Status::Stopped);
        h.get_by_label("Stopped");
        assert!(h.query_by_label("Failed").is_none());
        assert!(h.query_by_label_contains(&message).is_some());
        assert!(h.query_by_label("Add matching STEP").is_none());
        assert!(h.query_by_label_contains("Report this file").is_none());
        // The problem panel is not about it, and the footer does not count a failure.
        assert!(h.query_by_label("Report this design…").is_none());
        assert!(h
            .query_by_label_contains("0 converted, 1 stopped")
            .is_some());
        assert!(h.query_by_label_contains("1 failed").is_none());
        h.get_by_label("Continue fetching").click_accesskit();
        h.run_steps(3);
        // It asks before using more of the yearly calls, and says how many it needs.
        assert!(h
            .query_by_label_contains(
                "needs about 1,500 more of your yearly Onshape API calls (2,500 on Free plans)"
            )
            .is_some());
        h.get_by_label("Continue").click_accesskit();
        h.step();
        wait_for_conversion(&mut h);
        assert!(h.query_by_label("Continue fetching").is_none());
        assert_eq!(h.state().rows()[0].status, Status::Converted);
    }

    fn big_estimate(_link: String, _token: Option<PathBuf>) -> Result<u32, String> {
        Ok(120)
    }

    fn small_estimate(_link: String, _token: Option<PathBuf>) -> Result<u32, String> {
        Ok(50)
    }

    static COUNTED_FETCHES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    fn counted_fetch(
        link: String,
        folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        COUNTED_FETCHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        fetch(link, folder, progress)
    }
    static CANCELLED_FETCHES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    fn cancelled_fetch(
        link: String,
        folder: &Path,
        progress: &mut dyn dutchman::onshape::FetchProgress,
    ) -> Result<PathBuf, String> {
        CANCELLED_FETCHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        fetch(link, folder, progress)
    }

    fn estimating_window(
        estimate: fn(String, Option<PathBuf>) -> Result<u32, String>,
        fetch: fn(
            String,
            &Path,
            &mut dyn dutchman::onshape::FetchProgress,
        ) -> Result<PathBuf, String>,
    ) -> (Harness<'static, App>, tempfile::TempDir) {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), fetch);
        h.state_mut().set_estimate_for_test(estimate);
        output_picker(&mut h, output.path());
        paste(&mut h, LINK);
        h.run_steps(3);
        (h, output)
    }

    #[test]
    fn a_fetch_above_fifty_calls_asks_and_only_cancel_or_continue_decides() {
        let before = CANCELLED_FETCHES.load(std::sync::atomic::Ordering::SeqCst);
        let (mut h, _output) = estimating_window(big_estimate, cancelled_fetch);
        let started = Instant::now();
        while h
            .query_by_label_contains("This document needs about 120 of your yearly Onshape API calls (2,500 on Free plans). Continue?")
            .is_none()
        {
            assert!(started.elapsed() < Duration::from_secs(20), "no question");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        // The window is centred on the app window.
        let at = h
            .get_by_label_contains("This document needs about 120")
            .rect()
            .center();
        assert!(
            (at.x - 590.0).abs() < 150.0 && (at.y - 500.0).abs() < 150.0,
            "{at:?}"
        );
        // Nothing is fetched while it waits.
        assert_eq!(
            CANCELLED_FETCHES.load(std::sync::atomic::Ordering::SeqCst),
            before
        );
        assert!(!h.state().running());
        h.get_by_label("Cancel").click_accesskit();
        h.run_steps(3);
        assert_eq!(
            CANCELLED_FETCHES.load(std::sync::atomic::Ordering::SeqCst),
            before
        );
        assert!(h.query_by_label("Continue").is_none());
        assert!(h.state().rows().is_empty());
    }

    #[test]
    fn continuing_past_the_question_fetches_without_asking_again() {
        let before = COUNTED_FETCHES.load(std::sync::atomic::Ordering::SeqCst);
        let (mut h, _output) = estimating_window(big_estimate, counted_fetch);
        let started = Instant::now();
        while h.query_by_label("Continue").is_none() {
            assert!(started.elapsed() < Duration::from_secs(20), "no question");
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.get_by_label("Continue").click_accesskit();
        h.step();
        wait_for_conversion(&mut h);
        assert_eq!(
            COUNTED_FETCHES.load(std::sync::atomic::Ordering::SeqCst),
            before + 1
        );
        assert_eq!(h.state().rows()[0].status, Status::Converted);
    }

    #[test]
    fn a_fetch_of_fifty_calls_or_fewer_starts_without_asking() {
        let (mut h, _output) = estimating_window(small_estimate, fetch);
        wait_for_conversion(&mut h);
        assert!(h
            .query_by_label_contains("yearly Onshape API calls")
            .is_none());
        assert_eq!(h.state().rows()[0].status, Status::Converted);
    }

    #[test]
    fn a_panicking_onshape_fetch_still_ends_the_job() {
        assert!(fetch_error_message(panicking_fetch).contains("stopped unexpectedly"));
    }

    #[test]
    fn saving_without_a_pending_link_focuses_the_account_link_field() {
        let mut h = window(false);
        h.state_mut()
            .set_clipboard_for_test(|| (vec![], LINK.into()));
        h.state_mut().preview_onshape_saved("Test designer");
        h.run_steps(4);
        let field = h.get_by_label("Paste your Onshape Part Studio link");
        assert_eq!(field.accesskit_node().value().as_deref(), Some(LINK));
        assert!(field.accesskit_node().is_focused());
        assert!(h.query_by_label("Your Onshape API keys").is_none());
        assert!(h.state().rows().is_empty());
    }

    #[test]
    fn disconnected_paste_keeps_the_link_and_continues_after_save() {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(false);
        h.state_mut().set_out_dir(output.path().into());
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), fetch);
        paste(&mut h, LINK);
        h.run_steps(3);
        assert!(h.query_by_label("Your Onshape API keys").is_some());
        assert!(h.state().rows().is_empty());
        h.state_mut().preview_onshape_saved("Test designer");
        h.step();
        assert_eq!(h.state().rows().len(), 1);
        assert!(h.state().running());
        h.run_steps(2);
        assert!(h.query_by_label("Your Onshape API keys").is_none());
        assert!(h.query_by_label("Onshape connected ✓").is_some());
        wait_for_conversion(&mut h);
        assert_bundle_survives_exit(h, output.path());
    }

    #[test]
    fn opening_from_the_connected_dialog_closes_it_and_starts_a_main_job() {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_out_dir(output.path().into());
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), fetch);
        h.get_by_label("Onshape connected ✓").click_accesskit();
        h.run_steps(3);
        // The dialog gives its empty link editor keyboard focus.
        paste(&mut h, LINK);
        h.run_steps(3);
        h.get_all_by_label("Open")
            .find(|node| !node.accesskit_node().is_disabled())
            .unwrap()
            .click_accesskit();
        h.step();
        h.run_steps(2);
        assert!(h
            .query_by_label("Connected to Onshape as Test designer")
            .is_none());
        assert_eq!(h.state().rows().len(), 1);
        wait_for_conversion(&mut h);
        assert_bundle_survives_exit(h, output.path());
    }

    #[test]
    fn native_clipboard_files_take_precedence_over_text_and_paths_queue_like_drops() {
        let mut h = window(false);
        let input = fixture("generated_unit_cube.f3d");
        let native = input.clone();
        h.state_mut()
            .set_clipboard_for_test(move || (vec![native.clone()], LINK.into()));
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                command: true,
                ctrl: true,
                ..Default::default()
            },
        });
        h.step();
        h.run_steps(3);
        assert!(h.query_by_label("Your Onshape API keys").is_none());
        assert!(h
            .query_by_label_contains("generated_unit_cube.f3d")
            .is_some());
        let mut h = window(false);
        paste(
            &mut h,
            &format!(
                "{}\n{}",
                input.display(),
                fixture("onshape-revolve.onshape-bundle").display()
            ),
        );
        h.run_steps(3);
        assert!(h
            .query_by_label_contains("generated_unit_cube.f3d")
            .is_some());
        assert!(h
            .query_by_label_contains("onshape-revolve.onshape-bundle")
            .is_some());
    }

    #[test]
    fn https_paste_asks_before_downloading_then_converts_in_the_main_list() {
        let output = tempfile::tempdir().unwrap();
        let network = Arc::new(MockNetwork::default());
        let mut h = window(false);
        h.state_mut().set_out_dir(output.path().into());
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(network.clone(), fetch);
        paste(&mut h, "https://example.test/generated_unit_cube.f3d");
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.query_by_label("Download").is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
            h.step();
        }
        assert_eq!(*network.methods.lock().unwrap(), vec!["HEAD"]);
        assert!(h.query_by_label_contains("from example.test?").is_some());
        h.get_by_label("Download").click_accesskit();
        h.step();
        wait_for_conversion(&mut h);
        assert_eq!(*network.methods.lock().unwrap(), vec!["HEAD", "GET"]);
        assert_eq!(h.state().rows().len(), 1);
        assert!(h.state().rows()[0].report.as_ref().unwrap()["ok"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn unsupported_paste_gives_a_clear_notice() {
        let mut h = window(false);
        paste(&mut h, "https://example.test/share/123");
        h.run_steps(3);
        assert!(h
            .query_by_label("Dutchman can't open this link. Download the file and drop it here.")
            .is_some());
    }

    #[test]
    fn an_onshape_url_dropped_as_text_starts_the_same_main_job() {
        let output = tempfile::tempdir().unwrap();
        let mut h = window(true);
        h.state_mut().set_out_dir(output.path().into());
        h.state_mut().set_verify(false);
        h.state_mut()
            .set_link_transport_for_test(Arc::new(MockNetwork::default()), fetch);
        h.input_mut().dropped_files.push(egui::DroppedFile {
            name: LINK.into(),
            mime: "text/uri-list".into(),
            ..Default::default()
        });
        h.step();
        assert_eq!(h.state().rows().len(), 1);
        wait_for_conversion(&mut h);
        assert_bundle_survives_exit(h, output.path());
    }
}

/// The card names the feature the history is on and says when SolidCraft measures it.
/// With `DUTCHMAN_GUI_SCREENSHOTS`, it also saves both themes, plain and measured.
#[cfg(feature = "test-support")]
#[test]
fn a_running_card_shows_the_feature_and_the_measured_note() {
    use egui_kittest::kittest::NodeT as _;

    for (theme, label) in [
        (ThemePreference::Light, "Day"),
        (ThemePreference::Dark, "Night"),
    ] {
        for measured in [false, true] {
            let mut harness = screenshot_harness(theme);
            harness.state_mut().preview_solidcraft(Some("0.50.1"));
            harness.state_mut().set_checker_for_test(true);
            harness
                .state_mut()
                .preview_running_for_test(dutchman_gui::app::Row {
                    input: PathBuf::from("Bracket.f3d"),
                    output: None,
                    status: Status::Converting,
                    report: None,
                    job: None,
                    size: None,
                    progress: Some(dutchman_core::ProgressEvent {
                        stage: dutchman_core::Stage::Map,
                        elapsed_ms: 42_000,
                        designs_read: 1,
                        designs_total: Some(1),
                        heartbeat: true,
                        running: Some(dutchman_core::RunningStep {
                            kind: dutchman_core::StepKind::Preparing,
                            name: Some("Bracket".into()),
                            elapsed_ms: 12_000,
                            usual_ms: dutchman_core::StepKind::Preparing.usual_ms(),
                            limit_ms: None,
                            consequence: None,
                            feature: Some(dutchman_core::FeatureStep {
                                index: 29,
                                total: 37,
                                name: Some("Extrude 1".into()),
                                measured,
                            }),
                        }),
                    }),
                });
            harness.run_steps(8);
            assert!(harness
                .query_by_label("Building history for Bracket: 12 s")
                .is_some());
            let feature = harness.get_by_label("Feature 29 of 37 · Extrude 1").rect();
            assert!(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 800.0))
                    .contains_rect(feature)
            );
            // The design is already read (1/1), but feature mapping is only 29/37.
            let progress = harness.get_by_role(egui::accesskit::Role::ProgressIndicator);
            assert_eq!(progress.accesskit_node().numeric_value(), Some(78.0));
            let track = progress.rect();
            assert!(track.top() >= feature.bottom());
            assert!((track.left() - feature.left()).abs() < 1.0);
            // The results card has 436 points of inner width at this window size.
            assert!((track.width() - 436.0).abs() < 2.0, "{track:?}");
            let palette = if label == "Day" {
                dutchman_gui::theme::DAY
            } else {
                dutchman_gui::theme::NIGHT
            };
            assert!(harness.output().shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect)
                    if rect.rect == track && rect.fill == palette.surface_2)
            }));
            assert!(harness.output().shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect)
                    if rect.rect.min == track.min
                        && (rect.rect.width() / track.width() - 29.0 / 37.0).abs() < 0.001
                        && rect.fill == palette.spectre)
            }));
            // Only the input row names the format; the running card doesn't repeat it.
            assert_eq!(
                harness
                    .query_all_by_label("Fusion .f3d: reads geometry")
                    .count(),
                1
            );
            let note = dutchman_core::progress_words::measured();
            assert_eq!(harness.query_by_label(note).is_some(), measured);
            if let Some(dir) = std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS") {
                let dir = PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                let state = if measured { "measured" } else { "mapping" };
                harness
                    .render()
                    .unwrap()
                    .save(dir.join(format!("feature-progress-{state}-{label}.png")))
                    .unwrap();
            }
        }
    }
}

/// One case of the report-flow walk: the file, its saved report, and how it ended.
#[cfg(feature = "test-support")]
struct FlowCase {
    shot: &'static str,
    input: &'static str,
    report: &'static str,
}

/// Clicks the first of `labels` the window shows; whether one was there.
#[cfg(feature = "test-support")]
fn click_first(harness: &mut Harness<'static, App>, labels: &[&str]) -> bool {
    for label in labels {
        let found = match harness.query_all_by_label(label).next() {
            Some(node) => {
                node.click_accesskit();
                true
            }
            None => false,
        };
        if found {
            harness.run_steps(12);
            return true;
        }
    }
    false
}

/// Presses Copy and open DM, if the window shows it: what it copied.
#[cfg(feature = "test-support")]
fn press_dm_for_shot(harness: &mut Harness<'static, App>) -> Option<String> {
    let found = match harness.query_all_by_label("Copy and open DM").next() {
        Some(node) => {
            node.click_accesskit();
            true
        }
        None => false,
    };
    if !found {
        return None;
    }
    harness.step();
    let copied = last_copy(harness).unwrap_or_default();
    harness.run_steps(12);
    Some(copied)
}

/// What the last frame copied, if anything.
#[cfg(feature = "test-support")]
fn last_copy(harness: &Harness<'static, App>) -> Option<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
}

/// The report-flow walk's five cases, drawn from the reports a real run saved: each result,
/// its way to report, and the DM step, plus the top bar's link and a slow file's Stop. Run
/// on main and on a branch to compare. `DUTCHMAN_REPORT_FLOW_CASES` is the walk's `cases`
/// folder (the five files and the `1-full-FHCS (SolidCraft)` folder of reports).
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the report-flow walk; set DUTCHMAN_REPORT_FLOW_CASES"]
fn report_flow_case_screenshots() {
    let cases = PathBuf::from(std::env::var_os("DUTCHMAN_REPORT_FLOW_CASES").unwrap());
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let reports = cases.join("1-full-FHCS (SolidCraft)");
    let work = tempfile::tempdir().unwrap();
    let size = match std::env::var("DUTCHMAN_REPORT_FLOW_SIZE").as_deref() {
        Ok("1280x800") => egui::vec2(1280.0, 800.0),
        _ => egui::vec2(1600.0, 1000.0),
    };
    let mut index = String::from("# Report flow, five cases\n\n");
    let mut measurements = format!(
        "# Report flow measurements\n\nSaved walkthrough reports, rendered at {} × {}; running progress is replayed, not a conversion timing.\n\n| Case | Question top (px) | DM bottom (px) | DM right (px) | Step 1 bottom (px) | More bottom (px) | Footer top (px) |\n|---|---:|---:|---:|---:|---:|---:|\n", size.x, size.y,
    );
    let harness = || {
        let mut h = Harness::builder()
            .with_size(size)
            .build_eframe(|cc| test_app(cc));
        h.ctx.set_theme(ThemePreference::Light);
        h.state_mut().preview_solidcraft(Some("0.50.1"));
        h.state_mut().set_checker_for_test(true);
        h.state_mut().set_file_manager(|_| Ok(()));
        h.state_mut().set_out_dir(work.path().join("converted"));
        h
    };
    let mut h = harness();
    h.run_steps(8);
    shot(
        &mut h,
        &dir,
        &mut index,
        "T00-launch",
        "day",
        "Empty window.",
    );
    if click_first(&mut h, &["Report a problem"]) {
        shot(
            &mut h,
            &dir,
            &mut index,
            "T01-topbar-report",
            "day",
            "After the top bar's Report a problem.",
        );
    }
    let flow = [
        FlowCase {
            shot: "C1-full",
            input: "1-full-FHCS.f3d",
            report: "1-full-FHCS.report.json",
        },
        FlowCase {
            shot: "C2-partial",
            input: "2-partial-Shim.f3d",
            report: "2-partial-Shim.report.json",
        },
        FlowCase {
            shot: "C3-geometry",
            input: "3-geometry-Keystone.f3d",
            report: "3-geometry-Keystone.report.json",
        },
        FlowCase {
            shot: "C4-failed",
            input: "4-broken-FHCS.f3d",
            report: "4-broken-FHCS.failed.json",
        },
        FlowCase {
            shot: "C5-slow",
            input: "5-slow-Voron.f3z",
            report: "5-slow-Voron.report.json",
        },
    ];
    let only = std::env::var("DUTCHMAN_REPORT_FLOW_CASE").ok();
    for case in flow {
        if only.as_deref().is_some_and(|wanted| wanted != case.shot) {
            continue;
        }
        let input = cases.join(case.input);
        let mut report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(reports.join(case.report)).unwrap()).unwrap();
        let ok = report["ok"] == serde_json::json!(true);
        let output = work
            .path()
            .join("converted")
            .join(Path::new(case.input).with_extension("sc"));
        report["input"]["path"] = serde_json::json!(input);
        if ok {
            report["output"]["path"] = serde_json::json!(output);
        }
        let mut h = harness();
        if case.shot == "C5-slow" {
            h.state_mut().add_inputs([input.clone()]);
            wait_for_estimate(&mut h);
            shot(
                &mut h,
                &dir,
                &mut index,
                "C5-01-queued",
                "day",
                "Queued with its estimate.",
            );
            h.state_mut()
                .preview_running_for_test(dutchman_gui::app::Row {
                    input: input.clone(),
                    output: Some(output.clone()),
                    status: Status::Converting,
                    progress: Some(dutchman_core::ProgressEvent {
                        stage: dutchman_core::Stage::Decode,
                        elapsed_ms: 179_000,
                        designs_read: 0,
                        designs_total: None,
                        heartbeat: true,
                        running: None,
                    }),
                    report: None,
                    job: None,
                    size: None,
                });
            h.run_steps(8);
            shot(
                &mut h,
                &dir,
                &mut index,
                "C5-02-converting",
                "day",
                "Three minutes into reading.",
            );
            assert!(click_first(&mut h, &["Stop"]));
            h.get_by_label(dutchman_gui::report_panel::STOPPING_BUTTON);
            h.get_by_label(dutchman_gui::report_panel::STOPPING);
            shot(
                &mut h,
                &dir,
                &mut index,
                "C5-03-stop-clicked",
                "day",
                "Right after Stop.",
            );
            if let Some(text) = press_dm_for_shot(&mut h) {
                std::fs::write(dir.join("C5-04-dm-while-running.txt"), text).unwrap();
                shot(
                    &mut h,
                    &dir,
                    &mut index,
                    "C5-04-dm-while-running",
                    "day",
                    "The DM step while it runs.",
                );
            }
            h = harness();
        }
        let size_bytes = std::fs::metadata(&input).map(|m| m.len()).ok();
        h.state_mut()
            .preview_result_for_test(dutchman_gui::app::Row {
                input: input.clone(),
                output: ok.then(|| output.clone()),
                status: if ok {
                    Status::Converted
                } else {
                    Status::Failed
                },
                progress: None,
                report: Some(report),
                job: None,
                size: size_bytes,
            });
        h.run_steps(12);
        let question = h.get_by_label(dutchman_gui::report_panel::QUESTION).rect();
        let dm = h.get_by_label("Copy and open DM").rect();
        assert!(
            question.top() >= 80.0 && dm.bottom() < size.y - 60.0,
            "{}: {question:?}, {dm:?}",
            case.shot
        );
        assert!(
            dm.left() >= 0.0 && dm.right() < 850.0,
            "{}: {dm:?}",
            case.shot
        );
        let bottom = h.get_by_label("Convert").rect().top();
        for label in [
            "Join our Discord",
            dutchman::present::SEND_JOIN_BUTTON,
            dutchman::present::SEND_JOIN_NOTE,
        ] {
            let rect = h.get_by_label(label).rect();
            assert!(
                rect.top() >= question.bottom() && rect.bottom() < bottom,
                "{}: {label}: {rect:?}, footer {bottom}",
                case.shot
            );
        }
        if size.y == 1000.0 {
            let more = h.get_by_label("More").rect();
            assert!(
                more.bottom() < bottom,
                "{}: More {more:?}, footer {bottom}",
                case.shot
            );
        }
        measurements.push_str(&format!(
            "| {} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
            case.shot,
            question.top(),
            dm.bottom(),
            dm.right(),
            h.get_by_label(dutchman::present::SEND_JOIN_NOTE)
                .rect()
                .bottom(),
            h.get_by_label("More").rect().bottom(),
            bottom
        ));
        shot(
            &mut h,
            &dir,
            &mut index,
            &format!("{}-result", case.shot),
            "day",
            "The result as it first shows.",
        );
        assert!(click_first(&mut h, &[dutchman_gui::app::REPORT_THIS_FILE]));
        let dm = match h.query_all_by_label("Copy and open DM").next() {
            Some(node) => {
                node.scroll_to_me();
                true
            }
            None => false,
        };
        if dm {
            h.run_steps(12);
        }
        shot(
            &mut h,
            &dir,
            &mut index,
            &format!("{}-report", case.shot),
            "day",
            "Its way to report a problem.",
        );
        let text = press_dm_for_shot(&mut h).expect("Every result offers a DM");
        assert!(text.ends_with(if ok {
            "What's wrong or missing: "
        } else {
            "Anything else we should know: "
        }));
        {
            std::fs::write(dir.join(format!("{}-dm.txt", case.shot)), text).unwrap();
            h.run_steps(8);
            shot(
                &mut h,
                &dir,
                &mut index,
                &format!("{}-dm-copied", case.shot),
                "day",
                "After Copy and open DM.",
            );
        }
        if click_first(&mut h, &["More"]) {
            shot(
                &mut h,
                &dir,
                &mut index,
                &format!("{}-more", case.shot),
                "day",
                "More opened.",
            );
        }
    }
    if only.is_some() {
        std::fs::write(dir.join("C2-MEASUREMENTS.md"), measurements).unwrap();
    } else {
        std::fs::write(dir.join("MEASUREMENTS.md"), measurements).unwrap();
        std::fs::write(dir.join("INDEX.md"), index).unwrap();
    }
}

/// Reuses the normal screenshot encoder for a synthetic folder at two window widths.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn designs_header_screenshots() {
    if std::env::var_os("DUTCHMAN_DESIGNS_HEADER_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "designs_header_screenshots",
                "--nocapture",
            ])
            .env("DUTCHMAN_DESIGNS_HEADER_CHILD", "1")
            .env("HOME", home.path())
            .env("LOCALAPPDATA", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = work.path().join(
        "synthetic-designs/long-folder-for-mechanical-projects/working-copies/current-designs",
    );
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(
        fixture("generated_unit_cube.f3d"),
        folder.join("generated_unit_cube.f3d"),
    )
    .unwrap();
    let multiple = work.path().join("synthetic-designs/three-designs");
    std::fs::create_dir_all(&multiple).unwrap();
    for name in ["cube-one.f3d", "cube-two.f3d", "cube-three.f3d"] {
        std::fs::copy(fixture("generated_unit_cube.f3d"), multiple.join(name)).unwrap();
    }
    let mut index = String::from("# Designs header\n\nSynthetic designs selected by folder.\n\n");
    for (case, input, connected) in [
        ("single", &folder, false),
        ("three", &multiple, false),
        ("connected", &folder, true),
    ] {
        for (theme, name) in [
            (ThemePreference::Light, "light"),
            (ThemePreference::Dark, "dark"),
        ] {
            for width in [1180.0, 900.0] {
                let mut h = Harness::builder()
                    .with_size(egui::vec2(width, 800.0))
                    .build_eframe(|cc| test_app(cc));
                h.ctx.set_theme(theme);
                h.state_mut().set_checker_for_test(true);
                h.state_mut().preview_solidcraft(Some("0.44.9"));
                h.state_mut()
                    .preview_onshape(connected.then_some("Test designer"));
                h.state_mut().add_inputs([input.clone()]);
                h.state_mut().set_out_dir(work.path().join("converted"));
                wait_for_estimate(&mut h);
                shot(
                    &mut h,
                    &dir,
                    &mut index,
                    &format!("{case}-{width:.0}x800"),
                    name,
                    "Folder and conversion estimate.",
                );
            }
        }
    }
    std::fs::write(dir.join("index.md"), index).unwrap();
}

fn many_results_onshape_harness(
    out: &Path,
    theme: ThemePreference,
    connected: bool,
) -> Harness<'static, App> {
    let size = dutchman_gui::FIT_SIZE;
    let mut h = Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .build_eframe(|cc| test_app(cc));
    h.ctx.set_theme(theme);
    h.state_mut()
        .preview_onshape(connected.then_some("Test designer"));
    let folder = out.join("Designs");
    std::fs::create_dir_all(&folder).unwrap();
    for name in [
        "generated_unit_cube.f3d",
        "synthetic_cube.ipt",
        "solidworks_cube.sldprt",
    ] {
        std::fs::copy(fixture(name), folder.join(name)).unwrap();
    }
    h.state_mut().add_inputs([folder]);
    h.state_mut().set_out_dir(out.join("converted"));
    h.state_mut().set_verify(false);
    h.run_steps(8);
    h.get_by_label("Advanced").click_accesskit();
    h.run_steps(8);
    convert(&mut h);
    h.get_by_label("Change…").click_accesskit();
    h.run_steps(8);
    h
}

#[test]
fn connected_onshape_keeps_results_in_their_own_column() {
    for theme in [ThemePreference::Light, ThemePreference::Dark] {
        let out = tempfile::tempdir().unwrap();
        let mut h = many_results_onshape_harness(out.path(), theme, true);
        assert_eq!(h.state().rows().len(), 3);
        assert!(!h.state().page_scrolls());
        assert!(h
            .query_by_label("Paste your Onshape Part Studio link")
            .is_none());
        let convert = h.get_by_label("Convert").rect();
        assert!(h.get_by_label("Open diagnostics folder").rect().bottom() <= convert.top());
        h.get_by_label("Onshape connected ✓").click_accesskit();
        h.run_steps(4);
        h.get_by_label("Paste your Onshape Part Studio link");
        assert!(!h.state().page_scrolls());
    }
}

/// The as-is footer is one line under every control, in both themes, and the left column's
/// controls stay on screen with it.
#[cfg(feature = "test-support")]
#[test]
fn as_is_footer_is_one_quiet_line_below_the_controls() {
    for theme in [ThemePreference::Light, ThemePreference::Dark] {
        let out = tempfile::tempdir().unwrap();
        let h = many_results_onshape_harness(out.path(), theme, true);
        let footer = h.get_by_label_contains("provided as is").rect();
        let convert = h.get_by_label("Convert").rect();
        assert!(convert.bottom() <= footer.top());
        assert!(footer.bottom() <= h.ctx.content_rect().bottom());
        assert!(footer.height() < 24.0, "one line, not a banner");
        h.get_by_label("More about Dutchman and your files");
        assert!(!h.state().page_scrolls());
    }
}

/// Windows and macOS offer "Export from Fusion…" in the Convert bar (the tests offer it
/// everywhere): it must not make the left column overflow at 1280x800.
#[test]
fn the_fusion_button_in_the_convert_bar_keeps_the_left_column_inside() {
    for theme in [ThemePreference::Light, ThemePreference::Dark] {
        let out = tempfile::tempdir().unwrap();
        let h = many_results_onshape_harness(out.path(), theme, true);
        let button = h.get_by_label("Export from Fusion…").rect();
        let convert = h.get_by_label("Convert").rect();
        assert!(
            (button.center().y - convert.center().y).abs() < 1.0,
            "the button is on the Convert bar's row"
        );
        assert!(h.get_by_label("Open diagnostics folder").rect().bottom() <= convert.top());
        assert!(button.right() <= 1280.0);
        assert!(!h.state().page_scrolls());
    }
}

/// The main window with the footer at 1280x800, Day and Night: an English design name, and a
/// long German one (the app has no translations, so the footer stays English in both).
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn as_is_footer_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (language, file) in [
        ("en", "Cube.ipt"),
        (
            "de-DE",
            "Getriebegehäuse_Hauptwelle_Lagerdeckel_Konstruktionsstand_Revision_C.ipt",
        ),
    ] {
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            let docs = PathBuf::from("/Users/demo/Documents");
            let mut harness = screenshot_harness(theme);
            harness.set_size(egui::vec2(1280.0, 800.0));
            harness.state_mut().preview_solidcraft(Some("0.50.0"));
            harness.state_mut().set_out_dir(docs.join("Converted"));
            let mut report = developer_notes_report();
            report["mode"] = serde_json::json!("history");
            let stem = file.trim_end_matches(".ipt");
            harness
                .state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: docs.join(file),
                    output: Some(docs.join("Converted").join(format!("{stem}.sc"))),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: None,
                    report: Some(report),
                });
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("as-is-footer-{language}"),
                name,
                "The main window with the as-is line at the bottom.",
            );
        }
    }
    std::fs::write(dir.join("as-is-footer.md"), index).unwrap();
}

/// Synthetic result cards, identical in both revisions for screenshot comparison.
#[cfg(feature = "test-support")]
#[test]
fn report_labels_screenshots() {
    let Some(dir) = std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS") else {
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut index =
        String::from("# Result labels at 1180 × 800\n\nSynthetic reports in both themes.\n\n");
    for (theme, name) in [
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ] {
        for (case, mode) in [
            ("partial", "partial_history"),
            ("geometry-only", "import"),
            ("full", "history"),
        ] {
            let mut h = screenshot_harness(theme);
            h.set_size(egui::vec2(1180.0, 800.0));
            h.state_mut().add_inputs([PathBuf::from("synthetic.f3d")]);
            h.state_mut().preview_solidcraft(Some("0.44.1"));
            h.state_mut().set_checker_for_test(true);
            h.state_mut().set_out_dir(PathBuf::from("converted"));
            let report = serde_json::json!({
                "ok": true, "mode": mode,
                "history": {"mapped": if case == "partial" {2} else {3}, "total": 3},
                "verification": {"status": if case == "partial" {"partial"} else {"verified"}},
                "losses": []
            });
            h.state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: PathBuf::from("synthetic.f3d"),
                    output: Some(PathBuf::from("converted/synthetic.sc")),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: Some(100_000),
                    report: Some(report),
                });
            h.run_steps(16);
            let gap = h
                .get_by_label(dutchman_gui::report_panel::QUESTION)
                .rect()
                .top()
                - h.get_by_label("Change…").rect().bottom();
            let description = format!(
                "One synthetic result, Advanced closed. Report heading is {gap:.1} points below the output control."
            );
            shot(&mut h, &dir, &mut index, case, name, &description);
        }
    }
    std::fs::write(dir.join("index.md"), index).unwrap();
}

/// The 56-point allowance includes the output card's padding and the section gap.
#[cfg(feature = "test-support")]
#[test]
fn a_single_result_keeps_reporting_close_below_output() {
    for (theme, height) in [
        (ThemePreference::Light, 800.0),
        (ThemePreference::Dark, 800.0),
        (ThemePreference::Light, 1000.0),
        (ThemePreference::Dark, 1000.0),
    ] {
        let mut h = screenshot_harness(theme);
        h.set_size(egui::vec2(1180.0, height));
        h.state_mut().add_inputs([PathBuf::from("synthetic.f3d")]);
        h.state_mut()
            .preview_result_for_test(dutchman_gui::app::Row {
                input: PathBuf::from("synthetic.f3d"),
                output: Some(PathBuf::from("converted/synthetic.sc")),
                status: Status::Converted,
                progress: None,
                job: None,
                size: Some(100_000),
                report: Some(serde_json::json!({"ok": true, "mode": "partial_history"})),
            });
        h.run_steps(16);
        let output_bottom = h.get_by_label("Change…").rect().bottom();
        let report_top = h
            .get_by_label(dutchman_gui::report_panel::QUESTION)
            .rect()
            .top();
        let gap = report_top - output_bottom;
        assert!(
            (0.0..=56.0).contains(&gap),
            "report gap {gap} points exceeds 56"
        );
        h.get_by_label(dutchman::present::SEND_JOIN_BUTTON)
            .scroll_to_me();
        h.run_steps(8);
        assert!(
            h.get_by_label(dutchman::present::SEND_JOIN_BUTTON)
                .rect()
                .bottom()
                < h.get_by_label("Convert").rect().top()
        );
    }
}

/// The notes of a small Inventor part as issue 926 quotes them: one that matters, three that
/// change nothing the person sees (internal records, a statistic of nothing), and a closing line.
#[cfg(feature = "test-support")]
fn developer_notes_report() -> serde_json::Value {
    serde_json::json!({"ok": true, "mode": "import", "losses": [
        {"level": "Degraded", "code": "inventor/source.kernel-history-unclosed",
         "subject": "inventor/source.kernel-history-unclosed", "stage": "decode",
         "message": "The kernel carrier's delta_state chain does not frame or close; features have no history states."},
        {"level": "Degraded", "code": "inventor/rse.segment-pair-untyped",
         "subject": "inventor/rse.segment-pair-untyped", "stage": "decode",
         "message": "Retained 1 structurally paired RSe segment(s) without record semantics."},
        {"level": "Info", "code": null, "subject": "part positions", "stage": "map",
         "message": "0 parts placed as last shown in the original program (0 directly bound, 0 holders derived from displayed children), 0 at current poses, 0 as first placed (smart); first-placement reasons: 0 without a current pose, 0 ambiguous, 0 unsupported, 0 joint checks, 0 bounds checks, 0 inserted-design checks; 0 holder conflicts"},
        {"level": "Info", "code": "step/attribute.source-record-not-written",
         "subject": "step/attribute.source-record-not-written", "stage": "write",
         "message": "7 source attribute record(s) were not written to STEP"},
        {"level": "Info", "code": "step/native.source-record-omitted",
         "subject": "step/native.source-record-omitted", "stage": "write",
         "message": "67 source-native record(s) were not represented in STEP"},
        {"level": "Info", "code": null, "subject": "history", "stage": "map",
         "message": "flattened: the design carries no feature history"}
    ]})
}

#[cfg(feature = "test-support")]
fn developer_notes_row() -> dutchman_gui::app::Row {
    dutchman_gui::app::Row {
        input: PathBuf::from("synthetic_cube.ipt"),
        output: Some(PathBuf::from("converted/synthetic_cube.sc")),
        status: Status::Converted,
        progress: None,
        job: None,
        size: None,
        report: Some(developer_notes_report()),
    }
}

#[cfg(feature = "test-support")]
#[test]
fn notes_lead_with_a_plain_sentence_and_the_raw_codes_sit_behind_details() {
    let mut harness = harness();
    harness
        .state_mut()
        .preview_result_for_test(developer_notes_row());
    harness.run_steps(8);
    harness.get_by_label_contains("Losses:").click_accesskit();
    harness.run_steps(8);
    // The note that matters leads with a plain sentence; the raw words are not on the card.
    assert!(harness
        .query_by_label_contains("feature history couldn't be read")
        .is_some());
    assert!(harness.query_by_label_contains("delta_state").is_none());
    // Notes that change nothing are not listed, and are not counted.
    assert!(harness
        .query_by_label_contains("source-native record")
        .is_none());
    assert!(harness.query_by_label_contains("0 parts placed").is_none());
    assert!(harness
        .query_by_label("Losses: 1 Info, 1 Degraded")
        .is_some());
    // Details holds the raw words of the listed note, and nothing else.
    harness.get_by_label("Details").click_accesskit();
    harness.run_steps(8);
    assert!(harness.query_by_label_contains("delta_state").is_some());
    assert!(harness
        .query_by_label_contains("step/native.source-record-omitted")
        .is_none());
    assert!(harness
        .query_by_label_contains("step/attribute.source-record-not-written")
        .is_none());
    assert!(harness
        .query_by_label_contains("rse.segment-pair-untyped")
        .is_none());
    assert!(harness.query_by_label_contains("0 parts placed").is_none());
    // The notes that change nothing are in the full log.
    assert!(harness.query_by_label("Save the full log…").is_some());
}

/// Before/after evidence of the Results card with these notes. Needs a GPU renderer:
/// `DUTCHMAN_GUI_SCREENSHOTS=<dir> DUTCHMAN_SHOT_PREFIX=after cargo test -p dutchman-gui
/// --features test-support --test ui -- --ignored notes_card_screenshots`.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders with a GPU; set DUTCHMAN_GUI_SCREENSHOTS and DUTCHMAN_SHOT_PREFIX"]
fn notes_card_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    let prefix = std::env::var("DUTCHMAN_SHOT_PREFIX").unwrap_or_else(|_| "after".into());
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, name) in [
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ] {
        let mut harness = screenshot_harness(theme);
        harness.set_size(egui::vec2(760.0, 760.0));
        harness
            .state_mut()
            .preview_result_for_test(developer_notes_row());
        harness.run_steps(8);
        harness.get_by_label_contains("Losses:").click_accesskit();
        harness.run_steps(8);
        let mut index = String::new();
        shot(
            &mut harness,
            &dir,
            &mut index,
            &format!("{prefix}-notes"),
            name,
            "",
        );
        // Only the new card has a Details toggle.
        if harness.query_by_label("Details").is_some() {
            harness.get_by_label("Details").click_accesskit();
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("{prefix}-notes-details"),
                name,
                "",
            );
        }
    }
}

/// Import-note screenshots for the plain-sentence change: one finished conversion with several
/// notes (some that matter, some that change nothing), English names and long German names, Day
/// and Night, 1280x800. Run once per build under test with DUTCHMAN_SHOT_PREFIX=before|after.
/// Needs a GPU renderer; set DUTCHMAN_GUI_SCREENSHOTS to a folder.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders with a GPU; set DUTCHMAN_GUI_SCREENSHOTS and DUTCHMAN_SHOT_PREFIX"]
fn import_notes_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    let prefix = std::env::var("DUTCHMAN_SHOT_PREFIX").unwrap_or_else(|_| "after".into());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (variant, file, part, bolt) in [
        (
            "english",
            "Bracket.f3d",
            "Cover plate",
            "Cylinder head screw",
        ),
        (
            "german",
            "Getriebegehäuse_Abdeckplatte_Rückseite_Ausführung_B.f3d",
            "Getriebegehäuse_Abdeckplatte_Rückseite_Ausführung_B",
            "Schraubenverbindung Zylinderkopfschraube",
        ),
    ] {
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            let docs = PathBuf::from("/Users/demo/Documents");
            let mut harness = screenshot_harness(theme);
            harness.set_size(egui::vec2(1280.0, 800.0));
            harness.state_mut().set_checker_for_test(true);
            harness.state_mut().preview_solidcraft(Some("0.50.1"));
            harness.state_mut().set_out_dir(docs.join("Converted"));
            let output = docs.join("Converted").join(file.replace(".f3d", ".sc"));
            let report = serde_json::json!({
                "ok": true, "mode": "history",
                "history": {"mapped": 5, "total": 6},
                "losses": [
                {"level": "Degraded", "code": "history/body-fallback-imported",
                 "subject": format!("{part}: Extrude 3"), "stage": "map",
                 "message": format!("Extrude 3 of {part} could not be rebuilt from its sketch; the stored body was imported")},
                {"level": "Degraded", "code": "f3d/material.not-transferred",
                 "subject": bolt, "stage": "map",
                 "message": format!("Materials and appearances of {bolt} were not transferred")},
                {"level": "Degraded", "code": "history/rebuild-failed",
                 "subject": format!("{bolt}: Fillet 1"), "stage": "verify",
                 "message": "SolidCraft could not rebuild the feature"},
                {"level": "Info", "code": "dutchman/appearance.redundant-face",
                 "subject": part, "stage": "map",
                 "message": "A face color equal to its body color was not written"},
                {"level": "Info", "code": "history/body-scope-confirmed",
                 "subject": format!("{part}: Extrude 3"), "stage": "map",
                 "message": "The bodies this feature acts on were confirmed"},
                {"level": "Info", "code": "f3d/source.unindexed-records-retained",
                 "subject": "f3d/source.unindexed-records-retained", "stage": "decode",
                 "message": "Retained 3 unindexed record(s) without record semantics."},
                {"level": "Info", "code": null, "subject": "part positions", "stage": "map",
                 "message": "0 parts placed as last shown in the original program (0 directly bound), 0 at current poses, 0 as first placed (smart); 0 holder conflicts"},
                {"level": "Info", "code": "step/attribute.source-record-not-written",
                 "subject": "step/attribute.source-record-not-written", "stage": "write",
                 "message": "7 source attribute record(s) were not written to STEP"},
                {"level": "Info", "code": "step/native.source-record-omitted",
                 "subject": "step/native.source-record-omitted", "stage": "write",
                 "message": "67 source-native record(s) were not represented in STEP"}
            ]});
            harness
                .state_mut()
                .preview_result_for_test(dutchman_gui::app::Row {
                    input: docs.join(file),
                    output: Some(output),
                    status: Status::Converted,
                    progress: None,
                    job: None,
                    size: None,
                    report: Some(report),
                });
            harness.run_steps(8);
            if harness.query_by_label_contains("Losses:").is_some() {
                harness.get_by_label_contains("Losses:").click_accesskit();
                harness.run_steps(8);
            }
            if harness
                .query_by_label_contains("Report this file")
                .is_some()
            {
                harness
                    .get_by_label_contains("Report this file")
                    .scroll_to_me();
                harness.run_steps(8);
            }
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("import-notes-{prefix}-{variant}"),
                name,
                "Results card, Details closed.",
            );
            // Only the new card has a Details toggle.
            if harness.query_by_label("Details").is_some() {
                harness.get_by_label("Details").click_accesskit();
                harness.run_steps(8);
                // The card's header and buttons stay in view: scroll the panel to the top.
                harness.get_by_label("Open in SolidCraft").scroll_to_me();
                harness.run_steps(8);
                shot(
                    &mut harness,
                    &dir,
                    &mut index,
                    &format!("import-notes-{prefix}-{variant}-details"),
                    name,
                    "Results card, Details open.",
                );
            }
        }
    }
    std::fs::write(dir.join(format!("import-notes-{prefix}.md")), index).unwrap();
}

#[cfg(feature = "test-support")]
#[path = "../../dutchman/tests/support/net.rs"]
mod test_support;

/// A converted row with a report, shown in the panel as it is after a file finishes.
#[cfg(feature = "test-support")]
fn finished_cloud_app(dir: &Path) -> Harness<'static, App> {
    use serde_json::json;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().set_out_dir(dir.to_path_buf());
    let input = dir.join("Bracket.f3d");
    harness
        .state_mut()
        .preview_result_for_test(dutchman_gui::app::Row {
            input: input.clone(),
            output: Some(dir.join("Bracket.sc")),
            status: Status::Converted,
            progress: None,
            report: Some(json!({"ok": true, "input": {"path": input, "format": "fusion_archive"}})),
            job: None,
            size: Some(100),
        });
    harness.run_steps(8);
    harness
}

#[cfg(feature = "test-support")]
#[test]
fn cloud_report_button_is_visible_and_enabled_once_the_file_has_a_report() {
    use egui_kittest::kittest::NodeT;
    let dir = tempfile::tempdir().unwrap();
    let harness = finished_cloud_app(dir.path());
    // Visible without opening More, next to the GitHub link.
    let button = harness.get_by_label(CLOUD);
    assert!(!button.accesskit_node().is_disabled());
    assert!(harness
        .query_by_label(dutchman_gui::report_panel::CLOUD_REPORT_WAIT)
        .is_none());
    let github = harness.get_by_label("File a GitHub issue ↗").rect();
    let cloud = button.rect();
    assert!(github.right() <= cloud.left() || github.bottom() <= cloud.top());
}

#[cfg(feature = "test-support")]
#[test]
fn cloud_report_button_is_disabled_while_the_file_is_converting() {
    use egui_kittest::kittest::NodeT;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    harness
        .state_mut()
        .preview_running_for_test(dutchman_gui::app::Row {
            input: PathBuf::from("Assembly.iam"),
            output: None,
            status: Status::Converting,
            report: None,
            job: None,
            size: None,
            progress: None,
        });
    harness.run_steps(8);
    assert!(harness.get_by_label(CLOUD).accesskit_node().is_disabled());
    harness.get_by_label(dutchman_gui::report_panel::CLOUD_REPORT_WAIT);
    // Clicking a disabled button opens nothing.
    harness.get_by_label(CLOUD).click_accesskit();
    harness.run_steps(8);
    assert!(harness
        .query_by_label(dutchman_gui::report_consent::REVIEW_TITLE)
        .is_none());
}

#[cfg(feature = "test-support")]
#[test]
fn cloud_report_button_opens_the_review_screen_without_opening_more() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = finished_cloud_app(dir.path());
    assert!(harness
        .query_by_label(dutchman_gui::report_consent::REVIEW_TITLE)
        .is_none());
    harness.get_by_label(CLOUD).click_accesskit();
    harness.run_steps(8);
    harness.get_by_label(dutchman_gui::report_consent::REVIEW_TITLE);
    harness.get_by_label("Anonymous summary only");
    // Nothing duplicates the button under More.
    assert_eq!(harness.query_all_by_label(CLOUD).count(), 1);
}

/// The three step cards share rows, so their buttons line up, with English and with long German
/// titles, finished and mid-run.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the step cards; set DUTCHMAN_GUI_SCREENSHOTS"]
fn step_cards_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (language, titles) in [
        ("english", None),
        (
            "german",
            Some([
                "Unserem Discord-Server beitreten".to_owned(),
                "Dutchy eine Direktnachricht senden".to_owned(),
                "Bitte einen STEP-Export hinzufügen".to_owned(),
            ]),
        ),
    ] {
        *dutchman_gui::report_panel::TITLES_FOR_TEST.lock().unwrap() = titles;
        for (theme, name) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            for state in ["finished", "running"] {
                let docs = PathBuf::from("/Users/demo/Documents");
                let mut harness = Harness::builder()
                    .with_size(egui::vec2(1280.0, 800.0))
                    .build_eframe(|cc| test_app(cc));
                harness.ctx.set_theme(theme);
                harness.state_mut().set_checker_for_test(true);
                harness.state_mut().preview_solidcraft(Some("0.50.0"));
                harness.state_mut().set_out_dir(docs.join("Converted"));
                if state == "finished" {
                    harness
                        .state_mut()
                        .preview_result_for_test(dutchman_gui::app::Row {
                            input: docs.join("Bracket.f3d"),
                            output: Some(docs.join("Converted").join("Bracket.sc")),
                            status: Status::Converted,
                            progress: None,
                            report: Some(serde_json::json!({"ok": true,
                                "input": {"path": docs.join("Bracket.f3d"), "format": "fusion_archive"}})),
                            job: None,
                            size: Some(100),
                        });
                } else {
                    harness
                        .state_mut()
                        .preview_running_for_test(dutchman_gui::app::Row {
                            input: PathBuf::from("Assembly.iam"),
                            output: None,
                            status: Status::Converting,
                            report: None,
                            job: None,
                            size: None,
                            progress: None,
                        });
                }
                harness.run_steps(8);
                harness.run_steps(8);
                harness.get_by_label(CLOUD).scroll_to_me();
                harness.run_steps(8);
                let invite = harness.get_by_label("Open invite").rect().top();
                let dm = harness.get_by_label("Copy and open DM").rect().top();
                assert!(
                    (invite - dm).abs() <= 1.0,
                    "{language} {name} {state}: the two teal buttons are {invite} and {dm}"
                );
                shot(
                    &mut harness,
                    &dir,
                    &mut index,
                    &format!("step-cards-{language}-{state}"),
                    name,
                    "The three step cards share rows; the two teal buttons line up.",
                );
            }
        }
    }
    *dutchman_gui::report_panel::TITLES_FOR_TEST.lock().unwrap() = None;
    std::fs::write(dir.join("step-cards.md"), index).unwrap();
}

/// The report panel's two buttons, finished and mid-run, in Day and Night.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the report buttons; set DUTCHMAN_GUI_SCREENSHOTS"]
fn report_buttons_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (theme, name) in [
        (ThemePreference::Light, "day"),
        (ThemePreference::Dark, "night"),
    ] {
        // Neutral absolute paths: the app shows paths as they are, so none of the machine's.
        let docs = PathBuf::from("/Users/demo/Documents");
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| test_app(cc));
        harness.ctx.set_theme(theme);
        harness.state_mut().set_checker_for_test(true);
        harness.state_mut().preview_solidcraft(Some("0.50.0"));
        harness.state_mut().set_out_dir(docs.join("Converted"));
        harness
            .state_mut()
            .preview_result_for_test(dutchman_gui::app::Row {
                input: docs.join("Bracket.f3d"),
                output: Some(docs.join("Converted").join("Bracket.sc")),
                status: Status::Converted,
                progress: None,
                report: Some(serde_json::json!({"ok": true,
                    "input": {"path": docs.join("Bracket.f3d"), "format": "fusion_archive"}})),
                job: None,
                size: Some(100),
            });
        harness.run_steps(8);
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "report-buttons-top",
            name,
            "Top of the left column: Designs and Output folder at their natural height.",
        );
        harness.get_by_label(CLOUD).scroll_to_me();
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "report-buttons-finished",
            name,
            "Finished file: File a GitHub issue ↗ first, Review and send report last.",
        );

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| test_app(cc));
        harness.ctx.set_theme(theme);
        harness.state_mut().set_checker_for_test(true);
        harness.state_mut().preview_solidcraft(Some("0.50.0"));
        harness
            .state_mut()
            .set_out_dir(PathBuf::from("/Users/demo/Documents/Converted"));
        harness
            .state_mut()
            .preview_running_for_test(dutchman_gui::app::Row {
                input: PathBuf::from("Assembly.iam"),
                output: None,
                status: Status::Converting,
                report: None,
                job: None,
                size: None,
                progress: None,
            });
        harness.run_steps(8);
        harness.get_by_label(CLOUD).scroll_to_me();
        harness.run_steps(8);
        shot(
            &mut harness,
            &dir,
            &mut index,
            "report-buttons-running",
            name,
            "Mid-run: the review button disabled with its hint.",
        );
    }
    std::fs::write(dir.join("report-buttons.md"), index).unwrap();
}

/// A drawing whose sheets are not read yet is "Not supported yet", not a failure, and its
/// help asks for a PDF or DXF export, never a STEP.
#[test]
fn a_drawing_that_is_not_read_yet_is_not_supported_yet_not_failed() {
    for name in [
        "frame.idw",
        "frame.slddrw",
        "frame.f2d",
        "frame.dft",
        "frame.dwg",
        "frame.dxf",
    ] {
        let (mut harness, _dir) = failed_card_for_drawing(name);
        let view = &harness.state().rows()[0];
        assert!(view.report.is_some(), "{name}");
        assert!(
            harness.query_by_label("Not supported yet").is_some(),
            "{name}"
        );
        assert!(harness.query_by_label("Failed").is_none(), "{name}");
        assert!(
            harness
                .query_by_label("0 converted, 1 not supported yet")
                .is_some(),
            "{name}"
        );
        assert!(
            harness.query_by_label_contains("0 failed").is_none(),
            "{name}"
        );
        // One statement: recognized, but not supported yet. Never "accepted" beside "cannot".
        // The design list line and the card both say it, in the same words.
        assert_eq!(
            harness
                .query_all_by_label_contains(
                    "recognized, but reading its sheets isn't supported yet"
                )
                .count(),
            2,
            "{name}"
        );
        assert!(
            harness.query_by_label_contains("cannot be read").is_none(),
            "{name}"
        );
        assert!(
            harness.query_by_label_contains("accepted").is_none(),
            "{name}"
        );
        // No STEP anywhere: not the button, not the help step.
        assert!(
            harness.query_by_label("Add matching STEP").is_none(),
            "{name}"
        );
        assert!(
            harness.query_by_label("Please add a STEP export").is_none(),
            "{name}"
        );
        assert!(
            harness
                .query_by_label("Please add a PDF or DXF export")
                .is_some(),
            "{name}"
        );
        assert!(
            harness
                .query_by_label_contains("a PDF or DXF export of the same drawing")
                .is_some(),
            "{name}"
        );
        harness.run_steps(2);
    }
}

fn failed_card_for_drawing(name: &str) -> (Harness<'static, App>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join(name);
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    if name.ends_with(".dwg") {
        bytes = b"AC1032\0\0\0\0\0\0".to_vec();
    }
    bytes.extend([0_u8; 64]);
    std::fs::write(&input, bytes).unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| test_app(cc));
    harness.state_mut().add_inputs([input]);
    harness.state_mut().set_out_dir(dir.path().join("out"));
    harness.state_mut().set_verify(false);
    convert(&mut harness);
    harness.run_steps(8);
    (harness, dir)
}

/// A synthetic drawing file whose sheets are not decoded yet: the Results card reports it.
#[test]
#[ignore = "renders the drawing result cards with a GPU; set DUTCHMAN_GUI_SCREENSHOTS"]
fn drawing_screenshots() {
    if std::env::var_os("DUTCHMAN_DRAWING_SCREENSHOT_CHILD").is_none() {
        // Neutral paths, so no screenshot shows the machine that rendered it.
        let state = PathBuf::from("/tmp/demo");
        std::fs::create_dir_all(&state).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "drawing_screenshots"])
            .env("DUTCHMAN_DRAWING_SCREENSHOT_CHILD", "1")
            .env("HOME", &state)
            .env("LOCALAPPDATA", &state)
            .env("XDG_STATE_HOME", &state)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let work = PathBuf::from("/tmp/demo/in");
    std::fs::create_dir_all(&work).unwrap();
    let mut index = String::new();
    // The app has no German text yet: the German set shows a long German file name.
    for (language, name) in [
        ("en", "Frame-assembly.idw"),
        (
            "de",
            "Zeichnung-Rahmenbaugruppe-Seitenwand-Ueberarbeitung-Fertigung.dwg",
        ),
    ] {
        let input = work.join(name);
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        if name.ends_with(".dwg") {
            bytes = b"AC1032\0\0\0\0\0\0\0".to_vec();
        }
        bytes.extend([0_u8; 64]);
        std::fs::write(&input, bytes).unwrap();
        for (theme, mode) in [
            (ThemePreference::Light, "day"),
            (ThemePreference::Dark, "night"),
        ] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1280.0, 800.0))
                .build_eframe(|cc| test_app(cc));
            harness.ctx.set_theme(theme);
            harness.state_mut().add_inputs([input.clone()]);
            harness
                .state_mut()
                .set_out_dir(PathBuf::from("/tmp/demo/out"));
            harness
                .state_mut()
                .set_diagnostic_log(PathBuf::from("/tmp/demo/log.jsonl"));
            harness.state_mut().preview_solidcraft(Some("0.50.0"));
            harness.state_mut().set_verify(false);
            convert(&mut harness);
            assert_eq!(harness.state().rows()[0].status, Status::Failed);
            harness.run_steps(8);
            shot(
                &mut harness,
                &dir,
                &mut index,
                &format!("drawing-not-decoded-{language}"),
                mode,
                "A drawing file that is recognized and reported as not supported yet.",
            );
        }
    }
    std::fs::write(dir.join("drawings.md"), index).unwrap();
}

/// The Advanced "Up axis" choice, closed and open, with an English and a long German design
/// name (the app has no translations yet), at 1280x800. Needs a GPU renderer.
#[cfg(feature = "test-support")]
#[test]
#[ignore = "renders GPU screenshots of the Up axis choice; set DUTCHMAN_GUI_SCREENSHOTS"]
fn up_axis_screenshots() {
    // Isolate settings and paths so the image never shows the owner's home folder.
    if std::env::var_os("DUTCHMAN_UP_AXIS_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "up_axis_screenshots"])
            .env("DUTCHMAN_UP_AXIS_CHILD", "1")
            .env("HOME", home.path())
            .env("LOCALAPPDATA", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("XDG_DATA_HOME", home.path().join("data"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut index = String::new();
    for (language, name) in [
        ("english", "synthetic-part.f3d"),
        (
            "german",
            "Zusammengesetzte-Grundplatte-mit-Befestigungsschrauben.f3d",
        ),
    ] {
        let work = tempfile::tempdir().unwrap();
        let input = work.path().join(name);
        std::fs::copy(fixture("container_metadata_only.f3d"), &input).unwrap();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| test_app(cc));
        harness.ctx.set_theme(ThemePreference::Light);
        harness.state_mut().add_inputs([input]);
        // Neutral paths and checker, so the image shows nothing of this machine.
        harness.state_mut().set_checker_for_test(true);
        harness.state_mut().preview_solidcraft(Some("0.46.2"));
        harness
            .state_mut()
            .set_out_dir(PathBuf::from("/Users/demo/Documents/Converted"));
        wait_for_estimate(&mut harness);
        harness.get_by_label("Advanced").click_accesskit();
        harness.run_steps(8);
        harness.get_by_value("Automatic");
        shot(
            &mut harness,
            &dir,
            &mut index,
            &format!("up-axis-closed-{language}"),
            "day",
            "Advanced open; Up axis shows Automatic.",
        );
        harness.get_by_label("Up axis").click_accesskit();
        harness.run_steps(8);
        harness.get_by_label("Stand up (Y-up to Z-up)");
        harness.get_by_label("Keep the file's axes");
        shot(
            &mut harness,
            &dir,
            &mut index,
            &format!("up-axis-open-{language}"),
            "day",
            "Up axis choices listed: Automatic, Stand up, Keep the file's axes.",
        );
    }
}
