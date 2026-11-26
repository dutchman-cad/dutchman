//! The plain sentence each kind of import note leads with, in one table.
//!
//! A note's code and message are written for whoever fixes Dutchman. [`PLAIN_NOTES`] says, per
//! code family, what the note means for the person who opens the converted design and what they
//! can do, and whether the note changes anything they would see at all. Front ends show the
//! sentence first, leave notes that change nothing out of the default view, and keep the raw
//! code and message behind "Details" or `--verbose`. The JSON report keeps every note, with the
//! sentence added as `summary`. A note no row matches keeps its own text and stays visible.

use crate::report::Severity;

/// Whether a note appears in the default view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Always.
    Yes,
    /// Never: it changes nothing the person sees (a record not written to STEP).
    No,
    /// Unless every number in its message is zero (a placement statistic of nothing).
    UnlessAllCountsZero,
}

/// One row: which notes it covers, the sentence, and whether they are shown.
#[derive(Clone, Copy, Debug)]
pub struct PlainNote {
    /// The start of the note's loss code (`inventor/rse.`). Empty for a note without a code.
    pub code: &'static str,
    /// For a note without a code: its subject, exactly or after a part file's `name: ` prefix.
    pub subject: &'static str,
    /// For a note without a code: text its message must contain.
    pub message_has: &'static str,
    /// The sentence: what is missing or different, and what to do about it when there is something.
    pub sentence: &'static str,
    /// Whether the note is in the default view.
    pub shown: Shown,
}

const fn coded(code: &'static str, sentence: &'static str, shown: Shown) -> PlainNote {
    PlainNote {
        code,
        subject: "",
        message_has: "",
        sentence,
        shown,
    }
}

const fn uncoded(
    subject: &'static str,
    message_has: &'static str,
    sentence: &'static str,
    shown: Shown,
) -> PlainNote {
    PlainNote {
        code: "",
        subject,
        message_has,
        sentence,
        shown,
    }
}

/// Every note family with a plain sentence. The first matching row wins, so a specific code
/// goes before its family. Review and edit the sentences here and nowhere else.
pub const PLAIN_NOTES: &[PlainNote] = &[
    // The feature history could not be used.
    coded(
        "inventor/source.kernel-history-unclosed",
        "This part's feature history couldn't be read, so it was imported as a solid shape you can look at but not edit feature by feature.",
        Shown::Yes,
    ),
    coded(
        "history/body-fallback-imported",
        "A feature couldn't be rebuilt from its sketch, so its stored result was imported as a solid shape; that feature can't be edited, and later features continue on it.",
        Shown::Yes,
    ),
    coded(
        "history/body-mismatch",
        "Rebuilding this history did not reproduce the original shape, so the original shape was kept; compare the result with your source before relying on it.",
        Shown::Yes,
    ),
    coded(
        "history/rebuild-failed",
        "SolidCraft couldn't rebuild a feature, so that part of the editable history is missing and the original shape is kept instead.",
        Shown::Yes,
    ),
    coded(
        "history/profile-region-mismatch",
        "Which part of a sketch a feature uses could not be matched with the source, so that feature was not kept as an editable feature.",
        Shown::Yes,
    ),
    coded(
        "history/profile-region-inferred",
        "Dutchman worked out which part of a sketch a feature uses and the result matched the source; check it if the shape looks wrong.",
        Shown::Yes,
    ),
    coded(
        "history/profile-region-measured",
        "Dutchman worked out which part of a sketch a feature uses by trying the candidates in SolidCraft; check it if the shape looks wrong.",
        Shown::Yes,
    ),
    coded(
        "history/modeled-thread-retained",
        "A modeled thread was kept in the shape but is not an editable thread feature yet.",
        Shown::Yes,
    ),
    coded(
        "history/body-scope-confirmed",
        "Dutchman confirmed which bodies a feature acts on; there is nothing to do.",
        Shown::No,
    ),
    // Linked files and appearance.
    coded(
        "f3d/material.not-transferred",
        "The design's materials and appearances weren't carried over, so set them again in SolidCraft if you need them.",
        Shown::Yes,
    ),
    coded(
        "dutchman/appearance.redundant-face",
        "A face colour that repeated its body's colour was not written; nothing looks different.",
        Shown::No,
    ),
    coded(
        "dutchman/appearance.",
        "Some colours or appearances weren't carried over to the converted design.",
        Shown::Yes,
    ),
    coded(
        "dutchman/configuration.inactive-bodies-left-out",
        "Bodies that belong to configurations that aren't active were left out of the converted design.",
        Shown::Yes,
    ),
    // Without the saved configuration Dutchman can't tell which bodies to leave out.
    coded(
        "f3d/configuration.active-row-unbound",
        "The saved configuration couldn't be read, so bodies from other configurations may be included; check them.",
        Shown::Yes,
    ),
    // A dimension without its position in the sketch isn't written to the converted sketch.
    coded(
        "f3d/dimension.companion-untyped",
        "Some sketch dimensions couldn't be read and may be missing; add any you need again in SolidCraft.",
        Shown::Yes,
    ),
    // Records the reader kept or could not give a meaning to, and records STEP has no place for.
    // None of these changes the shape or the features.
    coded(
        "inventor/rse.",
        "Some of the file's internal records were kept but not interpreted; the shape and features are not affected.",
        Shown::No,
    ),
    coded(
        "f3d/source.unindexed-records-retained",
        "Some of the file's internal records were kept but not interpreted; the shape and features are not affected.",
        Shown::No,
    ),
    coded(
        "step/native.",
        "Some of the file's internal records have no STEP equivalent and were not written to the STEP copy; the shape is not affected.",
        Shown::No,
    ),
    coded(
        "step/attribute.",
        "Some of the file's internal attributes have no STEP equivalent and were not written to the STEP copy; the shape is not affected.",
        Shown::No,
    ),
    // STEP records that sit in every STEP file (the protocol definition, the product category,
    // the presentation of a representation) are kept as they are, and an edge tolerance has no
    // place in STEP. None of this changes what the person sees.
    coded(
        "step/decode.opaque-record-preserved",
        "Some of the file's standard STEP records were kept but not interpreted; the shape is not affected.",
        Shown::No,
    ),
    coded(
        "step/topology.metadata-not-represented",
        "Some edge tolerances have no STEP equivalent and were not written to the STEP copy; the shape is not affected.",
        Shown::No,
    ),
    // Folders in the timeline: a way the converted design is laid out, not something lost.
    uncoded(
        "timeline folders",
        "feature(s) grouped",
        "The features of each part were grouped into a folder in the timeline.",
        Shown::No,
    ),
    // Assembly placement.
    uncoded(
        "part positions",
        "parts placed as last shown",
        "Parts were placed with the position rule chosen for this conversion.",
        Shown::UnlessAllCountsZero,
    ),
    uncoded(
        "part positions",
        "sit at their design's origin",
        "Some parts sit at their design's origin because their placement couldn't be read, so they may not be where the source has them.",
        Shown::Yes,
    ),
    uncoded(
        "part positions",
        "rest on a stored pose",
        "Some parts use a stored position that nothing else confirms; check them if one looks misplaced.",
        Shown::Yes,
    ),
];

/// The row that covers a note, from its loss code (or its subject and message when it has no code).
pub fn row_for(code: Option<&str>, subject: &str, message: &str) -> Option<&'static PlainNote> {
    PLAIN_NOTES.iter().find(|row| match code {
        Some(code) => !row.code.is_empty() && code.starts_with(row.code),
        None => {
            row.code.is_empty()
                && (subject == row.subject || subject.ends_with(&format!(": {}", row.subject)))
                && message.contains(row.message_has)
        }
    })
}

/// A note as the default view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlainView {
    /// The plain sentence, or the note's own message when no row covers it.
    pub summary: String,
    /// Whether a row gave the sentence (the raw message is then detail).
    pub rewritten: bool,
    /// Whether the note is in the default view.
    pub shown: bool,
}

/// How a note reads in the default view. A note that was dropped is never hidden, whatever its
/// row says: only notes that change nothing are left out.
pub fn plain_view(
    severity: Severity,
    code: Option<&str>,
    subject: &str,
    message: &str,
) -> PlainView {
    let Some(row) = row_for(code, subject, message) else {
        // No sentence: the note keeps its own words and stays visible, unless it is a count
        // of nothing (every number zero), which says nothing to the person.
        return PlainView {
            summary: message.to_owned(),
            rewritten: false,
            shown: severity != Severity::Info || !all_counts_zero(message),
        };
    };
    let shown = severity == Severity::Dropped
        || match row.shown {
            Shown::Yes => true,
            Shown::No => false,
            Shown::UnlessAllCountsZero => !all_counts_zero(message),
        };
    PlainView {
        summary: row.sentence.to_owned(),
        rewritten: true,
        shown,
    }
}

/// Whether every number in `text` is zero, and there is at least one.
fn all_counts_zero(text: &str) -> bool {
    let mut any = false;
    for number in text.split(|c: char| !c.is_ascii_digit()) {
        if number.is_empty() {
            continue;
        }
        any = true;
        if !number.trim_start_matches('0').is_empty() {
            return false;
        }
    }
    any
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn view(code: Option<&str>, subject: &str, message: &str) -> PlainView {
        plain_view(Severity::Info, code, subject, message)
    }

    #[test]
    fn the_notes_from_issue_926_get_a_sentence() {
        let history = view(
            Some("inventor/source.kernel-history-unclosed"),
            "inventor/source.kernel-history-unclosed",
            "The kernel carrier's delta_state chain does not frame or close",
        );
        assert!(history.shown && history.rewritten);
        assert!(history
            .summary
            .starts_with("This part's feature history couldn't be read"));
        assert!(!history.summary.contains("delta_state"));
    }

    #[test]
    fn fusion_configuration_and_dimension_notes_get_a_sentence() {
        let configuration = view(
            Some("f3d/configuration.active-row-unbound"),
            "f3d/configuration.active-row-unbound",
            "1 Design configuration root record(s) were retained without an admitted active row of a unique table document.",
        );
        assert!(configuration.shown && configuration.rewritten);
        assert!(configuration
            .summary
            .starts_with("The saved configuration couldn't be read"));
        let dimension = view(
            Some("f3d/dimension.companion-untyped"),
            "f3d/dimension.companion-untyped",
            "2 payload-bearing Design dimension companion(s) were retained without a typed locus frame.",
        );
        assert!(dimension.shown && dimension.rewritten);
        assert!(dimension.summary.starts_with("Some sketch dimensions"));
        for note in [&configuration, &dimension] {
            assert!(!note.summary.contains("record"), "{}", note.summary);
            assert!(!note.summary.contains("companion"), "{}", note.summary);
        }
        // Their sibling codes say other things and keep their own words.
        assert!(row_for(Some("f3d/configuration.member-unassigned"), "", "").is_none());
    }

    #[test]
    fn notes_that_change_nothing_are_left_out() {
        for code in [
            "inventor/rse.segment-pair-untyped",
            "step/attribute.source-record-not-written",
            "step/native.source-record-omitted",
            "f3d/source.unindexed-records-retained",
            "history/body-scope-confirmed",
            "step/decode.opaque-record-preserved",
            "step/topology.metadata-not-represented",
        ] {
            let note = view(Some(code), code, "7 record(s)");
            assert!(note.rewritten && !note.shown, "{code}");
        }
    }

    #[test]
    fn timeline_folder_notes_are_left_out() {
        let folder = view(
            None,
            "timeline folders",
            "\"model-1\": 1 feature(s) grouped",
        );
        assert!(folder.rewritten && !folder.shown);
        // A dropped note is never hidden, whatever its row says.
        let dropped = plain_view(
            Severity::Dropped,
            Some("step/decode.opaque-record-preserved"),
            "x",
            "preserved 1 FOO instance(s) as named opaque STEP records",
        );
        assert!(dropped.shown);
    }

    #[test]
    fn zero_count_placement_statistics_are_left_out_but_real_ones_stay() {
        let zeros = "0 parts placed as last shown in the original program (0 directly bound, 0 holders derived from displayed children), 0 at current poses, 0 as first placed (smart); first-placement reasons: 0 without a current pose, 0 ambiguous, 0 unsupported, 0 joint checks, 0 bounds checks, 0 inserted-design checks; 0 holder conflicts";
        assert!(!view(None, "part positions", zeros).shown);
        let some = zeros.replace("0 holder conflicts", "2 holder conflicts");
        assert!(view(None, "part positions", &some).shown);
        let moved = view(
            None,
            "part positions",
            "3 parts sit at their design's origin because their placement could not be read",
        );
        assert!(moved.shown);
    }

    #[test]
    fn unknown_notes_keep_their_text_and_stay_visible() {
        let note = view(Some("f3d/something.new"), "f3d/something.new", "Raw words.");
        assert_eq!(
            note,
            PlainView {
                summary: "Raw words.".into(),
                rewritten: false,
                shown: true
            }
        );
        assert!(view(None, "joints", "2 joints").shown);
        // A count of nothing says nothing, whichever note it is on.
        assert!(!view(None, "joints", "0 joints, 0 loops").shown);
        assert!(!view(Some("f3d/something.new"), "x", "0 things").shown);
        // Unless the note was dropped.
        assert!(plain_view(Severity::Dropped, None, "joints", "0 joints").shown);
    }

    #[test]
    fn a_dropped_note_is_never_hidden() {
        let note = plain_view(
            Severity::Dropped,
            Some("step/native.source-record-omitted"),
            "",
            "1 record",
        );
        assert!(note.shown);
    }

    #[test]
    fn a_specific_code_beats_its_family() {
        let redundant = row_for(Some("dutchman/appearance.redundant-face"), "", "").unwrap();
        assert_eq!(redundant.shown, Shown::No);
        let wire = row_for(Some("dutchman/appearance.wire"), "", "").unwrap();
        assert_eq!(wire.shown, Shown::Yes);
    }

    #[test]
    fn every_row_has_a_sentence_that_ends_like_one() {
        for row in PLAIN_NOTES {
            assert!(row.sentence.ends_with('.'), "{}", row.sentence);
            assert!(row.code.is_empty() != row.subject.is_empty());
        }
    }
}
