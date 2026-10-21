//! Rigid placements, row-major 3 × 4 (a rotation and a translation in millimetres), and the
//! little arithmetic the assembly code needs on them.

/// A rigid placement: rows of a rotation followed by a translation, in millimetres.
pub type Placement3x4 = [[f64; 4]; 3];

/// The identity placement.
pub const IDENTITY: Placement3x4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// `outer` after `inner`: a point placed by `inner`, then by `outer`.
pub fn compose(outer: &Placement3x4, inner: &Placement3x4) -> Placement3x4 {
    let mut out = [[0.0; 4]; 3];
    for (row, out_row) in outer.iter().zip(out.iter_mut()) {
        for (col, value) in out_row.iter_mut().enumerate() {
            let mut sum: f64 = row
                .iter()
                .take(3)
                .zip(inner.iter())
                .map(|(r, inner_row)| r * inner_row.get(col).copied().unwrap_or(0.0))
                .sum();
            if col == 3 {
                sum += row.get(3).copied().unwrap_or(0.0);
            }
            *value = sum;
        }
    }
    out
}

/// The inverse of a rigid placement.
pub fn inverse(placement: &Placement3x4) -> Placement3x4 {
    let mut out = [[0.0; 4]; 3];
    for (i, out_row) in out.iter_mut().enumerate() {
        // The rotation transposed, and the translation taken back through it.
        let mut moved = 0.0;
        for (j, row) in placement.iter().enumerate() {
            let r = row.get(i).copied().unwrap_or(0.0);
            if let Some(cell) = out_row.get_mut(j) {
                *cell = r;
            }
            moved -= r * row.get(3).copied().unwrap_or(0.0);
        }
        out_row[3] = moved;
    }
    out
}

/// Column `c` of a placement: an axis for 0 to 2, the origin for 3.
pub fn column(placement: &Placement3x4, c: usize) -> [f64; 3] {
    [0, 1, 2].map(|r| {
        placement
            .get(r)
            .and_then(|row| row.get(c))
            .copied()
            .unwrap_or(0.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_and_its_inverse_compose_to_the_identity() {
        let turned = [
            [0.0, -1.0, 0.0, 30.0],
            [1.0, 0.0, 0.0, 5.0],
            [0.0, 0.0, 1.0, -2.0],
        ];
        let both = compose(&inverse(&turned), &turned);
        for (row, want) in both.iter().zip(IDENTITY) {
            for (got, want) in row.iter().zip(want) {
                assert!((got - want).abs() < 1e-12, "{both:?}");
            }
        }
        assert_eq!(column(&turned, 3), [30.0, 5.0, -2.0]);
    }
}
