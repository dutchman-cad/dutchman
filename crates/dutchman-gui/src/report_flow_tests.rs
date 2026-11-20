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
