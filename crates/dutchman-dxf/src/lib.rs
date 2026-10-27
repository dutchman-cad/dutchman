//! DXF pages from the neutral drawing arena. Native formats are decoded by cadmpeg.

pub mod entities;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::drawings::{Drawing, DrawingKind};
use cadmpeg_ir::presentation::ViewPresentation;
use dutchman_core::{Error, Model, Report, Severity, Stage};
use entities::{Document, Entity};

/// One encoded drawing page and its own mapping report.
pub struct Page {
    pub name: String,
    pub bytes: Vec<u8>,
    pub report: Report,
    pub entities_per_layer: BTreeMap<String, usize>,
    pub dimension_representation: &'static str,
}

/// Filename-safe source object name; suffixes are assigned by the caller on collisions.
pub fn safe_name(name: &str) -> String {
    let result: String = name
        .chars()
        .take(100)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if result.is_empty() {
        "Page".into()
    } else {
        result
    }
}

fn name(drawing: &Drawing) -> &str {
    drawing.object.rsplit('#').next().unwrap_or(&drawing.object)
}

/// Map selected pages. An absent selection exports all pages in source order.
pub fn pages(
    model: &Model,
    selected: Option<&str>,
    report: &mut Report,
) -> Result<Vec<Page>, Error> {
    let drawings = &model.ir.model.drawings;
    if drawings.len() > 100_000
        || model.ir.model.semantic_annotations.len() > 100_000
        || model.ir.model.view_presentations.len() > 100_000
        || drawings
            .iter()
            .flat_map(|d| d.relationships.values())
            .map(Vec::len)
            .sum::<usize>()
            > 100_000
    {
        return Err(Error::Encode("drawing exceeds 100,000 objects".into()));
    }
    let mut source: Vec<_> = drawings
        .iter()
        .filter(|d| d.kind == DrawingKind::Page)
        .collect();
    source.sort_by_key(|d| d.order);
    if source.len() > 1000 {
        return Err(Error::Encode("drawing exceeds 1000 pages".into()));
    }
    if source.is_empty() {
        return Err(Error::Encode("this file has no TechDraw page; add a page in FreeCAD and save it before converting to DXF".into()));
    }
    if let Some(selected) = selected {
        source.retain(|d| name(d) == selected);
    }
    if source.is_empty() {
        return Err(Error::Encode(format!(
            "no TechDraw page named {}; choose a page's object name in FreeCAD",
            selected.unwrap_or_default()
        )));
    }
    let by_id: BTreeMap<_, _> = drawings.iter().map(|d| (d.id.as_str(), d)).collect();
    let annotations: BTreeMap<_, _> = model
        .ir
        .model
        .semantic_annotations
        .iter()
        .map(|a| (a.object.as_str(), a))
        .collect();
    let mut presentations: BTreeMap<&str, Vec<&ViewPresentation>> = BTreeMap::new();
    for presentation in &model.ir.model.view_presentations {
        if let Some(object) = presentation.object.as_deref() {
            presentations.entry(object).or_default().push(presentation);
        }
    }
    let mut reached = BTreeSet::new();
    let mut result = Vec::new();
    let mut total_bytes = 0_usize;
    let mut mapped_objects = 0_usize;
    let mut text_lines = 0_usize;
    for page in source {
        let mut document = Document::default();
        document
            .layers
            .extend(["DIMENSIONS", "ANNOTATIONS", "TITLE_BLOCK", "FRAME"].map(str::to_owned));
        let mut notes = Report::default();
        let mut saw_dimensions = false;
        notes.push(Severity::Info, name(page), "DXF R2000 in millimetres; one file per page; page selection uses the source object name");
        notes.push_at(Stage::Project, Severity::Dropped, name(page), "page size and frame are not supported yet: Drawing has no typed sheet size or border geometry");
        let mut queue = vec![page.id.as_str()];
        if let Some(template) = &page.template {
            queue.push(template.as_str());
        }
        let mut visited = BTreeSet::new();
        while let Some(id) = queue.pop() {
            if !visited.insert(id) {
                continue;
            }
            mapped_objects = mapped_objects.saturating_add(1);
            if mapped_objects > 100_000 {
                return Err(Error::Encode(
                    "drawing exceeds 100,000 page object placements".into(),
                ));
            }
            reached.insert(id);
            let Some(drawing) = by_id.get(id).copied() else {
                notes.push(
                    Severity::Dropped,
                    name(page),
                    format!("drawing relationship {id} is not available in the IR"),
                );
                continue;
            };
            let providers = presentations
                .get(drawing.object.as_str())
                .map_or(&[][..], Vec::as_slice);
            let gui_visible = gui_visibility(providers);
            if providers.iter().any(|p| p.visible.is_some()) && gui_visible.is_none()
                || drawing
                    .visible
                    .zip(gui_visible)
                    .is_some_and(|(a, b)| a != b)
            {
                notes.push(Severity::Degraded, name(drawing), "conflicting visibility records: Drawing takes precedence, and an unresolved object is treated as shown");
            }
            if drawing.visible.is_none() && gui_visible.is_some() {
                notes.push(Severity::Info, name(drawing), "kept resolved drawing visibility from ViewPresentation; page visibility is UI state and does not filter selected pages");
            }
            if providers.iter().any(|p| {
                !p.properties.is_empty()
                    || p.line_width.is_some()
                    || p.point_size.is_some()
                    || p.display_mode.is_some()
                    || p.selection_style.is_some()
            }) {
                let properties: BTreeSet<_> = providers
                    .iter()
                    .flat_map(|p| p.properties.keys().map(|key| key.as_str()))
                    .collect();
                notes.push(Severity::Dropped, name(drawing), format!("GUI property metadata is not stored ({}); typed visibility is used when resolved, while display/selection modes and widths in display units are not mapped to DXF millimetres", properties.into_iter().collect::<Vec<_>>().join(", ")));
            }
            let visible = drawing.visible.or(gui_visible);
            for role in ["Views", "Group"] {
                if let Some(links) = drawing.relationships.get(role) {
                    for link in links.iter().rev() {
                        if let Some(target) = link.local_target() {
                            queue.push(target);
                        } else {
                            notes.push(
                                Severity::Dropped,
                                name(drawing),
                                format!("{role} relationship has no local drawing target"),
                            );
                        }
                    }
                }
            }
            if matches!(
                drawing.kind,
                DrawingKind::View
                    | DrawingKind::Projection
                    | DrawingKind::Section
                    | DrawingKind::Detail
            ) {
                document.layers.insert(format!(
                    "VIEW_{}_{}",
                    drawing.order,
                    safe_name(name(drawing))
                ));
            }
            if visible == Some(false) && drawing.kind != DrawingKind::Page {
                notes.push(
                    Severity::Info,
                    name(drawing),
                    "hidden drawing object left out",
                );
                continue;
            }
            match drawing.kind {
                DrawingKind::Page => {},
                DrawingKind::Template => notes.push_at(Stage::Project, Severity::Dropped, name(drawing), "title block and template frame are not supported yet: Drawing retains assets but has no typed template primitives or editable text placements"),
                DrawingKind::View | DrawingKind::Projection | DrawingKind::Section | DrawingKind::Detail => {
                    notes.push_at(Stage::Project, Severity::Dropped, name(drawing), "view geometry is not supported yet: Drawing has position, scale, direction and rotation but no cached 2D edges; no model projection is substituted");
                }
                DrawingKind::Annotation | DrawingKind::Balloon | DrawingKind::Leader | DrawingKind::Symbol => {
                    if drawing.kind != DrawingKind::Annotation { notes.push(Severity::Dropped, name(drawing), "callout, leader or symbol geometry is not typed in Drawing; only resolved text can be carried"); }
                    if let Some(annotation) = annotations.get(drawing.object.as_str()) {
                        if !annotation.parameters.is_empty() || !annotation.references.is_empty() || !annotation.assets.is_empty() || annotation.format.is_some() || annotation.value.is_some() {
                            notes.push(Severity::Dropped, name(drawing), "semantic annotation parameters, formatting expression, measurement, resource assets and associations are not carried; only resolved plain text and position are used");
                        }
                        let position = drawing.position.map(|p| p.get());
                        if let Some([x,y]) = position.filter(|_| !annotation.text.is_empty()) {
                            let rotation = drawing.rotation_degrees.map_or(0.0, |r| r.get());
                            let angle = rotation.to_radians();
                            let mut line_index = 0_u32;
                            let mut written_lines = 0_u32;
                            for fragment in &annotation.text {
                                for line in fragment.split('\n') {
                                    let line = line.strip_suffix('\r').unwrap_or(line);
                                    text_lines = text_lines.saturating_add(1);
                                    if text_lines > 100_000 { return Err(Error::Encode("drawing exceeds 100,000 text lines".into())); }
                                    if line.chars().take(4097).count() > 4096 { return Err(Error::Encode("DXF text needs lines of at most 4096 characters".into())); }
                                    if !line.is_empty() {
                                        let offset = f64::from(line_index) * 3.5;
                                        document.entities.push(("ANNOTATIONS".into(), Entity::Text { position: [x + offset * angle.sin(), y - offset * angle.cos()], height: 2.5, rotation, text: line.to_owned() }));
                                        written_lines = written_lines.saturating_add(1);
                                    }
                                    line_index = line_index.saturating_add(1);
                                }
                            }
                            notes.push(Severity::Info, name(drawing), format!("kept {written_lines} text entities across {line_index} lines at the page position on ANNOTATIONS"));
                            notes.push(Severity::Degraded, name(drawing), "text uses a 2.5 mm default font height and 3.5 mm line spacing; font, alignment, rich formatting and styling are not mapped yet");
                        } else { notes.push(Severity::Dropped, name(drawing), "annotation has no resolved text or page position in SemanticAnnotation / Drawing"); }
                    } else { notes.push(Severity::Dropped, name(drawing), "annotation has no SemanticAnnotation text record"); }
                }
                DrawingKind::Dimension => {
                    saw_dimensions = true;
                    notes.push_at(Stage::Project, Severity::Dropped, name(drawing), "dimension geometry and value are not supported yet: SemanticAnnotation.value is absent for FreeCAD dimensions and References2D has names without resolved page anchors; FormatSpec is not a measured value; neither DIMENSION nor lines plus text are written");
                },
                _ => notes.push(Severity::Dropped, name(drawing), format!("{} drawing geometry is not supported yet", drawing.runtime_type)),
            }
            if !drawing.parameters.is_empty()
                || !drawing.relationships.is_empty()
                || !drawing.assets.is_empty()
            {
                notes.push(Severity::Dropped, name(drawing), format!("remaining source metadata, associations and resources are not carried in DXF (parameters: {}; relationship roles: {}; assets: {}); typed placement is used only for written text, Views/Group and template links only for page membership", drawing.parameters.keys().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), drawing.relationships.keys().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), drawing.assets.join(", ")));
            }
        }
        let mut counts: BTreeMap<_, _> = document.layers.iter().map(|l| (l.clone(), 0)).collect();
        counts.entry("0".to_owned()).or_default();
        for (layer, _) in &document.entities {
            *counts.entry(layer.clone()).or_default() += 1;
        }
        let bytes = document.encode()?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > 64 * 1024 * 1024 {
            return Err(Error::Encode("drawing DXF outputs exceed 64 MiB".into()));
        }
        result.push(Page {
            name: name(page).to_owned(),
            bytes,
            report: notes,
            entities_per_layer: counts,
            dimension_representation: if saw_dimensions {
                "omitted_missing_ir"
            } else {
                "none"
            },
        });
    }
    if selected.is_none() {
        for drawing in drawings.iter().filter(|d| !reached.contains(d.id.as_str())) {
            report.push(
                Severity::Dropped,
                name(drawing),
                "drawing object has no resolved membership in an exported page",
            );
        }
        let reached_objects: BTreeSet<_> = drawings
            .iter()
            .filter(|d| reached.contains(d.id.as_str()))
            .map(|d| d.object.as_str())
            .collect();
        for annotation in &model.ir.model.semantic_annotations {
            if !reached_objects.contains(annotation.object.as_str()) {
                report.push(
                    Severity::Dropped,
                    &annotation.object,
                    "semantic annotation has no resolved membership in an exported drawing page",
                );
            }
        }
    }
    report.push(Severity::Info, "drawing output", "DXF carries pages only; 3D model geometry, feature history, sketches, assembly structure and non-drawing data are outside drawing output");
    Ok(result)
}

fn gui_visibility(providers: &[&ViewPresentation]) -> Option<bool> {
    let mut values = providers.iter().filter_map(|provider| provider.visible);
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}
