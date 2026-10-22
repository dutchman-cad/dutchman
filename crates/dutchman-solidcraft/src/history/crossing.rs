//! Lines drawn across a profile that the sketch's profile table takes whole.
//!
//! A sketch's profile table lists the closed regions its curves bound. A line from one point of
//! a region's boundary to another, through its inside, splits that region in two, so a region
//! the table lists whole cannot be crossed by one, unless the line is construction geometry. A
//! decoder that does not know which curves are construction (cadmpeg's Inventor decoder, for
//! now) leaves such a line solid; SolidCraft then finds the region split and no profile it can
//! match. Such lines are construction, and are written so.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::sketch::{number, P2};

/// Points sampled along a full circle.
const CIRCLE_SAMPLES: usize = 64;

/// Points sampled along an arc.
const ARC_SAMPLES: usize = 32;

/// The ids of the solid lines among `entities` (a sketch's) that cross a region its profile
/// table, `sketch["profiles"]`, takes whole.
pub(super) fn crossing(sketch: &Value, entities: &[&Value]) -> BTreeSet<String> {
    let by_id: BTreeMap<&str, &Value> = entities
        .iter()
        .filter_map(|entity| Some((entity.get("id")?.as_str()?, *entity)))
        .collect();
    let mut bounding: BTreeSet<&str> = BTreeSet::new();
    let mut polygons: Vec<Vec<P2>> = Vec::new();
    for members in loops(sketch) {
        let mut polygon = Vec::new();
        for member in members {
            let Some(id) = member.get("entity").and_then(Value::as_str) else {
                continue;
            };
            bounding.insert(id);
            let Some(mut samples) = by_id.get(id).and_then(|e| samples(e)) else {
                // A loop of a curve that cannot be followed is not tested.
                polygon.clear();
                break;
            };
            if member.get("reversed").and_then(Value::as_bool) == Some(true) {
                samples.reverse();
            }
            polygon.extend(samples);
        }
        if polygon.len() >= 3 {
            polygons.push(polygon);
        }
    }
    entities
        .iter()
        .filter(|entity| entity.get("construction").and_then(Value::as_bool) != Some(true))
        .filter_map(|entity| {
            let id = entity.get("id")?.as_str()?;
            if bounding.contains(id) {
                return None;
            }
            let geometry = entity.get("geometry")?;
            if geometry.get("kind").and_then(Value::as_str) != Some("line") {
                return None;
            }
            let start = super::sketch::p2(geometry.get("start"))?;
            let end = super::sketch::p2(geometry.get("end"))?;
            let middle = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0];
            polygons
                .iter()
                .any(|polygon| {
                    let tolerance = tolerance(polygon);
                    on_boundary(start, polygon, tolerance)
                        && on_boundary(end, polygon, tolerance)
                        && !on_boundary(middle, polygon, tolerance)
                        && inside(middle, polygon)
                })
                .then(|| id.to_owned())
        })
        .collect()
}

/// The loops of the sketch's profile table, each its members (`{entity, reversed}`) in order.
fn loops(sketch: &Value) -> Vec<&[Value]> {
    let mut out = Vec::new();
    for profile in sketch
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match profile {
            Value::Array(members) => out.push(members.as_slice()),
            Value::Object(fields) => {
                for members in fields
                    .get("loops")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(members) = members.as_array() {
                        out.push(members.as_slice());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Points along a sketch curve from its start to its end; `None` for a curve not followed.
fn samples(entity: &Value) -> Option<Vec<P2>> {
    let geometry = entity.get("geometry")?;
    match geometry.get("kind")?.as_str()? {
        "line" => Some(vec![
            super::sketch::p2(geometry.get("start"))?,
            super::sketch::p2(geometry.get("end"))?,
        ]),
        kind @ ("arc" | "circle") => {
            let center = super::sketch::p2(geometry.get("center"))?;
            let radius = number(geometry.get("radius"))?;
            let (start, sweep, count) = if kind == "arc" {
                let start = number(geometry.get("start_angle"))?;
                let end = number(geometry.get("end_angle"))?;
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                (start, sweep, ARC_SAMPLES)
            } else {
                (0.0, std::f64::consts::TAU, CIRCLE_SAMPLES)
            };
            Some(
                (0..=count)
                    .map(|i| {
                        let angle = start + sweep * i as f64 / count as f64;
                        [
                            center[0] + radius * angle.cos(),
                            center[1] + radius * angle.sin(),
                        ]
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// How near a point must be to a polygon's edge to be on it: a millionth of its size.
fn tolerance(polygon: &[P2]) -> f64 {
    let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
    for &[x, y] in polygon {
        low = [low[0].min(x), low[1].min(y)];
        high = [high[0].max(x), high[1].max(y)];
    }
    ((high[0] - low[0]).hypot(high[1] - low[1]) * 1e-6).max(1e-9)
}

/// The polygon's edges, the last closing it.
fn edges(polygon: &[P2]) -> impl Iterator<Item = (P2, P2)> + '_ {
    polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .map(|(a, b)| (*a, *b))
}

fn on_boundary(p: P2, polygon: &[P2], tolerance: f64) -> bool {
    edges(polygon).any(|(a, b)| {
        let along = [b[0] - a[0], b[1] - a[1]];
        let to = [p[0] - a[0], p[1] - a[1]];
        let length = along[0] * along[0] + along[1] * along[1];
        let t = if length > 0.0 {
            ((to[0] * along[0] + to[1] * along[1]) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let near = [a[0] + along[0] * t - p[0], a[1] + along[1] * t - p[1]];
        near[0].hypot(near[1]) <= tolerance
    })
}

/// Whether `p` is inside the polygon, by the even-odd rule.
fn inside(p: P2, polygon: &[P2]) -> bool {
    edges(polygon)
        .filter(|(a, b)| {
            (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        })
        .count()
        % 2
        == 1
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    fn line(id: &str, a: P2, b: P2) -> Value {
        json!({"id": id, "geometry": {"kind": "line",
            "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}})
    }

    fn member(id: &str) -> Value {
        json!({"entity": id, "reversed": false})
    }

    #[test]
    fn diagonals_of_a_square_taken_whole_are_construction() {
        let s = 10.0;
        let entities = [
            line("e1", [s, -s], [-s, -s]),
            line("e2", [-s, -s], [-s, s]),
            line("e3", [-s, s], [s, s]),
            line("e4", [s, s], [s, -s]),
            line("d1", [s, -s], [-s, s]),
            line("d2", [-s, -s], [s, s]),
            // From the boundary to outside, and wholly inside: neither splits the square.
            line("out", [s, 0.0], [2.0 * s, 0.0]),
            line("in", [-1.0, 0.0], [1.0, 0.0]),
            // Along an edge: on the boundary all the way.
            line("edge", [s, -s], [s, s]),
        ];
        let refs: Vec<&Value> = entities.iter().collect();
        let sketch = json!({"profiles": [[member("e1"), member("e2"), member("e3"),
                                          member("e4")]]});
        let crossing = crossing(&sketch, &refs);
        assert_eq!(
            crossing.iter().map(String::as_str).collect::<Vec<_>>(),
            ["d1", "d2"]
        );
    }

    #[test]
    fn a_line_already_construction_or_bounding_a_profile_is_left_alone() {
        let mut d1 = line("d1", [10.0, 0.0], [0.0, 10.0]);
        d1["construction"] = json!(true);
        let entities = [
            line("e1", [0.0, 0.0], [10.0, 0.0]),
            line("e2", [10.0, 0.0], [0.0, 10.0]),
            line("e3", [0.0, 10.0], [0.0, 0.0]),
            d1,
        ];
        let refs: Vec<&Value> = entities.iter().collect();
        let sketch = json!({"profiles": [[member("e1"), member("e2"), member("e3")]]});
        assert!(crossing(&sketch, &refs).is_empty());
    }

    #[test]
    fn a_chord_across_a_circle_taken_whole_is_construction() {
        let circle = json!({"id": "c", "geometry": {"kind": "circle",
            "center": {"u": 0.0, "v": 0.0}, "radius": 5.0}});
        let entities = [circle, line("chord", [-5.0, 0.0], [5.0, 0.0])];
        let refs: Vec<&Value> = entities.iter().collect();
        let sketch = json!({"profiles": [[member("c")]]});
        assert_eq!(crossing(&sketch, &refs).len(), 1);
    }
}
