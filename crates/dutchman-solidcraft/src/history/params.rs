//! Design parameters, as SolidCraft parameters.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::{DesignParameter, ParameterValue};
use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

/// Units SolidCraft's expressions accept.
const UNITS: [&str; 14] = [
    "mm", "cm", "m", "um", "µm", "in", "inch", "ft", "yd", "km", "mil", "nm", "deg", "rad",
];

/// Functions SolidCraft's expressions accept.
const FUNCTIONS: [&str; 23] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sqrt", "abs", "min", "max", "floor",
    "ceil", "round", "exp", "ln", "log", "log10", "sign", "hypot", "pow", "trunc", "mod",
];

/// Constants SolidCraft's expressions accept.
const CONSTANTS: [&str; 2] = ["pi", "PI"];

/// The model's parameters as SolidCraft `params`, in source order, and the SolidCraft name of
/// each one written, by CADIR parameter id. A parameter renamed (a name SolidCraft cannot use,
/// or one another parameter already has, as when merged designs each have a `d1`) is renamed in
/// the expressions that depend on it. An expression SolidCraft could not read the same way, or
/// that names a parameter it cannot tell apart from another of that name, is replaced by the
/// parameter's evaluated value.
pub(crate) fn map(
    parameters: &[DesignParameter],
    report: &mut Report,
) -> (Vec<Value>, BTreeMap<String, String>) {
    let mut ordered: Vec<&DesignParameter> = parameters.iter().collect();
    ordered.sort_by_key(|p| (p.owner.is_some(), p.ordinal));

    let mut names = BTreeSet::new();
    let mut renamed = Vec::with_capacity(ordered.len());
    for parameter in &ordered {
        let name = usable_name(&parameter.name, &names);
        if name != parameter.name {
            let (severity, why) =
                if usable_name(&parameter.name, &BTreeSet::new()) == parameter.name {
                    (Severity::Info, "another parameter already has that name")
                } else {
                    (Severity::Degraded, "SolidCraft cannot use that name")
                };
            report.push(
                severity,
                format!("parameter {}", parameter.name),
                format!("renamed to {name}: {why}"),
            );
        }
        names.insert(name.clone());
        renamed.push(name);
    }
    // Each parameter's written name by id, and by source name when only one parameter has it.
    let by_id: BTreeMap<&str, (&str, &str)> = ordered
        .iter()
        .zip(&renamed)
        .map(|(p, name)| (p.id.as_str(), (p.name.as_str(), name.as_str())))
        .collect();
    let mut by_name: BTreeMap<&str, Option<&str>> = BTreeMap::new();
    for (parameter, name) in ordered.iter().zip(&renamed) {
        by_name
            .entry(parameter.name.as_str())
            .and_modify(|only| *only = None)
            .or_insert(Some(name.as_str()));
    }

    let mut out = Vec::with_capacity(ordered.len());
    let mut written = BTreeMap::new();
    for (parameter, name) in ordered.iter().zip(&renamed) {
        let unit = unit(parameter.value.as_ref());
        let bound = bind(parameter, &by_id, &by_name);
        let expression = bound
            .as_ref()
            .ok()
            .filter(|expression| readable(expression, &names));
        let expr = if let Some(expression) = expression {
            expression.clone()
        } else if let Some(value) = parameter.value.as_ref().and_then(literal) {
            let why = match &bound {
                Err(shared) => format!(": more than one parameter is named {shared}"),
                Ok(_) => String::new(),
            };
            report.push(
                Severity::Degraded,
                format!("parameter {name}"),
                format!(
                    "expression `{}` kept as its value {value}{why}",
                    parameter.expression
                ),
            );
            value
        } else {
            report.push(
                Severity::Dropped,
                format!("parameter {name}"),
                format!(
                    "expression `{}` has no value SolidCraft can hold",
                    parameter.expression
                ),
            );
            continue;
        };
        written.insert(parameter.id.as_str().to_owned(), name.clone());
        out.push(json!({
            "name": name,
            "expr": expr,
            "unit": unit,
            "model": parameter.owner.is_some(),
        }));
    }
    (out, written)
}

/// Axis dimensions in SolidCraft measure magnitudes. Keep a signed source parameter
/// unchanged for its other uses, and drive the magnitude with its negation on the
/// negative branch. The solved points retain which side of the axis they occupy.
pub(super) fn unsigned_dimensions(history: &mut super::History, ctx: &super::Context<'_>) {
    let negative: BTreeSet<&str> = ctx
        .arena("parameters")
        .iter()
        .filter(|row| {
            row.pointer("/value/kind").and_then(Value::as_str) == Some("length")
                && row
                    .pointer("/value/value")
                    .and_then(Value::as_f64)
                    .is_some_and(|value| value < 0.0)
        })
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .filter_map(|id| ctx.params.get(id).map(String::as_str))
        .collect();
    let mut names: BTreeSet<String> = history
        .params
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let mut magnitudes = BTreeMap::new();
    for feature in &mut history.features {
        let Some(constraints) = feature
            .pointer_mut("/sketch/constraints")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for constraint in constraints {
            if !matches!(
                constraint.get("type").and_then(Value::as_str),
                Some("distance_x" | "distance_y")
            ) {
                continue;
            }
            let Some(source) = constraint.get("param").and_then(Value::as_str) else {
                continue;
            };
            if !negative.contains(source) {
                continue;
            }
            let magnitude = magnitudes.entry(source.to_owned()).or_insert_with(|| {
                let name = usable_name(&format!("{source}_magnitude"), &names);
                names.insert(name.clone());
                history.params.push(json!({
                    "name": name,
                    "expr": format!("-({source})"),
                    "unit": "mm",
                    "model": true,
                }));
                name
            });
            if let Some(fields) = constraint.as_object_mut() {
                fields.insert("param".to_owned(), json!(magnitude));
            }
        }
    }
}

/// `parameter`'s expression with each parameter it names written as that parameter is
/// written: its own dependencies first (`by_id`), then any parameter that alone has the name
/// (`by_name`). `Err` with the name when the expression names a parameter that is not a
/// dependency and shares its name with another.
fn bind(
    parameter: &DesignParameter,
    by_id: &BTreeMap<&str, (&str, &str)>,
    by_name: &BTreeMap<&str, Option<&str>>,
) -> Result<String, String> {
    let mut renamed: BTreeMap<String, String> = parameter
        .dependencies
        .iter()
        .filter_map(|id| by_id.get(id.as_str()))
        .map(|(source, name)| ((*source).to_owned(), (*name).to_owned()))
        .collect();
    let words = parameter
        .expression
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'));
    for word in words {
        if renamed.contains_key(word) || reserved(word) {
            continue;
        }
        match by_name.get(word) {
            Some(Some(name)) => {
                renamed.insert(word.to_owned(), (*name).to_owned());
            }
            Some(None) => return Err(word.to_owned()),
            None => {}
        }
    }
    Ok(rename_in(&parameter.expression, &renamed))
}

/// `expression` with each parameter name in `renamed` replaced by its new name; numbers, units,
/// functions and everything else are kept as written.
pub(crate) fn rename_in(expression: &str, renamed: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(expression.len());
    let mut word = String::new();
    let mut in_number = false;
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(
                renamed
                    .get(word.as_str())
                    .map_or(word.as_str(), String::as_str),
            );
            word.clear();
        }
    };
    for c in expression.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            if word.is_empty() && (c.is_ascii_digit() || in_number) {
                // A number, or a unit or exponent written against it (`10mm`, `1e-3`).
                in_number = true;
                out.push(c);
                continue;
            }
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            in_number = c == '.' && in_number;
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// The parameters of `params` (as [`map`] writes them) named in `used`, with every parameter
/// their expressions name in turn, in their order in `params`.
pub(crate) fn needed(params: Vec<Value>, mut used: BTreeSet<String>) -> Vec<Value> {
    let name = |param: &Value| param.get("name").and_then(Value::as_str).map(str::to_owned);
    let mut open: Vec<String> = used.iter().cloned().collect();
    while let Some(next) = open.pop() {
        let Some(expr) = params
            .iter()
            .find(|param| name(param).as_deref() == Some(next.as_str()))
            .and_then(|param| param.get("expr"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let words = expr
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| word.chars().next().is_some_and(|c| !c.is_ascii_digit()));
        for word in words {
            if used.insert(word.to_owned()) {
                open.push(word.to_owned());
            }
        }
    }
    params
        .into_iter()
        .filter(|param| name(param).is_some_and(|name| used.contains(&name)))
        .collect()
}

/// SolidCraft's unit for a parameter holding `value`.
fn unit(value: Option<&ParameterValue>) -> &'static str {
    match value {
        Some(ParameterValue::Length(_)) => "mm",
        Some(ParameterValue::Angle(_)) => "deg",
        _ => "",
    }
}

/// The value as a SolidCraft expression: lengths in millimetres, angles in degrees.
fn literal(value: &ParameterValue) -> Option<String> {
    match value {
        ParameterValue::Length(length) => Some(format!("{} mm", length.get())),
        ParameterValue::Angle(angle) => Some(format!("{} deg", angle.get().to_degrees())),
        ParameterValue::Real(real) => Some(format!("{}", real.get())),
        ParameterValue::Integer(integer) => Some(integer.to_string()),
        ParameterValue::Boolean(_) | ParameterValue::String(_) => None,
    }
}

/// `name` if SolidCraft can use it as a parameter name and it is not taken, else a usable
/// variant of it.
fn usable_name(name: &str, taken: &BTreeSet<String>) -> String {
    let mut base: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) || reserved(&base) {
        base = format!("p_{base}");
    }
    let mut candidate = base.clone();
    let mut n = 2u32;
    while taken.contains(&candidate) {
        candidate = format!("{base}_{n}");
        n = n.saturating_add(1);
    }
    candidate
}

/// A word SolidCraft's expressions already give a meaning.
fn reserved(word: &str) -> bool {
    UNITS.contains(&word) || FUNCTIONS.contains(&word) || CONSTANTS.contains(&word)
}

/// Whether SolidCraft reads `expression` as the source meant it: numbers, the arithmetic
/// operators, parentheses, and words that are units, functions, constants or `names`.
fn readable(expression: &str, names: &BTreeSet<String>) -> bool {
    let chars: Vec<char> = expression.chars().collect();
    let mut depth = 0usize;
    let mut i = 0usize;
    let mut tokens = 0usize;
    while let Some(&c) = chars.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            while chars
                .get(i)
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                i += 1;
            }
            if chars.get(i).is_some_and(|c| *c == 'e' || *c == 'E') {
                let sign = chars.get(i + 1).is_some_and(|c| *c == '-' || *c == '+');
                let first = if sign { i + 2 } else { i + 1 };
                if chars.get(first).is_some_and(char::is_ascii_digit) {
                    i = first;
                    while chars.get(i).is_some_and(char::is_ascii_digit) {
                        i += 1;
                    }
                }
            }
            tokens += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while chars
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                i += 1;
            }
            let word: String = chars.get(start..i).unwrap_or_default().iter().collect();
            let known = UNITS.contains(&word.as_str())
                || CONSTANTS.contains(&word.as_str())
                || names.contains(&word)
                || (FUNCTIONS.contains(&word.as_str()) && next_is_paren(&chars, i));
            if !known {
                return false;
            }
            tokens += 1;
        } else if "+-*/^,".contains(c) {
            i += 1;
        } else if c == '(' {
            depth += 1;
            i += 1;
        } else if c == ')' {
            let Some(d) = depth.checked_sub(1) else {
                return false;
            };
            depth = d;
            i += 1;
        } else {
            return false;
        }
    }
    depth == 0 && tokens > 0
}

fn next_is_paren(chars: &[char], mut i: usize) -> bool {
    while chars.get(i).is_some_and(|c| c.is_whitespace()) {
        i += 1;
    }
    chars.get(i) == Some(&'(')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn reads_plain_and_parametric_expressions() {
        let known = names(&["width", "d1"]);
        for ok in [
            "10 mm",
            "width / 2",
            "2 * d1 + 5 mm",
            "sqrt(width^2 + 1)",
            "30 deg",
            "1.5e-3 m",
            "pi * 2",
        ] {
            assert!(readable(ok, &known), "{ok}");
        }
    }

    #[test]
    fn refuses_what_solidcraft_would_misread() {
        let known = names(&["width"]);
        for bad in [
            "",
            "height / 2",
            "width / (2",
            "width)",
            "if(width > 2; 1; 2)",
            "sqrt",
            "10 mm # note",
        ] {
            assert!(!readable(bad, &known), "{bad}");
        }
    }

    #[test]
    fn renaming_touches_parameter_names_only() {
        let renamed: BTreeMap<String, String> = [
            ("d1".to_owned(), "Block_d1".to_owned()),
            ("w".to_owned(), "Block_w".to_owned()),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            rename_in("2 * d1 + w / 2", &renamed),
            "2 * Block_d1 + Block_w / 2"
        );
        assert_eq!(
            rename_in("sqrt(d1^2) + 10mm + 1e-3 m", &renamed),
            "sqrt(Block_d1^2) + 10mm + 1e-3 m"
        );
        assert_eq!(rename_in("d10 + xd1", &renamed), "d10 + xd1");
    }

    #[test]
    fn names_are_made_usable_and_unique() {
        let taken = names(&["width"]);
        assert_eq!(usable_name("d1", &taken), "d1");
        assert_eq!(usable_name("width", &taken), "width_2");
        assert_eq!(usable_name("2nd", &taken), "p_2nd");
        assert_eq!(usable_name("mm", &taken), "p_mm");
        assert_eq!(usable_name("hole Ø", &taken), "hole__");
    }
}

#[cfg(test)]
mod map_tests {
    use serde_json::json;

    use super::*;
    use crate::testing::{cube, with};

    fn parameter(key: u32, name: &str, expression: &str, value: Value, owned: bool) -> Value {
        let mut row = json!({
            "id": format!("f3d:model:parameter#{key}"),
            "ordinal": key,
            "name": name,
            "expression": expression,
            "value": value,
        });
        if owned {
            row["owner"] = json!("f3d:model:feature#1");
        }
        row
    }

    #[test]
    fn parameters_keep_readable_expressions_and_fall_back_to_values() {
        let features = json!([crate::testing::feature(
            1,
            0,
            "XY",
            json!({"definition": "datum_principal_plane", "plane": "top"})
        )]);
        let parameters = json!([
            parameter(
                1,
                "width",
                "40 mm",
                json!({"kind": "length", "value": 40.0}),
                false
            ),
            parameter(
                2,
                "d1",
                "width / 2",
                json!({"kind": "length", "value": 20.0}),
                true
            ),
            parameter(
                3,
                "d2",
                "iff(width > 1; 2; 3)",
                json!({"kind": "length", "value": 2.0}),
                true
            ),
            parameter(
                4,
                "a1",
                "tilt",
                json!({"kind": "angle", "value": std::f64::consts::FRAC_PI_2}),
                true
            ),
        ]);
        let model = with(cube(), features, parameters);
        let mut report = Report::default();
        let (params, names) = map(&model.ir.model.parameters, &mut report);
        assert_eq!(
            names.get("f3d:model:parameter#2").map(String::as_str),
            Some("d1")
        );
        assert_eq!(
            params,
            [
                json!({"name": "width", "expr": "40 mm", "unit": "mm", "model": false}),
                json!({"name": "d1", "expr": "width / 2", "unit": "mm", "model": true}),
                json!({"name": "d2", "expr": "2 mm", "unit": "mm", "model": true}),
                json!({"name": "a1", "expr": "90 deg", "unit": "deg", "model": true}),
            ]
        );
        assert_eq!(report.losses().len(), 2, "{:?}", report.losses());
    }

    fn depending(row: Value, on: &[u32]) -> Value {
        let mut row = row;
        row["dependencies"] = json!(on
            .iter()
            .map(|key| format!("f3d:model:parameter#{key}"))
            .collect::<Vec<_>>());
        row
    }

    #[test]
    fn merged_designs_sharing_names_keep_their_own_references() {
        let length = |v: f64| json!({"kind": "length", "value": v});
        // Two designs merged into one, each with its own d1 and a d2 that depends on it.
        let parameters = json!([
            parameter(1, "d1", "10 mm", length(10.0), false),
            depending(parameter(2, "d2", "d1 * 2", length(20.0), false), &[1]),
            parameter(3, "d1", "7 mm", length(7.0), false),
            depending(parameter(4, "d2", "d1 * 2", length(14.0), false), &[3]),
            // Naming a shared name without saying which is ambiguous.
            parameter(5, "d9", "d1 + 1 mm", length(11.0), false),
        ]);
        let model = with(cube(), json!([]), parameters);
        let mut report = Report::default();
        let (params, names) = map(&model.ir.model.parameters, &mut report);
        let expr = |name: &str| {
            params
                .iter()
                .find(|p| p["name"] == name)
                .and_then(|p| p["expr"].as_str())
                .map(str::to_owned)
        };
        assert_eq!(expr("d2").as_deref(), Some("d1 * 2"));
        assert_eq!(expr("d2_2").as_deref(), Some("d1_2 * 2"));
        assert_eq!(expr("d9").as_deref(), Some("11 mm"));
        assert_eq!(
            names.get("f3d:model:parameter#3").map(String::as_str),
            Some("d1_2")
        );
        let clash = report
            .losses()
            .iter()
            .find(|loss| loss.subject == "parameter d1")
            .unwrap();
        assert_eq!(clash.severity, Severity::Info);
        assert_eq!(
            clash.detail,
            "renamed to d1_2: another parameter already has that name"
        );
        let shared = report
            .losses()
            .iter()
            .find(|loss| loss.subject == "parameter d9")
            .unwrap();
        assert_eq!(
            shared.detail,
            "expression `d1 + 1 mm` kept as its value 11 mm: more than one parameter is named d1"
        );
    }

    #[test]
    fn a_name_solidcraft_cannot_use_is_renamed_where_it_is_used() {
        let length = |v: f64| json!({"kind": "length", "value": v});
        let parameters = json!([
            parameter(1, "pi", "5 mm", length(5.0), false),
            depending(parameter(2, "w", "pi * 3", length(15.0), false), &[1]),
        ]);
        let model = with(cube(), json!([]), parameters);
        let mut report = Report::default();
        let (params, _) = map(&model.ir.model.parameters, &mut report);
        assert_eq!(params[0]["name"], "p_pi");
        assert_eq!(params[1]["expr"], "p_pi * 3");
        let renamed = &report.losses()[0];
        assert_eq!(renamed.severity, Severity::Degraded);
        assert_eq!(
            renamed.detail,
            "renamed to p_pi: SolidCraft cannot use that name"
        );
    }

    #[test]
    fn signed_dimensions_share_a_magnitude_without_renaming_source_parameters() {
        let wire =
            json!({"parameters": [{"id": "negative", "value": {"kind": "length", "value": -3.0}}]});
        let mut ctx = super::super::Context::new(&wire);
        ctx.params
            .insert("negative".to_owned(), "offset".to_owned());
        let mut history = super::super::History {
            params: vec![
                json!({"name": "offset", "expr": "-width", "unit": "mm", "model": true}),
                json!({"name": "width", "expr": "3 mm", "unit": "mm", "model": false}),
                json!({"name": "offset_magnitude", "expr": "7 mm", "unit": "mm", "model": false}),
            ],
            features: vec![json!({"sketch": {"constraints": [
                {"type": "distance_x", "p": 1, "q": 0, "value": 3.0, "param": "offset"},
                {"type": "distance_y", "p": 2, "q": 0, "value": 3.0, "param": "offset", "driven": true},
                {"type": "length", "l": 0, "value": 3.0, "param": "offset"},
                {"type": "distance_x", "p": 0, "q": 3, "value": 3.0, "param": "width"}
            ]}})],
            ..Default::default()
        };
        unsigned_dimensions(&mut history, &ctx);
        assert_eq!(history.params.len(), 4);
        assert_eq!(history.params[0]["expr"], "-width");
        assert_eq!(history.params[3]["name"], "offset_magnitude_2");
        assert_eq!(history.params[3]["expr"], "-(offset)");
        let constraints = &history.features[0]["sketch"]["constraints"];
        assert_eq!(constraints[0]["param"], "offset_magnitude_2");
        assert_eq!(constraints[1]["param"], "offset_magnitude_2");
        assert_eq!(constraints[1]["driven"], true);
        assert_eq!(constraints[0]["p"], 1);
        assert_eq!(constraints[0]["q"], 0);
        assert_eq!(constraints[2]["param"], "offset");
        assert_eq!(constraints[3]["param"], "width");
        let kept = needed(
            history.params,
            BTreeSet::from(["offset_magnitude_2".to_owned()]),
        );
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0]["name"], "offset");
        assert_eq!(kept[1]["name"], "width");
    }
}
