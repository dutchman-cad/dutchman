//! Planar sketches, as SolidCraft sketch features.
//!
//! Geometry is written at its solved positions, so a sketch is exact whatever happens to its
//! constraints. Curves that meet share a SolidCraft point: by the source's endpoint records
//! where it gives them, else by position.

use std::collections::BTreeMap;

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::{constraints, name, Context};

/// Positions closer than this (mm) are one point.
const COINCIDENT: f64 = 1e-6;

/// Directions closer than this are the same axis.
const PARALLEL: f64 = 1e-9;

/// A 2D point in sketch coordinates.
pub(crate) type P2 = [f64; 2];

/// A 3D vector.
pub(crate) type V3 = [f64; 3];

/// A mapped sketch, as later features refer to it.
pub(crate) struct Mapped {
    /// The SolidCraft sketch feature's id.
    pub(crate) feature: u64,
    /// The sketch plane's unit normal, in model space.
    pub(crate) normal: V3,
    /// The sketch plane's origin, in model space.
    pub(crate) origin: V3,
    /// What each source entity became, by its id.
    pub(crate) shapes: BTreeMap<String, Shape>,
}

impl Mapped {
    /// The SolidCraft curve the source entity `entity` became.
    pub(crate) fn curve(&self, entity: &str) -> Option<&Curve> {
        match self.shapes.get(entity)? {
            Shape::Curve(curve) => Some(curve),
            Shape::Point(_) => None,
        }
    }
}

/// Maps a sketch feature whose operation is `definition`.
pub(crate) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    let binding = definition.get("sketch").unwrap_or(&Value::Null);
    if binding.get("space").and_then(Value::as_str) != Some("planar") {
        return Err("only planar sketches are mapped".to_owned());
    }
    let sketch_id = binding
        .get("sketch")
        .and_then(Value::as_str)
        .ok_or("the sketch it binds is not resolved")?;
    let sketch = ctx
        .arena("sketches")
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(sketch_id))
        .ok_or("the sketch it binds is missing")?;
    let (plane, normal, origin) = plane(sketch)?;

    let mut builder = Builder::default();
    for entity in ctx
        .arena("sketch_entities")
        .iter()
        .filter(|row| row.get("sketch").and_then(Value::as_str) == Some(sketch_id))
    {
        builder.entity(entity, report)?;
    }
    let id = ctx.id();
    let name = name(feature)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Sketch{id}"));
    let constraints = constraints::map(
        sketch_id,
        &format!("sketch \"{name}\""),
        ctx,
        &builder,
        report,
    );

    ctx.sketches.insert(
        sketch_id.to_owned(),
        Mapped {
            feature: id,
            normal,
            origin,
            shapes: builder.shapes.clone(),
        },
    );
    Ok(json!({
        "id": id,
        "name": name,
        "type": "sketch",
        "suppressed": false,
        "plane": plane,
        "sketch": {
            "points": builder.points,
            "curves": builder.curves,
            "constraints": constraints,
        },
    }))
}

/// The SolidCraft plane of a sketch, and its unit normal: an origin plane when the frame is
/// one exactly, else an explicit plane with the frame's axes.
fn plane(sketch: &Value) -> Result<(Value, V3, V3), String> {
    let placement = sketch.get("placement").unwrap_or(&Value::Null);
    if placement.get("kind").and_then(Value::as_str) != Some("resolved") {
        return Err("its plane is not resolved".to_owned());
    }
    let frame = placement.get("frame").unwrap_or(&Value::Null);
    let origin = v3(frame.get("origin")).ok_or("its plane has no origin")?;
    let normal = unit(v3(frame.get("normal")).ok_or("its plane has no normal")?)
        .ok_or("its plane's normal is zero")?;
    let x = unit(v3(frame.get("u_axis")).ok_or("its plane has no u axis")?)
        .ok_or("its plane's u axis is zero")?;
    if dot(x, normal).abs() > PARALLEL.sqrt() {
        return Err("its plane's u axis is not in the plane".to_owned());
    }
    let y = unit(cross(normal, x)).ok_or("its plane's axes are parallel")?;

    let at_origin = origin.iter().all(|c| c.abs() <= COINCIDENT);
    let named = [
        ("XY", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ("XZ", [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ("YZ", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ]
    .into_iter()
    .find(|(_, px, py)| same(x, *px) && same(y, *py))
    .map(|(name, _, _)| name);
    let plane = match named {
        Some(name) if at_origin => json!({"type": "origin", "name": name}),
        _ => json!({
            "type": "custom",
            "plane": {"origin": origin, "x_dir": x, "y_dir": y},
        }),
    };
    Ok((plane, normal, origin))
}

/// What a source sketch entity became.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Shape {
    /// A point, by index.
    Point(usize),
    /// A curve.
    Curve(Curve),
}

/// A SolidCraft curve a source entity became.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Curve {
    /// Its index among the sketch's curves.
    pub(crate) index: usize,
    /// Its id.
    pub(crate) id: String,
    /// `line`, `circle` or `arc`.
    pub(crate) kind: &'static str,
    /// The point indices of its stored start, end and centre, where it has them.
    pub(crate) start: Option<usize>,
    pub(crate) end: Option<usize>,
    pub(crate) center: Option<usize>,
}

/// SolidCraft sketch points and curves as they are built.
pub(crate) struct Builder {
    points: Vec<Value>,
    /// Positions of `points`, for sharing coincident ends.
    pub(crate) positions: Vec<P2>,
    /// Source endpoint records already placed, with their point index.
    endpoint_refs: Vec<(String, usize)>,
    curves: Vec<Value>,
    /// The last number used per id prefix (`p`, `l`, `c`, `a`).
    counters: BTreeMap<&'static str, u64>,
    /// What each source entity became, by its id.
    pub(crate) shapes: BTreeMap<String, Shape>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            points: vec![json!({"id": "origin", "pos": [0.0, 0.0], "fixed": true})],
            positions: vec![[0.0, 0.0]],
            endpoint_refs: Vec::new(),
            curves: Vec::new(),
            counters: BTreeMap::new(),
            shapes: BTreeMap::new(),
        }
    }
}

impl Builder {
    /// The curve at `index`, as written.
    pub(crate) fn curve_json(&self, index: usize) -> Option<&Value> {
        self.curves.get(index)
    }

    fn fresh(&mut self, prefix: &'static str) -> String {
        let n = self.counters.entry(prefix).or_insert(0);
        *n = n.saturating_add(1);
        format!("{prefix}{n}")
    }

    /// The point at `pos`: the one already placed for `endpoint_ref` or at that position
    /// (never the fixed sketch origin), else a new one.
    fn point(&mut self, pos: P2, endpoint_ref: Option<&str>) -> usize {
        let by_ref = endpoint_ref.and_then(|r| {
            self.endpoint_refs
                .iter()
                .find(|(known, _)| known == r)
                .map(|(_, index)| *index)
        });
        let index = by_ref.or_else(|| {
            self.positions
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, at)| distance(**at, pos) <= COINCIDENT)
                .map(|(index, _)| index)
        });
        let index = index.unwrap_or_else(|| {
            let id = self.fresh("p");
            self.points
                .push(json!({"id": id, "pos": pos, "fixed": false}));
            self.positions.push(pos);
            self.positions.len() - 1
        });
        if let Some(r) = endpoint_ref {
            if !self.endpoint_refs.iter().any(|(known, _)| known == r) {
                self.endpoint_refs.push((r.to_owned(), index));
            }
        }
        index
    }

    /// Adds a curve, recording it as the one the source `entity` became. Its start, end and
    /// centre are the `a`, `b` and `c` points of `kind`.
    fn curve(
        &mut self,
        entity: Option<&str>,
        prefix: &'static str,
        construction: bool,
        kind: Value,
        extra: &[(&str, Value)],
    ) {
        let id = self.fresh(prefix);
        if let Some(entity) = entity {
            let point = |key: &str| {
                kind.get(key)
                    .and_then(Value::as_u64)
                    .and_then(|i| usize::try_from(i).ok())
            };
            let curve = Curve {
                index: self.curves.len(),
                id: id.clone(),
                kind: match kind.get("type").and_then(Value::as_str) {
                    Some("line") => "line",
                    Some("circle") => "circle",
                    Some("arc") => "arc",
                    _ => "",
                },
                start: point("a"),
                end: point("b"),
                center: point("c"),
            };
            self.shapes.insert(entity.to_owned(), Shape::Curve(curve));
        }
        let mut curve = json!({"id": id, "construction": construction});
        if let (Some(fields), Some(kind)) = (curve.as_object_mut(), kind.as_object()) {
            fields.extend(kind.clone());
            for (key, value) in extra {
                fields.insert((*key).to_owned(), value.clone());
            }
        }
        self.curves.push(curve);
    }

    /// Adds one CADIR sketch entity.
    pub(crate) fn entity(&mut self, entity: &Value, report: &mut Report) -> Result<(), String> {
        let construction = entity
            .get("construction")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let refs: Vec<&str> = entity
            .get("endpoint_refs")
            .and_then(Value::as_array)
            .map(|refs| refs.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let source = entity.get("id").and_then(Value::as_str);
        let geometry = entity.get("geometry").unwrap_or(&Value::Null);
        let kind = geometry
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let bad = || format!("its {kind} sketch curve is malformed");
        match kind {
            "point" => {
                let at = p2(geometry.get("position")).ok_or_else(bad)?;
                let index = self.point(at, refs.first().copied());
                if let Some(source) = source {
                    self.shapes.insert(source.to_owned(), Shape::Point(index));
                }
            }
            "line" => {
                let start = p2(geometry.get("start")).ok_or_else(bad)?;
                let end = p2(geometry.get("end")).ok_or_else(bad)?;
                let a = self.point(start, refs.first().copied());
                let b = self.point(end, refs.get(1).copied());
                if a == b {
                    return Err(bad());
                }
                self.curve(
                    source,
                    "l",
                    construction,
                    json!({"type": "line", "a": a, "b": b}),
                    &[],
                );
            }
            "reference_line" => {
                let origin = p2(geometry.get("origin")).ok_or_else(bad)?;
                let direction = p2(geometry.get("direction")).ok_or_else(bad)?;
                let length = direction[0].hypot(direction[1]);
                if length <= PARALLEL {
                    return Err(bad());
                }
                let along = [
                    origin[0] + direction[0] / length,
                    origin[1] + direction[1] / length,
                ];
                let a = self.point(origin, None);
                let b = self.point(along, None);
                self.curve(
                    source,
                    "l",
                    true,
                    json!({"type": "line", "a": a, "b": b}),
                    &[("infinite", json!(true))],
                );
            }
            "circle" => {
                let center = p2(geometry.get("center")).ok_or_else(bad)?;
                let radius = number(geometry.get("radius")).ok_or_else(bad)?;
                let c = self.point(center, None);
                self.curve(
                    source,
                    "c",
                    construction,
                    json!({"type": "circle", "c": c, "r": radius}),
                    &[],
                );
            }
            "arc" => {
                let center = p2(geometry.get("center")).ok_or_else(bad)?;
                let radius = number(geometry.get("radius")).ok_or_else(bad)?;
                let start = number(geometry.get("start_angle")).ok_or_else(bad)?;
                let end = number(geometry.get("end_angle")).ok_or_else(bad)?;
                let at = |angle: f64| {
                    [
                        center[0] + radius * angle.cos(),
                        center[1] + radius * angle.sin(),
                    ]
                };
                let c = self.point(center, None);
                let a = self.point(at(start), refs.first().copied());
                let b = self.point(at(end), refs.get(1).copied());
                self.curve(
                    source,
                    "a",
                    construction,
                    json!({"type": "arc", "c": c, "a": a, "b": b}),
                    &[],
                );
            }
            other if construction => report.push(
                Severity::Degraded,
                "sketch",
                format!("a construction {other} curve is not carried"),
            ),
            other => return Err(format!("{other} sketch curves are not mapped yet")),
        }
        Ok(())
    }
}

pub(crate) fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.get("value").and_then(Value::as_f64))
        .filter(|n| n.is_finite())
}

fn p2(value: Option<&Value>) -> Option<P2> {
    let value = value?;
    Some([number(value.get("u"))?, number(value.get("v"))?])
}

pub(crate) fn v3(value: Option<&Value>) -> Option<V3> {
    let value = value?;
    Some([
        number(value.get("x"))?,
        number(value.get("y"))?,
        number(value.get("z"))?,
    ])
}

pub(crate) fn distance(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn unit(v: V3) -> Option<V3> {
    let length = dot(v, v).sqrt();
    (length > PARALLEL && length.is_finite()).then(|| [v[0] / length, v[1] / length, v[2] / length])
}

fn same(a: V3, b: V3) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x - y).abs() <= PARALLEL.sqrt())
}

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::Path;

    use super::*;

    fn frame(origin: V3, normal: V3, u: V3) -> Value {
        let v = |c: V3| json!({"x": c[0], "y": c[1], "z": c[2]});
        json!({"placement": {"kind": "resolved", "frame": {
            "origin": v(origin), "normal": v(normal), "u_axis": v(u)
        }}})
    }

    #[test]
    fn principal_frames_become_origin_planes() {
        let o = [0.0; 3];
        let cases = [
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], "XY"),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], "XZ"),
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], "YZ"),
        ];
        for (normal, u, name) in cases {
            let (plane, n, _) = plane(&frame(o, normal, u)).unwrap();
            assert_eq!(plane, json!({"type": "origin", "name": name}));
            assert_eq!(n, normal);
        }
    }

    #[test]
    fn other_frames_become_explicit_planes() {
        let (offset, _, _) =
            plane(&frame([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])).unwrap();
        assert_eq!(
            offset,
            json!({"type": "custom", "plane": {
                "origin": [0.0, 0.0, 5.0], "x_dir": [1.0, 0.0, 0.0], "y_dir": [0.0, 1.0, 0.0]
            }})
        );
        // XY seen from below: the normal is reversed, so it is not the XY origin plane.
        let (below, _, _) = plane(&frame([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0])).unwrap();
        assert_eq!(below["type"], "custom");
        assert_eq!(below["plane"]["y_dir"], json!([0.0, -1.0, 0.0]));
    }

    #[test]
    fn unresolved_or_degenerate_frames_are_refused() {
        assert!(plane(&json!({"placement": {"kind": "unresolved"}})).is_err());
        assert!(plane(&frame([0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0])).is_err());
    }

    fn entity(geometry: Value) -> Value {
        json!({"construction": false, "geometry": geometry})
    }

    fn line(a: P2, b: P2) -> Value {
        entity(json!({"kind": "line",
            "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}))
    }

    #[test]
    fn a_rectangle_shares_its_corners() {
        let mut builder = Builder::default();
        let mut report = Report::default();
        for (a, b) in [
            ([0.0, 0.0], [40.0, 0.0]),
            ([40.0, 0.0], [40.0, 30.0]),
            ([40.0, 30.0], [0.0, 30.0]),
            ([0.0, 30.0], [0.0, 0.0]),
        ] {
            builder.entity(&line(a, b), &mut report).unwrap();
        }
        // The origin, then four corners: the corner at (0, 0) is not the fixed origin.
        assert_eq!(builder.points.len(), 5);
        assert_eq!(
            builder.curves,
            [
                json!({"id": "l1", "type": "line", "a": 1, "b": 2, "construction": false}),
                json!({"id": "l2", "type": "line", "a": 2, "b": 3, "construction": false}),
                json!({"id": "l3", "type": "line", "a": 3, "b": 4, "construction": false}),
                json!({"id": "l4", "type": "line", "a": 4, "b": 1, "construction": false}),
            ]
        );
    }

    #[test]
    fn arcs_run_counter_clockwise_between_their_angles() {
        let mut builder = Builder::default();
        let arc = entity(json!({"kind": "arc", "center": {"u": 10.0, "v": 20.0},
            "radius": 30.0, "start_angle": 0.0, "end_angle": std::f64::consts::PI}));
        builder.entity(&arc, &mut Report::default()).unwrap();
        let curve = builder.curves.first().unwrap();
        assert_eq!(curve["type"], "arc");
        let pos = |i: &Value| builder.positions[usize::try_from(i.as_u64().unwrap()).unwrap()];
        assert_eq!(pos(&curve["c"]), [10.0, 20.0]);
        assert_eq!(pos(&curve["a"]), [40.0, 20.0]);
        let end = pos(&curve["b"]);
        assert!(distance(end, [-20.0, 20.0]) < 1e-9, "{end:?}");
    }

    #[test]
    fn endpoint_records_join_curves_whatever_their_positions() {
        let mut builder = Builder::default();
        let mut first = line([0.0, 0.0], [10.0, 0.0]);
        first["endpoint_refs"] = json!(["s", "m"]);
        let mut second = line([10.0, 0.001], [10.0, 10.0]);
        second["endpoint_refs"] = json!(["m", "e"]);
        let mut report = Report::default();
        builder.entity(&first, &mut report).unwrap();
        builder.entity(&second, &mut report).unwrap();
        assert_eq!(builder.curves[0]["b"], builder.curves[1]["a"]);
    }

    #[test]
    fn unsupported_curves_drop_only_as_construction() {
        let ellipse = json!({"kind": "ellipse", "center": {"u": 0.0, "v": 0.0},
            "major_angle": 0.0, "major_radius": 2.0, "minor_radius": 1.0});
        let mut report = Report::default();
        let mut builder = Builder::default();
        let mut construction = entity(ellipse.clone());
        construction["construction"] = json!(true);
        builder.entity(&construction, &mut report).unwrap();
        assert_eq!(report.losses().len(), 1);
        assert_eq!(
            builder.entity(&entity(ellipse), &mut report).unwrap_err(),
            "ellipse sketch curves are not mapped yet"
        );
    }

    /// The sketch of cadmpeg's synthetic `design_sketch_constraints.f3d`, compared with a
    /// golden copy. Set `UPDATE_GOLDEN=1` to rewrite it after an intended change.
    #[test]
    fn a_decoded_fusion_sketch_matches_its_golden_copy() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let model = dutchman_core::read(
            dutchman_core::Format::FusionArchive,
            &root.join("../dutchman-core/tests/fixtures/design_sketch_constraints.f3d"),
            &mut Report::default(),
        )
        .unwrap();
        let wire = serde_json::to_value(&model.ir.model).unwrap();
        let mut ctx = Context::new(&wire);
        ctx.params =
            super::super::params::map(&model.ir.model.parameters, &mut Report::default()).1;
        let feature = ctx.arena("features").first().unwrap();
        let sketch = map(
            feature,
            super::super::operation(feature),
            &mut ctx,
            &mut Report::default(),
        )
        .unwrap();

        let pretty = serde_json::to_string_pretty(&sketch).unwrap() + "\n";
        let golden = root.join("tests/golden/design_sketch_constraints.sketch.json");
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&golden, &pretty).unwrap();
        }
        assert_eq!(pretty, std::fs::read_to_string(&golden).unwrap());
    }
}
