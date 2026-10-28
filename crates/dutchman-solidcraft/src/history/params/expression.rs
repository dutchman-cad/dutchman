//! Bounded syntax and dimension checks for SolidCraft parameter expressions.

use std::collections::{BTreeMap, BTreeSet};

use super::{CONSTANTS, FUNCTIONS, UNITS};

#[derive(Clone, Copy)]
struct Quantity {
    dimension: Option<(f64, f64)>,
    constant: Option<f64>,
}

impl Quantity {
    fn scalar(constant: Option<f64>) -> Self {
        Self {
            dimension: Some((0.0, 0.0)),
            constant,
        }
    }

    fn unit(unit: &str) -> Self {
        Self {
            dimension: Some(match unit {
                "" => (0.0, 0.0),
                "deg" | "rad" => (0.0, 1.0),
                _ => (1.0, 0.0),
            }),
            constant: None,
        }
    }

    fn product(self, rhs: Self, divide: bool) -> Self {
        let sign = if divide { -1.0 } else { 1.0 };
        Self {
            dimension: self
                .dimension
                .zip(rhs.dimension)
                .map(|((l, a), (r, b))| (l + sign * r, a + sign * b)),
            constant: self
                .constant
                .zip(rhs.constant)
                .map(|(a, b)| if divide { a / b } else { a * b }),
        }
    }

    fn power(self, exponent: Self) -> Self {
        Self {
            dimension: self.dimension.and_then(|(l, a)| {
                if l == 0.0 && a == 0.0 {
                    Some((l, a))
                } else {
                    exponent.constant.map(|e| (l * e, a * e))
                }
            }),
            constant: self.constant.zip(exponent.constant).map(|(a, b)| a.powf(b)),
        }
    }

    fn sum(self, rhs: Self, subtract: bool) -> Self {
        Self {
            dimension: self
                .dimension
                .zip(rhs.dimension)
                .and_then(|(a, b)| (a == b).then_some(a)),
            constant: self
                .constant
                .zip(rhs.constant)
                .map(|(a, b)| if subtract { a - b } else { a + b }),
        }
    }
}

pub(super) fn validate(expression: &str, names: &BTreeSet<String>) -> Result<(), String> {
    parse(expression, names, &BTreeMap::new()).map(|_| ())
}

pub(super) fn infer_unit(
    expression: &str,
    names: &BTreeSet<String>,
    units: &BTreeMap<String, String>,
) -> Result<String, String> {
    match parse(expression, names, units)?.dimension {
        Some((0.0, 0.0)) => Ok(String::new()),
        Some((1.0, 0.0)) => Ok("mm".to_owned()),
        Some((0.0, 1.0)) => Ok("deg".to_owned()),
        Some((length, angle)) => Err(format!(
            "result unit length^{length} angle^{angle} is not supported yet"
        )),
        None => Err("result unit cannot be inferred from the decoded parameters".to_owned()),
    }
}

fn parse(
    expression: &str,
    names: &BTreeSet<String>,
    units: &BTreeMap<String, String>,
) -> Result<Quantity, String> {
    let mut parser = Expression {
        text: expression,
        position: 0,
        names,
        units,
    };
    let value = parser.sum(0)?;
    parser.space();
    if let Some(c) = parser.rest().chars().next() {
        Err(format!("unsupported expression token {c}"))
    } else {
        Ok(value)
    }
}

struct Expression<'a> {
    text: &'a str,
    position: usize,
    names: &'a BTreeSet<String>,
    units: &'a BTreeMap<String, String>,
}

impl Expression<'_> {
    fn rest(&self) -> &str {
        self.text.get(self.position..).unwrap_or_default()
    }

    fn space(&mut self) {
        self.position += self.rest().len() - self.rest().trim_start().len();
    }

    fn take(&mut self, c: char) -> bool {
        self.space();
        if self.rest().starts_with(c) {
            self.position += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn sum(&mut self, depth: usize) -> Result<Quantity, String> {
        let mut value = self.product(depth)?;
        loop {
            let add = self.take('+');
            if !add && !self.take('-') {
                break;
            }
            value = value.sum(self.product(depth)?, !add);
        }
        Ok(value)
    }

    fn product(&mut self, depth: usize) -> Result<Quantity, String> {
        let mut value = self.power(depth)?;
        loop {
            let multiply = self.take('*');
            if !multiply && !self.take('/') {
                break;
            }
            value = value.product(self.power(depth)?, !multiply);
        }
        Ok(value)
    }

    fn power(&mut self, depth: usize) -> Result<Quantity, String> {
        if depth > 64 {
            return Err("expression nesting exceeds 64 levels".to_owned());
        }
        if self.take('+') {
            return self.power(depth + 1);
        }
        if self.take('-') {
            let mut value = self.power(depth + 1)?;
            value.constant = value.constant.map(|n| -n);
            return Ok(value);
        }
        let value = self.atom(depth)?;
        if self.take('^') {
            Ok(value.power(self.power(depth + 1)?))
        } else {
            Ok(value)
        }
    }

    fn word(&mut self) -> String {
        self.space();
        let end = self
            .rest()
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(self.rest().len());
        let word = self.rest().get(..end).unwrap_or_default().to_owned();
        self.position += end;
        word
    }

    fn atom(&mut self, depth: usize) -> Result<Quantity, String> {
        if self.take('(') {
            let value = self.sum(depth + 1)?;
            return if self.take(')') {
                Ok(value)
            } else {
                Err("missing closing parenthesis".to_owned())
            };
        }
        self.space();
        let first = self
            .rest()
            .chars()
            .next()
            .ok_or_else(|| "missing expression operand".to_owned())?;
        if first.is_ascii_digit() || first == '.' {
            let mut chars = self.rest().char_indices().peekable();
            while chars
                .peek()
                .is_some_and(|(_, c)| c.is_ascii_digit() || *c == '.')
            {
                chars.next();
            }
            if chars.peek().is_some_and(|(_, c)| *c == 'e' || *c == 'E') {
                chars.next();
                if chars.peek().is_some_and(|(_, c)| *c == '+' || *c == '-') {
                    chars.next();
                }
                while chars.peek().is_some_and(|(_, c)| c.is_ascii_digit()) {
                    chars.next();
                }
            }
            let end = chars.peek().map_or(self.rest().len(), |(i, _)| *i);
            let number = self.rest().get(..end).unwrap_or_default();
            let number = number
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("number {number} is not a finite scalar"))?;
            self.position += end;
            self.space();
            let saved = self.position;
            let unit = self.word();
            if UNITS.contains(&unit.as_str()) {
                return Ok(Quantity::unit(&unit));
            }
            self.position = saved;
            return Ok(Quantity::scalar(Some(number)));
        }
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(format!("unsupported expression token {first}"));
        }
        let word = self.word();
        if FUNCTIONS.contains(&word.as_str()) && self.take('(') {
            let first = self.sum(depth + 1)?;
            let mut second = None;
            let mut count = 1;
            let mut compatible = first;
            while self.take(',') {
                count += 1;
                let argument = self.sum(depth + 1)?;
                second = Some(argument);
                compatible = compatible.sum(argument, false);
            }
            if !self.take(')') {
                return Err(format!("missing closing parenthesis for function {word}"));
            }
            let arity = match word.as_str() {
                "min" | "max" => count,
                "atan2" | "hypot" | "pow" | "mod" => 2,
                _ => 1,
            };
            if count != arity {
                return Err(format!(
                    "function {word} requires {arity} arguments, found {count}"
                ));
            }
            Ok(match word.as_str() {
                "sqrt" => first.power(Quantity::scalar(Some(0.5))),
                "pow" => first.power(second.unwrap_or(Quantity::scalar(None))),
                "sin" | "cos" | "tan" | "exp" | "ln" | "log" | "log10" | "sign" => {
                    Quantity::scalar(None)
                }
                "asin" | "acos" | "atan" | "atan2" => Quantity::unit("deg"),
                "min" | "max" | "hypot" | "mod" => Quantity {
                    constant: None,
                    ..compatible
                },
                _ => Quantity {
                    constant: None,
                    ..first
                },
            })
        } else if self.names.contains(&word) {
            Ok(self.units.get(&word).map_or(
                Quantity {
                    dimension: None,
                    constant: None,
                },
                |unit| Quantity::unit(unit),
            ))
        } else if CONSTANTS.contains(&word.as_str()) {
            Ok(Quantity::scalar(Some(std::f64::consts::PI)))
        } else {
            Err(format!("reference to parameter {word} not converted"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_only_cells_keep_length_angle_and_scalar_dimensions() {
        let units = BTreeMap::from([
            ("width".to_owned(), "mm".to_owned()),
            ("angle".to_owned(), "deg".to_owned()),
        ]);
        let names = units.keys().cloned().collect();
        for (expr, unit) in [
            ("width / 2", "mm"),
            ("sqrt(width^2)", "mm"),
            ("width / width", ""),
            ("min(width, 1 in, 20 mm)", "mm"),
            ("sin(angle)", ""),
            ("asin(0.5)", "deg"),
            ("angle / 2", "deg"),
            ("2 * pi", ""),
        ] {
            assert_eq!(infer_unit(expr, &names, &units).unwrap(), unit, "{expr}");
        }
        assert!(infer_unit("width^2", &names, &units)
            .unwrap_err()
            .contains("length^2"));
    }

    #[test]
    fn nesting_and_numbers_are_bounded() {
        let names = BTreeSet::new();
        let deep = format!("{}1{}", "(".repeat(70), ")".repeat(70));
        assert!(validate(&deep, &names)
            .unwrap_err()
            .contains("nesting exceeds"));
        assert!(validate("1e999", &names)
            .unwrap_err()
            .contains("finite scalar"));
    }
}
