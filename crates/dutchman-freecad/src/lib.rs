//! Rebuild scripts using FreeCAD's public Python scripting API. No FreeCAD code is linked.
mod generate;

use dutchman_core::{
    Check, Error, FeatureOutcome, FeatureStatus, Mode, Model, Outcome, Report, Severity, Writer,
};

/// The output report's format identifier.
pub const FORMAT: &str = "freecad-macro/1";

/// Writes a self-contained macro, including the original STEP geometry for recovery.
#[derive(Clone, Debug, Default)]
pub struct FreeCadWriter {
    import_only: bool,
}

impl FreeCadWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn import_only(mut self, enabled: bool) -> Self {
        self.import_only = enabled;
        self
    }

    fn part(
        &self,
        model: &Model,
        script: &mut String,
        report: &mut Report,
        outcomes: &mut Vec<FeatureOutcome>,
        depth: usize,
    ) -> Result<bool, Error> {
        if depth > dutchman_core::MAX_DEPTH {
            return Err(Error::Encode(
                "assembly nesting exceeds the macro limit".to_owned(),
            ));
        }
        if let Some(assembly) = &model.assembly {
            let mut rebuilt = false;
            for placement in &assembly.placements {
                let part = assembly
                    .parts
                    .get(placement.part)
                    .ok_or_else(|| Error::Encode("an assembly placement has no part".to_owned()))?;
                script.push_str("parents.append(container(\n");
                script.push_str(&generate::quoted(
                    placement.name.as_deref().unwrap_or(&part.name),
                )?);
                script.push_str(", ");
                script.push_str(&generate::data(&serde_json::json!(placement.transform))?);
                script.push_str("))\n");
                rebuilt |= self.part(part, script, report, outcomes, depth + 1)?;
                script.push_str("parents.pop()\n");
            }
            if !assembly.joints.is_empty() {
                report.push(
                    Severity::Degraded,
                    "joints",
                    "FreeCAD output keeps assembly placements; joints are not supported yet",
                );
            }
            return Ok(rebuilt);
        }
        let step = dutchman_solidcraft::restate_step(model, report)?;
        readable(&step)?;
        let mut mapping_report = Report::default();
        let mapping = dutchman_solidcraft::mapped_history(model, &mut mapping_report);
        let mut features = mapping.outcomes;
        let generated = if self.import_only {
            Err("geometry only was requested".to_owned())
        } else if model.ir.model.bodies.len() != 1 {
            Err("feature ownership across multiple bodies is not supported yet".to_owned())
        } else {
            mapping.features.and_then(|rows| generate::history(&rows))
        };
        let rebuilt = generated.is_ok();
        if rebuilt {
            report.append(mapping_report);
        } else {
            let reason = generated.as_ref().err().cloned().unwrap_or_default();
            for feature in &mut features {
                if feature.status == FeatureStatus::Mapped {
                    feature.status = FeatureStatus::Blocked;
                    feature.reason = Some(format!("part imported as STEP: {reason}"));
                }
            }
        }
        if depth > 0 {
            for feature in &mut features {
                feature.name = Some(format!(
                    "{}: {}",
                    model.name,
                    feature.name.as_deref().unwrap_or(&feature.kind),
                ));
            }
        }
        for feature in &features {
            if let Some(reason) = &feature.reason {
                report.push(
                    Severity::Info,
                    feature.name.as_deref().unwrap_or(&feature.kind),
                    reason,
                );
            }
        }
        outcomes.extend(features);
        let parameters = serde_json::to_value(&model.ir.model.parameters)
            .map_err(|error| Error::Encode(error.to_string()))?;
        if parameters.as_array().is_some_and(|rows| !rows.is_empty()) {
            script.push_str("parameter_table(");
            script.push_str(&generate::data(&parameters)?);
            script.push_str(")\n");
        }
        script.push_str("original = original_step(");
        script.push_str(&generate::quoted(&model.name)?);
        script.push_str(", ");
        script.push_str(&generate::quoted(&step)?);
        script.push_str(")\n");
        match generated {
            Ok(code) => {
                script.push_str("body = doc.addObject('PartDesign::Body', 'Body')\nbody.Label = ");
                script.push_str(&generate::quoted(&model.name)?);
                script.push_str("\nattach(body)\ntry:\n");
                for line in code.lines() {
                    script.push_str("    ");
                    script.push_str(line);
                    script.push('\n');
                }
                script.push_str("    doc.recompute()\n    if body.Shape.isNull() or not body.Shape.isValid():\n        raise RuntimeError('Rebuilt body has no valid shape')\n    compare_original(body.Shape, original.Shape)\n    visible(original, False)\nexcept Exception as error:\n    visible(body, False)\n    visible(original, True)\n    App.Console.PrintWarning('Rebuild failed; original STEP restored: ' + str(error) + '\\n')\n");
            }
            Err(reason) => {
                script.push_str("# Part kept as original STEP.\n");
                report.push(
                    Severity::Info,
                    &model.name,
                    format!("FreeCAD body imported as STEP: {reason}"),
                );
            }
        }
        Ok(rebuilt)
    }
}

/// Refuses STEP that FreeCAD's reader cannot take. FreeCAD 1.1 crashes the whole application on
/// trimming curves written as `CURVE_REPLICA`, so a macro embedding them would take the user's
/// other open documents down with it.
fn readable(step: &str) -> Result<(), Error> {
    if step.contains("CURVE_REPLICA(") {
        return Err(Error::Encode(
            "the geometry uses replicated trimming curves, which FreeCAD cannot read yet"
                .to_owned(),
        ));
    }
    Ok(())
}

impl Writer for FreeCadWriter {
    fn extension(&self) -> &'static str {
        "FCMacro"
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        let mut script = generate::PRELUDE.to_owned();
        let mut features = Vec::new();
        let rebuilt = self.part(model, &mut script, report, &mut features, 0)?;
        script.push_str("doc.recompute()\nif App.GuiUp:\n    Gui.activeDocument().activeView().viewAxonometric()\n    Gui.activeDocument().activeView().fitAll()\n");
        report.set_outcome(Outcome {
            mode: if rebuilt {
                Mode::History
            } else {
                Mode::Import
            },
            check: if rebuilt {
                Check::Unverified("run the macro in FreeCAD; original STEP is retained for comparison and recovery".to_owned())
            } else {
                Check::NotApplicable("parts kept as embedded STEP geometry".to_owned())
            },
            features,
        });
        out.write_all(script.as_bytes())
            .map_err(|error| Error::Encode(format!("cannot write the FreeCAD macro: {error}")))
    }
}

#[cfg(test)]
mod tests;
