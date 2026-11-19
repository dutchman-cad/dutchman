//! Closed-form real roots for analytic planar curve intersections. Coefficients
//! come from stored carriers, never a tessellation or a collection of samples.
use super::Mapping;

pub(super) fn roots(coefficients: &[f64]) -> Mapping<Vec<f64>> {
    if coefficients.iter().any(|v| !v.is_finite()) {
        return Err("the analytic intersection exceeds the finite range".into());
    }
    let Some((&leading, rest)) = coefficients.split_last() else {
        return Err("the analytic intersection carriers coincide".into());
    };
    if leading == 0.0 {
        return roots(rest);
    }
    let normalized: Vec<_> = rest.iter().map(|v| v / leading).collect();
    if normalized.iter().any(|n| !n.is_finite()) {
        return Err("the analytic intersection normalization exceeds the finite range".into());
    }
    let result = match normalized.as_slice() {
        [] => Vec::new(),
        [a] => vec![-a],
        [c, b] => quadratic(*b, *c)?,
        [c, b, a] => cubic(*a, *b, *c)?,
        [d, c, b, a] => {
            let p = b - 3.0 * a * a / 8.0;
            let q = a * a * a / 8.0 - a * b / 2.0 + c;
            let r = -3.0 * a.powi(4) / 256.0 + a * a * b / 16.0 - a * c / 4.0 + d;
            let mut result = Vec::new();
            if q == 0.0 {
                for z in quadratic(p, r)? {
                    if z != 0.0 && z.abs() <= 64.0 * f64::EPSILON * (p.abs() + r.abs()).max(1.0) {
                        return Err(
                            "the analytic intersection is tangent within arithmetic roundoff"
                                .into(),
                        );
                    }
                    if z >= 0.0 {
                        result.extend([z.sqrt(), -z.sqrt()]);
                    }
                }
            } else {
                // Ferrari factorization into two real quadratics. The largest
                // positive resolvent root avoids cancellation in sqrt(z).
                let z = cubic(2.0 * p, p * p - 4.0 * r, -q * q)?
                    .into_iter()
                    .filter(|z| *z > 0.0)
                    .max_by(f64::total_cmp)
                    .ok_or("the analytic intersection resolvent is unresolved")?;
                let u = z.sqrt();
                result.extend(quadratic(u, (p + z - q / u) / 2.0)?);
                result.extend(quadratic(-u, (p + z + q / u) / 2.0)?);
            }
            result.into_iter().map(|x| x - a / 4.0).collect()
        }
        _ => return Err("the analytic intersection degree is not supported yet".into()),
    };
    if result.iter().any(|v| !v.is_finite()) {
        return Err("the analytic intersection roots are unresolved".into());
    }
    for root in &result {
        let residual = coefficients.iter().rev().fold(0.0, |sum, c| sum * root + c);
        let magnitude = coefficients
            .iter()
            .rev()
            .fold(0.0, |sum, c| sum * root.abs() + c.abs());
        if !residual.is_finite()
            || !magnitude.is_finite()
            || residual.abs() > 64.0 * f64::EPSILON * magnitude
        {
            return Err("the analytic intersection root residual is unresolved".into());
        }
    }
    Ok(result)
}
fn quadratic(b: f64, c: f64) -> Mapping<Vec<f64>> {
    let discriminant = b * b - 4.0 * c;
    let uncertainty = 64.0 * f64::EPSILON * (b * b + 4.0 * c.abs());
    if discriminant != 0.0 && discriminant.abs() <= uncertainty {
        return Err("the analytic intersection is tangent within arithmetic roundoff".into());
    }
    if discriminant < 0.0 {
        return Ok(Vec::new());
    }
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    if q == 0.0 {
        Ok(vec![0.0])
    } else {
        Ok(vec![q, c / q])
    }
}
fn cubic(a: f64, b: f64, c: f64) -> Mapping<Vec<f64>> {
    let p = b - a * a / 3.0;
    let q = 2.0 * a * a * a / 27.0 - a * b / 3.0 + c;
    let discriminant = q * q / 4.0 + p * p * p / 27.0;
    let uncertainty = 64.0 * f64::EPSILON * (q * q / 4.0 + (p * p * p / 27.0).abs());
    if discriminant != 0.0 && discriminant.abs() <= uncertainty {
        return Err(
            "the analytic intersection resolvent is ambiguous within arithmetic roundoff".into(),
        );
    }
    if discriminant >= 0.0 {
        let u = (-q / 2.0 + discriminant.sqrt()).cbrt();
        let v = (-q / 2.0 - discriminant.sqrt()).cbrt();
        let mut result = vec![u + v - a / 3.0];
        if discriminant == 0.0 {
            result.push(-(u + v) / 2.0 - a / 3.0);
        }
        Ok(result)
    } else {
        let radius = 2.0 * (-p / 3.0).sqrt();
        let phase = ((3.0 * q / (2.0 * p)) * (-3.0 / p).sqrt())
            .clamp(-1.0, 1.0)
            .acos()
            / 3.0;
        Ok((0..3)
            .map(|i| radius * (phase - std::f64::consts::TAU * f64::from(i) / 3.0).cos() - a / 3.0)
            .collect())
    }
}
