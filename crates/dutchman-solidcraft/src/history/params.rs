//! Design parameters, as SolidCraft parameters.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::{DesignParameter, ParameterValue};
use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

mod expression;
use expression::validate;

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

const MAX_EXPRESSION_BYTES: usize = 65_536;

/// The model's parameters as SolidCraft `params`, in source order, and the SolidCraft name of
/// each one written, by CADIR parameter id. A parameter renamed (a name SolidCraft cannot use,
/// or one another parameter already has, as when merged designs each have a `d1`) is renamed in
/// the expressions that depend on it. An expression SolidCraft could not read the same way, or
/// that names a parameter it cannot tell apart from another of that name, is replaced by the
/// parameter's evaluated value.
pub(crate) fn map(
    model: &dutchman_core::Model,
    report: &mut Report,
) -> (Vec<Value>, BTreeMap<String, String>) {
    let parameters = &model.ir.model.parameters;
    let owners: BTreeMap<&str, &str> = model
        .ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.id.as_str(), feature.name.as_deref()?)))
        .collect();
    let cells: BTreeMap<&str, String> = model
        .ir
        .model
        .spreadsheets
        .iter()
        .flat_map(|sheet| sheet.cells())
        .map(|cell| (cell.parameter.as_str(), cell.address.a1()))
        .collect();
    let mut ordered: Vec<&DesignParameter> = parameters.iter().collect();
    ordered.sort_by_key(|p| (p.owner.is_some(), p.ordinal));
    // Text and empty spreadsheet cells are layout, not lost design parameters.
    ordered.retain(|p| {
        !cells.contains_key(p.id.as_str())
            || !matches!(
                p.value,
                Some(ParameterValue::String(_)) | Some(ParameterValue::Boolean(_))
            ) && (p
                .expression
                .trim_start()
                .strip_prefix('=')
                .is_some_and(|expr| !expr.trim_start().starts_with(['\'', '"']))
                || p.value.as_ref().and_then(literal).is_some())
    });

    let mut names = BTreeSet::new();
    let mut renamed = Vec::with_capacity(ordered.len());
    for parameter in &ordered {
        let source = cells
            .get(parameter.id.as_str())
            .filter(|address| *address == &parameter.name || parameter.name.trim().is_empty())
            .map(|address| {
                parameter
                    .owner
                    .as_ref()
                    .and_then(|id| owners.get(id.as_str()))
                    .map_or_else(|| address.clone(), |owner| format!("{owner}.{address}"))
            })
            .unwrap_or_else(|| parameter.name.clone());
        let name = usable_name(&source, &names);
        report.saw(&name);
        if name != source || source != parameter.name {
            let (severity, why) = if source != parameter.name {
                (Severity::Info, "spreadsheet cell name includes its sheet")
            } else if usable_name(&source, &BTreeSet::new()) == source {
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
    let bindings = Bindings::new(&ordered, &renamed, &owners, &cells);
    let mut expressions: Vec<Result<String, String>> = ordered
        .iter()
        .map(|parameter| bindings.bind(parameter))
        .collect();
    let references: Vec<BTreeSet<String>> =
        ordered.iter().map(|p| bindings.references(p)).collect();
    let used: BTreeSet<&str> = references.iter().flatten().map(String::as_str).collect();
    let units = settle(&ordered, &renamed, &references, &mut expressions);
    let mut out = Vec::with_capacity(ordered.len());
    let mut written = BTreeMap::new();
    for ((parameter, name), expression) in ordered.iter().zip(&renamed).zip(expressions) {
        let expr = match expression {
            Ok(expression) => expression,
            Err(why) => {
                if let Some(value) = parameter.value.as_ref().and_then(literal) {
                    report.push(
                        Severity::Degraded,
                        format!("parameter {name}"),
                        format!(
                            "expression `{}` kept as its value {value}: {why}",
                            parameter.expression
                        ),
                    );
                    value
                } else {
                    report.push(
                        Severity::Dropped,
                        format!("parameter {name}"),
                        format!(
                            "expression `{}` has no value SolidCraft can hold: {why}",
                            parameter.expression
                        ),
                    );
                    continue;
                }
            }
        };
        if cells.contains_key(parameter.id.as_str()) && !used.contains(parameter.id.as_str()) {
            report.push(
                Severity::Info,
                format!("parameter {name}"),
                "numeric spreadsheet cell kept as an unused parameter",
            );
        }
        written.insert(parameter.id.as_str().to_owned(), name.clone());
        out.push(json!({"name": name, "expr": expr,
            "unit": units.get(name).map_or("", String::as_str),
            "model": parameter.owner.is_some() && !cells.contains_key(parameter.id.as_str()),
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

/// Remove cycles and propagate unavailable formula-only parameters through their users.
fn settle(
    parameters: &[&DesignParameter],
    names: &[String],
    references: &[BTreeSet<String>],
    expressions: &mut [Result<String, String>],
) -> BTreeMap<String, String> {
    let known: BTreeSet<String> = names.iter().cloned().collect();
    let mut units: BTreeMap<String, String> = parameters
        .iter()
        .zip(names)
        .filter(|(p, _)| p.value.as_ref().and_then(literal).is_some())
        .map(|(p, name)| (name.clone(), unit(p.value.as_ref()).to_owned()))
        .collect();
    let positions: BTreeMap<&str, usize> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();
    let mut waiting = BTreeMap::new();
    let mut users: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, ((p, refs), expr)) in parameters
        .iter()
        .zip(references)
        .zip(expressions.iter())
        .enumerate()
    {
        let refs = if expr.is_ok() {
            refs.clone()
        } else {
            BTreeSet::new()
        };
        waiting.insert(p.id.as_str(), refs.len());
        for id in refs {
            if let Some((source, _)) = positions.get_key_value(id.as_str()) {
                users.entry(source).or_default().push(i);
            }
        }
    }
    let mut ready: Vec<&str> = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    while let Some(id) = ready.pop() {
        if let Some(i) = positions.get(id) {
            if let (Some(p), Some(name), Some(expr), Some(refs)) = (
                parameters.get(*i),
                names.get(*i),
                expressions.get_mut(*i),
                references.get(*i),
            ) {
                if p.value.is_none() && expr.is_ok() {
                    let missing = refs.iter().find_map(|id| {
                        positions.get(id.as_str()).and_then(|i| {
                            names
                                .get(*i)
                                .filter(|name| !units.contains_key(*name))
                                .zip(parameters.get(*i))
                        })
                    });
                    let inferred = if let Some((_, missing)) = missing {
                        Err(format!(
                            "reference to parameter {} not converted",
                            missing.name
                        ))
                    } else {
                        expr.as_ref()
                            .map_err(Clone::clone)
                            .and_then(|expr| expression::infer_unit(expr, &known, &units))
                    };
                    match inferred {
                        Ok(unit) => {
                            units.insert(name.clone(), unit);
                        }
                        Err(why) => {
                            *expr = Err(why);
                        }
                    }
                }
            }
        }
        for i in users.get(id).into_iter().flatten() {
            if let Some(p) = parameters.get(*i) {
                if let Some(count) = waiting.get_mut(p.id.as_str()) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.push(p.id.as_str());
                    }
                }
            }
        }
    }
    for (p, expr) in parameters.iter().zip(expressions.iter_mut()) {
        if waiting.get(p.id.as_str()).is_some_and(|count| *count > 0) {
            *expr = Err("cyclic parameter dependency is not supported yet".to_owned());
        }
    }
    let mut unavailable: Vec<usize> = parameters
        .iter()
        .zip(expressions.iter())
        .enumerate()
        .filter(|(_, (p, expr))| expr.is_err() && p.value.as_ref().and_then(literal).is_none())
        .map(|(i, _)| i)
        .collect();
    while let Some(i) = unavailable.pop() {
        let Some(p) = parameters.get(i) else { continue };
        for user in users.get(p.id.as_str()).into_iter().flatten() {
            let Some(expr) = expressions.get_mut(*user).filter(|expr| expr.is_ok()) else {
                continue;
            };
            *expr = Err(format!("reference to parameter {} not converted", p.name));
            if parameters
                .get(*user)
                .is_some_and(|p| p.value.as_ref().and_then(literal).is_none())
            {
                unavailable.push(*user);
            }
        }
    }
    units
}

struct Bindings<'a> {
    parameters: BTreeMap<&'a str, (&'a DesignParameter, &'a str)>,
    aliases: BTreeMap<(Option<&'a str>, String), BTreeSet<&'a str>>,
    names: BTreeSet<String>,
    objects: BTreeSet<String>,
}

impl<'a> Bindings<'a> {
    fn new(
        parameters: &[&'a DesignParameter],
        names: &'a [String],
        owners: &BTreeMap<&str, &str>,
        cells: &BTreeMap<&str, String>,
    ) -> Self {
        let mut out = Self {
            parameters: BTreeMap::new(),
            aliases: BTreeMap::new(),
            names: names.iter().cloned().collect(),
            objects: owners.values().map(|name| (*name).to_owned()).collect(),
        };
        for (p, name) in parameters.iter().zip(names) {
            out.parameters.insert(p.id.as_str(), (p, name));
            let owner = p.owner.as_ref().map(|id| id.as_str());
            let mut aliases = BTreeSet::from([p.name.clone()]);
            for key in ["source_name", "reference_name"] {
                if let Some(source) = p.properties.get(key) {
                    aliases.insert(source.clone());
                }
            }
            if let Some(address) = cells.get(p.id.as_str()) {
                aliases.insert(address.clone());
            }
            for alias in aliases {
                let local = alias.trim_start_matches('.');
                out.aliases
                    .entry((owner, local.to_owned()))
                    .or_default()
                    .insert(p.id.as_str());
                out.aliases
                    .entry((None, alias.clone()))
                    .or_default()
                    .insert(p.id.as_str());
                if let Some(object) = owner.and_then(|id| owners.get(id)) {
                    out.aliases
                        .entry((None, format!("{object}.{local}")))
                        .or_default()
                        .insert(p.id.as_str());
                }
                if let Some(label) = p.properties.get("owner_label") {
                    out.objects.insert(format!("<<{label}>>"));
                    out.aliases
                        .entry((None, format!("<<{label}>>.{local}")))
                        .or_default()
                        .insert(p.id.as_str());
                }
            }
        }
        out
    }

    fn resolve(&self, p: &DesignParameter, word: &str) -> Result<(&'a str, &'a str), String> {
        let owner = p.owner.as_ref().map(|id| id.as_str());
        let local = self
            .aliases
            .get(&(owner, word.trim_start_matches('.').to_owned()));
        let global = self.aliases.get(&(None, word.to_owned()));
        let candidates = local.or(global);
        let Some(candidates) = candidates else {
            let separator = if word.starts_with("<<") {
                word.find(">>.").map(|i| i + 2)
            } else {
                word.find('.')
            };
            return Err(
                if let Some(object) = separator.and_then(|i| word.get(..i)) {
                    if object.is_empty() || self.objects.contains(object) {
                        format!("reference to property {word} has no converted numeric parameter")
                    } else {
                        format!("reference to object {object} not converted: property {word} has no parameter")
                    }
                } else {
                    format!("reference to parameter {word} not converted")
                },
            );
        };
        // An explicit decoded dependency identifies the source even when a local
        // property has the same name. Scope is the fallback when no id disambiguates it.
        let mut matching = p
            .dependencies
            .iter()
            .map(|id| id.as_str())
            .filter(|id| candidates.contains(id) || global.is_some_and(|all| all.contains(id)));
        let first = matching.next();
        let id = if matching.next().is_some() {
            None
        } else if first.is_some() {
            first
        } else if candidates.len() == 1 {
            candidates.first().copied()
        } else {
            None
        };
        let Some(id) = id else {
            return Err(format!("more than one parameter is named {word}"));
        };
        let (id, (_, name)) = self
            .parameters
            .get_key_value(id)
            .ok_or_else(|| format!("reference to parameter {word} not converted"))?;
        Ok((id, name))
    }

    fn references(&self, p: &DesignParameter) -> BTreeSet<String> {
        words(&p.expression)
            .into_iter()
            .filter(|(_, stop, word)| {
                !p.expression
                    .get(*stop..)
                    .is_some_and(|rest| rest.trim_start().starts_with('('))
                    && (!reserved(word)
                        || p.dependencies.iter().any(|id| {
                            self.parameters
                                .get(id.as_str())
                                .is_some_and(|(dependency, _)| dependency.name == *word)
                        }))
            })
            .filter_map(|(_, _, word)| self.resolve(p, word).ok().map(|(id, _)| id.to_owned()))
            .collect()
    }

    fn bind(&self, p: &DesignParameter) -> Result<String, String> {
        let expression = p
            .expression
            .trim()
            .strip_prefix('=')
            .unwrap_or(p.expression.trim());
        if expression.len() > MAX_EXPRESSION_BYTES {
            return Err("expression exceeds 65536 bytes".to_owned());
        }
        let append = |out: &mut String, text: &str| -> Result<(), String> {
            if text.len() > MAX_EXPRESSION_BYTES.saturating_sub(out.len()) {
                return Err("rewritten expression exceeds 65536 bytes".to_owned());
            }
            out.push_str(text);
            Ok(())
        };
        let mut out = String::with_capacity(expression.len());
        let mut end = 0;
        for (start, stop, word) in words(expression) {
            append(&mut out, expression.get(end..start).unwrap_or_default())?;
            let call = expression
                .get(stop..)
                .is_some_and(|rest| rest.trim_start().starts_with('('));
            if call {
                if !FUNCTIONS.contains(&word) {
                    return Err(format!("unsupported function {word}"));
                }
                append(&mut out, word)?;
            } else if reserved(word)
                && !p.dependencies.iter().any(|id| {
                    self.parameters
                        .get(id.as_str())
                        .is_some_and(|(dependency, _)| dependency.name == word)
                })
            {
                append(&mut out, word)?;
            } else {
                append(&mut out, self.resolve(p, word)?.1)?;
            }
            end = stop;
        }
        append(&mut out, expression.get(end..).unwrap_or_default())?;
        if out.contains('?') || out.contains(':') {
            return Err("conditional expressions are not supported yet".to_owned());
        }
        validate(&out, &self.names)?;
        Ok(out)
    }
}

/// Whole identifiers, including qualified properties and label references. Numeric
/// exponents are consumed with their number, so `1e-3` never names a parameter `e`.
fn words(expression: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut chars = expression.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_ascii_digit() || c == '.' && chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
            while chars
                .peek()
                .is_some_and(|(_, c)| c.is_ascii_digit() || *c == '.')
            {
                chars.next();
            }
            if chars.peek().is_some_and(|(_, c)| *c == 'e' || *c == 'E') {
                chars.next();
                if chars.peek().is_some_and(|(_, c)| *c == '-' || *c == '+') {
                    chars.next();
                }
                while chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
                    chars.next();
                }
            }
            continue;
        }
        if !(c.is_alphabetic()
            || c == '_'
            || c == '.'
            || c == '<' && chars.peek().is_some_and(|(_, c)| *c == '<'))
        {
            continue;
        }
        if c == '<' {
            chars.next();
            while let Some((_, c)) = chars.next() {
                if c == '>' && chars.peek().is_some_and(|(_, c)| *c == '>') {
                    chars.next();
                    break;
                }
            }
        }
        while chars
            .peek()
            .is_some_and(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '.')
        {
            chars.next();
        }
        let stop = chars.peek().map_or(expression.len(), |(i, _)| *i);
        if let Some(word) = expression.get(start..stop) {
            out.push((start, stop, word));
        }
    }
    out
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
#[cfg(test)]
fn readable(expression: &str, names: &BTreeSet<String>) -> bool {
    validate(expression, names).is_ok()
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
            "min(1, 2, 3)",
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
            "1.2.3 mm",
            "1e- mm",
            "width +",
            "sqrt(1, 2)",
            "pow(2)",
            "1 mm 2 mm",
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
        if row["value"].is_null() {
            row.as_object_mut().unwrap().remove("value");
        }
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
        let (params, names) = map(&model, &mut report);
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
        let (params, names) = map(&model, &mut report);
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
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[0]["name"], "p_pi");
        assert_eq!(params[1]["expr"], "p_pi * 3");
        let renamed = &report.losses()[0];
        assert_eq!(renamed.severity, Severity::Degraded);
        assert_eq!(
            renamed.detail,
            "renamed to p_pi: SolidCraft cannot use that name"
        );
    }
    fn sheets(parameters: Value) -> dutchman_core::Model {
        let features = json!([
            crate::testing::feature(
                1,
                0,
                "Sheet",
                json!({"definition": "datum_principal_plane", "plane": "top"})
            ),
            crate::testing::feature(
                2,
                1,
                "Pad",
                json!({"definition": "datum_principal_plane", "plane": "top"})
            ),
        ]);
        let model = with(cube(), features, parameters);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let cells: Vec<_> = model
            .ir
            .model
            .parameters
            .iter()
            .filter(|p| p.owner.as_ref().map(|id| id.as_str()) == Some("f3d:model:feature#1"))
            .enumerate()
            .map(|(i, p)| json!({"address": format!("A{}", i+1), "parameter": p.id}))
            .collect();
        wire["model"]["spreadsheets"] = json!([{
            "id": "f3d:model:spreadsheet#1", "feature": "f3d:model:feature#1", "cells": cells,
        }]);
        dutchman_core::Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        }
    }

    #[test]
    fn qualified_sheet_references_keep_aliases_and_formula_only_cells_live() {
        let length = |v: f64| json!({"kind": "length", "value": v});
        let mut depth = parameter(
            4,
            "Length",
            "Sheet.width / 2 + Sheet.A2",
            length(20.0),
            true,
        );
        depth["owner"] = json!("f3d:model:feature#2");
        let model = sheets(json!([
            parameter(1, "width", "=20 mm", length(20.0), true),
            parameter(2, "A2", "=width / 2", Value::Null, true),
            parameter(
                3,
                "label",
                "'Width",
                json!({"kind": "string", "value": "Width"}),
                true
            ),
            depth,
        ]));
        let mut report = Report::default();
        let (params, names) = map(&model, &mut report);
        assert_eq!(
            params
                .iter()
                .map(|p| (&p["name"], &p["expr"]))
                .collect::<Vec<_>>(),
            vec![
                (&json!("width"), &json!("20 mm")),
                (&json!("Sheet_A2"), &json!("width / 2")),
                (&json!("Length"), &json!("width / 2 + Sheet_A2"))
            ]
        );
        assert_eq!(params[0]["model"], false);
        assert_eq!(params[1]["unit"], "mm");
        assert!(report.names().contains("Sheet_A2"));
        assert!(!names.contains_key("f3d:model:parameter#3"));
        assert!(
            report
                .losses()
                .iter()
                .all(|loss| loss.severity == Severity::Info),
            "{:?}",
            report.losses()
        );
    }

    #[test]
    fn qualified_properties_do_not_bind_to_an_unrelated_global_name() {
        let length = json!({"kind": "length", "value": 10.0});
        let model = sheets(json!([
            parameter(1, "width", "=10 mm", length.clone(), true),
            parameter(2, "result", "Other.width / 2", length, true),
        ]));
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[1]["expr"], "10 mm");
        assert!(report.losses().iter().any(|loss| loss
            .detail
            .contains("reference to object Other not converted")));
    }

    #[test]
    fn function_and_conditional_losses_name_the_gap() {
        let model = with(
            cube(),
            json!([]),
            json!([
                parameter(
                    1,
                    "w",
                    "cbrt(8)",
                    json!({"kind": "real", "value": 2.0}),
                    false
                ),
                parameter(
                    2,
                    "h",
                    "1 > 0 ? 3 : 4",
                    json!({"kind": "real", "value": 3.0}),
                    false
                ),
                parameter(
                    3,
                    "r",
                    "sqrt(4) + abs(-1) + min(2, 3) + max(2, 3) + sin(30) + pi",
                    Value::Null,
                    false
                ),
            ]),
        );
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[0]["expr"], "2");
        assert_eq!(params[1]["expr"], "3");
        assert_eq!(
            params[2]["expr"],
            "sqrt(4) + abs(-1) + min(2, 3) + max(2, 3) + sin(30) + pi"
        );
        assert!(report.losses()[0]
            .detail
            .ends_with("unsupported function cbrt"));
        assert!(report.losses()[1]
            .detail
            .ends_with("conditional expressions are not supported yet"));
    }

    #[test]
    fn unavailable_formula_only_dependencies_are_not_written_as_dangling_references() {
        let model = sheets(json!([
            parameter(1, "a", "=cbrt(8)", Value::Null, true),
            parameter(2, "b", "=a * 2", Value::Null, true),
            parameter(
                3,
                "c",
                "=b * 2",
                json!({"kind": "real", "value": 8.0}),
                true
            ),
        ]));
        let mut report = Report::default();
        let (params, names) = map(&model, &mut report);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0]["expr"], "8");
        assert_eq!(names.len(), 1);
        assert_eq!(
            report
                .losses()
                .iter()
                .filter(|l| l.severity == Severity::Dropped)
                .count(),
            2
        );
        assert!(report
            .losses()
            .iter()
            .any(|l| l.detail.ends_with("reference to parameter b not converted")));
    }

    #[test]
    fn cyclic_formulas_use_values_without_leaving_a_live_cycle() {
        let value = json!({"kind": "real", "value": 1.0});
        let model = with(
            cube(),
            json!([]),
            json!([
                parameter(1, "a", "b + 1", value.clone(), false),
                parameter(2, "b", "a - 1", value, false),
            ]),
        );
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert!(params.iter().all(|p| p["expr"] == "1"));
        assert!(report
            .losses()
            .iter()
            .all(|l| l.detail.contains("cyclic parameter dependency")));
    }
    #[test]
    fn label_references_require_explicit_label_identity() {
        let value = json!({"kind": "length", "value": 12.0});
        let mut width = parameter(1, "width", "=12 mm", value.clone(), true);
        width["properties"] = json!({"owner_label": "Plate dimensions"});
        let mut depth = parameter(
            2,
            "Length",
            "<<Plate dimensions>>.width / 2",
            value.clone(),
            true,
        );
        depth["owner"] = json!("f3d:model:feature#2");
        let model = sheets(json!([
            width,
            depth,
            parameter(3, "unresolved", "<<Sheet>>.width", value, true),
        ]));
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[1]["expr"], "width / 2");
        assert_eq!(params[2]["expr"], "12 mm");
        assert!(report.losses().iter().any(|l| l
            .detail
            .contains("reference to object <<Sheet>> not converted")));
    }

    #[test]
    fn property_paths_resolve_only_in_their_decoded_owner_scope() {
        let value = json!({"kind": "length", "value": 12.0});
        let mut width = parameter(1, "Constraint10", "12 mm", value.clone(), true);
        width["properties"] =
            json!({"reference_name": "Constraints.width", "source_name": "width"});
        let model = sheets(json!([
            width,
            parameter(2, "offset", "-.Constraints.width / 2", value.clone(), true),
            parameter(3, "cross", "Sheet.Constraints.width / 2", value, true),
        ]));
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[1]["expr"], "-Constraint10 / 2");
        assert_eq!(params[2]["expr"], "Constraint10 / 2");
        assert!(report.losses().iter().all(|l| l.severity == Severity::Info));
    }
    #[test]
    fn text_and_empty_cells_are_silent() {
        let model = sheets(json!([
            parameter(
                1,
                "caption",
                "'Width",
                json!({"kind": "string", "value": "Width"}),
                true
            ),
            parameter(2, "other", "Height", Value::Null, true),
            parameter(3, "text_formula", "='caption'", Value::Null, true),
            parameter(4, "empty", "", Value::Null, true),
        ]));
        let mut report = Report::default();
        let (params, names) = map(&model, &mut report);
        assert!(params.is_empty());
        assert!(names.is_empty());
        assert!(report.losses().is_empty());
    }
    #[test]
    fn feature_property_references_keep_their_owner_identity() {
        let value = json!({"kind": "length", "value": 10.0});
        let mut depth = parameter(2, "Length", "Sheet.width / 2", value.clone(), true);
        depth["owner"] = json!("f3d:model:feature#2");
        let model = sheets(json!([
            parameter(1, "width", "=20 mm", value.clone(), true),
            depth,
            parameter(3, "result", "Pad.Length * 2", value, true),
        ]));
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[1]["expr"], "width / 2");
        assert_eq!(params[2]["expr"], "Length * 2");
        assert!(report.losses().iter().all(|l| l.severity == Severity::Info));
    }
    #[test]
    fn explicit_dependencies_take_precedence_over_a_shadowing_local_name() {
        let value = json!({"kind": "length", "value": 10.0});
        let model = with(
            cube(),
            json!([crate::testing::feature(
                1,
                0,
                "Feature",
                json!({"definition": "datum_principal_plane", "plane": "top"})
            )]),
            json!([
                parameter(1, "width", "10 mm", value.clone(), false),
                depending(parameter(2, "width", "width * 2", value, true), &[1]),
            ]),
        );
        let mut report = Report::default();
        let (params, _) = map(&model, &mut report);
        assert_eq!(params[1]["name"], "width_2");
        assert_eq!(params[1]["expr"], "width * 2");
        assert!(report.losses().iter().all(|l| l.severity == Severity::Info));
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
