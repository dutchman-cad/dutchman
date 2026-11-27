//! Patterns: linear, circular and mirror layouts of the features or bodies a feature repeats.

use serde_json::{json, Value};

use super::json::{count, kind, literal, number, payload, resolved_vector, reversed, xyz};
use super::ops::{refuse_stated, Built};
use super::Plan;

/// The most instances along one run the projection writes out.
const MAX_INSTANCES: u32 = 10_000;

/// The pattern operation as the model-history `pattern` row.
pub(super) fn project(operation: &Value, plan: &Plan) -> Result<Built, String> {
    refuse_stated(
        operation,
        &[
            "seeds",
            "layout",
            "instances",
            "native_options",
            "source_settings",
        ],
        &[],
    )?;
    let instances = operation.get("instances").unwrap_or(&Value::Null);
    if kind(instances) != Some("implicit")
        || payload(instances)
            .as_array()
            .is_some_and(|overrides| !overrides.is_empty())
    {
        return Err(
            "it states instances that differ from the seed, which is not projected yet".to_owned(),
        );
    }
    let (seeds, dependencies) = seeds(operation.get("seeds").unwrap_or(&Value::Null), plan)?;
    let layout = layout(operation.get("layout").unwrap_or(&Value::Null))?;
    Ok(Built::new(
        json!({"kind": "pattern", "value": {"seeds": seeds, "pattern": layout}}),
        dependencies,
    ))
}

/// The members of a selection read in its input state, one pick each.
fn picks<'a>(selection: &'a Value, what: &str) -> Result<&'a [Value], String> {
    if selection.pointer("/at/kind").and_then(Value::as_str) != Some("input") {
        return Err(format!(
            "its {what} are read in a state other than its input"
        ));
    }
    if selection.get("native").is_some() {
        return Err(format!(
            "its {what} have a native selection, which is not projected yet"
        ));
    }
    let picks = selection.get("picks").unwrap_or(&Value::Null);
    if kind(picks) != Some("named") {
        return Err(format!("its {what} are not named picks"));
    }
    let named = payload(picks).as_array().map_or(&[][..], Vec::as_slice);
    if named.is_empty() {
        return Err(format!("it names no {what}"));
    }
    Ok(named)
}

fn seeds(seeds: &Value, plan: &Plan) -> Result<(Value, Vec<String>), String> {
    match kind(seeds) {
        Some("features") => {
            if seeds.get("method").is_some() {
                return Err("it states a pattern method, which is not projected yet".to_owned());
            }
            let mut ids = Vec::new();
            let mut rows = Vec::new();
            for pick in picks(
                seeds.get("features").unwrap_or(&Value::Null),
                "seed features",
            )? {
                let target = pick.get("target").unwrap_or(&Value::Null);
                if kind(target) != Some("feature_result")
                    || pick.as_object().is_none_or(|pick| pick.len() != 1)
                {
                    return Err(
                        "a seed is not a whole feature, which is all that is projected yet"
                            .to_owned(),
                    );
                }
                let id = payload(target)
                    .as_str()
                    .ok_or("a seed feature is malformed")?;
                if !plan.features.contains_key(id) {
                    return Err(format!("its seed {id} is not part of the history"));
                }
                ids.push(id.to_owned());
                rows.push(json!({"kind": "feature", "value": id}));
            }
            Ok((Value::Array(rows), ids))
        }
        Some("bodies") => {
            if seeds.get("join").is_some() {
                return Err("it states how seed bodies join, which is not projected yet".to_owned());
            }
            let selection = seeds.get("bodies").unwrap_or(&Value::Null);
            let mut bodies: Vec<&str> = Vec::new();
            for pick in picks(selection, "seed bodies")? {
                let resolved = pick
                    .get("resolved")
                    .and_then(Value::as_array)
                    .filter(|resolved| !resolved.is_empty())
                    .ok_or("a seed body is not resolved to a body")?;
                for entity in resolved {
                    let id = (kind(entity) == Some("body"))
                        .then(|| payload(entity).as_str())
                        .flatten()
                        .ok_or("a seed body resolves to something other than a body")?;
                    if !bodies.contains(&id) {
                        bodies.push(id);
                    }
                }
            }
            Ok((
                json!([{"kind": "bodies", "value": {"kind": "bodies", "value": bodies}}]),
                Vec::new(),
            ))
        }
        Some("faces") => Err("a pattern of faces is not projected".to_owned()),
        _ => Err("its seeds are malformed".to_owned()),
    }
}

/// How the design reads a run: the instances and the distance or angle between them, with
/// the direction of travel turned around when the design has the run go against it.
struct Run {
    count: u32,
    step: f64,
    reversed: bool,
}

fn run(run: &Value, what: &str) -> Result<Run, String> {
    refuse_stated(
        run,
        &["spacing", "sides", "increments"],
        &[("seed", "a seed instance other than the first")],
    )?;
    if run
        .get("increments")
        .and_then(Value::as_array)
        .is_some_and(|increments| !increments.is_empty())
    {
        return Err(format!(
            "its {what} states increments, which is not projected yet"
        ));
    }
    let spacing = run.get("spacing").unwrap_or(&Value::Null);
    if kind(spacing) != Some("pitch") {
        return Err(format!(
            "its {what} is not stated by a count and a pitch, which is all that is projected yet"
        ));
    }
    let body = payload(spacing);
    let instances = u32::try_from(count(body.get("count").ok_or("the run has no count")?)?)
        .ok()
        .filter(|instances| *instances <= MAX_INSTANCES)
        .ok_or_else(|| format!("its {what} has more than {MAX_INSTANCES} instances"))?;
    let step = number(body.get("pitch").ok_or("the run has no pitch")?)?;
    if step == 0.0 {
        return Err(format!("the pitch of its {what} is zero"));
    }
    let against = match run.get("sides") {
        None => false,
        Some(sides) => match kind(sides) {
            Some("one") => false,
            Some("against") => true,
            Some(other) => return Err(format!("a {other} run is not projected yet")),
            None => return Err("its sides are malformed".to_owned()),
        },
    };
    Ok(Run {
        count: instances,
        step: step.abs(),
        reversed: against != (step < 0.0),
    })
}

fn direction_row(direction: Option<[f64; 3]>, run: &Run) -> Option<Value> {
    direction.map(|direction| {
        xyz(if run.reversed {
            reversed(direction)
        } else {
            direction
        })
    })
}

fn linear_run(linear: &Value, what: &str) -> Result<(Option<Value>, f64, u32), String> {
    let run = run(linear.get("run").unwrap_or(&Value::Null), what)?;
    let direction = direction_row(linear.get("direction").and_then(resolved_vector), &run);
    Ok((direction, run.step, run.count))
}

fn layout(layout: &Value) -> Result<Value, String> {
    let body = payload(layout);
    match kind(layout) {
        Some("linear") => {
            refuse_stated(body, &["kind", "first", "second", "seed_only"], &[])?;
            if let Some(only) = body.get("seed_only") {
                if literal(only).and_then(Value::as_bool) != Some(false) {
                    return Err("it keeps only the seed, which is not projected yet".to_owned());
                }
            }
            let (direction, spacing, instances) =
                linear_run(body.get("first").ok_or("it has no first run")?, "first run")?;
            let mut row = json!({
                "kind": "linear",
                "spacing": spacing,
                "count": instances,
                "omitted_instances": [],
                "symmetric": false,
            });
            let fields = row.as_object_mut().ok_or("the layout is malformed")?;
            if let Some(direction) = direction {
                fields.insert("direction".to_owned(), direction);
            }
            if let Some(second) = body.get("second") {
                let (direction, spacing, instances) = linear_run(second, "second run")?;
                // The row's second direction has no unresolved state of its own.
                let direction = direction.ok_or("its second run has no resolved direction")?;
                fields.insert(
                    "second".to_owned(),
                    json!({"direction": direction, "spacing": spacing, "count": instances,
                        "symmetric": false}),
                );
            }
            Ok(row)
        }
        Some("circular") => {
            refuse_stated(
                body,
                &["kind", "axis", "angular"],
                &[
                    ("radial", "a radial run"),
                    ("orientation", "an instance orientation"),
                ],
            )?;
            let run = run(body.get("angular").unwrap_or(&Value::Null), "angular run")?;
            let axis = body.get("axis").and_then(|axis| axis.get("resolved"));
            let origin = axis.and_then(|axis| axis.get("origin"));
            let direction = axis.and_then(|axis| axis.get("direction"));
            let (Some(origin), Some(direction)) = (origin, direction) else {
                return Err("it states no resolved axis".to_owned());
            };
            let direction = super::json::vector(direction).ok_or("its axis is malformed")?;
            let direction = if run.reversed {
                reversed(direction)
            } else {
                direction
            };
            let angles: Vec<f64> = (0..run.count).map(|k| run.step * f64::from(k)).collect();
            Ok(json!({
                "kind": "circular_angles",
                "axis_origin": origin,
                "axis_dir": xyz(direction),
                "angles": angles,
            }))
        }
        Some("mirror") => {
            refuse_stated(
                body,
                &["kind", "plane"],
                &[("stitch_tolerance", "a stitch tolerance")],
            )?;
            let plane = body.get("plane").and_then(|plane| plane.get("resolved"));
            let origin = plane.and_then(|plane| plane.get("origin"));
            let normal = plane.and_then(|plane| plane.get("normal"));
            let (Some(origin), Some(normal)) = (origin, normal) else {
                return Err("it states no resolved mirror plane".to_owned());
            };
            Ok(json!({"kind": "mirror", "plane_origin": origin, "plane_normal": normal}))
        }
        Some(other) => Err(format!("a {other} layout is not projected yet")),
        None => Err("its layout is malformed".to_owned()),
    }
}
