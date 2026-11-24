//! Counts-only reports. No output field can hold source text.
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

macro_rules! words {
    ($name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        enum $name { $(#[serde(rename = $word)] $variant),+ }
        impl $name {
            fn read(text: Option<&str>) -> Self {
                match text { $(Some($word) => Self::$variant,)+ _ => Self::Unknown }
            }
        }
    };
}

words!(SourceFormat {
    InventorPart => "inventor_part",
    InventorAssembly => "inventor_assembly",
    FusionArchive => "fusion_archive",
    FusionMultiDocument => "fusion_multi_document",
    SolidworksPart => "solidworks_part",
    SolidworksAssembly => "solidworks_assembly",
    FreecadDocument => "freecad_document",
    CatiaPart => "catia_part",
    CreoPart => "creo_part",
    NxPart => "nx_part",
    CreoView => "creo_view",
    Rhino => "rhino",
    Acis => "acis",
    Step => "step",
    Iges => "iges",
    OnshapeBundle => "onshape_bundle",
    UnclassifiedPart => "unclassified_part",
    Unknown => "unknown"
});
words!(Kind {
    SheetMetalEdgeFlange => "sheet_metal_edge_flange",
    MeshConvert => "mesh_convert",
    Form => "form",
    TreeNode => "tree_node",
    BaseFeature => "base_feature",
    MeshImport => "mesh_import",
    MeshGenerateFaceGroups => "mesh_generate_face_groups",
    MeshCombineFaceGroups => "mesh_combine_face_groups",
    MeshSection => "mesh_section",
    MeshModify => "mesh_modify",
    InsertBodies => "insert_bodies",
    InsertComponent => "insert_component",
    InsertComponents => "insert_components",
    ComponentTransfer => "component_transfer",
    CapturedComponentPositions => "captured_component_positions",
    AssemblyJoint => "assembly_joint",
    CosmeticThread => "cosmetic_thread",
    ReferenceImage => "reference_image",
    Decal => "decal",
    DatumPrincipalPlane => "datum_principal_plane",
    DatumPlane => "datum_plane",
    DatumThreePointPlane => "datum_three_point_plane",
    DatumOffsetPlane => "datum_offset_plane",
    DatumAxis => "datum_axis",
    DatumPoint => "datum_point",
    PointGeometry => "point_geometry",
    LineSegment => "line_segment",
    CircularArc => "circular_arc",
    EllipticArc => "elliptic_arc",
    Polyline => "polyline",
    RegularPolygonCurve => "regular_polygon_curve",
    PlanarPatch => "planar_patch",
    FaceFromShapes => "face_from_shapes",
    DatumCoordinateSystem => "datum_coordinate_system",
    Block => "block",
    EquationCurve => "equation_curve",
    ProjectedCurve => "projected_curve",
    ProjectOnSurface => "project_on_surface",
    CompositeCurve => "composite_curve",
    Helix => "helix",
    HelixNativeAxis => "helix_native_axis",
    Coil => "coil",
    Sphere => "sphere",
    Torus => "torus",
    Wrap => "wrap",
    Sketch => "sketch",
    SpatialSketch => "spatial_sketch",
    SketchBlockDefinition => "sketch_block_definition",
    SketchBlockInstance => "sketch_block_instance",
    StoredGeometry => "stored_geometry",
    ExtractBody => "extract_body",
    DerivedGeometry => "derived_geometry",
    ImportedGeometry => "imported_geometry",
    Primitive => "primitive",
    Revolve => "revolve",
    Sweep => "sweep",
    HelicalSweep => "helical_sweep",
    Binder => "binder",
    Rib => "rib",
    SheetMetalBaseFlange => "sheet_metal_base_flange",
    SheetMetalAttachedPlate => "sheet_metal_attached_plate",
    SheetMetalBend => "sheet_metal_bend",
    SheetMetalContourFlange => "sheet_metal_contour_flange",
    SheetMetalUnfold => "sheet_metal_unfold",
    SheetMetalRefold => "sheet_metal_refold",
    SheetMetalBendLines => "sheet_metal_bend_lines",
    SheetMetalCorner => "sheet_metal_corner",
    SheetMetalHem => "sheet_metal_hem",
    Fillet => "fillet",
    FullRoundFillet => "full_round_fillet",
    FaceBlend => "face_blend",
    Chamfer => "chamfer",
    Shell => "shell",
    OffsetShape => "offset_shape",
    Compound => "compound",
    RefineShape => "refine_shape",
    ReverseShape => "reverse_shape",
    RuledBetweenCurves => "ruled_between_curves",
    SectionShape => "section_shape",
    MirrorShape => "mirror_shape",
    Thicken => "thicken",
    OffsetSurface => "offset_surface",
    KnitSurface => "knit_surface",
    SewBodies => "sew_bodies",
    UnstitchFaces => "unstitch_faces",
    FilledSurface => "filled_surface",
    TrimSurface => "trim_surface",
    ExtendSurface => "extend_surface",
    UntrimSurface => "untrim_surface",
    RuledSurface => "ruled_surface",
    Draft => "draft",
    Combine => "combine",
    BoundaryFill => "boundary_fill",
    CutWithSurface => "cut_with_surface",
    TrimBodies => "trim_bodies",
    TrimBodiesAtPlane => "trim_bodies_at_plane",
    SplitBody => "split_body",
    SplitFace => "split_face",
    DeleteBody => "delete_body",
    DeleteFace => "delete_face",
    ReplaceFace => "replace_face",
    MoveFace => "move_face",
    MoveBody => "move_body",
    Dome => "dome",
    Flex => "flex",
    Scale => "scale",
    Hole => "hole",
    Pattern => "pattern",
    Unresolved => "unresolved",
    Native => "native",
    Extrude => "extrude",
    Loft => "loft",
    Unknown => "unknown"
});
words!(Verification {
    Verified => "verified", Partial => "partial", Differs => "differs",
    Unverified => "unverified", Flattened => "flattened",
    NotApplicable => "not_applicable", Unknown => "unknown"
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Document {
    Part,
    Assembly,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Timing {
    Unknown,
    UnderSecond,
    Seconds,
    Minutes,
    OverHour,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Mapped,
    Changed,
    Dropped,
    Stopped,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    None,
    NotNeeded,
    LeftOut,
    MappingLimit,
    Replay,
    Inferred,
    Reapplied,
    Unknown,
}
#[derive(Serialize)]
enum Schema {
    #[serde(rename = "dutchman.report-counts/1")]
    V1,
}
/// The release a source program saved the file with, as a fixed word. Only a version the
/// converter read as numbers (`major.minor.revision`, never a name or message) can select one.
macro_rules! bands {
    ($($variant:ident => $word:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        enum VersionBand { $(#[serde(rename = $word)] $variant),+ }
    };
}
bands!(
    Unknown => "unknown",
    InventorOlder => "inventor_older",
    Inventor2016 => "inventor_2016",
    Inventor2017 => "inventor_2017",
    Inventor2018 => "inventor_2018",
    Inventor2019 => "inventor_2019",
    Inventor2020 => "inventor_2020",
    Inventor2021 => "inventor_2021",
    Inventor2022 => "inventor_2022",
    Inventor2023 => "inventor_2023",
    Inventor2024 => "inventor_2024",
    Inventor2025 => "inventor_2025",
    Inventor2026 => "inventor_2026",
    Inventor2027 => "inventor_2027",
    InventorNewer => "inventor_newer",
);

impl VersionBand {
    /// Inventor's file version number is its release year minus 1996 (20 is 2016, 30 is 2026).
    fn inventor(major: u8) -> Self {
        match major {
            0 => Self::Unknown,
            1..=19 => Self::InventorOlder,
            20 => Self::Inventor2016,
            21 => Self::Inventor2017,
            22 => Self::Inventor2018,
            23 => Self::Inventor2019,
            24 => Self::Inventor2020,
            25 => Self::Inventor2021,
            26 => Self::Inventor2022,
            27 => Self::Inventor2023,
            28 => Self::Inventor2024,
            29 => Self::Inventor2025,
            30 => Self::Inventor2026,
            31 => Self::Inventor2027,
            32..=40 => Self::InventorNewer,
            // Far beyond any release: a damaged record, not a version.
            41..=u8::MAX => Self::Unknown,
        }
    }

    /// `version` is the report's `source_version`. Anything that is not exactly three
    /// dot-separated decimal numbers, or comes from a format with no reliable version, is
    /// unknown.
    fn read(format: SourceFormat, version: Option<&str>) -> Self {
        let mut numbers = version.unwrap_or_default().split('.').map(|part| {
            let digits =
                !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse::<u8>().ok()).flatten()
        });
        let (Some(Some(major)), Some(Some(_)), Some(Some(_)), None) = (
            numbers.next(),
            numbers.next(),
            numbers.next(),
            numbers.next(),
        ) else {
            return Self::Unknown;
        };
        match format {
            SourceFormat::InventorPart | SourceFormat::InventorAssembly => Self::inventor(major),
            _ => Self::Unknown,
        }
    }
}

/// An allow-listed report. Fields are private; serialization cannot accept arbitrary text,
/// JSON values, user-controlled keys, paths, identifiers, or measurements.
#[derive(Serialize)]
pub struct Report {
    schema: Schema,
    converter_version: [u64; 3],
    files: Vec<FileCount>,
    features_by_kind: BTreeMap<Kind, u64>,
    features: Vec<FeatureCount>,
}
#[derive(Serialize)]
struct FileCount {
    source_format: SourceFormat,
    source_version_band: VersionBand,
    document: Document,
    verification: Verification,
    timing: Timing,
    converted: bool,
    count: u64,
}
#[derive(Serialize)]
struct FeatureCount {
    kind: Kind,
    outcome: Outcome,
    reason: Reason,
    count: u64,
}
fn field<'a>(value: &'a Value, path: &str) -> Option<&'a str> {
    value.pointer(path).and_then(Value::as_str)
}
fn increment<K: Ord>(counts: &mut BTreeMap<K, u64>, key: K) {
    let count = counts.entry(key).or_default();
    *count = count.saturating_add(1);
}
fn feature(feature: &Value) -> (Kind, Outcome, Reason) {
    let (outcome, reason) = match field(feature, "/status") {
        Some("mapped") if feature.get("inferred").is_some() => (Outcome::Changed, Reason::Inferred),
        Some("mapped") if feature.get("reapplied").and_then(Value::as_bool) == Some(true) => {
            (Outcome::Changed, Reason::Reapplied)
        }
        Some("mapped") => (Outcome::Mapped, Reason::None),
        Some("not_needed") => (Outcome::Mapped, Reason::NotNeeded),
        Some("left_out") => (Outcome::Dropped, Reason::LeftOut),
        Some("blocked") if field(feature, "/category").is_some() => {
            (Outcome::Stopped, Reason::Replay)
        }
        Some("blocked") => (Outcome::Stopped, Reason::MappingLimit),
        _ => (Outcome::Unknown, Reason::Unknown),
    };
    (Kind::read(field(feature, "/kind")), outcome, reason)
}
/// Build from a batch report, or a single conversion report. No strings are copied.
pub fn report(input: &Value) -> Report {
    let mut files = BTreeMap::new();
    let mut features = BTreeMap::new();
    let mut features_by_kind = BTreeMap::new();
    let results = input.get("results").and_then(Value::as_array);
    let results = results.map_or(std::slice::from_ref(input), Vec::as_slice);
    for file in results {
        let format = SourceFormat::read(field(file, "/input/format"));
        let document = if file.get("assembly").is_some_and(Value::is_object)
            || file.get("completeness").is_some_and(Value::is_object)
            || file
                .pointer("/history/parts")
                .and_then(Value::as_array)
                .is_some_and(|parts| !parts.is_empty())
            || matches!(
                format,
                SourceFormat::InventorAssembly
                    | SourceFormat::SolidworksAssembly
                    | SourceFormat::FusionMultiDocument
            ) {
            Document::Assembly
        } else if format == SourceFormat::Unknown {
            Document::Unknown
        } else {
            Document::Part
        };
        let band = VersionBand::read(format, file.get("source_version").and_then(Value::as_str));
        let timing = match file.get("elapsed_ms").and_then(Value::as_u64) {
            None => Timing::Unknown,
            Some(0..=999) => Timing::UnderSecond,
            Some(1000..=59999) => Timing::Seconds,
            Some(60000..=3599999) => Timing::Minutes,
            Some(_) => Timing::OverHour,
        };
        increment(
            &mut files,
            (
                format,
                band,
                document,
                Verification::read(field(file, "/verification/status")),
                timing,
                file.get("ok").and_then(Value::as_bool) == Some(true),
            ),
        );
        if let Some(rows) = file.pointer("/history/features").and_then(Value::as_array) {
            for row in rows {
                let key = feature(row);
                increment(&mut features_by_kind, key.0);
                increment(&mut features, key);
            }
        }
    }
    let mut version = env!("CARGO_PKG_VERSION")
        .split(['.', '-', '+'])
        .filter_map(|s| s.parse::<u64>().ok());
    Report {
        schema: Schema::V1,
        converter_version: [
            version.next().unwrap_or(0),
            version.next().unwrap_or(0),
            version.next().unwrap_or(0),
        ],
        files: files
            .into_iter()
            .map(
                |(
                    (source_format, source_version_band, document, verification, timing, converted),
                    count,
                )| FileCount {
                    source_format,
                    source_version_band,
                    document,
                    verification,
                    timing,
                    converted,
                    count,
                },
            )
            .collect(),
        features_by_kind,
        features: features
            .into_iter()
            .map(|((kind, outcome, reason), count)| FeatureCount {
                kind,
                outcome,
                reason,
                count,
            })
            .collect(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;
    #[allow(dead_code)]
    fn every_operation_has_a_word(operation: &cadmpeg_ir::features::FeatureOperation) -> Kind {
        use cadmpeg_ir::features::FeatureOperation;
        match operation {
            FeatureOperation::SheetMetalEdgeFlange { .. } => Kind::SheetMetalEdgeFlange,
            FeatureOperation::MeshConvert { .. } => Kind::MeshConvert,
            FeatureOperation::Form { .. } => Kind::Form,
            FeatureOperation::TreeNode { .. } => Kind::TreeNode,
            FeatureOperation::BaseFeature { .. } => Kind::BaseFeature,
            FeatureOperation::MeshImport { .. } => Kind::MeshImport,
            FeatureOperation::MeshGenerateFaceGroups { .. } => Kind::MeshGenerateFaceGroups,
            FeatureOperation::MeshCombineFaceGroups { .. } => Kind::MeshCombineFaceGroups,
            FeatureOperation::MeshSection { .. } => Kind::MeshSection,
            FeatureOperation::MeshModify { .. } => Kind::MeshModify,
            FeatureOperation::InsertBodies { .. } => Kind::InsertBodies,
            FeatureOperation::InsertComponent { .. } => Kind::InsertComponent,
            FeatureOperation::InsertComponents { .. } => Kind::InsertComponents,
            FeatureOperation::ComponentTransfer { .. } => Kind::ComponentTransfer,
            FeatureOperation::CapturedComponentPositions { .. } => Kind::CapturedComponentPositions,
            FeatureOperation::AssemblyJoint { .. } => Kind::AssemblyJoint,
            FeatureOperation::CosmeticThread { .. } => Kind::CosmeticThread,
            FeatureOperation::ReferenceImage { .. } => Kind::ReferenceImage,
            FeatureOperation::Decal { .. } => Kind::Decal,
            FeatureOperation::DatumPrincipalPlane { .. } => Kind::DatumPrincipalPlane,
            FeatureOperation::DatumPlane { .. } => Kind::DatumPlane,
            FeatureOperation::DatumThreePointPlane { .. } => Kind::DatumThreePointPlane,
            FeatureOperation::DatumOffsetPlane { .. } => Kind::DatumOffsetPlane,
            FeatureOperation::DatumAxis { .. } => Kind::DatumAxis,
            FeatureOperation::DatumPoint { .. } => Kind::DatumPoint,
            FeatureOperation::PointGeometry { .. } => Kind::PointGeometry,
            FeatureOperation::LineSegment { .. } => Kind::LineSegment,
            FeatureOperation::CircularArc { .. } => Kind::CircularArc,
            FeatureOperation::EllipticArc { .. } => Kind::EllipticArc,
            FeatureOperation::Polyline { .. } => Kind::Polyline,
            FeatureOperation::RegularPolygonCurve { .. } => Kind::RegularPolygonCurve,
            FeatureOperation::PlanarPatch { .. } => Kind::PlanarPatch,
            FeatureOperation::FaceFromShapes { .. } => Kind::FaceFromShapes,
            FeatureOperation::DatumCoordinateSystem { .. } => Kind::DatumCoordinateSystem,
            FeatureOperation::Block { .. } => Kind::Block,
            FeatureOperation::EquationCurve { .. } => Kind::EquationCurve,
            FeatureOperation::ProjectedCurve { .. } => Kind::ProjectedCurve,
            FeatureOperation::ProjectOnSurface { .. } => Kind::ProjectOnSurface,
            FeatureOperation::CompositeCurve { .. } => Kind::CompositeCurve,
            FeatureOperation::Helix { .. } => Kind::Helix,
            FeatureOperation::HelixNativeAxis { .. } => Kind::HelixNativeAxis,
            FeatureOperation::Coil { .. } => Kind::Coil,
            FeatureOperation::Sphere { .. } => Kind::Sphere,
            FeatureOperation::Torus { .. } => Kind::Torus,
            FeatureOperation::Wrap { .. } => Kind::Wrap,
            FeatureOperation::Sketch { .. } => Kind::Sketch,
            FeatureOperation::SpatialSketch { .. } => Kind::SpatialSketch,
            FeatureOperation::SketchBlockDefinition { .. } => Kind::SketchBlockDefinition,
            FeatureOperation::SketchBlockInstance { .. } => Kind::SketchBlockInstance,
            FeatureOperation::StoredGeometry { .. } => Kind::StoredGeometry,
            FeatureOperation::ExtractBody { .. } => Kind::ExtractBody,
            FeatureOperation::DerivedGeometry { .. } => Kind::DerivedGeometry,
            FeatureOperation::ImportedGeometry { .. } => Kind::ImportedGeometry,
            FeatureOperation::Primitive { .. } => Kind::Primitive,
            FeatureOperation::Revolve { .. } => Kind::Revolve,
            FeatureOperation::Sweep { .. } => Kind::Sweep,
            FeatureOperation::HelicalSweep { .. } => Kind::HelicalSweep,
            FeatureOperation::Binder { .. } => Kind::Binder,
            FeatureOperation::Rib { .. } => Kind::Rib,
            FeatureOperation::SheetMetalBaseFlange { .. } => Kind::SheetMetalBaseFlange,
            FeatureOperation::SheetMetalAttachedPlate { .. } => Kind::SheetMetalAttachedPlate,
            FeatureOperation::SheetMetalBend { .. } => Kind::SheetMetalBend,
            FeatureOperation::SheetMetalContourFlange { .. } => Kind::SheetMetalContourFlange,
            FeatureOperation::SheetMetalUnfold { .. } => Kind::SheetMetalUnfold,
            FeatureOperation::SheetMetalRefold { .. } => Kind::SheetMetalRefold,
            FeatureOperation::SheetMetalBendLines { .. } => Kind::SheetMetalBendLines,
            FeatureOperation::SheetMetalCorner { .. } => Kind::SheetMetalCorner,
            FeatureOperation::SheetMetalHem { .. } => Kind::SheetMetalHem,
            FeatureOperation::Fillet { .. } => Kind::Fillet,
            FeatureOperation::FullRoundFillet { .. } => Kind::FullRoundFillet,
            FeatureOperation::FaceBlend { .. } => Kind::FaceBlend,
            FeatureOperation::Chamfer { .. } => Kind::Chamfer,
            FeatureOperation::Shell { .. } => Kind::Shell,
            FeatureOperation::OffsetShape { .. } => Kind::OffsetShape,
            FeatureOperation::Compound { .. } => Kind::Compound,
            FeatureOperation::RefineShape { .. } => Kind::RefineShape,
            FeatureOperation::ReverseShape { .. } => Kind::ReverseShape,
            FeatureOperation::RuledBetweenCurves { .. } => Kind::RuledBetweenCurves,
            FeatureOperation::SectionShape { .. } => Kind::SectionShape,
            FeatureOperation::MirrorShape { .. } => Kind::MirrorShape,
            FeatureOperation::Thicken { .. } => Kind::Thicken,
            FeatureOperation::OffsetSurface { .. } => Kind::OffsetSurface,
            FeatureOperation::KnitSurface { .. } => Kind::KnitSurface,
            FeatureOperation::SewBodies { .. } => Kind::SewBodies,
            FeatureOperation::UnstitchFaces { .. } => Kind::UnstitchFaces,
            FeatureOperation::FilledSurface { .. } => Kind::FilledSurface,
            FeatureOperation::TrimSurface { .. } => Kind::TrimSurface,
            FeatureOperation::ExtendSurface { .. } => Kind::ExtendSurface,
            FeatureOperation::UntrimSurface { .. } => Kind::UntrimSurface,
            FeatureOperation::RuledSurface { .. } => Kind::RuledSurface,
            FeatureOperation::Draft { .. } => Kind::Draft,
            FeatureOperation::Combine { .. } => Kind::Combine,
            FeatureOperation::BoundaryFill { .. } => Kind::BoundaryFill,
            FeatureOperation::CutWithSurface { .. } => Kind::CutWithSurface,
            FeatureOperation::TrimBodies { .. } => Kind::TrimBodies,
            FeatureOperation::TrimBodiesAtPlane { .. } => Kind::TrimBodiesAtPlane,
            FeatureOperation::SplitBody { .. } => Kind::SplitBody,
            FeatureOperation::SplitFace { .. } => Kind::SplitFace,
            FeatureOperation::DeleteBody { .. } => Kind::DeleteBody,
            FeatureOperation::DeleteFace { .. } => Kind::DeleteFace,
            FeatureOperation::ReplaceFace { .. } => Kind::ReplaceFace,
            FeatureOperation::MoveFace { .. } => Kind::MoveFace,
            FeatureOperation::MoveBody { .. } => Kind::MoveBody,
            FeatureOperation::Dome { .. } => Kind::Dome,
            FeatureOperation::Flex { .. } => Kind::Flex,
            FeatureOperation::Scale { .. } => Kind::Scale,
            FeatureOperation::Hole { .. } => Kind::Hole,
            FeatureOperation::Pattern { .. } => Kind::Pattern,
            FeatureOperation::Unresolved { .. } => Kind::Unresolved,
            FeatureOperation::Native { .. } => Kind::Native,
            FeatureOperation::Extrude { .. } => Kind::Extrude,
            FeatureOperation::Loft { .. } => Kind::Loft,
        }
    }

    #[test]
    fn closed_lists_cover_converter_enums() {
        use dutchman_core::{Check, FeatureStatus, Format};
        for format in Format::ALL {
            assert_ne!(SourceFormat::read(Some(format.id())), SourceFormat::Unknown);
        }
        for status in [
            FeatureStatus::Mapped,
            FeatureStatus::NotNeeded,
            FeatureStatus::LeftOut,
            FeatureStatus::Blocked,
        ] {
            // Exhaustive matches intentionally fail compilation when the converter grows.
            let word = match status {
                FeatureStatus::Mapped => "mapped",
                FeatureStatus::NotNeeded => "not_needed",
                FeatureStatus::LeftOut => "left_out",
                FeatureStatus::Blocked => "blocked",
            };
            assert_ne!(feature(&json!({"status": word})).1, Outcome::Unknown);
        }
        for check in [
            Check::Verified,
            Check::Partial(String::new()),
            Check::Differs(String::new()),
            Check::Unverified(String::new()),
            Check::Flattened(String::new()),
            Check::NotApplicable(String::new()),
        ] {
            let word = match check {
                Check::Verified => "verified",
                Check::Partial(_) => "partial",
                Check::Differs(_) => "differs",
                Check::Unverified(_) => "unverified",
                Check::Flattened(_) => "flattened",
                Check::NotApplicable(_) => "not_applicable",
            };
            assert_ne!(Verification::read(Some(word)), Verification::Unknown);
        }
    }

    #[test]
    fn every_band_has_an_explicit_fixed_word() {
        // A new band fails compilation here until it is listed, and its word is pinned.
        for band in [
            VersionBand::Unknown,
            VersionBand::InventorOlder,
            VersionBand::Inventor2016,
            VersionBand::Inventor2017,
            VersionBand::Inventor2018,
            VersionBand::Inventor2019,
            VersionBand::Inventor2020,
            VersionBand::Inventor2021,
            VersionBand::Inventor2022,
            VersionBand::Inventor2023,
            VersionBand::Inventor2024,
            VersionBand::Inventor2025,
            VersionBand::Inventor2026,
            VersionBand::Inventor2027,
            VersionBand::InventorNewer,
        ] {
            let word = match band {
                VersionBand::Unknown => "unknown",
                VersionBand::InventorOlder => "inventor_older",
                VersionBand::Inventor2016 => "inventor_2016",
                VersionBand::Inventor2017 => "inventor_2017",
                VersionBand::Inventor2018 => "inventor_2018",
                VersionBand::Inventor2019 => "inventor_2019",
                VersionBand::Inventor2020 => "inventor_2020",
                VersionBand::Inventor2021 => "inventor_2021",
                VersionBand::Inventor2022 => "inventor_2022",
                VersionBand::Inventor2023 => "inventor_2023",
                VersionBand::Inventor2024 => "inventor_2024",
                VersionBand::Inventor2025 => "inventor_2025",
                VersionBand::Inventor2026 => "inventor_2026",
                VersionBand::Inventor2027 => "inventor_2027",
                VersionBand::InventorNewer => "inventor_newer",
            };
            assert_eq!(serde_json::to_value(band).unwrap(), json!(word));
        }
    }

    #[test]
    fn inventor_file_versions_map_to_release_years_with_open_ends() {
        let band = |version: &str| {
            let input = json!({"input": {"format": "inventor_part"}, "source_version": version});
            serde_json::to_value(report(&input)).unwrap()["files"][0]["source_version_band"].clone()
        };
        assert_eq!(band("19.9.9"), "inventor_older");
        assert_eq!(band("1.0.0"), "inventor_older");
        assert_eq!(band("0.1.2"), "unknown");
        for (major, year) in (20..=31).zip(2016..=2027) {
            assert_eq!(band(&format!("{major}.0.3")), format!("inventor_{year}"));
        }
        assert_eq!(band("32.0.0"), "inventor_newer");
        assert_eq!(band("40.0.0"), "inventor_newer");
        assert_eq!(band("41.0.0"), "unknown");
        assert_eq!(band("255.255.255"), "unknown");
    }

    #[test]
    fn a_version_that_is_not_three_plain_numbers_is_unknown() {
        for version in [
            "",
            "25",
            "25.2",
            "25.2.1.0",
            "256.0.0",
            "-25.2.1",
            "+25.2.1",
            "25.2.x",
            "25..1",
            " 25.2.1",
            "25.2.1 ",
            "0025.2.1",
            "Inventor 2021",
            "2021",
            "٢٥.٢.١",
        ] {
            let input = json!({"input": {"format": "inventor_part"}, "source_version": version});
            let counts = serde_json::to_value(report(&input)).unwrap();
            assert_eq!(
                counts["files"][0]["source_version_band"], "unknown",
                "{version:?}"
            );
        }
        let numbers = json!({"input": {"format": "inventor_part"}, "source_version": 25});
        let counts = serde_json::to_value(report(&numbers)).unwrap();
        assert_eq!(counts["files"][0]["source_version_band"], "unknown");
    }

    #[test]
    fn only_formats_with_a_reliable_version_get_a_band() {
        for format in [
            "fusion_archive",
            "solidworks_part",
            "freecad_document",
            "step",
            "nope",
        ] {
            let input = json!({"input": {"format": format}, "source_version": "25.2.1"});
            let counts = serde_json::to_value(report(&input)).unwrap();
            assert_eq!(
                counts["files"][0]["source_version_band"], "unknown",
                "{format}"
            );
        }
        let input = json!({"input": {"format": "inventor_assembly"}, "source_version": "28.1.1"});
        let counts = serde_json::to_value(report(&input)).unwrap();
        assert_eq!(counts["files"][0]["source_version_band"], "inventor_2024");
    }

    #[test]
    fn versions_split_counts_by_band_without_file_identity() {
        let part = |version: &str| json!({"input": {"format": "inventor_part"}, "source_version": version});
        let all = json!({"results": [part("25.0.0"), part("25.9.9"), part("26.0.0")]});
        let counts = serde_json::to_value(report(&all)).unwrap();
        let rows = counts["files"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["source_version_band"], "inventor_2021");
        assert_eq!(rows[0]["count"], 2);
        assert_eq!(rows[1]["source_version_band"], "inventor_2022");
    }

    #[test]
    fn identical_categories_aggregate_without_preserving_file_identity() {
        let (_, input, _) = fixtures().remove(0);
        let mut second = input.clone();
        second["input"]["path"] = json!("/different/private/file.f3d");
        let counts = serde_json::to_value(report(&json!({"results": [input, second]}))).unwrap();
        assert_eq!(counts["files"].as_array().unwrap().len(), 1);
        assert_eq!(counts["files"][0]["count"], 2);
        for row in counts["features"].as_array().unwrap() {
            assert_eq!(row["count"], 2);
        }
        for count in counts["features_by_kind"].as_object().unwrap().values() {
            assert_eq!(*count, 2);
        }
    }

    #[test]
    fn timing_bands_do_not_preserve_exact_durations() {
        for (ms, band) in [
            (0, "under_second"),
            (999, "under_second"),
            (1000, "seconds"),
            (59999, "seconds"),
            (60000, "minutes"),
            (3599999, "minutes"),
            (3600000, "over_hour"),
            (u64::MAX, "over_hour"),
        ] {
            let counts = serde_json::to_value(report(&json!({"elapsed_ms": ms}))).unwrap();
            assert_eq!(counts["files"][0]["timing"], band);
        }
    }

    #[test]
    fn assemblies_are_detected_from_conversion_structure() {
        for extra in [
            json!({"completeness": {"placed": 2}}),
            json!({"history": {"parts": [{"name": "private-part"}]}}),
        ] {
            let mut input = json!({"ok": true, "input": {"format": "fusion_archive"}});
            input
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let counts = serde_json::to_value(report(&input)).unwrap();
            assert_eq!(counts["files"][0]["document"], "assembly");
        }
    }

    #[test]
    fn every_reason_has_an_explicit_fixed_word() {
        // This match must be updated alongside any new reason; a new variant alone fails.
        for reason in [
            Reason::None,
            Reason::NotNeeded,
            Reason::LeftOut,
            Reason::MappingLimit,
            Reason::Replay,
            Reason::Inferred,
            Reason::Reapplied,
            Reason::Unknown,
        ] {
            let word = match reason {
                Reason::None => "none",
                Reason::NotNeeded => "not_needed",
                Reason::LeftOut => "left_out",
                Reason::MappingLimit => "mapping_limit",
                Reason::Replay => "replay",
                Reason::Inferred => "inferred",
                Reason::Reapplied => "reapplied",
                Reason::Unknown => "unknown",
            };
            assert_eq!(serde_json::to_value(reason).unwrap(), json!(word));
        }
    }

    #[test]
    fn generated_unicode_and_long_fields_cannot_escape_the_allow_list() {
        let mut seed = 9_u32;
        for n in 0..256 {
            let mut secret = format!("private-{n}-");
            for _ in 0..(n * 7) {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if let Some(c) = char::from_u32(0x80 + seed % 0x10000) {
                    secret.push(c);
                }
            }
            for (_, mut input, _) in fixtures() {
                source_strings(&mut input, &secret);
                let text = serde_json::to_string(&report(&input)).unwrap();
                assert!(!text.contains("private-"));
                assert!(text.is_ascii());
            }
        }
    }

    fn fixtures() -> Vec<(&'static str, Value, &'static str)> {
        vec![
            (
                "part",
                serde_json::from_str(include_str!("../tests/counts/part.input.json")).unwrap(),
                include_str!("../tests/counts/part.golden.json"),
            ),
            (
                "assembly",
                serde_json::from_str(include_str!("../tests/counts/assembly.input.json")).unwrap(),
                include_str!("../tests/counts/assembly.golden.json"),
            ),
            (
                "failed",
                serde_json::from_str(include_str!("../tests/counts/failed.input.json")).unwrap(),
                include_str!("../tests/counts/failed.golden.json"),
            ),
        ]
    }
    #[test]
    fn every_fixture_matches_its_golden() {
        for (name, input, golden) in fixtures() {
            let mut actual = serde_json::to_value(report(&input)).unwrap();
            // Package releases do not change the profile's golden counts.
            actual["converter_version"] = json!([0, 0, 0]);
            assert_eq!(
                actual,
                serde_json::from_str::<Value>(golden).unwrap(),
                "{name}"
            );
        }
    }
    fn source_strings(value: &mut Value, secret: &str) {
        match value {
            Value::String(s) => *s = secret.to_owned(),
            Value::Array(rows) => {
                for row in rows {
                    source_strings(row, secret);
                }
            }
            Value::Object(fields) => {
                for row in fields.values_mut() {
                    source_strings(row, secret);
                }
            }
            _ => (),
        }
    }
    #[test]
    fn hostile_text_in_every_source_field_never_serializes() {
        let secrets = [
            "private-name-8237".to_owned(),
            "/private/design/secret-file.f3d".into(),
            "C:\\private\\design\\secret-file.ipt".into(),
            "秘密🦀\u{202e}\"\\\n".into(),
            "private-long-string-".repeat(4096),
        ];
        for (_, input, _) in fixtures() {
            for secret in &secrets {
                let mut hostile = input.clone();
                source_strings(&mut hostile, secret);
                let text = serde_json::to_string(&report(&hostile)).unwrap();
                assert!(!text.contains(secret));
                assert!(!text.contains("private"));
                // Also exercise every field individually, leaving all sibling controls active.
                fn visit(original: &Value, paths: &mut Vec<Vec<String>>, path: Vec<String>) {
                    match original {
                        Value::Object(fields) => {
                            for (key, value) in fields {
                                let mut p = path.clone();
                                p.push(key.clone());
                                visit(value, paths, p);
                            }
                        }
                        Value::Array(rows) => {
                            for (i, value) in rows.iter().enumerate() {
                                let mut p = path.clone();
                                p.push(i.to_string());
                                visit(value, paths, p);
                            }
                        }
                        _ => paths.push(path),
                    }
                }
                let mut paths = Vec::new();
                visit(&input, &mut paths, Vec::new());
                for path in paths {
                    let mut hostile = input.clone();
                    let pointer = format!("/{}", path.join("/"));
                    *hostile.pointer_mut(&pointer).unwrap() = json!(secret);
                    let text = serde_json::to_string(&report(&hostile)).unwrap();
                    let value: Value = serde_json::from_str(&text).unwrap();
                    fn contains(value: &Value, secret: &str) -> bool {
                        match value {
                            Value::String(s) => s.contains(secret),
                            Value::Array(a) => a.iter().any(|v| contains(v, secret)),
                            Value::Object(o) => o
                                .iter()
                                .any(|(k, v)| k.contains(secret) || contains(v, secret)),
                            _ => false,
                        }
                    }
                    assert!(!contains(&value, secret), "{pointer}");
                }
            }
        }
    }
}
