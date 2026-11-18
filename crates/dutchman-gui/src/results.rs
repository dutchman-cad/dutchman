//! Readable RESULTS detail, using the existing conversion report only.
use crate::theme::Palette;
use eframe::egui::{self, RichText};
use serde_json::Value;

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Specific codes precede families. Codec namespaces do not change a family's meaning.
/// Keep the words here, never derive a surface title from an internal identifier.
const LOSS_TITLES: &[(&str, &str)] = &[
    (
        "native.source-record-omitted",
        "Internal records not needed for the shape",
    ),
    ("material.", "Materials weren't carried over"),
    (
        "appearance.face-pick-missed",
        "Some face colours couldn't be placed",
    ),
    (
        "appearance.redundant-face",
        "Repeated face colours weren't needed",
    ),
    (
        "appearance.",
        "Some appearance details weren't carried over",
    ),
    (
        "profile-region-inferred",
        "A sketch region couldn't be checked",
    ),
    (
        "profile-region-mismatch",
        "A sketch region differs from the original",
    ),
    ("body-mismatch", "Rebuilt features differ from the original"),
    (
        "configuration.inactive-bodies-left-out",
        "Bodies in other configurations were left out",
    ),
    (
        "export.no-exportable-solids",
        "No solid shape could be carried over",
    ),
    (
        "topology.face-no-writable-bounds",
        "Some face boundaries couldn't be carried over",
    ),
    (
        "topology.region-solid-reduced-to-sheet",
        "A solid was kept as a surface",
    ),
    (
        "geometry.procedural-definition-not-written",
        "The shape was kept without its construction rules",
    ),
    ("step-fallback", "A matching STEP supplied the final shape"),
    ("feature.", "Some feature details aren't supported yet"),
    ("history.", "Some feature history couldn't be rebuilt"),
    ("sketch.", "Some sketch details weren't carried over"),
    ("parameter.", "Some parameter details weren't carried over"),
    ("assembly.", "Some assembly details weren't carried over"),
    ("scene.", "Some saved display settings weren't carried over"),
    ("source.", "Extra source records were reported"),
    ("container.", "Some source records couldn't be read"),
    ("geometry.", "Some geometry details weren't carried over"),
    ("topology.", "Some shape connections weren't carried over"),
    ("native.", "Some native records weren't carried over"),
    ("metadata.", "Some document details weren't carried over"),
    (
        "attribute.",
        "Some labels or properties weren't carried over",
    ),
    (
        "configuration.",
        "Some configuration details weren't carried over",
    ),
    ("joint.", "Some joints weren't carried over"),
    ("annotation.", "Some annotations weren't carried over"),
    ("arena.", "Some internal records weren't carried over"),
    ("asset.", "Some attached assets weren't carried over"),
    (
        "attributes.",
        "Some labels or properties weren't carried over",
    ),
    ("body.", "Some body details weren't carried over"),
    (
        "body-recipe.",
        "Some body construction rules weren't carried over",
    ),
    (
        "body-reference.",
        "Some body references weren't carried over",
    ),
    ("brep.", "Some boundary geometry wasn't carried over"),
    ("cage.", "Some deformation controls weren't carried over"),
    ("card.", "Some source records weren't carried over"),
    (
        "carrier.",
        "Some curve or surface definitions weren't carried over",
    ),
    (
        "composition.",
        "Some nested design structure wasn't carried over",
    ),
    ("config.", "Some configuration details weren't carried over"),
    ("curve.", "Some curves weren't carried over"),
    (
        "curve-expression.",
        "Some curve expressions weren't carried over",
    ),
    (
        "curve-on-surface.",
        "Some curves on surfaces weren't carried over",
    ),
    ("curves.", "Some curves weren't carried over"),
    ("decode.", "Some source details weren't carried over"),
    ("deltas.", "Some stored shape changes weren't carried over"),
    ("depth.", "Some requested details weren't carried over"),
    (
        "design.",
        "Some editable design details weren't carried over",
    ),
    ("detail.", "Some detail views weren't carried over"),
    ("dimension.", "Some dimensions weren't carried over"),
    ("directory.", "Some source records weren't carried over"),
    ("drawing.", "Some drawing details weren't carried over"),
    ("edge.", "Some edges weren't carried over"),
    (
        "element-map.",
        "Some element references weren't carried over",
    ),
    ("entity.", "Some source objects weren't carried over"),
    ("exact.", "Some exact geometry wasn't carried over"),
    ("export.", "Some output details weren't carried over"),
    ("expression.", "Some expressions weren't carried over"),
    ("framing.", "Some source records weren't carried over"),
    ("global.", "Some document settings weren't carried over"),
    ("graph.", "Some source connections weren't carried over"),
    ("hatch.", "Some hatch details weren't carried over"),
    ("header.", "Some document details weren't carried over"),
    (
        "intersection.",
        "Some curve intersections weren't carried over",
    ),
    ("layer.", "Some layers weren't carried over"),
    ("legacy.", "Some older source records weren't carried over"),
    ("mesh.", "Some mesh details weren't carried over"),
    ("morph.", "Some deformations weren't carried over"),
    ("nurbs.", "Some spline details weren't carried over"),
    ("object.", "Some source objects weren't carried over"),
    ("occurrence.", "Some part placements weren't carried over"),
    ("parse.", "Some source records weren't carried over"),
    (
        "pcurve.",
        "Some surface boundary curves weren't carried over",
    ),
    ("physics.", "Some physical properties weren't carried over"),
    ("placement.", "Some placements weren't carried over"),
    (
        "pmi.",
        "Some manufacturing annotations weren't carried over",
    ),
    ("polyedge.", "Some connected edges weren't carried over"),
    (
        "presentation.",
        "Some display settings weren't carried over",
    ),
    ("prim.", "Some shape primitives weren't carried over"),
    ("product.", "Some product structure wasn't carried over"),
    ("property-set.", "Some properties weren't carried over"),
    ("protein.", "Some native records weren't carried over"),
    ("reference.", "Some references weren't carried over"),
    ("rse.", "Some feature references weren't carried over"),
    (
        "scope.",
        "Some feature body selections weren't carried over",
    ),
    ("section.", "Some section details weren't carried over"),
    ("stream.", "Some source records weren't carried over"),
    ("structure.", "Some assembly structure wasn't carried over"),
    ("target.", "Some target details weren't carried over"),
    (
        "tessellation.",
        "Some display mesh details weren't carried over",
    ),
    ("tspline.", "Some sculpted surfaces weren't carried over"),
    ("ufrx.", "Some native feature details weren't carried over"),
    ("validation.", "Some source checks weren't carried over"),
    ("vertex.", "Some points weren't carried over"),
    ("viewport.", "Some saved views weren't carried over"),
    (
        "work-plane.",
        "Some construction planes weren't carried over",
    ),
    ("writer.", "Some output details weren't carried over"),
    ("xform.", "Some transformations weren't carried over"),
    ("xref.", "Some linked files weren't carried over"),
    ("missing_geometry_stream", "Geometry could not be read"),
    (
        "topology_not_transferred",
        "Shape connections were not carried over",
    ),
    (
        "source_topology_invalid",
        "Some source shape connections could not be retained",
    ),
    (
        "geometry_not_transferred",
        "Some geometry was not carried over",
    ),
    (
        "geometry_agreement_sampled",
        "Shape agreement was checked at sample points",
    ),
    (
        "reference_graph_not_closed",
        "Some shape references could not be joined",
    ),
    (
        "topology_gauge_substituted",
        "Some shape connections were inferred",
    ),
    (
        "carrier_axis_inferred",
        "Some curve or surface directions were inferred",
    ),
    ("carrier_summary", "Curve and surface records were counted"),
    ("material_not_transferred", "Materials weren't carried over"),
    (
        "metadata_not_transferred",
        "Some document details were not carried over",
    ),
    (
        "attributes_not_transferred",
        "Some labels or properties were not carried over",
    ),
    (
        "feature_history_retained",
        "Native feature records were retained",
    ),
    (
        "assembly_components_external",
        "Some parts need separate files",
    ),
    (
        "assembly_placements_not_transferred",
        "Some assembly placements were not carried over",
    ),
    (
        "record_not_typed",
        "Some source records are not supported yet",
    ),
    ("decode_diagnostic", "Some source details could not be read"),
    (
        "integrity_failure",
        "Some source checks could not be completed",
    ),
    (
        "noncanonical_source_syntax",
        "Some source syntax was interpreted",
    ),
    (
        "source_dialect_unverified",
        "This source version has not been checked yet",
    ),
    (
        "source_dialect_displaced",
        "Some version details were inferred",
    ),
    ("mesh_vertex_precision", "Mesh point precision changed"),
    (
        "object_records_untransferred",
        "Some source objects were not carried over",
    ),
    (
        "unsupported_object_family",
        "Some object types are not supported yet",
    ),
    (
        "asset_not_transferred",
        "Some attached assets were not carried over",
    ),
    (
        "no_exportable_solids",
        "No solid shape could be carried over",
    ),
    ("hidden_body_omitted", "Hidden bodies were left out"),
    (
        "body_transform_not_applied",
        "Some body placements were not applied",
    ),
    (
        "analytic_surface_normalized",
        "Some surfaces were expressed in a standard form",
    ),
    (
        "elliptical_cone_reduced",
        "An elliptical cone was simplified",
    ),
    (
        "curveless_edge_omitted",
        "Some edges without curves were left out",
    ),
    (
        "unknown_surface_face_omitted",
        "Some faces with unresolved surfaces were left out",
    ),
    (
        "pcurve_omitted",
        "Some surface boundary curves were left out",
    ),
    ("subd_omitted", "Subdivision surfaces were left out"),
    ("tessellation_omitted", "Display meshes were left out"),
    ("point_cloud_omitted", "Point clouds were left out"),
    ("pmi_omitted", "Manufacturing annotations were left out"),
    (
        "source_association_omitted",
        "Some source associations were left out",
    ),
    (
        "passthrough_record_omitted",
        "Internal records not needed for the shape",
    ),
    (
        "procedural_reduced",
        "Some procedural shapes were simplified",
    ),
    (
        "parametric_record_omitted",
        "Some parametric records were left out",
    ),
    (
        "appearance_reduced",
        "Some appearance details were simplified",
    ),
    (
        "preserved_source_unavailable",
        "Some retained source records were unavailable",
    ),
    (
        "physics_not_transferred",
        "Some physical properties were not carried over",
    ),
    (
        "process_record_omitted",
        "Some manufacturing process records were left out",
    ),
];

fn known_title(code: &str) -> Option<&'static str> {
    let key = code.split_once('/').map_or(code, |(_, key)| key);
    LOSS_TITLES.iter().find_map(|(pattern, title)| {
        (if pattern.ends_with('.') {
            key.starts_with(pattern)
        } else {
            key == *pattern
        })
        .then_some(*title)
    })
}
fn loss_title(code: &str) -> &'static str {
    known_title(code).unwrap_or("Some details weren't carried over")
}

/// Dropped data is Lost; Degraded is Changed; Info is Note. Unknown levels stay visible.
fn severity(value: &Value) -> usize {
    match text(value, "level") {
        "Dropped" => 0,
        "Degraded" => 1,
        _ => 2,
    }
}
fn colour(p: &Palette, rank: usize) -> egui::Color32 {
    match rank {
        0 => p.danger,
        1 => p.lantern_text,
        _ => p.tide,
    }
}
const SEVERITIES: [&str; 3] = ["Lost", "Changed", "Note"];

fn checked(report: &Value) -> bool {
    text(report, "status") == "verified"
        || (text(report, "status") == "partial"
            && text(report, "reason").contains("retained features replay in SolidCraft;"))
}
fn verification(report: &Value) -> Value {
    report.get("verification").cloned().unwrap_or(Value::Null)
}
fn plain_reason(reason: &str) -> &str {
    match reason {
        "--import-only" => "Feature history was turned off for this conversion.",
        reason => reason,
    }
}
fn rebuilt(feature: &Value) -> bool {
    text(feature, "status") == "mapped" && feature.get("reapplied") != Some(&Value::Bool(true))
}
fn feature_checked(feature: &Value, check: &Value) -> bool {
    checked(check) && feature.get("profile_unverified") != Some(&Value::Bool(true))
}
fn feature_check<'a>(feature: &Value, report: &'a Value, overall: &'a Value) -> &'a Value {
    let parts = report.pointer("/history/parts").and_then(Value::as_array);
    if let Some(parts) = parts.filter(|parts| !parts.is_empty()) {
        let mut matches = parts
            .iter()
            .filter(|part| text(feature, "name").starts_with(&format!("{}: ", text(part, "name"))));
        if let Some(part) = matches.next().filter(|_| matches.next().is_none()) {
            return part;
        }
        if text(overall, "status") != "verified" {
            return &Value::Null;
        }
    }
    overall
}
fn chip(ui: &mut egui::Ui, p: &Palette, label: String, color: egui::Color32) {
    // Measure before allocation: a Frame would lay its text out in the remaining sliver
    // of a wrapped row, turning a short chip into a column of letters.
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.clone(), font, color);
    let padding = egui::vec2(7.0, 3.0);
    let size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_stroke(
        rect,
        4,
        egui::Stroke::new(1.0_f32, p.hairline),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(rect.min + padding, galley, color);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.as_str()));
}

pub(crate) fn summary(ui: &mut egui::Ui, p: &Palette, report: &Value) {
    let history = report.get("history").unwrap_or(&Value::Null);
    let check = verification(report);
    let features = history.get("features").and_then(Value::as_array);
    let parts = history.get("parts").and_then(Value::as_array);
    let regions = features
        .into_iter()
        .flatten()
        .filter(|feature| {
            rebuilt(feature) && feature.get("profile_unverified") == Some(&Value::Bool(true))
        })
        .count();
    let assembly = parts.is_some_and(|parts| !parts.is_empty());
    let unchecked: u64 = if let Some(parts) = parts.filter(|parts| !parts.is_empty()) {
        parts
            .iter()
            .filter(|part| {
                !checked(part)
                    && part
                        .get("rebuilt")
                        .and_then(Value::as_u64)
                        .unwrap_or_else(|| number(part, "mapped"))
                        > 0
            })
            .count() as u64
    } else if let Some(features) = features.filter(|rows| !rows.is_empty()) {
        features
            .iter()
            .filter(|f| rebuilt(f) && !feature_checked(f, &check))
            .count() as u64
    } else if checked(&check) {
        0
    } else {
        number(history, "rebuilt").max(number(history, "mapped"))
    };
    ui.horizontal_wrapped(|ui| {
        let count = history
            .get("rebuilt")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| number(history, "mapped"));
        let total = history
            .get("needing")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| number(history, "total"));
        chip(ui, p, format!("{count} of {total} features rebuilt"), p.ink);
        let reapplied = number(history, "reapplied");
        if reapplied > 0 {
            chip(ui, p, format!("{reapplied} re-applied"), p.lantern_text);
        }
        let shape = if text(report, "mode") == "history" {
            "rebuilt"
        } else {
            "imported"
        };
        chip(ui, p, format!("Final shape: {shape}"), p.ink);
        if unchecked > 0 {
            let unit = if assembly && unchecked == 1 {
                " part"
            } else if assembly {
                " parts"
            } else {
                ""
            };
            chip(
                ui,
                p,
                format!("{unchecked}{unit} unchecked"),
                p.lantern_text,
            );
        } else if checked(&check) && regions == 0 {
            let application = if report
                .pointer("/output/format")
                .and_then(Value::as_str)
                .is_some_and(|format| format.starts_with("freecad-"))
            {
                "FreeCAD"
            } else {
                "SolidCraft"
            };
            chip(ui, p, format!("Checked in {application} ✓"), p.spectre);
        } else if !assembly || regions == 0 {
            chip(ui, p, "Shape check unavailable".into(), p.tide);
        }
        if assembly && regions > 0 {
            chip(
                ui,
                p,
                format!("{regions} sketch regions unchecked"),
                p.lantern_text,
            );
        }
        for (rank, label) in SEVERITIES.iter().enumerate() {
            let count = report
                .get("losses")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|loss| severity(loss) == rank)
                .count();
            chip(ui, p, format!("{label}: {count}"), colour(p, rank));
        }
    });
}

fn timeline(ui: &mut egui::Ui, p: &Palette, report: &Value) {
    ui.label(RichText::new("Feature timeline").strong());
    let check = verification(report);
    let mut rows: Vec<_> = report
        .pointer("/history/features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    // Stable sort keeps source order when ordinals are absent or equal.
    if report
        .pointer("/history/parts")
        .and_then(Value::as_array)
        .is_none_or(|parts| parts.is_empty())
        && rows
            .iter()
            .all(|f| f.get("ordinal").and_then(Value::as_u64).is_some())
    {
        rows.sort_by_key(|f| number(f, "ordinal"));
    }
    if rows.is_empty() {
        ui.label(
            RichText::new("No feature rows were recorded.")
                .small()
                .color(p.tide),
        );
    }
    for feature in rows {
        let mapped = rebuilt(feature);
        let check = feature_check(feature, report, &check);
        let (icon, status, color) = if mapped && feature_checked(feature, check) {
            ("✓", "Rebuilt and checked", p.spectre)
        } else if mapped {
            ("!", "Rebuilt but unchecked", p.lantern_text)
        } else if text(feature, "status") == "blocked" {
            ("—", "Not rebuilt", p.lantern_text)
        } else {
            ("—", "Not rebuilt", p.tide)
        };
        let reason = match text(feature, "reason") {
            "" => match text(feature, "status") {
                "not_needed" => "No separate feature needed",
                "left_out" => "Left out of editable history",
                "blocked" => "Not supported yet",
                _ if feature.get("reapplied") == Some(&Value::Bool(true)) => {
                    "Re-applied to the final body"
                }
                _ if feature.get("final_body") == Some(&Value::Bool(true)) => {
                    "Stores the final body"
                }
                _ if feature.get("profile_unverified") == Some(&Value::Bool(true)) => {
                    "Rebuilt; sketch region unchecked"
                }
                _ => status,
            },
            reason => reason,
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).color(color))
                .on_hover_text(status);
            ui.add(egui::Label::new(format!("{}: {reason}", text(feature, "name"))).truncate())
                .on_hover_text(format!("{status}. {reason}"));
        });
    }
}

fn loss_cards(ui: &mut egui::Ui, p: &Palette, losses: &[&Value]) {
    for (rank, label) in SEVERITIES.iter().enumerate() {
        let mut groups: std::collections::BTreeMap<&str, Vec<&Value>> = Default::default();
        for loss in losses.iter().filter(|loss| severity(loss) == rank) {
            groups.entry(text(loss, "code")).or_default().push(loss);
        }
        if groups.is_empty() {
            continue;
        }
        ui.label(RichText::new(*label).strong().color(colour(p, rank)));
        for (code, notes) in groups {
            ui.push_id((rank, code), |ui| {
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0_f32, p.hairline))
                    .inner_margin(8)
                    .corner_radius(4)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let mut title = loss_title(code).to_owned();
                        if code.ends_with("native.source-record-omitted") {
                            // The existing message states the source record count; this is not decoding.
                            let records: u64 = notes
                                .iter()
                                .filter_map(|n| {
                                    text(n, "message")
                                        .split_whitespace()
                                        .next()?
                                        .parse::<u64>()
                                        .ok()
                                })
                                .fold(0_u64, u64::saturating_add);
                            if records > 0 {
                                title =
                                    format!("{records} internal records not needed for the shape");
                            }
                        }
                        ui.label(RichText::new(format!("{title} · {}", notes.len())).color(p.ink));
                        egui::CollapsingHeader::new("Details").show(ui, |ui| {
                            if !code.is_empty() {
                                ui.add(egui::Label::new(RichText::new(code).monospace()).wrap());
                            }
                            for note in notes {
                                ui.add(egui::Label::new(text(note, "message")).wrap());
                            }
                        });
                    });
            });
        }
    }
}

pub(crate) fn details(ui: &mut egui::Ui, p: &Palette, report: &Value) {
    let parts = report.pointer("/history/parts").and_then(Value::as_array);
    if parts.is_none_or(|parts| parts.is_empty()) {
        if let Some(reason) = report
            .pointer("/verification/reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty())
        {
            ui.add(egui::Label::new(plain_reason(reason)).wrap());
        }
    }
    let components = dutchman::json::components(report);
    if parts.is_none_or(|p| p.is_empty()) && components.iter().all(|c| c.file.is_none()) {
        for note in report
            .get("losses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|note| {
                text(note, "subject") == "history"
                    && text(note, "level") == "Info"
                    && text(note, "message").starts_with("feature history left out:")
            })
        {
            ui.add(egui::Label::new(text(note, "message")).wrap());
        }
        timeline(ui, p, report);
        ui.label(RichText::new("Loss details").strong());
        let losses: Vec<_> = components
            .iter()
            .flat_map(|c| c.losses.iter().copied())
            .collect();
        loss_cards(ui, p, &losses);
        return;
    }
    ui.label(RichText::new("Parts").strong());
    let mut names: Vec<String> = parts
        .into_iter()
        .flatten()
        .map(|p| text(p, "name").to_owned())
        .collect();
    for c in &components {
        if let Some(name) = &c.file {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    let id = ui.make_persistent_id("show-all-parts");
    let all = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    for (index, name) in names
        .iter()
        .enumerate()
        .take(if all { usize::MAX } else { 10 })
    {
        let width = (ui.available_width() - ui.spacing().indent - 8.0).max(40.0);
        let font = egui::TextStyle::Button.resolve(ui.style());
        let heading = ui.painter().layout(name.clone(), font, p.ink, width);
        egui::CollapsingHeader::new(egui::WidgetText::Galley(heading))
            .id_salt(("part", index))
            .default_open(index == 0)
            .show(ui, |ui| {
                // Part names can repeat; the source report's row identifies the part.
                if let Some(part) = parts.and_then(|parts| parts.get(index)) {
                    let rebuilt = part
                        .get("rebuilt")
                        .and_then(Value::as_u64)
                        .unwrap_or_else(|| number(part, "mapped"));
                    ui.label(format!(
                        "{rebuilt} of {} features rebuilt",
                        part.get("needing")
                            .and_then(Value::as_u64)
                            .unwrap_or_else(|| number(part, "total"))
                    ));
                    if checked(part) {
                        ui.label(RichText::new("Checked in SolidCraft ✓").color(p.spectre));
                    } else if rebuilt > 0 {
                        ui.label(RichText::new("Rebuilt history unchecked").color(p.lantern_text));
                    }
                    if !text(part, "reason").is_empty() {
                        ui.add(egui::Label::new(plain_reason(text(part, "reason"))).wrap());
                    }
                }
                for component in components.iter().filter(|c| c.file.as_ref() == Some(name)) {
                    loss_cards(ui, p, &component.losses);
                }
            });
    }
    if names.len() > 10
        && ui
            .button(if all {
                "Show first 10".to_owned()
            } else {
                format!("Show all {}", names.len())
            })
            .clicked()
    {
        ui.data_mut(|d| d.insert_temp(id, !all));
    }
    for component in components
        .iter()
        .filter(|component| component.file.is_none())
    {
        ui.label(RichText::new("Design loss details").strong());
        loss_cards(ui, p, &component.losses);
    }
    timeline(ui, p, report);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_dutchman_emitted_loss_code_has_a_title() {
        // Production code owns these fixed codes; scan the emitters so a new code requires a title.
        for source in [
            include_str!("../../dutchman-core/src/report.rs"),
            include_str!("../../dutchman-core/src/decode.rs"),
            include_str!("../../dutchman-solidcraft/src/lib.rs"),
            include_str!("../../dutchman-solidcraft/src/step.rs"),
            include_str!("../../dutchman-solidcraft/src/history/mod.rs"),
            include_str!("../../dutchman-solidcraft/src/history/infer.rs"),
        ] {
            for literal in source.split('"').skip(1).step_by(2) {
                if [
                    "dutchman/appearance.",
                    "dutchman/configuration.",
                    "history/",
                    "step/export.",
                    "step/geometry.",
                    "step/topology.",
                ]
                .iter()
                .any(|prefix| literal.starts_with(prefix))
                    && !literal.contains(char::is_whitespace)
                {
                    assert!(
                        known_title(literal).is_some(),
                        "missing title for {literal}"
                    );
                }
            }
        }
        let inventory = include_str!("../tests/support/loss-codes.txt");
        let pin = inventory
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(6)
            .unwrap()
            .trim_end_matches(';');
        assert!(
            include_str!("../../../Cargo.toml").contains(pin),
            "Refresh the loss-code inventory from the public pinned cadmpeg source"
        );
        for code in inventory.lines().filter(|line| !line.starts_with('#')) {
            assert!(
                known_title(code).is_some(),
                "missing title for codec loss {code}"
            );
        }
        assert_eq!(
            loss_title("future/new-code"),
            "Some details weren't carried over"
        );
        assert_eq!(
            loss_title("f3d/material.not-transferred"),
            "Materials weren't carried over"
        );
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use crate::theme;
    use egui_kittest::{kittest::Queryable, Harness};
    use serde_json::json;
    fn sample() -> Value {
        json!({"ok":true,"mode":"partial_history",
        "verification":{"status":"partial","reason":"history stopped at Chamfer 1; retained features replay in SolidCraft; the full history is not verified"},
        "history":{"rebuilt":2,"needing":3,"features":[
            {"name":"Chamfer 1","ordinal":3,"status":"blocked","reason":"Blends on closed edges aren't supported yet"},
            {"name":"Sketch 1","ordinal":1,"status":"mapped"},
            {"name":"Extrude 1","ordinal":2,"status":"mapped","profile_unverified":true}]},
        "losses":[
            {"level":"Info","code":"step/native.source-record-omitted","message":"784 source-native record(s) were omitted"},
            {"level":"Degraded","code":"history/profile-region-inferred","message":"raw profile reason"},
            {"level":"Dropped","code":"f3d/material.not-transferred","message":"raw material reason"}]})
    }
    #[test]
    fn chips_timeline_stop_and_losses_are_open_in_source_and_severity_order() {
        let report = sample();
        let mut h = Harness::builder()
            .with_size(egui::vec2(760.0, 1000.0))
            .build_ui(|ui| {
                let p = *theme::palette(ui);
                summary(ui, &p, &report);
                details(ui, &p, &report);
            });
        h.run_steps(6);
        for label in [
            "2 of 3 features rebuilt",
            "Final shape: imported",
            "1 unchecked",
            "Lost: 1",
            "Changed: 1",
            "Note: 1",
        ] {
            h.get_by_label(label);
        }
        let sketch = h.get_by_label("Sketch 1: Rebuilt and checked").rect();
        let extrude = h
            .get_by_label("Extrude 1: Rebuilt; sketch region unchecked")
            .rect();
        let stop = h
            .get_by_label("Chamfer 1: Blends on closed edges aren't supported yet")
            .rect();
        assert!(sketch.top() < extrude.top() && extrude.top() < stop.top());
        let lost = h.get_by_label("Lost").rect();
        let changed = h.get_by_label("Changed").rect();
        let note = h.get_by_label("Note").rect();
        assert!(lost.top() < changed.top() && changed.top() < note.top());
        h.get_by_label("784 internal records not needed for the shape · 1");
        assert!(h.query_by_label("f3d/material.not-transferred").is_none());
        h.query_all_by_label("Details")
            .next()
            .unwrap()
            .click_accesskit();
        h.run_steps(6);
        h.get_by_label("f3d/material.not-transferred");
        h.get_by_label("raw material reason");
    }
    #[test]
    fn chips_wrap_as_whole_labels_in_a_narrow_card() {
        let report = sample();
        let mut h = Harness::builder()
            .with_size(egui::vec2(432.0, 600.0))
            .build_ui(|ui| {
                theme::install(ui.ctx());
                let p = *theme::palette(ui);
                summary(ui, &p, &report);
            });
        h.run_steps(6);
        for label in [
            "2 of 3 features rebuilt",
            "Final shape: imported",
            "1 unchecked",
            "Lost: 1",
            "Changed: 1",
            "Note: 1",
        ] {
            let rect = h.get_by_label(label).rect();
            assert!(rect.height() < 35.0, "{label}: {rect:?}");
            assert!(
                rect.left() >= 0.0 && rect.right() <= 432.0,
                "{label}: {rect:?}"
            );
        }
    }
    #[test]
    fn history_choice_and_single_unchecked_part_use_plain_messages() {
        for (report, label) in [
            (
                json!({"mode":"import", "verification":{"status":"not_applicable","reason":"--import-only"}}),
                "Feature history was turned off for this conversion.",
            ),
            (
                json!({"history":{"rebuilt":1,"needing":1,"parts":[{"name":"Part 1","mapped":1,"total":1,"status":"unverified"}]}}),
                "1 part unchecked",
            ),
        ] {
            let mut h = Harness::builder().build_ui(|ui| {
                let p = *theme::palette(ui);
                summary(ui, &p, &report);
                details(ui, &p, &report);
            });
            h.run_steps(6);
            h.get_by_label(label);
            assert!(h.query_by_label("--import-only").is_none());
            assert!(h.query_by_label("1 parts unchecked").is_none());
        }
    }
    #[test]
    fn part_groups_stop_after_ten_until_show_all() {
        let mut report = sample();
        report["history"]["parts"]=json!((1..=14).map(|n|json!({"name":format!("Part {n}"),"rebuilt":1,"needing":2,"status":"unverified"})).collect::<Vec<_>>());
        let mut h = Harness::builder()
            .with_size(egui::vec2(760.0, 2000.0))
            .build_ui(|ui| {
                let p = *theme::palette(ui);
                details(ui, &p, &report);
            });
        h.run_steps(6);
        h.get_by_label("Part 10");
        assert!(h.query_by_label("Part 11").is_none());
        h.get_by_label("Show all 14").click_accesskit();
        h.run_steps(6);
        h.get_by_label("Part 14");
    }
    #[test]
    fn assembly_ordinals_keep_part_order_and_use_only_attributable_checks() {
        let report = json!({"verification":{"status":"partial","reason":"retained features replay in SolidCraft; one part only"}, "history":{
            "parts":[{"name":"A","status":"verified"},{"name":"B","status":"unverified"}],
            "features":[{"name":"A: Sketch","ordinal":1,"status":"mapped"},{"name":"A: Extrude","ordinal":2,"status":"mapped"},{"name":"B: Sketch","ordinal":1,"status":"mapped"},{"name":"Unattributed: Sketch","ordinal":1,"status":"mapped"}]}});
        let mut h = Harness::builder().build_ui(|ui| {
            let p = *theme::palette(ui);
            timeline(ui, &p, &report);
        });
        h.run_steps(6);
        let a = h.get_by_label("A: Sketch: Rebuilt and checked").rect();
        let b = h.get_by_label("A: Extrude: Rebuilt and checked").rect();
        let c = h.get_by_label("B: Sketch: Rebuilt but unchecked").rect();
        assert!(a.top() < b.top() && b.top() < c.top());
        h.get_by_label("Unattributed: Sketch: Rebuilt but unchecked");
    }
    #[test]
    fn mismatch_and_reapplied_features_never_claim_checked_rebuilt_history() {
        let mut report = sample();
        report["verification"]["reason"] =
            json!("retained features replay in SolidCraft, but bodies differ");
        report["history"]["features"][1]["reapplied"] = json!(true);
        let mut h = Harness::builder().build_ui(|ui| {
            let p = *theme::palette(ui);
            summary(ui, &p, &report);
            details(ui, &p, &report);
        });
        h.run_steps(6);
        assert!(h.query_by_label("Checked in SolidCraft ✓").is_none());
        h.get_by_label("Sketch 1: Re-applied to the final body");
    }
}
