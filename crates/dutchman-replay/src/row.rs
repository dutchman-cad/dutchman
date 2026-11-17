//! Lossless conversion between mapped rows and stable plan references.
use crate::{Plan, Quantity, SourceLink, Step};
use dutchman_core::FeatureOutcome;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Convert a mapped row to typed fields and stable references without losing options.
pub fn from_row(
    row: &Value,
    parameters: &BTreeMap<String, String>,
    outcomes: &[FeatureOutcome],
    rows: &[Value],
) -> Result<Step, String> {
    Context::new(parameters, outcomes, rows).step(row)
}

pub(crate) struct Context<'a> {
    parameters: &'a BTreeMap<String, String>,
    sources: BTreeMap<&'a str, Option<u64>>,
    ambiguous: std::collections::BTreeSet<&'a str>,
    features: BTreeMap<&'a str, Option<u64>>,
}
impl<'a> Context<'a> {
    pub(crate) fn new(
        parameters: &'a BTreeMap<String, String>,
        outcomes: &'a [FeatureOutcome],
        rows: &'a [Value],
    ) -> Self {
        let mut sources = BTreeMap::new();
        for outcome in outcomes {
            if let Some(name) = outcome.name.as_deref() {
                sources
                    .entry(name)
                    .and_modify(|ordinal| *ordinal = None)
                    .or_insert(outcome.ordinal);
            }
        }
        let mut features = BTreeMap::new();
        let mut ambiguous = std::collections::BTreeSet::new();
        for row in rows {
            if let Some(name) = row.get("name").and_then(Value::as_str) {
                features
                    .entry(name)
                    .and_modify(|id| {
                        *id = None;
                        ambiguous.insert(name);
                    })
                    .or_insert(row.get("id").and_then(Value::as_u64));
            }
        }
        Self {
            parameters,
            sources,
            features,
            ambiguous,
        }
    }
    pub(crate) fn step(&self, row: &Value) -> Result<Step, String> {
        let parameters = self.parameters;
        let name = row.get("name").and_then(Value::as_str).unwrap_or("unknown");
        validate_row(row).map_err(|reason| format!("feature {name}: {reason}"))?;
        let fail = || format!("feature {name}: a sketch reference is out of range");
        let mut fields = row
            .as_object()
            .cloned()
            .ok_or_else(|| format!("feature {name}: a mapped row is not an object"))?;
        let kind = fields
            .remove("type")
            .ok_or_else(|| format!("feature {name}: a mapped row has no type"))?;
        let kind_text = kind
            .as_str()
            .ok_or_else(|| format!("feature {name}: a mapped row has no type"))?
            .to_owned();
        let mut common = Map::new();
        common.insert("kind".into(), kind);
        for key in ["id", "name"] {
            common.insert(
                key.into(),
                fields
                    .remove(key)
                    .ok_or_else(|| format!("feature {name}: a mapped row has no {key}"))?,
            );
        }
        if fields.get("suppressed") == Some(&json!(false)) {
            fields.remove("suppressed");
        }
        let source = SourceLink {
            id: None,
            ordinal: self.sources.get(name).copied().flatten(),
        };
        common.insert("source".into(), json!(source));
        let known: &[&str] = match kind_text.as_str() {
            "sketch" => &["plane", "points", "curves", "constraints"],
            "extrude" => &["sketch", "profile", "operation", "extent", "bodies"],
            "revolve" => &[
                "sketch",
                "profile",
                "operation",
                "axis",
                "angle",
                "symmetric",
                "bodies",
            ],
            "hole" => &[
                "sketch",
                "points",
                "form",
                "diameter",
                "extent",
                "direction",
            ],
            "fillet" => &["radius", "edges"],
            "chamfer" => &["distance", "edges"],
            "pattern" => &["features", "layout"],
            "mirror" => &["features", "plane"],
            "shell" => &["faces", "thickness", "direction", "body", "tangent_chain"],
            "loft" => &["sections", "operation", "bodies"],
            "sweep" => &[
                "sketch",
                "profile",
                "operation",
                "path_sketch",
                "path",
                "bodies",
            ],
            "coil" => &[
                "sketch",
                "profile",
                "operation",
                "base",
                "coil_axis",
                "pitch",
                "turns",
                "diameter",
                "section",
                "section_size",
                "start_angle",
                "position",
                "clockwise",
                "bodies",
            ],
            "sphere" => &["center", "radius", "operation", "bodies"],
            "torus" => &["center", "major", "minor", "operation", "bodies"],
            _ => &[],
        };
        if known.is_empty() {
            common.insert("extra".into(), Value::Object(fields));
            return serde_json::from_value(Value::Object(common))
                .map_err(|_| format!("feature {name}: its mapped fields are not supported yet"));
        }
        let row_keys: Vec<&str> = if kind_text == "sketch" {
            vec!["plane", "sketch"]
        } else if kind_text == "hole" {
            vec!["points", "hole", "diameter", "depth", "direction"]
        } else {
            known
                .iter()
                .map(|key| match *key {
                    "profile" => "profiles",
                    "bodies" => "body_names",
                    "layout" => "pattern",
                    "coil_axis" => "axis",
                    other => other,
                })
                .collect()
        };
        let mut extra = Map::new();
        for (key, value) in std::mem::take(&mut fields) {
            if row_keys.contains(&key.as_str()) {
                fields.insert(key, value);
            } else {
                extra.insert(key, value);
            }
        }
        if kind_text == "coil" {
            rename_map(&mut fields, "axis", "coil_axis");
        }
        if let Some(axis) = fields.get_mut("axis").and_then(Value::as_object_mut) {
            let mut metadata = Map::new();
            for (key, value) in std::mem::take(axis) {
                if matches!(key.as_str(), "type" | "origin" | "dir" | "curve") {
                    axis.insert(key, value);
                } else {
                    metadata.insert(key, value);
                }
            }
            if !metadata.is_empty() {
                extra.insert("axis".into(), Value::Object(metadata));
            }
        }
        if kind_text == "sketch" {
            let mut data = fields.remove("sketch").ok_or_else(fail)?;
            for key in ["points", "curves", "constraints"] {
                let values = data.get(key).and_then(Value::as_array).ok_or_else(fail)?;
                if values.len() > 100_000 {
                    return Err(format!(
                        "feature {name}: sketch size exceeds the script limit"
                    ));
                }
                fields.insert(key.into(), json!(values));
            }
            if let Some(data) = data.as_object_mut() {
                for key in ["points", "curves", "constraints"] {
                    data.remove(key);
                }
                if !data.is_empty() {
                    fields.insert("sketch".into(), Value::Object(data.clone()));
                }
            }
            let points = fields
                .get("points")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(fail)?;
            let curves = fields
                .get("curves")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(fail)?;
            if let Some(values) = fields.get_mut("points").and_then(Value::as_array_mut) {
                for point in values {
                    rename(point, "pos", "at");
                }
            }
            if let Some(values) = fields.get_mut("curves").and_then(Value::as_array_mut) {
                for curve in values {
                    for key in ["a", "b", "c", "m", "apex"] {
                        index_to_id(curve, key, &points).map_err(|_| fail())?;
                    }
                    if let Some(ids) = curve.get_mut("pts").and_then(Value::as_array_mut) {
                        for id in ids {
                            *id = get_id(id, &points).ok_or_else(fail)?;
                        }
                    }
                }
            }
            if let Some(values) = fields.get_mut("constraints").and_then(Value::as_array_mut) {
                for c in values {
                    for key in ["p", "q", "center"] {
                        index_to_id(c, key, &points).map_err(|_| fail())?;
                    }
                    for key in ["l", "a", "b", "c"] {
                        index_to_id(c, key, &curves).map_err(|_| fail())?;
                    }
                    rename(c, "l", "line");
                    let param = c.as_object_mut().and_then(|c| c.remove("param"));
                    let angle = matches!(
                        c.get("type").and_then(Value::as_str),
                        Some("angle" | "line_angle" | "arc_sweep")
                    );
                    if c.get("value")
                        .and_then(Value::as_str)
                        .is_some_and(|s| serde_json::from_str::<Value>(s).is_ok())
                    {
                        if let Some(m) = c.as_object_mut() {
                            m.insert("value_is_text".into(), json!(true));
                        }
                    }
                    if let Some(value) = c.get_mut("value") {
                        let expr = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        let expr = if angle { format!("{expr} rad") } else { expr };
                        let mut q = Quantity::from_row(&expr, parameters);
                        q.param = param.and_then(|p| p.as_str().map(str::to_owned));
                        *value = json!(q);
                    }
                }
            }
        }
        if let Some(plane) = fields.get_mut("plane") {
            plane_forward(plane)
                .map_err(|_| format!("feature {name}: its mapped fields are not supported yet"))?;
        }
        if known.contains(&"profile") {
            rename_map(&mut fields, "profiles", "profile");
        }
        if let Some(p) = fields.get_mut("profile") {
            profile_forward(p);
        }
        if known.contains(&"bodies") {
            rename_map(&mut fields, "body_names", "bodies");
        }
        if kind_text == "extrude" {
            if let Some(e) = fields.get_mut("extent").and_then(Value::as_object_mut) {
                let kind = if e.get("through_all") == Some(&json!(true)) {
                    e.remove("through_all");
                    "through_all"
                } else if e.contains_key("distance2") {
                    "two_sided"
                } else if e.get("direction").and_then(Value::as_str) == Some("symmetric") {
                    "symmetric"
                } else {
                    "distance"
                };
                if kind == "symmetric" {
                    e.insert("direction".into(), json!("positive"));
                }
                e.insert("type".into(), json!(kind));
            }
        }
        if kind_text == "hole" {
            rename_map(&mut fields, "hole", "form");
            if let Some(Value::Object(mut p)) = fields.remove("points") {
                if let Some(s) = p.remove("sketch") {
                    fields.insert("sketch".into(), s);
                }
                if let Some(ids) = p.remove("ids") {
                    fields.insert("points".into(), ids);
                }
            }
            let extent = match fields.remove("depth") {
                Some(d) => json!({"type":"distance","depth":d}),
                None => json!({"type":"through_all"}),
            };
            fields.insert("extent".into(), extent);
        }
        if kind_text == "pattern" || kind_text == "mirror" {
            if let Some(features) = fields.get_mut("features").and_then(Value::as_array_mut) {
                for feature in features {
                    let feature_name = feature.as_str().ok_or_else(|| {
                        format!("feature {name}: a feature reference has no name")
                    })?;
                    let id = self.features.get(feature_name).ok_or_else(|| {
                        format!("feature {name}: a feature reference was not found")
                    })?;
                    let id = id.ok_or_else(|| {
                        if self.ambiguous.contains(feature_name) {
                            format!("feature {name}: a feature reference is ambiguous")
                        } else {
                            format!("feature {name}: a feature reference id is missing")
                        }
                    })?;
                    *feature = json!(id);
                }
            }
            rename_map(&mut fields, "pattern", "layout");
        }
        if let Some(sections) = fields.get_mut("sections").and_then(Value::as_array_mut) {
            for section in sections {
                rename(section, "profiles", "profile");
                if let Some(p) = section.get_mut("profile") {
                    profile_forward(p);
                }
            }
        }
        if matches!(kind_text.as_str(), "fillet" | "chamfer") {
            if let Some(edges) = fields.get_mut("edges").and_then(Value::as_array_mut) {
                for edge in edges {
                    if edge.is_array() {
                        *edge = json!({"at":edge,"proof":"unique_on_solidcraft_replay"});
                    }
                }
            }
        }
        // Only typed quantities are lifted; unrecognised options stay in extra untouched.
        for key in known {
            if let Some(mut value) = fields.remove(*key) {
                lift_quantities(&mut value, key, parameters);
                common.insert((*key).into(), value);
            }
        }
        if kind_text == "pattern" {
            if let Some(layout) = common.get("layout") {
                let mut instances = 1.0;
                for key in ["count", "count1", "count2"] {
                    if let Some(n) = layout
                        .get(key)
                        .and_then(|q| q.get("value"))
                        .and_then(Value::as_f64)
                    {
                        if !(1.0..=10_000.0).contains(&n) || n.fract() != 0.0 {
                            return Err(format!(
                                "feature {name}: pattern count exceeds the script limit"
                            ));
                        }
                        instances *= n;
                    }
                }
                if instances > 10_000.0 {
                    return Err(format!(
                        "feature {name}: pattern count exceeds the script limit"
                    ));
                }
            }
        }
        extra.extend(fields);
        common.insert("extra".into(), Value::Object(extra));
        serde_json::from_value(Value::Object(common))
            .map_err(|_| format!("feature {name}: its mapped fields are not supported yet"))
    }
}
fn get_id(index: &Value, values: &[Value]) -> Option<Value> {
    values
        .get(usize::try_from(index.as_u64()?).ok()?)?
        .get("id")
        .cloned()
}
fn index_to_id(v: &mut Value, key: &str, values: &[Value]) -> Result<(), ()> {
    if let Some(i) = v.get_mut(key) {
        if i.as_str() == Some("origin") {
            return Ok(());
        }
        *i = get_id(i, values).ok_or(())?;
    }
    Ok(())
}
fn rename_map(map: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(v) = map.remove(from) {
        map.insert(to.into(), v);
    }
}
fn rename(value: &mut Value, from: &str, to: &str) {
    if let Some(map) = value.as_object_mut() {
        rename_map(map, from, to);
    }
}
fn plane_forward(v: &mut Value) -> Result<(), ()> {
    if let Some(base) = v.get_mut("base") {
        plane_forward(base)?;
    }
    if v.get("type").and_then(Value::as_str) == Some("custom") {
        if v.as_object()
            .is_some_and(|m| m.keys().any(|k| !matches!(k.as_str(), "type" | "plane")))
        {
            return Err(());
        }
        if let Some(mut frame) = v.get("plane").cloned() {
            if let Some(m) = frame.as_object_mut() {
                m.insert("type".into(), json!("frame"));
            }
            *v = frame;
        }
    }
    Ok(())
}
fn profile_forward(v: &mut Value) {
    if let Some(m) = v.as_object_mut() {
        match m.get("type").and_then(Value::as_str) {
            Some("curves") => {
                m.insert("type".into(), json!("loops"));
            }
            Some("whole_boundaries") => {
                m.insert("type".into(), json!("regions"));
                rename_map(m, "profiles", "regions");
            }
            _ => {}
        }
    }
}
fn is_quantity(key: &str) -> bool {
    matches!(
        key,
        "distance"
            | "distance2"
            | "depth"
            | "angle"
            | "diameter"
            | "radius"
            | "thickness"
            | "pitch"
            | "turns"
            | "major"
            | "minor"
            | "section_size"
            | "start_angle"
            | "tip_angle"
            | "cb_diameter"
            | "cb_depth"
            | "cs_diameter"
            | "cs_angle"
            | "count"
            | "count1"
            | "count2"
            | "spacing1"
            | "spacing2"
    )
}
fn lift_quantities(v: &mut Value, key: &str, parameters: &BTreeMap<String, String>) {
    if is_quantity(key) {
        if let Some(s) = v.as_str() {
            *v = json!(Quantity::from_row(s, parameters));
        }
    } else if let Some(m) = v.as_object_mut() {
        for (k, v) in m {
            if is_quantity(k) || k == "base" {
                lift_quantities(v, k, parameters);
            }
        }
    }
}

impl Step {
    /// Restore the mapper's row vocabulary and array references.
    pub fn to_row(&self, plan: &Plan) -> Result<Value, String> {
        let fail = || format!("feature {}: a referenced id was not found", self.name);
        let mut value = serde_json::to_value(self).map_err(|_| fail())?;
        let Some(m) = value.as_object_mut() else {
            return Err(fail());
        };
        m.remove("source");
        m.remove("kind");
        m.remove("extra");
        m.insert("type".into(), json!(self.kind));
        m.insert("suppressed".into(), json!(false));
        if self.kind == "sketch" {
            let mut points = m.remove("points").unwrap_or(json!([]));
            let mut curves = m.remove("curves").unwrap_or(json!([]));
            let mut constraints = m.remove("constraints").unwrap_or(json!([]));
            let point_ids = points.as_array().cloned().unwrap_or_default();
            let curve_ids = curves.as_array().cloned().unwrap_or_default();
            if let Some(ps) = points.as_array_mut() {
                for p in ps {
                    rename(p, "at", "pos");
                }
            }
            if let Some(cs) = curves.as_array_mut() {
                for c in cs {
                    for key in ["a", "b", "c", "m", "apex"] {
                        id_to_index(c, key, &point_ids).map_err(|_| fail())?;
                    }
                    if let Some(ids) = c.get_mut("pts").and_then(Value::as_array_mut) {
                        for id in ids {
                            *id = find_index(id, &point_ids).ok_or_else(fail)?;
                        }
                    }
                }
            }
            if let Some(cs) = constraints.as_array_mut() {
                for c in cs {
                    for key in ["p", "q", "center"] {
                        id_to_index(c, key, &point_ids).map_err(|_| fail())?;
                    }
                    for key in ["line", "a", "b", "c"] {
                        id_to_index(c, key, &curve_ids).map_err(|_| fail())?;
                    }
                    rename(c, "line", "l");
                    if let Some(q) = c.get("value").cloned() {
                        if let Some(p) = q.get("param").filter(|p| !p.is_null()) {
                            if let Some(m) = c.as_object_mut() {
                                m.insert("param".into(), p.clone());
                            }
                        }
                        let text = q.get("expr").and_then(Value::as_str).unwrap_or("");
                        let numeric = if matches!(
                            c.get("type").and_then(Value::as_str),
                            Some("angle" | "line_angle" | "arc_sweep")
                        ) {
                            text.strip_suffix(" rad").unwrap_or(text)
                        } else {
                            text
                        };
                        if let Some(m) = c.as_object_mut() {
                            let restored = if m.remove("value_is_text") == Some(json!(true)) {
                                json!(numeric)
                            } else {
                                serde_json::from_str(numeric).unwrap_or(json!(text))
                            };
                            m.insert("value".into(), restored);
                        }
                    }
                }
            }
            m.insert(
                "sketch".into(),
                json!({"points":points,"curves":curves,"constraints":constraints}),
            );
        }
        if let Some(p) = m.get_mut("plane") {
            plane_back(p);
        }
        if let Some(p) = m.get_mut("profile") {
            profile_back(p);
        }
        rename_map(m, "coil_axis", "axis");
        rename_map(m, "profile", "profiles");
        rename_map(m, "bodies", "body_names");
        if self.kind == "extrude" {
            if let Some(e) = m.get_mut("extent").and_then(Value::as_object_mut) {
                let kind = e.remove("type");
                match kind.as_ref().and_then(Value::as_str) {
                    Some("symmetric") => {
                        e.insert("direction".into(), json!("symmetric"));
                    }
                    Some("through_all") => {
                        e.insert("through_all".into(), json!(true));
                    }
                    _ => {}
                }
            }
        }
        if self.kind == "hole" {
            rename_map(m, "form", "hole");
            if let Some(points) = m.remove("points") {
                let sketch = m.remove("sketch").unwrap_or(Value::Null);
                m.insert("points".into(), json!({"sketch":sketch,"ids":points}));
            }
            if let Some(mut e) = m.remove("extent").and_then(|e| e.as_object().cloned()) {
                if let Some(d) = e.remove("depth") {
                    m.insert("depth".into(), d);
                }
            }
        }
        if matches!(self.kind.as_str(), "pattern" | "mirror") {
            if let Some(fs) = m.get_mut("features").and_then(Value::as_array_mut) {
                for f in fs {
                    if let Some(step) = plan.steps.iter().find(|s| Some(s.id) == f.as_u64()) {
                        *f = json!(step.name);
                    } else {
                        return Err(fail());
                    }
                }
            }
            rename_map(m, "layout", "pattern");
        }
        if let Some(sections) = m.get_mut("sections").and_then(Value::as_array_mut) {
            for s in sections {
                if let Some(p) = s.get_mut("profile") {
                    profile_back(p);
                }
                rename(s, "profile", "profiles");
            }
        }
        if matches!(self.kind.as_str(), "fillet" | "chamfer") {
            if let Some(es) = m.get_mut("edges").and_then(Value::as_array_mut) {
                for e in es {
                    *e = e.get("at").cloned().unwrap_or(Value::Null);
                }
            }
        }
        for (k, v) in m.iter_mut() {
            lower_quantity_field(v, k);
        }
        for (k, v) in &self.extra {
            if let Some(current) = m.get_mut(k) {
                merge_extra(current, v);
            } else {
                m.insert(k.clone(), v.clone());
            }
        }
        Ok(value)
    }
}
fn find_index(id: &Value, values: &[Value]) -> Option<Value> {
    values
        .iter()
        .position(|v| v.get("id") == Some(id))
        .map(|i| json!(i))
}
fn id_to_index(v: &mut Value, key: &str, values: &[Value]) -> Result<(), ()> {
    if let Some(id) = v.get_mut(key) {
        if id.as_str() == Some("origin") && !values.iter().any(|v| v.get("id") == Some(id)) {
            return Ok(());
        }
        *id = find_index(id, values).ok_or(())?;
    }
    Ok(())
}
fn plane_back(v: &mut Value) {
    if let Some(base) = v.get_mut("base") {
        plane_back(base);
    }
    if v.get("type").and_then(Value::as_str) == Some("frame") {
        if let Some(m) = v.as_object_mut() {
            m.remove("type");
        }
        *v = json!({"type":"custom","plane":v});
    }
}
fn profile_back(v: &mut Value) {
    if let Some(m) = v.as_object_mut() {
        match m.get("type").and_then(Value::as_str) {
            Some("loops") => {
                m.insert("type".into(), json!("curves"));
            }
            Some("regions") => {
                m.insert("type".into(), json!("whole_boundaries"));
                rename_map(m, "regions", "profiles");
            }
            _ => {}
        }
    }
}
fn lower_quantity_field(v: &mut Value, key: &str) {
    if is_quantity(key) {
        if let Some(expr) = v.get("expr").and_then(Value::as_str) {
            *v = json!(expr);
        }
    } else if matches!(key, "extent" | "hole" | "pattern" | "plane" | "base") {
        if let Some(m) = v.as_object_mut() {
            for (k, v) in m {
                if is_quantity(k) || k == "base" {
                    lower_quantity_field(v, k);
                }
            }
        }
    }
}

fn validate_row(row: &Value) -> Result<(), String> {
    let mut pending = vec![(row, 0usize)];
    while let Some((value, depth)) = pending.pop() {
        if depth > 64 {
            return Err("replay data exceeds the nesting limit".into());
        }
        match value {
            Value::Array(a) => {
                if a.len() > 100_000 {
                    return Err("replay data exceeds the script limit".into());
                }
                pending.extend(a.iter().map(|v| (v, depth + 1)));
            }
            Value::Object(o) => {
                if o.len() > 100_000 {
                    return Err("replay data exceeds the script limit".into());
                }
                pending.extend(o.values().map(|v| (v, depth + 1)));
            }
            Value::String(s) => validate_text_size(s.len() as u64)?,
            _ => {}
        }
    }
    Ok(())
}

fn merge_extra(target: &mut Value, extra: &Value) {
    match (target, extra) {
        (Value::Object(target), Value::Object(extra)) => {
            for (key, value) in extra {
                if let Some(current) = target.get_mut(key) {
                    merge_extra(current, value);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        (target, extra) => *target = extra.clone(),
    }
}

fn validate_text_size(bytes: u64) -> Result<(), String> {
    if bytes > dutchman_core::MAX_INPUT_BYTES {
        return Err("replay text exceeds the input size limit".into());
    }
    Ok(())
}

#[cfg(test)]
mod limits {
    #[test]
    fn row_text_uses_the_project_input_limit() {
        assert!(super::validate_text_size(65_537).is_ok());
        assert!(super::validate_text_size(dutchman_core::MAX_INPUT_BYTES).is_ok());
        assert!(super::validate_text_size(dutchman_core::MAX_INPUT_BYTES + 1).is_err());
    }
}
