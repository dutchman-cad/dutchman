use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Identifier of the versioned replay plan JSON schema.
pub const SCHEMA: &str = "dutchman.replay-plan/1";

/// Ordered replay data and source mapping outcomes for one part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Plan {
    /// Versioned schema identifier.
    pub schema: String,
    /// Generating Dutchman version.
    pub generator: String,
    /// Part name.
    pub part: String,
    /// Units of evaluated quantities.
    pub units: Units,
    /// Source format and optional input metadata.
    pub source: Source,
    /// Parameters named by written expressions.
    pub parameters: Vec<Parameter>,
    /// Ordered rebuild steps.
    pub steps: Vec<Step>,
    /// Independent reference sketches.
    pub sketches: Vec<Step>,
    /// First rebuild refusal, when present.
    pub stop: Option<Stop>,
    /// Final geometry retained beside the rebuild.
    pub fallback: Fallback,
    /// Optional independent expected measurements.
    pub expected: Option<Expected>,
    /// One result per source feature.
    pub outcomes: Vec<Outcome>,
}
/// Units used by evaluated plan quantities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Units {
    /// Length unit.
    pub length: String,
    /// Unit used for evaluated angles.
    pub angle: String,
}
/// Source metadata available without inspecting native bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Source {
    /// Source format word.
    pub format: String,
    /// Program version, when available.
    pub program_version: Option<String>,
    /// Lower-case SHA-256, when available.
    pub sha256: Option<String>,
}
/// A written parameter and its original source name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Parameter {
    /// Written name.
    pub name: String,
    /// Original expression text.
    pub expr: String,
    /// Evaluated value, or null when evaluation is unavailable.
    pub value: Option<f64>,
    /// Quantity dimension: length, angle, or number.
    pub kind: String,
    /// Original parameter name, when its written identity is unambiguous.
    pub source: Option<String>,
}
/// Original expression, evaluated value, and optional parameter link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Quantity {
    /// Evaluated value, or null when evaluation is unavailable.
    pub value: Option<f64>,
    /// Original expression text.
    pub expr: String,
    /// Written parameter name, when the expression names one.
    pub param: Option<String>,
}
/// An unambiguous link to a source history outcome.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourceLink {
    /// Stable entity or feature identifier.
    pub id: Option<String>,
    /// Source timeline ordinal, when unambiguous.
    pub ordinal: Option<u64>,
}
/// The first feature that cannot be rebuilt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Stop {
    /// Zero-based index of the first refused step.
    pub step_index: u32,
    /// Feature name.
    pub feature: String,
    /// Plain reason identifying the affected feature.
    pub reason: String,
}
/// Embedded final geometry retained for recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Fallback {
    /// Embedded STEP file name.
    pub step_file: String,
}
/// Optional independent source geometry measurements.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Expected {
    /// Number of solid bodies.
    pub bodies: u32,
    /// Finite nonnegative volume in cubic millimetres.
    pub volume_mm3: f64,
    /// Finite nonnegative area in square millimetres.
    pub area_mm2: f64,
    /// Minimum and maximum part-coordinate corners.
    pub bbox_mm: [[f64; 3]; 2],
    /// Program and version that measured the source.
    pub by: String,
}
/// Mapping status and reason for one source feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Outcome {
    /// Source feature name, when available.
    pub name: Option<String>,
    /// Source feature kind reported by the mapper.
    pub kind: String,
    /// Mapping status.
    pub status: OutcomeStatus,
    /// Plain reason identifying the affected feature.
    pub reason: Option<String>,
}

/// One rebuild step with typed fields and preserved untyped options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Step {
    /// Mapper feature id used by references to this step.
    pub id: u64,
    /// Mapped feature kind that selects the typed fields.
    pub kind: String,
    /// Written name.
    pub name: String,
    /// Link to the source feature that produced this step.
    pub source: SourceLink,
    /// Typed data for this step kind.
    #[serde(flatten)]
    pub fields: Fields,
    /// Untyped options that the adapter requires the emitter to declare.
    pub extra: Map<String, Value>,
}

/// Sketch support in part coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Plane {
    /// Principal origin plane.
    Origin {
        /// Principal plane name: XY, XZ or YZ.
        name: String,
    },
    /// Plane offset from another support.
    Offset {
        /// Base support plane.
        base: Box<Plane>,
        /// Signed offset in millimetres.
        distance: Quantity,
    },
    /// Explicit sketch frame in part coordinates.
    Frame {
        /// Frame origin in millimetres.
        origin: [f64; 3],
        /// Unit sketch X direction.
        x_dir: [f64; 3],
        /// Unit sketch Y direction.
        y_dir: [f64; 3],
    },
}
/// Selection of sketch regions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Profile {
    /// All bounded sketch regions.
    All {},
    /// Region interiors picked in sketch coordinates.
    Points {
        /// Interior point coordinates in millimetres.
        points: Vec<[f64; 2]>,
    },
    /// Boundaries named by curve ids.
    Loops {
        /// Ordered closed curve loops.
        loops: Vec<Vec<String>>,
    },
    /// Outer boundaries and their holes.
    Regions {
        /// Selected regions.
        regions: Vec<Region>,
    },
}
/// Outer boundary and interior holes, named by curve ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// Outer boundary curve ids.
    pub outer: Vec<String>,
    /// Interior boundary curve ids.
    pub holes: Vec<Vec<String>>,
}
/// One sketch point in sketch coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Point {
    /// Stable entity or feature identifier.
    pub id: String,
    /// Sketch point coordinates in millimetres.
    pub at: [f64; 2],
    /// Whether the point is fixed.
    pub fixed: bool,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
/// Sketch entities or selected hole point ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Points {
    /// Sketch point entities.
    Sketch(Vec<Point>),
    /// Hole placement point ids.
    Hole(Vec<String>),
}
/// Geometric edge selection with its proof provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct EdgeRef {
    /// Selection point in part coordinates, in millimetres.
    pub at: [f64; 3],
    /// Optional edge endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends: Option<[[f64; 3]; 2]>,
    /// Edge length in millimetres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    /// Provenance of the geometric selection.
    pub proof: String,
}
/// Geometric face selection with its proof provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct FaceRef {
    /// Selection point in part coordinates, in millimetres.
    pub at: [f64; 3],
    /// Face normal in part coordinates.
    pub normal: [f64; 3],
    /// Provenance of the geometric selection.
    pub proof: String,
}
/// A revolve axis defined in part coordinates or by a sketch line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Axis {
    /// Axis defined by a line of the profile sketch.
    SketchLine {
        /// Sketch curve id.
        curve: String,
    },
    /// Explicit axis frame in part coordinates.
    #[serde(rename = "line")]
    Frame {
        /// Axis origin in millimetres.
        origin: [f64; 3],
        /// Unit axis direction.
        dir: [f64; 3],
    },
}
/// A named direction or part-coordinate vector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Direction {
    /// Explicit unit direction.
    Vector([f64; 3]),
    /// Named direction.
    Named(String),
}

/// Typed fields present according to the step kind.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Fields {
    /// Sketch support plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<Plane>,
    /// Sketch point entities or selected hole point ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Points>,
    /// Sketch curves using stable point ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curves: Option<Vec<Curve>>,
    /// Sketch relations using stable point and curve ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Vec<Constraint>>,
    /// Referenced sketch feature id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<u64>,
    /// Selected sketch regions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Boolean operation or new-body operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Extrusion or hole termination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<Extent>,
    /// Selected body names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bodies: Option<Vec<String>>,
    /// Revolve axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Axis>,
    /// Coil axis direction in part coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coil_axis: Option<[f64; 3]>,
    /// Revolve sweep angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Quantity>,
    /// Whether the revolve is symmetric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symmetric: Option<bool>,
    /// Hole form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<Form>,
    /// Diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<Quantity>,
    /// Named or explicit feature direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    /// Feature radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<Quantity>,
    /// Chamfer distance or extrusion distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<Quantity>,
    /// Proven edge selections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<EdgeRef>>,
    /// Referenced seed feature ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<u64>>,
    /// Pattern layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    /// Interior points of removed shell faces in part coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faces: Option<Vec<[f64; 3]>>,
    /// Shell thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Quantity>,
    /// Target body name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Whether tangent-connected faces are included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangent_chain: Option<bool>,
    /// Ordered loft sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<Section>>,
    /// Path sketch feature id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_sketch: Option<u64>,
    /// Ordered path curve ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    /// Base point in part coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<[f64; 3]>,
    /// Axial travel per turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Quantity>,
    /// Turn count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<Quantity>,
    /// Primitive coil section shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Primitive coil section size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_size: Option<Quantity>,
    /// Primitive coil starting angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_angle: Option<Quantity>,
    /// Primitive coil section positioning factor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
    /// Coil turning direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clockwise: Option<bool>,
    /// Primitive center in part coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 3]>,
    /// Torus major radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major: Option<Quantity>,
    /// Torus minor radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minor: Option<Quantity>,
}

/// Sketch curve with point ids instead of array indices.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Curve {
    /// Conic apex point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apex: Option<String>,
    /// Stable entity or feature identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Sketch curve type, such as line or circle.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    /// First endpoint point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    /// Second endpoint point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    /// Center point id for a circle, arc, or ellipse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
    /// Ellipse major-axis point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m: Option<String>,
    /// Radius in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r: Option<f64>,
    /// Spline control point ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pts: Option<Vec<String>>,
    /// Spline degree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degree: Option<u32>,
    /// Whether the curve is construction geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub construction: Option<bool>,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Sketch relation with stable entity references.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Constraint {
    /// Pattern center point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<String>,
    /// Stable entity or feature identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Sketch relation type, such as coincident or length.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    /// Referenced line curve id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// First point or curve id, according to the entity kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    /// Second point or curve id, according to the entity kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
    /// Center point or curve id, according to the entity kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
    /// Referenced sketch point id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p: Option<String>,
    /// Second referenced point id; origin may be implicit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    /// Dimension expression, evaluated quantity, and optional parameter link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Quantity>,
    /// Whether the mapped constraint value was JSON text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_is_text: Option<bool>,
    /// Whether the constraint is a reference dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driven: Option<bool>,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Extrusion or hole extent and explicitly checked options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Extent {
    /// Termination type for an extrusion or hole.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    /// Named or explicit feature direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Chamfer distance or extrusion distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<Quantity>,
    /// Second extrusion distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<Quantity>,
    /// Hole depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<Quantity>,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Hole form and its dimensions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Form {
    /// Hole form type and the dimensions it requires.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    /// Drill tip angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_angle: Option<Quantity>,
    /// Counterbore diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cb_diameter: Option<Quantity>,
    /// Counterbore depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cb_depth: Option<Quantity>,
    /// Countersink diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cs_diameter: Option<Quantity>,
    /// Countersink angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cs_angle: Option<Quantity>,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Pattern layout and dimensions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Layout {
    /// Pattern arrangement type: rectangular, circular, or path.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    /// First rectangular pattern direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir1: Option<[f64; 3]>,
    /// First rectangular instance count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count1: Option<Quantity>,
    /// First rectangular instance spacing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing1: Option<Quantity>,
    /// Second rectangular pattern direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir2: Option<[f64; 3]>,
    /// Second rectangular instance count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count2: Option<Quantity>,
    /// Second rectangular instance spacing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing2: Option<Quantity>,
    /// Circular pattern axis origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f64; 3]>,
    /// Circular pattern axis direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<[f64; 3]>,
    /// Circular or path instance count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Quantity>,
    /// Total circular pattern sweep angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Quantity>,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Loft section selection or apex.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Section {
    /// Referenced sketch feature id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<u64>,
    /// Selected sketch regions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Apex point id in the section sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    /// Untyped options that the adapter requires the emitter to declare.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Mapping result for one source feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OutcomeStatus {
    /// Mapped to editable data.
    Mapped,
    /// Needs no written feature.
    NotNeeded,
    /// Deliberately omitted.
    LeftOut,
    /// Could not be rebuilt.
    Blocked,
}
impl PartialEq<&str> for OutcomeStatus {
    fn eq(&self, value: &&str) -> bool {
        matches!(
            (self, *value),
            (Self::Mapped, "mapped")
                | (Self::NotNeeded, "not_needed")
                | (Self::LeftOut, "left_out")
                | (Self::Blocked, "blocked")
        )
    }
}
