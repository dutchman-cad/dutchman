//! What Dutchman can map of each source feature, found without SolidCraft and without writing:
//! the history mapping alone, with every step that only SolidCraft can confirm named as such
//! instead of being run.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use dutchman_core::{FeatureOutcome, FeatureStatus, Model, Report};
use serde_json::Value;

use crate::{history, repeats, SolidCraftWriter};

/// Why a feature that maps is kept only when SolidCraft confirms it while converting.
const INFERRED: &str = "its profile region is inferred from the sketch selection; SolidCraft's \
                        body check confirms it while converting";
const WHOLE_SKETCH: &str = "it takes the whole of a sketch with several profiles; SolidCraft's \
                            check confirms the result while converting";
const BLEND: &str = "its input edges are found on SolidCraft's replay of the history before it, \
                     while converting";
const PROFILE: &str = "its profile region is not checked against the source; SolidCraft's check \
                       of the converted history compares the result";

/// What mapping says of one source feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It maps to SolidCraft features as it is.
    Mapped,
    /// It maps, but is kept only when SolidCraft confirms it while converting; the text says
    /// what SolidCraft checks.
    NeedsCheck(&'static str),
    /// SolidCraft needs no feature for it (a principal datum plane, a component insert).
    NotNeeded,
    /// It is left out: suppressed in the source, or it changes no geometry there.
    LeftOut,
    /// Dutchman cannot map it yet; the outcome's reason says why.
    NotSupported,
    /// It is not looked at because an earlier feature blocks the history.
    AfterBlocker,
}

/// One source feature: what mapping made of it and the verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureVerdict {
    pub outcome: FeatureOutcome,
    pub verdict: Verdict,
}

/// The verdicts of one part's features.
#[derive(Debug)]
pub struct PartVerdicts<'a> {
    /// The part's file inside an assembly, or its name when it comes from the assembly's own
    /// file; `None` for a single part.
    pub part: Option<String>,
    /// The part's model, or a container's own records after removing child slices.
    pub model: Cow<'a, Model>,
    /// Its source features, in timeline order.
    pub features: Vec<FeatureVerdict>,
    /// Why none of its history maps, when none does and the features don't say.
    pub history: Option<String>,
}

/// Lists every feature whose profile region or blend edges would be inferred: each candidate is
/// taken as matching, as the source body check would take a match, so later features map as a
/// conversion maps them. Nothing is measured; the ids say which features were inferred.
#[derive(Default)]
struct AcceptEvery {
    inferred: Vec<u64>,
}

impl history::infer::Host for AcceptEvery {
    fn fit(
        &mut self,
        prefix: &history::History,
        _: Option<&[crate::verify::StoredBody]>,
        _: bool,
        _: bool,
    ) -> history::infer::Fit {
        if let Some(id) = prefix
            .features
            .last()
            .and_then(|feature| feature.get("id"))
            .and_then(Value::as_u64)
        {
            self.inferred.push(id);
        }
        history::infer::Fit::Matches(history::infer::Measure {
            against: dutchman_core::InferredAgainst::OriginalShape,
            deviation: None,
        })
    }
}

impl SolidCraftWriter {
    /// What mapping says of each source feature of `model`, part by part, without running
    /// SolidCraft or writing anything. Parts an assembly repeats are listed once, as
    /// conversion writes them once. Without an explicit target version, use the newest
    /// release known at build time, without discovering an installed CLI.
    pub fn verdicts<'a>(&self, model: &'a Model) -> Vec<PartVerdicts<'a>> {
        let Some(assembly) = &model.assembly else {
            return vec![self.part_verdicts(model, model, None)];
        };
        if let Some(flat) = crate::flattened_assembly(model) {
            return vec![self.part_verdicts(&flat, model, None)];
        }
        let repeated = repeats::find(assembly);
        let mut parts = Vec::new();
        repeats::leaves(assembly, &repeated, &mut Vec::new(), &mut parts);
        let mut listed: Vec<_> = parts
            .into_iter()
            .map(|part| {
                // A part read from the assembly's own file (a FreeCAD or multi-component
                // Fusion document) is named by its component, not by that shared file.
                let label = match &part.source_file {
                    Some(file) if part.source_file != model.source_file => file.clone(),
                    _ => part.name.clone(),
                };
                self.part_verdicts(part, part, Some(label))
            })
            .collect();
        self.container_verdicts(
            model,
            &format!("/{}", model.name),
            &repeated,
            &mut Vec::new(),
            &mut listed,
        );
        listed
    }

    // Containers retain the complete source CADIR, including records copied into component
    // slices. Only list records not already held below this container in the same file.
    fn container_verdicts<'a>(
        &self,
        model: &'a Model,
        path: &str,
        repeated: &BTreeMap<repeats::PartPath, repeats::PartPath>,
        indices: &mut repeats::PartPath,
        listed: &mut Vec<PartVerdicts<'a>>,
    ) {
        let Some(assembly) = &model.assembly else {
            return;
        };
        for (index, part) in assembly.parts.iter().enumerate() {
            indices.push(index);
            if !repeated.contains_key(indices) {
                self.container_verdicts(
                    part,
                    &format!("{path}/{}[{}]", part.name, index + 1),
                    repeated,
                    indices,
                    listed,
                );
            }
            indices.pop();
        }
        let mut features = BTreeSet::new();
        let mut parameters = BTreeSet::new();
        descendant_records(model, model, &mut features, &mut parameters);
        let mut ir = model.ir.clone();
        ir.model
            .features
            .retain(|row| !features.contains(row.id.as_str()));
        ir.model
            .parameters
            .retain(|row| !parameters.contains(row.id.as_str()));
        if ir.model.features.is_empty() && ir.model.parameters.is_empty() {
            return;
        }
        let mut own = Model {
            ir,
            fidelity: None,
            name: model.name.clone(),
            source_file: model.source_file.clone(),
            assembly: None,
            source_step: None,
            merge_bodies: false,
        };
        let mut verdicts = self.part_verdicts(&own, model, Some(format!("assembly: {path}")));
        // Map against the source geometry, but do not inventory aggregate bodies again.
        own.ir.geometry.bodies.clear();
        verdicts.model = Cow::Owned(own);
        listed.push(verdicts);
    }

    fn part_verdicts<'a>(
        &self,
        mapped: &Model,
        shown: &'a Model,
        part: Option<String>,
    ) -> PartVerdicts<'a> {
        // Each inferred profile candidate is accepted as SolidCraft's check would accept it,
        // so later features are mapped as converting maps them; the ids say which ones.
        let mut host = AcceptEvery::default();
        let mapping = history::map_with_profile_check(
            mapped,
            &mut Report::default(),
            self.target_version.unwrap_or_default(),
            self.sketches,
            &mut host,
            None,
        );
        let inferred = host.inferred;
        let mut checks: BTreeMap<usize, &'static str> = BTreeMap::new();
        if let Ok(history) = &mapping.result {
            for pending in &history.blends {
                checks.insert(pending.outcome, BLEND);
            }
            for id in &history.unconfirmed {
                if let Some(&row) = history.sources.get(id) {
                    checks.insert(row, WHOLE_SKETCH);
                }
            }
            for id in &inferred {
                if let Some(&row) = history.sources.get(id) {
                    checks.insert(row, INFERRED);
                }
            }
        }
        let features: Vec<FeatureVerdict> = mapping
            .features
            .into_iter()
            .enumerate()
            .map(|(row, outcome)| {
                let verdict = match outcome.status {
                    FeatureStatus::Mapped => match checks.get(&row) {
                        Some(&why) => Verdict::NeedsCheck(why),
                        None if outcome.profile_unverified => Verdict::NeedsCheck(PROFILE),
                        None => Verdict::Mapped,
                    },
                    FeatureStatus::NotNeeded => Verdict::NotNeeded,
                    FeatureStatus::LeftOut => Verdict::LeftOut,
                    FeatureStatus::Blocked
                        if outcome.reason.as_deref() == Some(history::AFTER_BLOCKER) =>
                    {
                        Verdict::AfterBlocker
                    }
                    FeatureStatus::Blocked => Verdict::NotSupported,
                };
                FeatureVerdict { outcome, verdict }
            })
            .collect();
        PartVerdicts {
            part,
            model: Cow::Borrowed(shown),
            history: mapping.result.err(),
            features,
        }
    }
}

fn descendant_records<'a>(
    root: &Model,
    model: &'a Model,
    features: &mut BTreeSet<&'a str>,
    parameters: &mut BTreeSet<&'a str>,
) {
    if let Some(assembly) = &model.assembly {
        for part in &assembly.parts {
            if part.source_file == root.source_file {
                features.extend(part.ir.model.features.iter().map(|row| row.id.as_str()));
                parameters.extend(part.ir.model.parameters.iter().map(|row| row.id.as_str()));
            }
            descendant_records(root, part, features, parameters);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testing::{cube, feature, with};

    #[test]
    fn a_design_without_history_has_one_part_and_says_why_nothing_maps() {
        let model = cube();
        let writer = SolidCraftWriter::new();
        let parts = writer.verdicts(&model);
        assert!(writer.detected_version.get().is_none());
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].part, None);
        assert!(parts[0].features.is_empty());
        assert_eq!(
            parts[0].history.as_deref(),
            Some("the design carries no feature history")
        );
    }

    #[test]
    fn each_feature_gets_the_mappers_verdict() {
        let native = json!({"kind": "native", "value": {"kind": "X", "parameters": {}}});
        let mut suppressed = feature(2, 1, "Hidden", native.clone());
        suppressed["suppressed"] = json!(true);
        let model = with(
            cube(),
            json!([
                feature(
                    1,
                    0,
                    "XY",
                    json!({"kind": "datum_principal_plane", "value": {"plane": "top"}})
                ),
                suppressed,
                feature(3, 2, "Odd", native.clone()),
                feature(4, 3, "Later", native),
            ]),
            json!([]),
        );
        let parts = SolidCraftWriter::new().verdicts(&model);
        let verdicts: Vec<(Option<&str>, Verdict)> = parts[0]
            .features
            .iter()
            .map(|f| (f.outcome.name.as_deref(), f.verdict))
            .collect();
        assert_eq!(
            verdicts,
            [
                (Some("XY"), Verdict::NotNeeded),
                (Some("Hidden"), Verdict::LeftOut),
                (Some("Odd"), Verdict::NotSupported),
                (Some("Later"), Verdict::AfterBlocker),
            ]
        );
        assert!(parts[0].features[2].outcome.reason.is_some());
    }
}
