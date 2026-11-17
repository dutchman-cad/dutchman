use crate::Quantity;
use std::collections::{BTreeMap, BTreeSet};

impl Quantity {
    /// Evaluate mapper quantity text while retaining its expression and parameter link.
    pub fn from_row(text: &str, parameter_values: &BTreeMap<String, String>) -> Self {
        if text.len() > 65_536 {
            return Self {
                value: None,
                expr: text.into(),
                param: None,
            };
        }
        let mut name = strip(text);
        for _ in 0..64 {
            if let Some(rest) = name.strip_prefix('-') {
                name = strip(rest);
            } else {
                break;
            }
        }
        Self {
            value: evaluate(text, parameter_values, &mut BTreeSet::new(), 0, &mut 4096),
            expr: text.to_owned(),
            param: parameter_values.contains_key(name).then(|| name.to_owned()),
        }
    }
}
fn strip(text: &str) -> &str {
    let mut text = text.trim();
    for _ in 0..64 {
        let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) else {
            break;
        };
        // Only remove a pair enclosing the complete expression.
        let mut depth = 0i32;
        let encloses = inner.chars().all(|c| {
            if c == '(' {
                depth += 1;
            }
            if c == ')' {
                depth -= 1;
            }
            depth >= 0
        });
        if !encloses || depth != 0 {
            break;
        }
        text = inner.trim();
    }
    text
}
fn evaluate(
    text: &str,
    values: &BTreeMap<String, String>,
    seen: &mut BTreeSet<String>,
    depth: usize,
    budget: &mut usize,
) -> Option<f64> {
    if depth > 64 || text.len() > 65_536 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    let text = strip(text);
    let (sign, name) = text
        .strip_prefix('-')
        .map_or((1.0, text), |s| (-1.0, s.trim()));
    let result = if sign < 0.0 && name.starts_with('(') && strip(name) != name {
        -evaluate(name, values, seen, depth + 1, budget)?
    } else if let Some(value) = values.get(name) {
        if !seen.insert(name.to_owned()) {
            return None;
        }
        let result = evaluate(value, values, seen, depth + 1, budget).map(|v| v * sign);
        seen.remove(name);
        result?
    } else if let Some((a, b)) = text.split_once('/') {
        if b.contains('/') {
            return None;
        }
        evaluate(a, values, seen, depth + 1, budget)?
            / evaluate(b, values, seen, depth + 1, budget)?
    } else {
        let split = text
            .find(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let factor = match unit.trim() {
            "" | "mm" | "deg" => 1.0,
            "cm" => 10.0,
            "m" => 1000.0,
            "in" => 25.4,
            "ft" => 304.8,
            "rad" => 180.0 / std::f64::consts::PI,
            _ => return None,
        };
        number.trim().strip_prefix('-').map_or_else(
            || number.trim().parse::<f64>().ok(),
            |n| n.trim().parse::<f64>().ok().map(|v| -v),
        )? * factor
    };
    result.is_finite().then_some(result)
}
