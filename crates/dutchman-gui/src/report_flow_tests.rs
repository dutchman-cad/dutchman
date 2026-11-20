//! Synthetic report-flow layout, routing and colour regressions.
use super::*;
use egui_kittest::{kittest::Queryable, Harness};
use serde_json::json;

fn report(case: usize) -> Value {
    match case {
        1 => json!({"ok":true,"mode":"history","history":{"mapped":4,"total":4},"losses":[]}),
        2 => {
            json!({"ok":true,"mode":"partial_history","history":{"mapped":2,"total":3},"losses":[{"level":"Degraded","message":"Chamfer is not supported yet.","subject":"history"}]})
        }
        3 => json!({"ok":true,"mode":"import","losses":[]}),
        _ => {
            json!({"ok":false,"error":{"message":"The design could not be read. Try exporting it again.","kind":"decode","stage":"decode"}})
        }
    }
}

fn row(case: usize) -> Row {
    let mut report = report(case);
    if let Some(fields) = report.as_object_mut() {
        fields.insert(
            "input".into(),
            json!({"path": format!("synthetic-{case}.f3d"), "format":"fusion_archive"}),
        );
        if case == 1 {
            fields.insert("verification".into(), json!({"status":"verified"}));
            fields.insert("solidcraft".into(), json!({"found":"0.44.1"}));
        }
    }
    Row {
        input: PathBuf::from(format!("synthetic-{case}.f3d")),
        output: (case < 4).then(|| PathBuf::from(format!("converted/synthetic-{case}.sc"))),
        status: if case < 4 {
            Status::Converted
        } else {
            Status::Failed
        },
        progress: None,
        job: None,
        size: Some(100_000),
        report: Some(report),
    }
}

fn harness(cases: &[usize], dark: bool) -> Harness<'static, App> {
    let cases = cases.to_vec();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1180.0, 800.0))
        .build_eframe(move |cc| {
            let mut app = App::new(cc);
            app.rows = cases.into_iter().map(row).collect();
            app.inputs = app.rows.iter().map(|row| row.input.clone()).collect();
            app.size_inputs = Some(app.inputs.clone());
            app.out_dir = Some(PathBuf::from("converted"));
            app.found_version = Some("0.44.1".into());
            app.checker = Ok(PathBuf::from("solidcraft-cli"));
            app.estimate_key = Some(app.current_estimate_key());
            app
        });
    h.ctx.set_theme(if dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    h.state_mut().set_file_manager(|_| Ok(()));
    h.run_steps(16);
    h
}

fn expand(h: &mut Harness<'static, App>) {
    h.state_mut().output_expanded = true;
    h.run_steps(8);
    h.get_by_label("Advanced").click_accesskit();
    h.run_steps(16);
}

#[test]
fn report_question_and_actions_stay_inside_the_wide_window() {
    for dark in [false, true] {
        let mut h = harness(&[1, 2, 3], dark);
        expand(&mut h);
        let window = egui::Rect::from_min_max(
            egui::pos2(0.0, 80.0),
            egui::pos2(1180.0, h.get_by_label("Convert").rect().top()),
        );
        for label in [
            report_panel::QUESTION,
            present::SEND_JOIN_BUTTON,
            present::SEND_DM_BUTTON,
            present::SEND_STEP_TITLE,
        ] {
            let rect = h.get_by_label(label).rect();
            assert!(
                window.contains_rect(rect),
                "{label}: {rect:?} outside {window:?}"
            );
        }
        assert!(h.query_by_label("Feature history").is_some());
        h.get_by_label("Report a problem").click_accesskit();
        h.run_steps(12);
        assert!(window.contains_rect(h.get_by_label(report_panel::QUESTION).rect()));
        h.query_all_by_label(REPORT_THIS_FILE)
            .last()
            .unwrap()
            .click_accesskit();
        h.run_steps(12);
        assert_eq!(h.state().report_row, Some(2));
        let question = h.get_by_label(report_panel::QUESTION).rect();
        assert!(window.contains_rect(question));
        h.get_by_label(report_panel::MORE).click_accesskit();
        h.run_steps(12);
        let question = h.get_by_label(report_panel::QUESTION).rect();
        assert!(window.contains_rect(question));
        h.get_by_label("Anonymous report").scroll_to_me();
        h.run_steps(12);
        assert_eq!(h.get_by_label(report_panel::QUESTION).rect(), question);
    }
}

#[test]
fn converted_results_with_missing_history_or_degraded_losses_use_warning_colour() {
    for p in [theme::DAY, theme::NIGHT] {
        let degraded = json!({"ok":true,"mode":"history","losses":[{"level":"Degraded","subject":"body","message":"Colour is missing."}]});
        let info = json!({"ok":true,"mode":"history","losses":[{"level":"Info","subject":"body","message":"Converted."}]});
        for (report, expected) in [
            (report(1), p.spectre),
            (report(2), p.lantern_text),
            (report(3), p.lantern_text),
            (degraded, p.lantern_text),
            (info, p.spectre),
            (report(4), p.danger),
        ] {
            let view = FileView::from_report(&report);
            assert_eq!(
                result_colour(&p, Some(&report), &view),
                expected,
                "{report}"
            );
        }
    }
}

#[test]
fn rendered_partial_and_geometry_results_are_amber_in_both_themes() {
    for dark in [false, true] {
        let p = if dark { theme::NIGHT } else { theme::DAY };
        for case in [1, 2, 3] {
            let h = harness(&[case], dark);
            let expected = if case == 1 { p.spectre } else { p.lantern_text };
            let text = h
                .output()
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Converted" => Some(text),
                    _ => None,
                })
                .unwrap();
            for section in &text.galley.job.sections {
                let colour = text.override_text_color.unwrap_or_else(|| {
                    if section.format.color == egui::Color32::PLACEHOLDER {
                        text.fallback_color
                    } else {
                        section.format.color
                    }
                });
                assert_eq!(colour, expected, "case {case}, dark {dark}");
            }
        }
    }
}

// Screenshot evidence uses synthetic reports only, without opening or sending a design.
#[test]
#[ignore = "renders GPU screenshots; set DUTCHMAN_GUI_SCREENSHOTS"]
fn report_panel_release_screenshots() {
    let dir = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let before = std::env::var_os("DUTCHMAN_SCREENSHOT_BEFORE").is_some();
    let mut index = String::from(
        "# Report panel screenshots\n\nSynthetic reports and held progress; every PNG is 1180 × 800. No design was decoded or sent.\n\n| Image | Panel visible | Partial / geometry amber | Selected file route |\n|---|---|---|---|\n",
    );
    for dark in [false, true] {
        if before && dark {
            continue;
        }
        let theme = if dark { "dark" } else { "light" };
        for (cases, name) in [
            (vec![1], "C1-full-success"),
            (vec![2], "C2-partial-history"),
            (vec![3], "C3-geometry-only"),
            (vec![4], "C4-failure"),
            (vec![5], "C5-slow-progress"),
            (vec![1, 2, 3], "C6-three-results-expanded"),
        ] {
            let mut h = harness(&cases, dark);
            if name == "C5-slow-progress" {
                let mut running = row(5);
                running.input = PathBuf::from("synthetic-large-assembly.f3z");
                running.status = Status::Converting;
                running.report = None;
                running.size = Some(65_000_000);
                running.progress = Some(dutchman_core::ProgressEvent {
                    stage: dutchman_core::Stage::Decode,
                    elapsed_ms: 179_000,
                    designs_read: 0,
                    designs_total: None,
                    heartbeat: true,
                    running: None,
                });
                h.state_mut().preview_running_for_test(running);
                h.run_steps(16);
                h.get_by_label("Stop");
            }
            if cases.len() == 3 {
                expand(&mut h);
            }
            save_report_panel_shot(
                &mut h,
                &dir,
                &format!("{name}-{theme}"),
                "not clicked",
                &mut index,
            )
            .unwrap();
            if cases.len() == 3 {
                assert!(h.state().output_expanded);
                h.get_by_label("Feature history").scroll_to_me();
                h.run_steps(16);
                save_report_panel_shot(
                    &mut h,
                    &dir,
                    &format!("{name}-controls-{theme}"),
                    "not clicked; Advanced open, output choices expanded",
                    &mut index,
                )
                .unwrap();
            }
            if name != "C5-slow-progress" {
                for (position, case) in cases.iter().enumerate() {
                    h.query_all_by_label(REPORT_THIS_FILE)
                        .nth(position)
                        .unwrap()
                        .click_accesskit();
                    h.run_steps(16);
                    assert_eq!(h.state().report_row, Some(position));
                    assert_eq!(
                        h.state().rows.get(position).unwrap().input,
                        PathBuf::from(format!("synthetic-{case}.f3d"))
                    );
                    save_report_panel_shot(
                        &mut h,
                        &dir,
                        &format!("{name}-route-{case}-{theme}"),
                        &format!("PASS: synthetic-{case}.f3d"),
                        &mut index,
                    )
                    .unwrap();
                }
            }
        }
    }
    std::fs::write(dir.join("index.md"), index).unwrap();
}

fn save_report_panel_shot(
    h: &mut Harness<'static, App>,
    dir: &std::path::Path,
    name: &str,
    route: &str,
    index: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let viewport = egui::Rect::from_min_max(
        egui::pos2(0.0, 80.0),
        egui::pos2(
            1180.0,
            h.query_by_label("Convert")
                .or_else(|| h.query_by_label("Stop"))
                .ok_or("The footer has neither Convert nor Stop")?
                .rect()
                .top(),
        ),
    );
    let question = h
        .query_by_label(report_panel::QUESTION)
        .map(|node| node.rect());
    let dm = h
        .query_by_label(present::SEND_DM_BUTTON)
        .map(|node| node.rect());
    let visible = question.is_some_and(|rect| viewport.contains_rect(rect))
        && dm.is_some_and(|rect| viewport.contains_rect(rect));
    let p = if h.ctx.style().visuals.dark_mode {
        theme::NIGHT
    } else {
        theme::DAY
    };
    let amber = h
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Converted" => Some(text),
            _ => None,
        })
        .filter(|text| {
            text.galley.job.sections.iter().any(|section| {
                text.override_text_color.unwrap_or_else(|| {
                    if section.format.color == egui::Color32::PLACEHOLDER {
                        text.fallback_color
                    } else {
                        section.format.color
                    }
                }) == p.lantern_text
            })
        })
        .count();
    let file = format!("{name}.png");
    let image = h.render()?;
    assert_eq!(image.dimensions(), (1180, 800));
    image.save(dir.join(&file))?;
    index.push_str(&format!("| [{file}]({file}) | {} (question {question:?}; DM {dm:?}) | {amber} amber result labels | {route} |\n", if visible { "Yes" } else { "No" }));
    Ok(())
}
