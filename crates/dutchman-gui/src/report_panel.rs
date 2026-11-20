//! "Is there a problem? Is something missing?": the panel in the bottom left of the window that
//! is always there, before, during and after a run. Its numbered steps reach the Dutchman team
//! on Discord first; a GitHub issue is the backup. Nothing is sent or copied until a button is
//! pressed.

use dutchman::report_policy::{MessageFacts, ReportPolicy};
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText, Stroke};

use crate::theme::{self, Palette};

/// The panel's heading: the question every result leaves open.
pub const QUESTION: &str = "Is there a problem? Is something missing?";
/// Under the heading before any file has been converted.
pub const NO_FILE_INTRO: &str =
    "Tell us on Discord, before or after you convert. A GitHub issue is the backup.";
/// Under the heading while the file it is about is still converting.
pub const RUNNING_INTRO: &str = "Taking a long time, or stuck? Tell us while it runs.";
/// The STEP card's advice, shorter than the web page's so the cards stay one height.
pub const STEP_TEXT: &str =
    "A STEP export of the same design, at the same version and part positions, lets us check \
     every part.";
/// The DM card's hint when no finished file is chosen.
pub const DM_GENERAL: &str = "Paste the message and say what's wrong.";
/// Shown on the Discord card once its invite has opened.
pub const JOIN_DONE: &str = "The invite opened in your browser. Then go to step 2.";
/// Shown on the DM card once its message is copied, for a file dragged into the DM.
pub const DM_DONE_DRAG: &str =
    "Copied. Paste it into the DM, say what's wrong, then drag the file in.";
/// Shown on the DM card once its message is copied, for a file sent by upload link.
pub const DM_DONE_LINK: &str =
    "Copied. Paste it into the DM and say what's wrong; Dutchy replies with an upload link.";
/// Shown on the DM card once its message is copied, with no file to send.
pub const DM_DONE: &str = "Copied. Paste it into the DM and say what's wrong.";
/// Shown under the GitHub link once the blank form opened.
pub const GITHUB_OPENED: &str = "The GitHub form opened in your browser. Never attach the design.";
/// The folded section with the anonymous summary, the AI prompt and Diagnose.
pub const MORE: &str = "More";
/// Shown once a claim code is copied for updates on a sent report.
pub const UPDATES_COPIED: &str = "A claim code is copied. Join our Discord server (step 1) if you \
     haven't, then paste the code on its own in Dutchy's DM to get updates on this report.";
/// The bottom bar's line after Stop.
pub const STOPPING: &str = "Stopping the converter and its children. No other file starts.";
/// The Stop button once pressed.
pub const STOPPING_BUTTON: &str = "Stopping…";

/// The step a confirmation belongs to, shown on that step's card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Join,
    Dm,
    Github,
}

/// One numbered card.
pub struct Card<'a> {
    pub title: &'a str,
    /// Its one filled button and the step it takes; none for advice.
    pub button: Option<(&'a str, Step)>,
    /// The short muted line under the button.
    pub hint: &'a str,
    /// What happened when its button was last pressed, shown instead of the hint.
    pub done: Option<&'a str>,
}

/// The narrowest a card gets before the cards wrap onto another line.
const MIN_CARD_WIDTH: f32 = 175.0;
/// The step number's circle.
const BADGE: f32 = 20.0;

/// The numbered cards side by side, wrapping when narrow, all of one height. Returns the step
/// whose button was pressed.
pub fn cards(
    ui: &mut egui::Ui,
    p: &Palette,
    salt: impl std::hash::Hash,
    all: &[Card<'_>],
) -> Option<Step> {
    let mut pressed = None;
    let gap = theme::SPACE_2;
    let across = ((ui.available_width() + gap) / (MIN_CARD_WIDTH + gap)).floor() as usize;
    let across = across.clamp(1, all.len().max(1));
    let base = ui.id().with(("report-cards", salt, across));
    // Every card is as tall as last frame's tallest; the room left over goes between the
    // title and the button, so a card never ends in empty space.
    let tallest_id = base.with("tallest");
    let tallest = ui
        .data(|data| data.get_temp::<f32>(tallest_id))
        .unwrap_or(0.0);
    let mut measured = 0.0_f32;
    for (line, chunk) in all.chunks(across).enumerate() {
        ui.columns(across, |columns| {
            for (column, (ui, card)) in columns.iter_mut().zip(chunk).enumerate() {
                let number = line * across + column + 1;
                let natural_id = base.with(("natural", number));
                let natural = ui
                    .data(|data| data.get_temp::<f32>(natural_id))
                    .unwrap_or(0.0);
                let spare = (tallest - natural).max(0.0);
                let advice = card.button.is_none();
                let frame = if advice {
                    Frame::new()
                } else {
                    Frame::new()
                        .fill(p.surface_2)
                        .stroke(Stroke::new(1.0_f32, p.hairline))
                };
                frame
                    .corner_radius(CornerRadius::same(theme::RADIUS_LG))
                    .inner_margin(Margin::same(theme::CARD_PADDING as i8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        theme::compact_rows(ui);
                        let top = ui.cursor().top();
                        ui.horizontal(|ui| {
                            badge(ui, p, number, advice);
                            ui.add(
                                egui::Label::new(RichText::new(card.title).strong().color(p.ink))
                                    .wrap(),
                            );
                        });
                        ui.add_space(spare);
                        if let Some((label, step)) = card.button {
                            if ui.add(filled(p, label)).clicked() {
                                pressed = Some(step);
                            }
                        }
                        // Left-aligned: a justified column would stretch the gaps between words.
                        ui.with_layout(Layout::top_down(Align::Min), |ui| match card.done {
                            Some(done) => {
                                ui.label(RichText::new(done).small().strong().color(p.ink));
                            }
                            None => {
                                ui.label(RichText::new(card.hint).small().color(p.tide));
                            }
                        });
                        let height = ui.cursor().top() - top - spare;
                        ui.data_mut(|data| data.insert_temp(natural_id, height));
                        measured = measured.max(height);
                    });
            }
        });
        ui.add_space(theme::SPACE_1);
    }
    if (measured - tallest).abs() > 0.5 {
        ui.data_mut(|data| data.insert_temp(tallest_id, measured));
        ui.ctx().request_repaint();
    }
    pressed
}

/// The step's number in a circle: filled for a step with a button, outlined for advice.
fn badge(ui: &mut egui::Ui, p: &Palette, number: usize, advice: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(BADGE, BADGE), egui::Sense::hover());
    let painter = ui.painter();
    let (fill, text) = if advice {
        painter.circle_stroke(
            rect.center(),
            BADGE / 2.0 - 0.5,
            Stroke::new(1.0_f32, p.tide),
        );
        (None, p.tide)
    } else {
        (Some(p.spectre), p.on_spectre)
    };
    if let Some(fill) = fill {
        painter.circle_filled(rect.center(), BADGE / 2.0, fill);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        egui::FontId::proportional(12.0),
        text,
    );
}

/// The one primary button of a card: filled with the accent colour, no outline.
pub fn filled<'a>(p: &Palette, label: &'a str) -> egui::Button<'a> {
    egui::Button::new(
        RichText::new(label)
            .color(p.on_spectre)
            .text_style(egui::TextStyle::Button),
    )
    .fill(p.spectre)
    .stroke(Stroke::NONE)
}

/// A button drawn as a link, for the backup route and other quiet actions.
pub fn link(ui: &mut egui::Ui, p: &Palette, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).color(p.spectre).underline()).frame(false))
}

/// A small backup link below the primary reporting steps.
pub fn backup_link(ui: &mut egui::Ui, p: &Palette, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).small().color(p.spectre).underline()).frame(false),
    )
}

/// The message copied for Dutchy's DM about a finished file: what happened, the details line
/// Dutchy reads (in the policy's fixed wording, so automatic upload links keep working) and a
/// last line for the person's own words, where the cursor lands after pasting.
pub fn dm_message(
    policy: &ReportPolicy,
    facts: &MessageFacts<'_>,
    converted: bool,
    log: Option<&str>,
) -> String {
    let mut text = String::new();
    if converted {
        let facts = MessageFacts {
            code: Some(dutchman::report_send::CONVERTED_PROBLEM_SIGNATURE),
            ..*facts
        };
        text.push_str("Dutchman converted my file, but something is wrong or missing.\n");
        text.push_str("Details for Dutchy: ");
        text.push_str(&policy.message(&facts));
    } else {
        text.push_str(&policy.message(facts));
    }
    if let Some(log) = log {
        text.push_str(&format!(
            " Run log: {log}. It contains no names or contents from the design."
        ));
    }
    text.push_str(if converted {
        "\nWhat's wrong or missing: "
    } else {
        "\nAnything else we should know: "
    });
    text
}

/// The message copied for Dutchy's DM with no finished file: the version, and how long the
/// file has been converting when it still is. Nothing about the file itself.
pub fn general_message(elapsed_ms: Option<u64>) -> String {
    let version = dutchman::report_policy::report_version();
    match elapsed_ms {
        Some(ms) => format!(
            "Dutchman {version} is taking a long time with my file ({}).\nWhat's happening: ",
            dutchman_core::progress_words::total_elapsed(ms)
        ),
        None => format!("I have a problem with Dutchman {version}.\nWhat's wrong or missing: "),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn facts(id: &str) -> MessageFacts<'_> {
        MessageFacts {
            code: Some("decode"),
            report_id: id,
            size_bytes: Some(2048),
            step: None,
        }
    }

    #[test]
    fn a_converted_file_says_so_and_leaves_a_line_for_the_problem() {
        let policy = ReportPolicy::bundled();
        let text = dm_message(policy, &facts("0123456789abcdef"), true, Some("run.jsonl"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(
            lines[0],
            "Dutchman converted my file, but something is wrong or missing."
        );
        // Dutchy finds the details line in the policy's own wording.
        assert!(lines[1].contains("Error: converted-with-problem. Report id: 0123456789abcdef."));
        assert!(lines[1]
            .ends_with("Run log: run.jsonl. It contains no names or contents from the design."));
        assert!(text.ends_with("What's wrong or missing: "));
    }

    #[test]
    fn a_failed_file_keeps_its_one_details_line_then_a_line_for_more() {
        let policy = ReportPolicy::bundled();
        let text = dm_message(policy, &facts("0123456789abcdef"), false, None);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0], policy.message(&facts("0123456789abcdef")));
        assert!(text.ends_with("Anything else we should know: "));
    }

    #[test]
    fn a_general_message_names_no_file() {
        let text = general_message(None);
        assert!(text.contains(dutchman::report_policy::report_version()));
        assert!(text.ends_with("What's wrong or missing: "));
        let running = general_message(Some(61_000));
        assert!(running.contains("(1 min 1 s elapsed)"), "{running}");
    }
}
