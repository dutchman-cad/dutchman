//! Drawing files: identifying them by content, and the neutral plan of the sheets and views
//! that can be read, which a writer turns into a SolidCraft drawing.
//!
//! Every drawing file gets a report. FreeCAD TechDraw pages are read into a [`Plan`];
//! the other drawing formats are identified here and reported as not decoded yet.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use cadmpeg_ir::drawings::{Drawing, DrawingKind};

use crate::{Format, Model, Report, Severity, Stage};

/// The schema of the `drawing` section of a report.
pub const SCHEMA: &str = "dutchman.drawing/1";

/// Reason codes of the `drawing` section, parallel to the model pipeline's loss codes.
pub mod code {
    /// The format is accepted and identified, but its contents are not decoded yet.
    pub const FORMAT_NOT_DECODED: &str = "drawing.format.not_decoded";
    /// The file's contents do not look like the format its suffix names.
    pub const FORMAT_CONTENT_MISMATCH: &str = "drawing.format.content_mismatch";
    /// A sheet's contents could not be read.
    pub const SHEET_UNREADABLE: &str = "drawing.sheet.unreadable";
    /// A view's model is not among the converted files.
    pub const VIEW_MODEL_MISSING: &str = "drawing.view.model_missing";
    /// The file could not be read to identify it.
    pub const FORMAT_UNREADABLE: &str = "drawing.format.unreadable";
    /// The file has no drawing sheet.
    pub const SHEET_NONE: &str = "drawing.sheet.none";
    /// A sheet's paper size is not in the file; A4 landscape was used.
    pub const SHEET_SIZE_ASSUMED: &str = "drawing.sheet.size_assumed";
    /// A sheet could not be written.
    pub const SHEET_NOT_WRITTEN: &str = "drawing.sheet.not_written";
    /// A view kind SolidCraft's drawing commands cannot take from this file.
    pub const VIEW_UNSUPPORTED_KIND: &str = "drawing.view.unsupported_kind";
    /// A view has no projection direction.
    pub const VIEW_NO_DIRECTION: &str = "drawing.view.no_direction";
    /// A view's scale is outside what SolidCraft takes; it was fitted to the sheet.
    pub const VIEW_SCALE_FITTED: &str = "drawing.view.scale_fitted";
    /// A view's rotation is not carried.
    pub const VIEW_ROTATION_DROPPED: &str = "drawing.view.rotation_dropped";
    /// The up direction of a view was not in the file; it was derived.
    pub const VIEW_UP_ASSUMED: &str = "drawing.view.up_assumed";
    /// A view could not be written.
    pub const VIEW_NOT_WRITTEN: &str = "drawing.view.not_written";
    /// A view draws the whole converted model, not only the objects the source named.
    pub const VIEW_SOURCE_WHOLE_MODEL: &str = "drawing.view.source_whole_model";
    /// Dimensions are not written yet.
    pub const DIMENSION_UNSUPPORTED: &str = "drawing.dimension.unsupported";
    /// Notes, balloons, leaders, symbols and images are not written yet.
    pub const ANNOTATION_UNSUPPORTED: &str = "drawing.annotation.unsupported";
    /// A page template (border and title block) is not carried; SolidCraft's own is used.
    pub const TEMPLATE_NOT_CARRIED: &str = "drawing.template.not_carried";
    /// No drawing could be written: SolidCraft's command line is not available.
    pub const WRITE_UNAVAILABLE: &str = "drawing.write.unavailable";
    /// SolidCraft refused the drawing commands.
    pub const WRITE_REFUSED: &str = "drawing.write.refused";
    /// The converted model is not a SolidCraft design, so no drawing can be added to it.
    pub const WRITE_NO_DESIGN: &str = "drawing.write.no_design";
    /// More objects than Dutchman reads from one drawing.
    pub const LIMIT: &str = "drawing.limit";
}

/// The most sheets, views and drawing objects read from one file.
const MAX_OBJECTS: usize = 100_000;
const MAX_SHEETS: usize = 1000;
const MAX_VIEWS: usize = 10_000;

/// What the contents of a file say it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sniff {
    /// The container or text family the bytes are: `compound_document`, `zip`, `dwg`, `dxf`,
    /// `text`, or `unknown`.
    pub family: &'static str,
    /// The version the file names itself, when it does (`AC1032` for a DWG).
    pub version: Option<String>,
    /// A plain description of the version, when known.
    pub version_name: Option<&'static str>,
    /// Whether the contents are what the format's suffix names.
    pub matches_suffix: bool,
    /// Whether a compound document holds Solid Edge's draft stream.
    pub solid_edge_draft: bool,
    /// Bytes read.
    pub size: u64,
}

const HEAD: usize = 1 << 20;

/// Identifies `format`'s file at `path` from its first bytes.
pub fn sniff(format: Format, path: &Path) -> Result<Sniff, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = Vec::new();
    file.take(HEAD as u64).read_to_end(&mut head)?;
    Ok(sniff_bytes(format, &head, size))
}

/// [`sniff`] on the first bytes of a file of `size` bytes.
pub fn sniff_bytes(format: Format, head: &[u8], size: u64) -> Sniff {
    let starts = |magic: &[u8]| head.starts_with(magic);
    let mut sniff = Sniff {
        family: "unknown",
        version: None,
        version_name: None,
        matches_suffix: false,
        solid_edge_draft: false,
        size,
    };
    if starts(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        sniff.family = "compound_document";
        sniff.solid_edge_draft = utf16_contains(head, "Draft")
            && utf16_contains(head, "Solid Edge")
            || utf16_contains(head, "SolidEdge");
        sniff.matches_suffix = match format {
            Format::SolidEdgeDrawing => sniff.solid_edge_draft,
            _ => matches!(
                format,
                Format::InventorDrawing | Format::SolidWorksDrawing | Format::DwgDrawing
            ),
        };
    } else if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        sniff.family = "zip";
        sniff.matches_suffix = matches!(
            format,
            Format::FusionDrawing | Format::SolidWorksDrawing | Format::InventorDrawing
        );
    } else if starts(b"AC10") && head.len() >= 6 {
        let version = String::from_utf8_lossy(head.get(..6).unwrap_or_default()).into_owned();
        sniff.version_name = dwg_version(&version);
        sniff.version = Some(version);
        sniff.family = "dwg";
        sniff.matches_suffix = format == Format::DwgDrawing;
    } else if starts(b"AutoCAD Binary DXF") || looks_like_dxf(head) {
        sniff.family = "dxf";
        sniff.matches_suffix = format == Format::DxfDrawing;
    } else if head.first().is_some_and(|b| *b == b'{' || *b == b'<')
        || std::str::from_utf8(head).is_ok_and(|text| text.trim_start().starts_with(['{', '<']))
    {
        sniff.family = "text";
        sniff.matches_suffix = format == Format::FusionDrawing;
    }
    sniff
}

fn utf16_contains(bytes: &[u8], needle: &str) -> bool {
    let wide: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    !wide.is_empty()
        && bytes
            .windows(wide.len())
            .any(|window| window == wide.as_slice())
}

fn looks_like_dxf(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    lines.next() == Some("0") && lines.next() == Some("SECTION")
}

fn dwg_version(version: &str) -> Option<&'static str> {
    Some(match version {
        "AC1006" => "AutoCAD R10",
        "AC1009" => "AutoCAD R11/R12",
        "AC1012" => "AutoCAD R13",
        "AC1014" => "AutoCAD R14",
        "AC1015" => "AutoCAD 2000",
        "AC1018" => "AutoCAD 2004",
        "AC1021" => "AutoCAD 2007",
        "AC1024" => "AutoCAD 2010",
        "AC1027" => "AutoCAD 2013",
        "AC1032" => "AutoCAD 2018",
        _ => return None,
    })
}

/// What to tell a user whose drawing is recognized but whose sheets are not read yet. The
/// result card shows it under the line that says the sheets are not supported yet, and above
/// the help steps that ask for a PDF or DXF export, so it says only what was not done.
pub fn not_decoded_message(format: Format) -> String {
    format!(
        "{}: no drawing was written. Please send the file so the reader can be built.",
        format.name()
    )
}

/// One view to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewPlan {
    /// The view's name in the source.
    pub name: String,
    /// Where its centre goes, in paper millimetres from the sheet's lower-left corner.
    pub position_mm: [f64; 2],
    /// The scale as paper:model text, when SolidCraft takes it.
    pub scale: Option<String>,
    /// The line of sight, from the viewer into the model.
    pub direction: [f64; 3],
    /// The way up the paper.
    pub up: [f64; 3],
}

/// One sheet to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetPlan {
    /// The sheet's name in the source.
    pub name: String,
    /// The paper, width and height in millimetres, when the source says.
    pub size_mm: Option<[f64; 2]>,
    /// The views kept, in source order.
    pub views: Vec<ViewPlan>,
}

/// What a file's drawing holds that can be written, and counts of what was read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The sheets, in source order.
    pub sheets: Vec<SheetPlan>,
    /// Pages read, kept or not.
    pub sheets_read: usize,
    /// Views read, kept or not.
    pub views_read: usize,
    /// Views read by kind: base, projected, section, detail, other.
    pub by_kind: [usize; 5],
    /// Dimensions read.
    pub dimensions_read: usize,
    /// Notes, balloons, leaders, symbols and images read.
    pub annotations_read: usize,
}

fn name(drawing: &Drawing) -> &str {
    drawing.object.rsplit('#').next().unwrap_or(&drawing.object)
}

fn drawing_target<'a>(
    link: &cadmpeg_ir::ReferenceSelection,
    objects: &BTreeMap<&str, Option<&'a str>>,
) -> Option<&'a str> {
    if !matches!(link.target, cadmpeg_ir::ReferenceTarget::Unresolved)
        || link.native.as_ref()?.codec.as_str() != "fcstd"
    {
        return None;
    }
    objects.get(link.native_text()?).copied().flatten()
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 1e-12).then(|| [a[0] / length, a[1] / length, a[2] / length])
}

/// The scale as `paper:model` text within SolidCraft's 1:100000 to 1000:1.
pub fn scale_text(scale: f64) -> Option<String> {
    if !scale.is_finite() || !(1e-5..=1000.0).contains(&scale) {
        return None;
    }
    let trimmed = |value: f64| {
        let text = format!("{value:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    Some(if scale >= 1.0 {
        format!("{}:1", trimmed(scale))
    } else {
        format!("1:{}", trimmed(1.0 / scale))
    })
}

/// Reads the pages of a FreeCAD TechDraw drawing into a plan of sheets and views, recording
/// everything it leaves out in `report` under the codes in [`code`].
pub fn plan(model: &Model, report: &mut Report) -> Plan {
    let drawings = &model.ir.model.drawings;
    let mut plan = Plan::default();
    if drawings.len() > MAX_OBJECTS {
        report.push_coded(
            Severity::Dropped,
            "drawing",
            code::LIMIT,
            format!(
                "the file has {} drawing objects; Dutchman reads up to {MAX_OBJECTS}",
                drawings.len()
            ),
        );
        return plan;
    }
    let mut pages: Vec<_> = drawings
        .iter()
        .filter(|d| d.kind == DrawingKind::Page)
        .collect();
    pages.sort_by_key(|d| d.order);
    plan.sheets_read = pages.len();
    if pages.len() > MAX_SHEETS {
        report.push_coded(
            Severity::Dropped,
            "drawing",
            code::LIMIT,
            format!(
                "the file has {} pages; Dutchman reads the first {MAX_SHEETS}",
                pages.len()
            ),
        );
        pages.truncate(MAX_SHEETS);
    }
    let by_id: BTreeMap<_, _> = drawings.iter().map(|d| (d.id.as_str(), d)).collect();
    let mut objects = BTreeMap::new();
    for drawing in drawings {
        objects
            .entry(name(drawing))
            .and_modify(|target| *target = None)
            .or_insert(Some(drawing.id.as_str()));
    }
    let mut views_total = 0_usize;
    let mut source_noted = false;
    for page in pages {
        let sheet = page.sheet.as_ref().or_else(|| {
            page.template
                .as_ref()
                .and_then(|id| by_id.get(id.as_str()))
                .and_then(|template| template.sheet.as_ref())
        });
        let mut sheet_plan = SheetPlan {
            name: name(page).to_owned(),
            size_mm: sheet.map(|s| [s.width_mm.get(), s.height_mm.get()]),
            views: Vec::new(),
        };
        if sheet_plan.size_mm.is_none() {
            report.push_coded(
                Severity::Degraded,
                name(page),
                code::SHEET_SIZE_ASSUMED,
                "the page has no paper size in the file; A4 landscape was used",
            );
        }
        if page.template.is_some() {
            report.push_coded(
                Severity::Degraded,
                name(page),
                code::TEMPLATE_NOT_CARRIED,
                "the page's template (border and title block) is not carried; SolidCraft's own border and title block are used",
            );
        }
        let mut queue = vec![(page.id.as_str(), [0.0_f64, 0.0])];
        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        while let Some((id, offset)) = queue.pop() {
            if !visited.insert(id) || visited.len() > MAX_OBJECTS {
                continue;
            }
            let Some(drawing) = by_id.get(id).copied() else {
                continue;
            };
            order.push((drawing, offset));
            for role in ["Views", "Group"] {
                for link in drawing.relationships.get(role).into_iter().flatten().rev() {
                    if let Some(target) = drawing_target(link, &objects) {
                        let child = if drawing.kind == DrawingKind::Projection && role == "Views" {
                            let [x, y] = drawing.position.map_or([0.0, 0.0], |p| p.get());
                            [offset[0] + x, offset[1] + y]
                        } else {
                            offset
                        };
                        queue.push((target, child));
                    }
                }
            }
        }
        for (drawing, offset) in order {
            match drawing.kind {
                DrawingKind::Page | DrawingKind::Template => {}
                DrawingKind::Dimension => plan.dimensions_read += 1,
                DrawingKind::Annotation
                | DrawingKind::Balloon
                | DrawingKind::Symbol
                | DrawingKind::Image
                | DrawingKind::Leader => plan.annotations_read += 1,
                DrawingKind::Other => {}
                DrawingKind::Section | DrawingKind::Detail => {
                    plan.views_read += 1;
                    let slot = if drawing.kind == DrawingKind::Section {
                        2
                    } else {
                        3
                    };
                    if let Some(count) = plan.by_kind.get_mut(slot) {
                        *count += 1;
                    }
                    report.push_coded(
                        Severity::Dropped,
                        name(drawing),
                        code::VIEW_UNSUPPORTED_KIND,
                        "section and detail views are not written yet; the view is left out",
                    );
                }
                DrawingKind::View | DrawingKind::Projection => {
                    let is_group = drawing.kind == DrawingKind::Projection
                        && drawing
                            .relationships
                            .get("Views")
                            .is_some_and(|v| !v.is_empty());
                    if is_group {
                        continue;
                    }
                    plan.views_read += 1;
                    let slot = usize::from(drawing.kind == DrawingKind::Projection);
                    if let Some(count) = plan.by_kind.get_mut(slot) {
                        *count += 1;
                    }
                    views_total += 1;
                    if views_total > MAX_VIEWS {
                        continue;
                    }
                    let Some(direction) = drawing
                        .direction
                        .map(|d| *d.as_raw())
                        .and_then(unit)
                        .map(|d| [-d[0], -d[1], -d[2]])
                    else {
                        report.push_coded(
                            Severity::Dropped,
                            name(drawing),
                            code::VIEW_NO_DIRECTION,
                            "the view has no projection direction in the file; it is left out",
                        );
                        continue;
                    };
                    // The paper's right is the view's x direction; up is the view normal
                    // (toward the viewer) crossed with it.
                    let toward_viewer = [-direction[0], -direction[1], -direction[2]];
                    let up = drawing
                        .x_direction
                        .map(|x| *x.as_raw())
                        .and_then(unit)
                        .and_then(|x| unit(cross(toward_viewer, x)))
                        .filter(|up| dot(*up, direction).abs() < 0.999);
                    let up = if let Some(up) = up {
                        up
                    } else {
                        report.push_coded(
                            Severity::Info,
                            name(drawing),
                            code::VIEW_UP_ASSUMED,
                            "the view's up direction is not in the file; the model's Z axis (or Y for a view along Z) is up",
                        );
                        let hint = if direction[2].abs() > 0.99 {
                            [0.0, 1.0, 0.0]
                        } else {
                            [0.0, 0.0, 1.0]
                        };
                        let k = dot(hint, direction);
                        unit([
                            hint[0] - k * direction[0],
                            hint[1] - k * direction[1],
                            hint[2] - k * direction[2],
                        ])
                        .unwrap_or([0.0, 0.0, 1.0])
                    };
                    let scale = drawing.scale.map(|s| s.get()).and_then(|s| {
                        let text = scale_text(s);
                        if text.is_none() {
                            report.push_coded(
                                Severity::Degraded,
                                name(drawing),
                                code::VIEW_SCALE_FITTED,
                                format!("the scale {s} is outside SolidCraft's 1:100000 to 1000:1; the view is fitted to the sheet"),
                            );
                        }
                        text
                    });
                    if drawing.rotation.is_some_and(|r| r.get().abs() > 1e-9) {
                        report.push_coded(
                            Severity::Dropped,
                            name(drawing),
                            code::VIEW_ROTATION_DROPPED,
                            "the view's rotation on the page is not carried",
                        );
                    }
                    if !source_noted {
                        source_noted = true;
                        report.push_coded(
                            Severity::Info,
                            "drawing",
                            code::VIEW_SOURCE_WHOLE_MODEL,
                            "each view draws the whole converted model; the objects a view named in the source are not carried",
                        );
                    }
                    let [x, y] = drawing.position.map_or([0.0, 0.0], |p| p.get());
                    sheet_plan.views.push(ViewPlan {
                        name: name(drawing).to_owned(),
                        position_mm: [offset[0] + x, offset[1] + y],
                        scale,
                        direction,
                        up,
                    });
                }
            }
        }
        plan.sheets.push(sheet_plan);
    }
    if plan.dimensions_read > 0 {
        report.push_coded(
            Severity::Dropped,
            "dimensions",
            code::DIMENSION_UNSUPPORTED,
            format!("{} dimensions are not written yet; they need model edge references that the drawing does not carry", plan.dimensions_read),
        );
    }
    if plan.annotations_read > 0 {
        report.push_coded(
            Severity::Dropped,
            "annotations",
            code::ANNOTATION_UNSUPPORTED,
            format!(
                "{} notes, balloons, leaders, symbols and images are not written yet",
                plan.annotations_read
            ),
        );
    }
    plan
}

/// The loss stage the drawing codes are recorded at.
pub const STAGE: Stage = Stage::Map;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_paper_to_model_text() {
        assert_eq!(scale_text(1.0).as_deref(), Some("1:1"));
        assert_eq!(scale_text(0.5).as_deref(), Some("1:2"));
        assert_eq!(scale_text(2.0).as_deref(), Some("2:1"));
        assert_eq!(scale_text(0.4).as_deref(), Some("1:2.5"));
        assert_eq!(scale_text(0.0), None);
        assert_eq!(scale_text(5000.0), None);
    }

    #[test]
    fn dwg_versions_are_named_from_the_magic() {
        let sniff = sniff_bytes(Format::DwgDrawing, b"AC1032\0\0\0\0\0", 11);
        assert_eq!(sniff.family, "dwg");
        assert_eq!(sniff.version.as_deref(), Some("AC1032"));
        assert_eq!(sniff.version_name, Some("AutoCAD 2018"));
        assert!(sniff.matches_suffix);
    }

    #[test]
    fn content_is_checked_against_the_suffix() {
        let cfb = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0];
        assert!(sniff_bytes(Format::InventorDrawing, &cfb, 10).matches_suffix);
        assert!(!sniff_bytes(Format::DwgDrawing, b"hello", 5).matches_suffix);
        assert!(!sniff_bytes(Format::SolidEdgeDrawing, &cfb, 10).matches_suffix);
        let mut dft = cfb.to_vec();
        dft.extend("SolidEdge".encode_utf16().flat_map(u16::to_le_bytes));
        let sniff = sniff_bytes(Format::SolidEdgeDrawing, &dft, 30);
        assert!(sniff.matches_suffix && sniff.solid_edge_draft);
        assert!(sniff_bytes(Format::DxfDrawing, b"  0\nSECTION\n  2\nHEADER\n", 20).matches_suffix);
    }
}
