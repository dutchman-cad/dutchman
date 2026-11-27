//! The operations projected: extrusions and revolutions of whole sketches, holes, patterns,
//! sheet-metal walls, and fillets and chamfers of witnessed straight edges.

use serde_json::{json, Map, Value};

use super::json::{kind, literal, number, payload, vector, xyz};
use super::sketch::Sketches;
use super::{blend, extent, hole, pattern, sheet, Plan};

/// What the projection of one operation writes.
pub(super) struct Built {
    /// The model-history operation.
    pub definition: Value,
    /// The ids of the feature rows the operation reads.
    pub dependencies: Vec<String>,
    /// The `model.feature_input_topologies` rows its edge selections name.
    pub topologies: Vec<Value>,
    /// What the projection leaves out of the operation, to report once the row stands.
    pub notes: Vec<Note>,
}

impl Built {
    pub(super) fn new(definition: Value, dependencies: Vec<String>) -> Self {
        Self {
            definition,
            dependencies,
            topologies: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// A coded report entry for a projected feature.
pub(super) struct Note {
    pub code: &'static str,
    pub message: String,
}

type Projection = Result<Built, String>;

/// The model-history operation of a design operation, with the ids of the feature rows it
/// reads. An operation outside the projected kinds, or one stating an operand not projected
/// yet, is an `Err` naming what stops it. `feature` is the id of the feature that states it.
pub(super) fn project(
    feature: &str,
    operation: &Value,
    plan: &Plan,
    sketches: &Sketches<'_>,
) -> Projection {
    match kind(operation) {
        Some("extrude") => extrude(payload(operation), plan, sketches),
        Some("revolve") => revolve(payload(operation), plan, sketches),
        Some("hole") => hole::project(payload(operation)),
        Some("pattern") => pattern::project(payload(operation), plan),
        Some("sheet_metal") => sheet::project(payload(operation), plan, sketches),
        Some("fillet") => blend::fillet(feature, payload(operation)),
        Some("chamfer") => blend::chamfer(feature, payload(operation)),
        Some("unresolved") => Err("the source states only its family".to_owned()),
        Some("native") => Err("the source states only its own kind".to_owned()),
        Some(other) => Err(format!("a {other} is not projected yet")),
        None => Err("its operation is malformed".to_owned()),
    }
}

/// The whole sketch a profile names, and the feature row that carries it.
pub(super) fn profile<'a>(
    selection: Option<&'a Value>,
    plan: &'a Plan,
    sketches: &Sketches<'_>,
) -> Result<(&'a str, &'a str), String> {
    let selection = selection.ok_or("it states no profile")?;
    if selection.pointer("/at/kind").and_then(Value::as_str) != Some("input") {
        return Err("its profile is read in a state other than its input".to_owned());
    }
    if selection.get("native").is_some() {
        return Err("its profile has a native selection, which is not projected yet".to_owned());
    }
    let picks = selection.get("picks").unwrap_or(&Value::Null);
    if kind(picks) != Some("named") {
        return Err("its profile is not a named pick".to_owned());
    }
    let named = payload(picks).as_array().map_or(&[][..], Vec::as_slice);
    let [pick] = named else {
        return Err("its profile does not name exactly one sketch".to_owned());
    };
    let target = pick.get("target").unwrap_or(&Value::Null);
    let only_target = pick.as_object().is_some_and(|pick| pick.len() == 1);
    if kind(target) != Some("sketch") || !only_target {
        return Err(
            "its profile is not a whole sketch, which is all that is projected yet".to_owned(),
        );
    }
    let sketch = payload(target)
        .as_str()
        .ok_or("its profile sketch is malformed")?;
    let carrier = plan
        .sketch_features
        .get(sketch)
        .ok_or_else(|| format!("its profile sketch {sketch} is not part of the history"))?;
    sketches
        .build(sketch)
        .map_err(|reason| format!("its profile sketch {sketch} is not projected: {reason}"))?;
    Ok((sketch, carrier))
}

fn profile_row(sketch: &str) -> Value {
    json!({"kind": "planar", "value": {"kind": "sketch", "value": sketch}})
}

/// The Boolean of a feature's output: `Some` when the design states one.
fn operation_of(output: &Value) -> Result<Option<&'static str>, String> {
    let Some(boolean) = output.get("boolean") else {
        return Ok(None);
    };
    match kind(boolean) {
        Some("new_body") => Ok(Some("new_body")),
        Some("apply") => {
            if boolean.get("targets").is_some() {
                return Err(
                    "it combines with bodies it names, which is not projected yet".to_owned(),
                );
            }
            let op = boolean.get("op").and_then(kind);
            match op {
                Some("join") => Ok(Some("join")),
                Some("cut") => Ok(Some("cut")),
                Some("intersect") => Ok(Some("intersect")),
                Some(other) => Err(format!("a {other} combination is not projected yet")),
                None => Err("its combination is malformed".to_owned()),
            }
        }
        _ => Err("its combination is malformed".to_owned()),
    }
}

/// The Boolean of an output. A combination the design does not state is written as the model
/// row's own unresolved state, which the mapper refuses with its own reason.
fn required_op(output: &Value) -> Result<&'static str, String> {
    Ok(operation_of(output)?.unwrap_or("unresolved"))
}

/// Whether the output is a solid. A form the design leaves unstated is left unstated.
fn solid(output: &Value) -> Result<Option<bool>, String> {
    match output.get("body").and_then(kind) {
        None => Ok(None),
        Some("solid") => Ok(Some(true)),
        Some(other) => Err(format!("a {other} result is not projected yet")),
    }
}

/// Whether the design states a value: present, and not null or empty.
fn stated(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        _ => true,
    }
}

/// Refuses every stated operand outside `known`, the operand keys the projection understands
/// for the operation. A key in `named` is refused with its own description; any other stated
/// key is refused by name, so an operand added to the design later is refused, not ignored.
pub(super) fn refuse_stated(
    operation: &Value,
    known: &[&str],
    named: &[(&str, &str)],
) -> Result<(), String> {
    if operation.is_null() {
        return Ok(());
    }
    let Some(fields) = operation.as_object() else {
        return Err("its operation is malformed".to_owned());
    };
    for (key, what) in named {
        if fields.get(*key).is_some_and(stated) {
            return Err(format!("it states {what}, which is not projected yet"));
        }
    }
    for (key, value) in fields {
        if !known.contains(&key.as_str()) && stated(value) {
            return Err(format!("it states {key}, which is not projected yet"));
        }
    }
    Ok(())
}

/// The operand keys of an extrusion that the projection reads or ignores by design.
pub(super) const EXTRUDE_KEYS: &[&str] = &[
    "profile",
    "direction",
    "origin",
    "span",
    "output",
    "native_options",
    "source_settings",
];

/// The operand keys of a revolution that the projection reads or ignores by design.
pub(super) const REVOLVE_KEYS: &[&str] = &[
    "profile",
    "axis",
    "span",
    "output",
    "native_options",
    "source_settings",
];

fn extrude(operation: &Value, plan: &Plan, sketches: &Sketches<'_>) -> Projection {
    refuse_stated(
        operation,
        EXTRUDE_KEYS,
        &[
            ("loops", "a loop nesting"),
            ("draft", "a draft"),
            ("fit", "a profile fit"),
            ("outside", "an outside setting"),
            ("sheet_cut", "a sheet cut"),
        ],
    )?;
    let (sketch, carrier) = profile(operation.get("profile"), plan, sketches)?;
    let output = operation.get("output").unwrap_or(&Value::Null);
    let mut row = Map::new();
    row.insert("profile".to_owned(), profile_row(sketch));
    // The model row cannot leave a direction or a start out; what the design does not state
    // is written as the row's own unresolved state, which the mapper refuses by name.
    row.insert("start".to_owned(), json!({"kind": "unresolved"}));
    let mut reversed = match operation.get("direction") {
        None => None,
        Some(direction) if kind(direction) == Some("profile_normal") => {
            match direction.get("reversed").map(literal) {
                None => Some(false),
                Some(Some(Value::Bool(state))) => Some(*state),
                Some(_) => {
                    return Err(
                        "its direction is bound to a parameter, which is not projected yet"
                            .to_owned(),
                    )
                }
            }
        }
        Some(_) => {
            return Err(
                "its direction is not the profile normal, which is all that is projected yet"
                    .to_owned(),
            )
        }
    };
    match operation.get("origin") {
        None => {}
        Some(origin) if kind(origin) == Some("profile_plane") => {
            row.insert("start".to_owned(), json!({"kind": "profile_plane"}));
        }
        Some(origin) if kind(origin) == Some("offset") => {
            let offset = number(payload(origin))?;
            row.insert(
                "start".to_owned(),
                json!({"kind": "offset_profile_plane", "offset": offset}),
            );
        }
        Some(_) => {
            return Err(
                "its origin is not the profile plane, which is all that is projected yet"
                    .to_owned(),
            )
        }
    }
    if let Some(span) = operation.get("span") {
        let (extent, against) = extent::extrude(span)?;
        row.insert("extent".to_owned(), extent);
        if against {
            let Some(state) = reversed else {
                return Err(
                    "its direction is not stated, so a span against it cannot be placed".to_owned(),
                );
            };
            reversed = Some(!state);
        }
    }
    match reversed {
        Some(true) => {
            row.insert(
                "direction".to_owned(),
                json!({"kind": "reversed_profile_normal"}),
            );
        }
        Some(false) => {
            row.insert("direction".to_owned(), json!({"kind": "profile_normal"}));
        }
        None => {
            row.insert("direction".to_owned(), json!({"kind": "unresolved"}));
        }
    }
    row.insert("op".to_owned(), json!(required_op(output)?));
    if let Some(solid) = solid(output)? {
        row.insert("solid".to_owned(), json!(solid));
    }
    if !row.contains_key("extent") {
        return Err("it states no extent".to_owned());
    }
    Ok(Built::new(
        json!({"kind": "extrude", "value": row}),
        vec![carrier.to_owned()],
    ))
}

fn revolve(operation: &Value, plan: &Plan, sketches: &Sketches<'_>) -> Projection {
    refuse_stated(
        operation,
        REVOLVE_KEYS,
        &[("loops", "a loop nesting"), ("fit", "a profile fit")],
    )?;
    let (sketch, carrier) = profile(operation.get("profile"), plan, sketches)?;
    let output = operation.get("output").unwrap_or(&Value::Null);
    let mut construction = Map::new();
    // A revolve names its profile without the planar wrapper an extrude puts around it.
    construction.insert(
        "profile".to_owned(),
        json!({"kind": "sketch", "value": sketch}),
    );
    if let Some(axis) = operation.get("axis") {
        // The design states a construction; only what it resolved to is read.
        let resolved = axis.get("resolved");
        let origin = resolved
            .and_then(|axis| axis.get("origin"))
            .and_then(vector);
        let direction = resolved
            .and_then(|axis| axis.get("direction"))
            .and_then(vector);
        if let (Some(origin), Some(direction)) = (origin, direction) {
            construction.insert(
                "axis".to_owned(),
                json!({"origin": xyz(origin), "direction": xyz(direction)}),
            );
        }
    }
    if let Some(span) = operation.get("span") {
        construction.insert("extent".to_owned(), extent::revolve(span)?);
    }
    if let Some(solid) = solid(output)? {
        construction.insert("solid".to_owned(), json!(solid));
    }
    for (key, what) in [("axis", "an axis"), ("extent", "an extent")] {
        if !construction.contains_key(key) {
            return Err(format!("it states no resolved {what}"));
        }
    }
    let mut row = Map::new();
    row.insert(
        "construction".to_owned(),
        json!({"kind": "resolved", "value": construction}),
    );
    row.insert("op".to_owned(), json!(required_op(output)?));
    Ok(Built::new(
        json!({"kind": "revolve", "value": row}),
        vec![carrier.to_owned()],
    ))
}
