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
    ("record.unreadable", "Part of the source could not be read"),
    (
        "crate.value-not-read",
        "Stored attribute values could not be read",
    ),
    (
        "feature.thread-definition-partial",
        "Thread definition is incomplete",
    ),
    (
        "feature.thread-live-face-unproven",
        "Thread face couldn't be checked",
    ),
    (
        "appearance.reduced-to-base-color",
        "Textures and shaders reduced to base colours",
    ),
    (
        "appearance.binding-metadata-reduced",
        "Appearance binding properties weren't carried over",
    ),
    (
        "appearance.material-not-transferred",
        "Materials weren't carried over",
    ),
    (
        "appearance.face-color-unresolved",
        "Face colours couldn't be resolved",
    ),
    (
        "appearance.face-override-unresolved",
        "Face appearance overrides couldn't be resolved",
    ),
    (
        "appearance.texture-unlocated",
        "Texture images couldn't be located",
    ),
    (
        "appearance.wire-body-transparency-omitted",
        "Wire body transparency wasn't carried over",
    ),
    ("appearance.decal-", "Decals couldn't be fully carried over"),
    (
        "sketch.name-composed-from-feature-ordinal",
        "Sketch names inferred from feature order",
    ),
    (
        "sketch.curve-region-boundary",
        "Sketch curves bound a profile region",
    ),
    (
        "sketch.curve-unprofiled",
        "Sketch curves outside closed profiles",
    ),
    (
        "dimension.companion-untyped",
        "Dimension reference frames couldn't be resolved",
    ),
    (
        "configuration.active-row-unbound",
        "Active configuration couldn't be resolved",
    ),
    (
        "assembly.operation-table-pose-native",
        "Assembly operation placements couldn't be resolved",
    ),
    (
        "source.unindexed-records-retained",
        "Unindexed source records retained without decoding",
    ),
    (
        "design.parametric-records-omitted",
        "Parametric design records weren't written to STEP",
    ),
    (
        "attribute.source-record-not-written",
        "Source attributes weren't written to STEP",
    ),
    ("appearance.wire", "Wire appearance wasn't carried over"),
    (
        "native.source-record-omitted",
        "Internal records not needed for the shape",
    ),
    ("material.", "Materials weren't carried over"),
    (
        "appearance.face-pick-missed",
        "Face colours couldn't be placed",
    ),
    (
        "appearance.redundant-face",
        "Repeated face colours weren't needed",
    ),
    ("appearance.", "Appearance wasn't carried over"),
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
        "Face boundaries couldn't be carried over",
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
    (
        "feature.",
        "Feature construction rules aren't supported yet",
    ),
    ("history.", "Feature history couldn't be rebuilt"),
    ("sketch.", "Sketch entities weren't carried over"),
    ("parameter.", "Parameter expressions weren't carried over"),
    ("assembly.", "Assembly relationships weren't carried over"),
    ("scene.", "Saved display settings weren't carried over"),
    ("source.", "Extra source records were reported"),
    ("container.", "Source records couldn't be read"),
    ("geometry.", "Curves and surfaces weren't carried over"),
    ("topology.", "Shape connections weren't carried over"),
    ("native.", "Native records weren't carried over"),
    ("metadata.", "Document properties weren't carried over"),
    ("attribute.", "Labels and properties weren't carried over"),
    ("configuration.", "Configurations weren't carried over"),
    ("joint.", "Joints weren't carried over"),
    ("annotation.", "Annotations weren't carried over"),
    ("arena.", "Internal records weren't carried over"),
    ("asset.", "Attached assets weren't carried over"),
    ("attributes.", "Labels and properties weren't carried over"),
    ("body.", "Body definitions weren't carried over"),
    (
        "body-recipe.",
        "Body construction rules weren't carried over",
    ),
    ("body-reference.", "Body references weren't carried over"),
    ("brep.", "Boundary geometry wasn't carried over"),
    ("cage.", "Deformation controls weren't carried over"),
    ("card.", "Source records weren't carried over"),
    (
        "carrier.",
        "Curve and surface definitions weren't carried over",
    ),
    (
        "composition.",
        "Nested design structure wasn't carried over",
    ),
    ("config.", "Configurations weren't carried over"),
    ("curve.", "Curves weren't carried over"),
    (
        "curve-expression.",
        "Curve expressions weren't carried over",
    ),
    (
        "curve-on-surface.",
        "Curves on surfaces weren't carried over",
    ),
    ("curves.", "Curves weren't carried over"),
    ("decode.", "Source records weren't carried over"),
    ("deltas.", "Stored shape changes weren't carried over"),
    ("depth.", "Requested geometry wasn't carried over"),
    ("design.", "Editable design records weren't carried over"),
    ("detail.", "Detail views weren't carried over"),
    ("dimension.", "Dimensions weren't carried over"),
    ("directory.", "Source records weren't carried over"),
    ("drawing.", "Drawing views weren't carried over"),
    ("edge.", "Edges weren't carried over"),
    ("element-map.", "Element references weren't carried over"),
    ("entity.", "Source objects weren't carried over"),
    ("exact.", "Exact geometry wasn't carried over"),
    ("export.", "Output records weren't carried over"),
    ("expression.", "Expressions weren't carried over"),
    ("framing.", "Source records weren't carried over"),
    ("global.", "Document settings weren't carried over"),
    ("graph.", "Source connections weren't carried over"),
    ("hatch.", "Hatch patterns weren't carried over"),
    ("header.", "Document properties weren't carried over"),
    ("intersection.", "Curve intersections weren't carried over"),
    ("layer.", "Layers weren't carried over"),
    ("legacy.", "Older source records weren't carried over"),
    ("mesh.", "Display meshes weren't carried over"),
    ("morph.", "Deformations weren't carried over"),
    ("nurbs.", "Spline definitions weren't carried over"),
    ("object.", "Source objects weren't carried over"),
    ("occurrence.", "Part placements weren't carried over"),
    ("parse.", "Source records weren't carried over"),
    ("pcurve.", "Surface boundary curves weren't carried over"),
    ("physics.", "Physical properties weren't carried over"),
    ("placement.", "Placements weren't carried over"),
    ("pmi.", "Manufacturing annotations weren't carried over"),
    ("polyedge.", "Connected edges weren't carried over"),
    ("presentation.", "Display settings weren't carried over"),
    ("prim.", "Shape primitives weren't carried over"),
    ("product.", "Product structure wasn't carried over"),
    ("property-set.", "Properties weren't carried over"),
    ("protein.", "Native records weren't carried over"),
    ("reference.", "References weren't carried over"),
    ("rse.", "Feature references weren't carried over"),
    ("scope.", "Feature body selections weren't carried over"),
    ("section.", "Section views weren't carried over"),
    ("stream.", "Source records weren't carried over"),
    ("structure.", "Assembly structure wasn't carried over"),
    ("target.", "Selection targets weren't carried over"),
    ("tessellation.", "Display meshes weren't carried over"),
    ("tspline.", "Sculpted surfaces weren't carried over"),
    ("ufrx.", "Native feature definitions weren't carried over"),
    ("validation.", "Source checks weren't carried over"),
    ("vertex.", "Points weren't carried over"),
    ("viewport.", "Saved views weren't carried over"),
    ("work-plane.", "Construction planes weren't carried over"),
    ("writer.", "Output records weren't carried over"),
    ("xform.", "Transformations weren't carried over"),
    ("xref.", "Linked files weren't carried over"),
    ("missing_geometry_stream", "Geometry could not be read"),
    (
        "topology_not_transferred",
        "Shape connections were not carried over",
    ),
    (
        "source_topology_invalid",
        "Source shape connections could not be retained",
    ),
    ("geometry_not_transferred", "Geometry was not carried over"),
    (
        "geometry_agreement_sampled",
        "Shape agreement was checked at sample points",
    ),
    (
        "reference_graph_not_closed",
        "Shape references could not be joined",
    ),
    (
        "topology_gauge_substituted",
        "Shape connections were inferred",
    ),
    (
        "carrier_axis_inferred",
        "Curve or surface directions were inferred",
    ),
    ("carrier_summary", "Curve and surface records were counted"),
    ("material_not_transferred", "Materials weren't carried over"),
    (
        "metadata_not_transferred",
        "Document properties were not carried over",
    ),
    (
        "attributes_not_transferred",
        "Labels and properties were not carried over",
    ),
    (
        "feature_history_retained",
        "Native feature records were retained",
    ),
    ("assembly_components_external", "Parts need separate files"),
    (
        "assembly_placements_not_transferred",
        "Assembly placements were not carried over",
    ),
    ("record_not_typed", "Source records are not supported yet"),
    ("decode_diagnostic", "Source records could not be read"),
    ("integrity_failure", "Source checks could not be completed"),
    (
        "noncanonical_source_syntax",
        "Source syntax was interpreted",
    ),
    (
        "source_dialect_unverified",
        "This source version has not been checked yet",
    ),
    ("source_dialect_displaced", "Source version was inferred"),
    ("mesh_vertex_precision", "Mesh point precision changed"),
    (
        "object_records_untransferred",
        "Source objects were not carried over",
    ),
    (
        "unsupported_object_family",
        "Object types are not supported yet",
    ),
    (
        "asset_not_transferred",
        "Attached assets were not carried over",
    ),
    (
        "no_exportable_solids",
        "No solid shape could be carried over",
    ),
    ("hidden_body_omitted", "Hidden bodies were left out"),
    (
        "body_transform_not_applied",
        "Body placements were not applied",
    ),
    (
        "analytic_surface_normalized",
        "Surfaces were expressed in a standard form",
    ),
    (
        "elliptical_cone_reduced",
        "An elliptical cone was simplified",
    ),
    (
        "curveless_edge_omitted",
        "Edges without curves were left out",
    ),
    (
        "unknown_surface_face_omitted",
        "Faces with unresolved surfaces were left out",
    ),
    ("pcurve_omitted", "Surface boundary curves were left out"),
    ("subd_omitted", "Subdivision surfaces were left out"),
    ("tessellation_omitted", "Display meshes were left out"),
    ("point_cloud_omitted", "Point clouds were left out"),
    ("pmi_omitted", "Manufacturing annotations were left out"),
    (
        "source_association_omitted",
        "Source associations were left out",
    ),
    (
        "passthrough_record_omitted",
        "Internal records not needed for the shape",
    ),
    ("procedural_reduced", "Procedural shapes were simplified"),
    (
        "parametric_record_omitted",
        "Parametric records were left out",
    ),
    ("appearance_reduced", "Appearance were simplified"),
    (
        "preserved_source_unavailable",
        "Retained source records were unavailable",
    ),
    (
        "physics_not_transferred",
        "Physical properties were not carried over",
    ),
    (
        "process_record_omitted",
        "Manufacturing process records were left out",
    ),
];

fn known_title(code: &str) -> Option<&'static str> {
    let key = code.split_once('/').map_or(code, |(_, key)| key);
    LOSS_TITLES.iter().find_map(|(pattern, title)| {
        (if pattern.ends_with(['.', '-']) {
            key.starts_with(pattern)
        } else {
            key == *pattern
        })
        .then_some(*title)
    })
}
fn loss_title(code: &str) -> &'static str {
    known_title(code).unwrap_or("Conversion data wasn't carried over")
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
fn history_off(report: &Value) -> bool {
    text(&verification(report), "reason") == "--import-only"
        || report
            .get("losses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|loss| {
                text(loss, "subject") == "history"
                    && text(loss, "message").contains("--import-only")
            })
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
        if history_off(report) {
            chip(ui, p, "History off".into(), p.tide);
        } else {
            chip(ui, p, format!("{count} of {total} features rebuilt"), p.ink);
        }
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
            if count > 0 {
                chip(ui, p, format!("{label}: {count}"), colour(p, rank));
            }
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

fn observation_title(loss: &Value) -> &'static str {
    let message = text(loss, "message");
    if message.contains("thread") {
        "Thread cosmetic data"
    } else if message.contains("appearance") || text(loss, "subject") == "appearance" {
        "Appearance assignments weren't carried over"
    } else if message.starts_with("history stopped") {
        "Feature history stopped before the final shape"
    } else if message.contains("--import-only") {
        "Feature history was turned off"
    } else if message.starts_with("feature history left out:") {
        "Feature history left out to fit available memory"
    } else if message.contains("parts placed") {
        "Assembly part placements"
    } else if message.contains("sketch curve") {
        "Sketch curves kept as construction geometry"
    } else if message.contains("seam edges") {
        "Surface seam edges added"
    } else if message.contains("constraints not carried") {
        "Sketch constraints weren't carried over"
    } else if message.contains("feature(s) grouped") {
        "Features grouped by operation"
    } else if message.contains("parts rest on a stored pose") {
        "Part placements need checking"
    } else if message.contains("empty component") {
        "Empty components kept without bodies"
    } else if message.contains("sketch") && message.contains("for reference") {
        "Sketches kept for reference"
    } else if message.contains("grounded, inferred") {
        "Fixed assembly parts inferred"
    } else if message.starts_with("renamed to ") {
        "Parameter names changed to avoid duplicates"
    } else if message.contains("component of another design") {
        "Joints between separate designs couldn't be rebuilt"
    } else if message.contains("whole copy of the component") {
        "Joints applied to repeated component copies"
    } else if message.contains("placed by other joints") {
        "Conflicting joints kept suppressed"
    } else if message.contains("suppressed in the source") {
        "Suppressed features left out"
    } else if message.contains("helper not converted") {
        "Unused construction helpers left out"
    } else if message.contains("user-hidden") {
        "Hidden source bodies kept hidden"
    } else if message.contains("repeated component(s) written once") {
        "Repeated components share one definition"
    } else if message.contains("faces turned over") {
        "Face orientations corrected"
    } else if message.contains("edges lie off their faces") {
        "Face and edge tolerances changed"
    } else if message.contains("changes no geometry") {
        "Features without shape changes left out"
    } else if message.contains("joint(s) written as built") {
        "Joints kept at stored part positions"
    } else if message.contains("editable-history component placement") {
        "Editable history placements kept hidden"
    } else if message.starts_with("not checked:") && message.contains("timed out") {
        "Shape check timed out"
    } else if message.contains("original geometry") {
        "Original geometry kept beside editable history"
    } else if text(loss, "subject") == "history" {
        "Feature history mapping"
    } else if text(loss, "subject").starts_with("feature") {
        "Feature construction rules couldn't be rebuilt"
    } else {
        "Conversion observations"
    }
}

fn loss_heading(code: &str, notes: &[&Value]) -> String {
    if code.ends_with("native.source-record-omitted") {
        // The existing message supplies a count, without reading native file data.
        let records = notes
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
            let noun = if records == 1 { "record" } else { "records" };
            return format!("{records} internal {noun} not needed for the shape");
        }
    }
    let title = if code.is_empty() {
        notes
            .first()
            .map_or("Conversion observations", |note| observation_title(note))
    } else {
        loss_title(code)
    };
    if notes.len() == 1 {
        title.to_owned()
    } else {
        format!("{title} · {} reports", notes.len())
    }
}

fn loss_cards(ui: &mut egui::Ui, p: &Palette, losses: &[&Value]) {
    for (rank, label) in SEVERITIES.iter().enumerate() {
        let mut groups: std::collections::BTreeMap<(&str, &str), Vec<&Value>> = Default::default();
        for loss in losses.iter().filter(|loss| severity(loss) == rank) {
            let code = text(loss, "code");
            let title = if code.is_empty() {
                observation_title(loss)
            } else {
                loss_title(code)
            };
            groups.entry((code, title)).or_default().push(loss);
        }
        if groups.is_empty() {
            continue;
        }
        ui.label(RichText::new(*label).strong().color(colour(p, rank)));
        for ((code, group_title), notes) in groups {
            ui.push_id((rank, code, group_title), |ui| {
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0_f32, p.hairline))
                    .inner_margin(8)
                    .corner_radius(4)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(loss_heading(code, &notes)).color(p.ink));
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

fn part_counts(part: &Value) -> (u64, u64) {
    (
        part.get("rebuilt")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| number(part, "mapped")),
        part.get("needing")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| number(part, "total")),
    )
}

fn part_rank(part: Option<&Value>) -> (u8, u64, u64) {
    let Some(part) = part else { return (0, 0, 1) };
    let (rebuilt, total) = part_counts(part);
    if rebuilt < total {
        (0, rebuilt, total)
    } else if !checked(part) {
        (1, 0, 1)
    } else {
        (2, 0, 1)
    }
}

fn part_order(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    let (rank_a, rebuilt_a, total_a) = part_rank(a);
    let (rank_b, rebuilt_b, total_b) = part_rank(b);
    rank_a.cmp(&rank_b).then_with(|| {
        (u128::from(rebuilt_a) * u128::from(total_b))
            .cmp(&(u128::from(rebuilt_b) * u128::from(total_a)))
    })
}

fn part_heading(name: &str, part: Option<&Value>, report: &Value) -> (String, usize) {
    let Some(part) = part else {
        return (format!("— {name} · no history recorded"), 0);
    };
    let (rebuilt, total) = part_counts(part);
    if history_off(report) {
        return (format!("— {name} · History off"), 3);
    }
    let reason = text(part, "reason");
    let stop = reason
        .split_once("feature \"")
        .and_then(|(_, tail)| tail.split_once('"'))
        .map(|(name, _)| name)
        .or_else(|| {
            reason
                .strip_prefix("history stopped at ")
                .and_then(|tail| tail.split_once(';'))
                .map(|(name, _)| name)
        });
    let (icon, short, rank) = if rebuilt < total {
        (
            "!",
            stop.map_or_else(
                || "history incomplete".to_owned(),
                |name| format!("stops at {name}"),
            ),
            0,
        )
    } else if checked(part) {
        ("✓", "checked".to_owned(), 2)
    } else {
        ("!", "unchecked".to_owned(), 1)
    };
    (
        format!("{icon} {name} · {rebuilt}/{total} rebuilt · {short}"),
        rank,
    )
}

/// Keep the full verification explanation in the desktop's saved log.
pub(crate) fn full_log(report: &Value) -> String {
    let mut log = dutchman::json::full_log(report, &dutchman::batch::line(report));
    if let Some(reason) = report
        .pointer("/verification/reason")
        .and_then(Value::as_str)
    {
        if !reason.is_empty() && !log.contains(reason) {
            log.push_str("\nVerification: ");
            log.push_str(reason);
            log.push('\n');
        }
    }
    log
}

// Older reports provide display names for history and filenames for losses.
// Join only one-to-one matches, accepting a filename's stem for external parts.
// Duplicate stems stay unassigned rather than acquiring another part's status.
fn component_parts(
    parts: &[Value],
    components: &[dutchman::json::Component<'_>],
) -> Vec<Option<usize>> {
    let mut owners: Vec<Option<usize>> = components
        .iter()
        .map(|component| {
            let file = component.file.as_deref()?;
            let basename = file.rsplit(['/', '\\']).next().unwrap_or(file);
            let stem = basename.rsplit_once('.').map_or(basename, |(stem, _)| stem);
            let candidates: Vec<_> = parts
                .iter()
                .enumerate()
                .filter(|(_, part)| {
                    let name = text(part, "name");
                    name.eq_ignore_ascii_case(file) || name.eq_ignore_ascii_case(stem)
                })
                .map(|(index, _)| index)
                .collect();
            if candidates.len() == 1 {
                candidates.first().copied()
            } else {
                None
            }
        })
        .collect();
    let mut counts = std::collections::BTreeMap::new();
    for index in owners.iter().flatten() {
        *counts.entry(*index).or_insert(0_usize) += 1;
    }
    for owner in &mut owners {
        if owner.is_some_and(|index| counts.get(&index).is_some_and(|count| *count > 1)) {
            *owner = None;
        }
    }
    owners
}

pub(crate) fn details(ui: &mut egui::Ui, p: &Palette, report: &Value) {
    let parts = report.pointer("/history/parts").and_then(Value::as_array);
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
    let order_id = ui.make_persistent_id("source-part-order");
    let mut source_order = ui.data(|d| d.get_temp::<bool>(order_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Parts").strong());
        if ui
            .toggle_value(&mut source_order, RichText::new("Source order").small())
            .changed()
        {
            ui.data_mut(|d| d.insert_temp(order_id, source_order));
        }
    });
    let owners = component_parts(parts.map_or(&[], Vec::as_slice), &components);
    let mut names: Vec<(usize, String)> = parts
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, p)| (index, text(p, "name").to_owned()))
        .collect();
    if !source_order {
        names.sort_by(|(a, _), (b, _)| {
            part_order(parts.and_then(|p| p.get(*a)), parts.and_then(|p| p.get(*b)))
        });
    }
    let id = ui.make_persistent_id("show-all-parts");
    let all = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    for (position, (index, name)) in
        names
            .iter()
            .enumerate()
            .take(if all { usize::MAX } else { 10 })
    {
        let part = parts.and_then(|parts| parts.get(*index));
        let (label, rank) = part_heading(name, part, report);
        let width = (ui.available_width() - ui.spacing().indent - 8.0).max(40.0);
        let font = egui::TextStyle::Button.resolve(ui.style());
        let heading = ui.painter().layout(
            label,
            font,
            if rank == 2 {
                p.spectre
            } else {
                colour(p, rank)
            },
            width,
        );
        egui::CollapsingHeader::new(egui::WidgetText::Galley(heading))
            .id_salt(("part", index))
            .default_open(position == 0)
            .show(ui, |ui| {
                // Part names can repeat; the source report's row identifies the part.
                if let Some(part) = part {
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
                for (_, component) in
                    components
                        .iter()
                        .enumerate()
                        .filter(|(component_index, _)| {
                            owners.get(*component_index) == Some(&Some(*index))
                        })
                {
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
    for (index, component) in components
        .iter()
        .enumerate()
        .filter(|(index, _)| owners.get(*index) == Some(&None))
    {
        ui.push_id(("unassigned-losses", index), |ui| {
            ui.label(
                RichText::new(component.file.as_ref().map_or_else(
                    || "Design loss details".to_owned(),
                    |file| format!("Other part loss details: {file}"),
                ))
                .strong(),
            );
            loss_cards(ui, p, &component.losses);
        });
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
        assert!(inventory.lines().any(|code| code == "record.unreadable"));
        assert_eq!(
            known_title("decode/record.unreadable"),
            Some("Part of the source could not be read")
        );
        assert_eq!(
            loss_title("future/new-code"),
            "Conversion data wasn't carried over"
        );
        assert_eq!(
            loss_title("f3d/material.not-transferred"),
            "Materials weren't carried over"
        );
    }
    #[test]
    fn known_codes_never_use_a_generic_title() {
        let fallback = loss_title("future/new-code");
        for code in include_str!("../tests/support/loss-codes.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            let title = loss_title(code);
            assert_ne!(title, fallback, "generic fallback for {code}");
            assert!(
                !title.contains("details"),
                "vague title for {code}: {title}"
            );
        }
        for (code, title) in LOSS_TITLES {
            assert_ne!(*title, fallback, "generic fallback for {code}");
            assert!(
                !title.contains("details"),
                "vague title for {code}: {title}"
            );
        }
    }

    #[test]
    fn uncoded_observations_name_the_affected_data() {
        for (message, expected) in [
            (
                "1 of 2 constraints not carried; geometry is exact",
                "Sketch constraints weren't carried over",
            ),
            (
                "kept 2 sketches for reference",
                "Sketches kept for reference",
            ),
            (
                "renamed to width_2: another parameter already has that name",
                "Parameter names changed to avoid duplicates",
            ),
            (
                "placed as an empty component: it has no bodies",
                "Empty components kept without bodies",
            ),
            (
                "1 parts rest on a stored pose that no joint confirms",
                "Part placements need checking",
            ),
            (
                "solid #1: 2 faces turned over",
                "Face orientations corrected",
            ),
            (
                "suppressed in the source; left out",
                "Suppressed features left out",
            ),
            (
                "changes no geometry in the source; left out",
                "Features without shape changes left out",
            ),
        ] {
            let note = serde_json::json!({"message": message});
            assert_eq!(loss_heading("", &[&note]), expected);
        }
    }

    #[test]
    fn loss_counts_name_the_item_and_pluralize_without_counting_reports_as_faces() {
        let one = serde_json::json!({"message":"1 source-native record was omitted"});
        let two = serde_json::json!({"message":"2 source-native records were omitted"});
        assert_eq!(
            loss_heading("step/native.source-record-omitted", &[&one]),
            "1 internal record not needed for the shape"
        );
        assert_eq!(
            loss_heading("step/native.source-record-omitted", &[&two]),
            "2 internal records not needed for the shape"
        );
        assert_eq!(
            loss_heading("f3d/material.not-transferred", &[&one]),
            "Materials weren't carried over"
        );
        assert_eq!(
            loss_heading("f3d/material.not-transferred", &[&one, &two]),
            "Materials weren't carried over · 2 reports"
        );
        assert_eq!(
            loss_heading("f3d/appearance.face-color-unresolved", &[&one, &two]),
            "Face colours couldn't be resolved · 2 reports"
        );
        let thread = serde_json::json!({"message":"its source does not resolve whether the thread is modeled; kept as a cosmetic annotation"});
        assert_eq!(loss_heading("", &[&thread]), "Thread cosmetic data");
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
    fn saved_log_keeps_full_verification_reason() {
        let report = sample();
        let reason = report
            .pointer("/verification/reason")
            .and_then(Value::as_str)
            .unwrap();
        assert!(full_log(&report).contains(reason));
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
        h.get_by_label("784 internal records not needed for the shape");
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
                "History off",
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
    fn history_off_replaces_empty_counts_and_zero_loss_chips_are_hidden() {
        let report = json!({"mode":"import", "verification":{"status":"not_applicable", "reason":"--import-only"},
            "history":{"rebuilt":0,"needing":0},"losses":[]});
        let mut h = Harness::builder().build_ui(|ui| {
            let p = *theme::palette(ui);
            summary(ui, &p, &report);
        });
        h.run_steps(6);
        h.get_by_label("History off");
        for hidden in [
            "0 of 0 features rebuilt",
            "Lost: 0",
            "Changed: 0",
            "Note: 0",
        ] {
            assert!(h.query_by_label(hidden).is_none(), "{hidden}");
        }
    }

    #[test]
    fn parts_sort_worst_first_and_source_toggle_preserves_row_identity() {
        let report = json!({"history":{"parts":[
            {"name":"Complete", "rebuilt":3,"needing":3,"status":"verified","reason":"complete reason"},
            {"name":"Partial", "rebuilt":2,"needing":3,"status":"partial","reason":"history stopped at feature \"Chamfer 2\": edge not resolved"},
            {"name":"Blocked", "rebuilt":0,"needing":3,"status":"flattened","reason":"feature \"Sketch 1\": sketch not resolved"},
            {"name":"Unchecked", "rebuilt":3,"needing":3,"status":"unverified"}]}});
        let mut h = Harness::builder()
            .with_size(egui::vec2(760.0, 1600.0))
            .build_ui(|ui| {
                let p = *theme::palette(ui);
                details(ui, &p, &report);
            });
        h.run_steps(6);
        let labels = [
            "! Blocked · 0/3 rebuilt · stops at Sketch 1",
            "! Partial · 2/3 rebuilt · stops at Chamfer 2",
            "! Unchecked · 3/3 rebuilt · unchecked",
            "✓ Complete · 3/3 rebuilt · checked",
        ];
        let rows: Vec<_> = labels
            .iter()
            .map(|label| h.get_by_label(label).rect())
            .collect();
        assert!(rows.windows(2).all(|r| r[0].top() < r[1].top()));
        h.get_by_label("feature \"Sketch 1\": sketch not resolved");
        h.get_by_label(labels[1]).click_accesskit();
        h.run_steps(6);
        h.get_by_label("history stopped at feature \"Chamfer 2\": edge not resolved");
        h.get_by_label("Source order").click_accesskit();
        h.run_steps(6);
        let complete = h.get_by_label(labels[3]).rect();
        let partial = h.get_by_label(labels[1]).rect();
        let blocked = h.get_by_label(labels[0]).rect();
        assert!(complete.top() < partial.top() && partial.top() < blocked.top());
        // The disclosure retains the source part's own count and reason after sorting.
        h.get_by_label("2 of 3 features rebuilt");
        assert!(h.query_by_label("complete reason").is_none());
    }

    #[test]
    fn external_part_filenames_join_history_without_doubling_the_group_cap() {
        let report = json!({"history":{"parts":(1..=10).map(|n| json!({
            "name":format!("Part {n}"),"rebuilt":1,"needing":1,"status":"verified"
        })).collect::<Vec<_>>()}, "losses":(1..=10).map(|n| json!({
            "subject":format!("Part {n}.ipt: appearance"),"level":"Degraded",
            "code":"dutchman/appearance.material-not-transferred","message":"material omitted"
        })).collect::<Vec<_>>()});
        let parts = report["history"]["parts"].as_array().unwrap();
        let components = dutchman::json::components(&report);
        let owners = component_parts(parts, &components);
        for (component, owner) in components.iter().zip(&owners) {
            let part = &parts[owner.unwrap()];
            assert_eq!(
                component.file.as_ref().unwrap(),
                &format!("{}.ipt", text(part, "name"))
            );
        }
        let mut h = Harness::builder()
            .with_size(egui::vec2(760.0, 2000.0))
            .build_ui(|ui| {
                let p = *theme::palette(ui);
                details(ui, &p, &report);
            });
        h.run_steps(6);
        h.get_by_label("Materials weren't carried over");
        h.get_by_label("✓ Part 10 · 1/1 rebuilt · checked");
        assert!(h.query_by_label_contains("no history recorded").is_none());
        assert!(h.query_by_label_contains("Show all").is_none());
        assert!(h
            .query_by_label_contains("Other part loss details")
            .is_none());
    }
    #[test]
    fn ambiguous_names_and_filenames_never_assign_another_parts_losses() {
        for (names, files, expected) in [
            (vec!["Pin", "Pin"], vec!["Pin.ipt"], vec![None]),
            (vec!["Pin"], vec!["Pin.ipt", "Pin.sldprt"], vec![None, None]),
            (vec!["Pin", "Pin.ipt"], vec!["Pin.ipt"], vec![None]),
            (
                vec!["Pin.ipt", "Pin.sldprt"],
                vec!["Pin.ipt", "Pin.sldprt"],
                vec![Some(0), Some(1)],
            ),
            (vec!["Pin"], vec!["folder\\Pin.ipt"], vec![Some(0)]),
            (vec!["Pin"], vec!["folder/pin.ipt"], vec![Some(0)]),
        ] {
            let report = json!({"history":{"parts":names.iter().map(|name| json!({"name":name})).collect::<Vec<_>>()},
                "losses":files.iter().map(|file| json!({"subject":format!("{file}: appearance"),"level":"Degraded","message":"colour omitted"})).collect::<Vec<_>>()});
            let parts = report["history"]["parts"].as_array().unwrap();
            assert_eq!(
                component_parts(parts, &dutchman::json::components(&report)),
                expected
            );
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
        h.get_by_label("! Part 10 · 1/2 rebuilt · history incomplete");
        assert!(h
            .query_by_label("! Part 11 · 1/2 rebuilt · history incomplete")
            .is_none());
        h.get_by_label("Show all 14").click_accesskit();
        h.run_steps(6);
        h.get_by_label("! Part 14 · 1/2 rebuilt · history incomplete");
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
