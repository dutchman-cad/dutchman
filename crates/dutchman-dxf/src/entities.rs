//! Small ASCII DXF R2000 encoder. Coordinates and text heights are millimetres.

use std::collections::BTreeSet;
use std::fmt::Write;

use dutchman_core::Error;

/// Geometry already placed in page coordinates. Splines use the DXF knot convention.
#[derive(Clone, Debug)]
pub enum Entity {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    Arc {
        center: [f64; 2],
        radius: f64,
        start_degrees: f64,
        end_degrees: f64,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    Polyline {
        points: Vec<[f64; 2]>,
        closed: bool,
    },
    Spline {
        degree: u16,
        knots: Vec<f64>,
        control_points: Vec<[f64; 2]>,
        weights: Vec<f64>,
    },
    Text {
        position: [f64; 2],
        height: f64,
        rotation: f64,
        text: String,
    },
}

/// A bounded page with explicit layers, including those whose geometry is unavailable.
#[derive(Default)]
pub struct Document {
    pub layers: BTreeSet<String>,
    pub hidden_layers: BTreeSet<String>,
    pub center_layers: BTreeSet<String>,
    pub sheet: Option<[f64; 2]>,
    pub entities: Vec<(String, Entity)>,
}

const MAX_ITEMS: usize = 100_000;

impl Document {
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        if self.entities.len() > MAX_ITEMS || self.layers.len() > 10_000 {
            return Err(Error::Encode(
                "DXF page exceeds 100,000 entities or 10,000 layers".into(),
            ));
        }
        let mut out = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n4\n9\n$MEASUREMENT\n70\n1\n");
        if let Some([width, height]) = self.sheet {
            positive(width)?;
            positive(height)?;
            tag(&mut out, 9, "$LIMMIN");
            point(&mut out, 10, [0.0, 0.0])?;
            tag(&mut out, 9, "$LIMMAX");
            point(&mut out, 10, [width, height])?;
        }
        out.push_str("0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n5\n1\n70\n3\n");
        for (handle, name, description, pattern) in [
            (2, "CONTINUOUS", "Solid line", &[][..]),
            (4, "HIDDEN", "Hidden lines", &[2.0_f64, -1.0][..]),
            (5, "CENTER", "Center lines", &[8.0_f64, -1.0, 2.0, -1.0][..]),
        ] {
            tag(&mut out, 0, "LTYPE");
            tag(&mut out, 5, handle);
            tag(&mut out, 2, name);
            tag(&mut out, 70, 0);
            tag(&mut out, 3, description);
            tag(&mut out, 72, 65);
            tag(&mut out, 73, pattern.len());
            number(&mut out, 40, pattern.iter().map(|v| v.abs()).sum())?;
            for length in pattern {
                number(&mut out, 49, *length)?;
                tag(&mut out, 74, 0);
            }
        }
        out.push_str("0\nENDTAB\n0\nTABLE\n2\nLAYER\n5\n3\n");
        tag(
            &mut out,
            70,
            self.layers.len() + usize::from(!self.layers.contains("0")),
        );
        for (index, layer) in std::iter::once("0")
            .chain(self.layers.iter().map(String::as_str).filter(|l| *l != "0"))
            .enumerate()
        {
            if !valid_layer(layer) {
                return Err(Error::Encode("DXF layer name is not supported yet".into()));
            }
            out.push_str("0\nLAYER\n");
            tag(&mut out, 5, format!("{:X}", 0x10 + index));
            tag(&mut out, 2, layer);
            out.push_str("70\n0\n62\n7\n");
            tag(
                &mut out,
                6,
                if self.hidden_layers.contains(layer) {
                    "HIDDEN"
                } else if self.center_layers.contains(layer) {
                    "CENTER"
                } else {
                    "CONTINUOUS"
                },
            );
        }
        out.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        for (index, (layer, entity)) in self.entities.iter().enumerate() {
            if !self.layers.contains(layer) {
                return Err(Error::Encode(
                    "DXF entity refers to an undeclared layer".into(),
                ));
            }
            entity.write(&mut out, layer, 0x10_0000 + index)?;
            if out.len() > 64 * 1024 * 1024 {
                return Err(Error::Encode("DXF page exceeds 64 MiB".into()));
            }
        }
        out.push_str("0\nENDSEC\n0\nEOF\n");
        Ok(out.into_bytes())
    }
}

fn valid_layer(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

fn tag(out: &mut String, code: u16, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{code}\n{value}");
}

fn number(out: &mut String, code: u16, value: f64) -> Result<(), Error> {
    if !value.is_finite() || value.abs() > 1e12 {
        return Err(Error::Encode(
            "DXF coordinate is outside the supported finite range".into(),
        ));
    }
    tag(out, code, value);
    Ok(())
}

fn point(out: &mut String, code: u16, [x, y]: [f64; 2]) -> Result<(), Error> {
    number(out, code, x)?;
    number(out, code + 10, y)
}

fn positive(value: f64) -> Result<(), Error> {
    if value > 0.0 && value.is_finite() {
        Ok(())
    } else {
        Err(Error::Encode(
            "DXF radius or height must be positive and finite".into(),
        ))
    }
}

fn escaped(text: &str) -> String {
    let mut out = String::new();
    for c in text.encode_utf16() {
        if (32..127).contains(&c)
            && c != u16::from(b'\\')
            && c != u16::from(b'%')
            && c != u16::from(b'{')
            && c != u16::from(b'}')
        {
            if let Some(c) = char::from_u32(u32::from(c)) {
                out.push(c);
            }
        } else {
            let _ = write!(out, "\\U+{c:04X}");
        }
    }
    out
}

impl Entity {
    fn write(&self, out: &mut String, layer: &str, handle: usize) -> Result<(), Error> {
        if let Self::Text { text, .. } = self {
            if text.chars().take(4097).count() > 4096 || text.contains(['\n', '\r']) {
                return Err(Error::Encode(
                    "DXF text needs a single line of at most 4096 characters".into(),
                ));
            }
        }
        let kind = match self {
            Self::Line { .. } => "LINE",
            Self::Arc { .. } => "ARC",
            Self::Circle { .. } => "CIRCLE",
            Self::Polyline { .. } => "LWPOLYLINE",
            Self::Spline { .. } => "SPLINE",
            Self::Text { text, .. } if escaped(text).len() > 250 => "MTEXT",
            Self::Text { .. } => "TEXT",
        };
        tag(out, 0, kind);
        tag(out, 5, format!("{handle:X}"));
        tag(out, 100, "AcDbEntity");
        tag(out, 8, layer);
        match self {
            Self::Line { start, end } => {
                tag(out, 100, "AcDbLine");
                point(out, 10, *start)?;
                point(out, 11, *end)?;
            }
            Self::Circle { center, radius } | Self::Arc { center, radius, .. } => {
                positive(*radius)?;
                tag(out, 100, "AcDbCircle");
                point(out, 10, *center)?;
                number(out, 40, *radius)?;
                if let Self::Arc {
                    start_degrees,
                    end_degrees,
                    ..
                } = self
                {
                    tag(out, 100, "AcDbArc");
                    number(out, 50, start_degrees.rem_euclid(360.0))?;
                    number(out, 51, end_degrees.rem_euclid(360.0))?;
                }
            }
            Self::Polyline { points, closed } => {
                if points.len() < 2 || points.len() > MAX_ITEMS {
                    return Err(Error::Encode("DXF polyline needs 2–100,000 points".into()));
                }
                tag(out, 100, "AcDbPolyline");
                tag(out, 90, points.len());
                tag(out, 70, u8::from(*closed));
                for p in points {
                    point(out, 10, *p)?;
                }
            }
            Self::Spline {
                degree,
                knots,
                control_points,
                weights,
            } => {
                if *degree == 0
                    || *degree > 10
                    || control_points.len() <= usize::from(*degree)
                    || control_points.len() > 32_000
                    || knots.len() != control_points.len() + usize::from(*degree) + 1
                    || (!weights.is_empty() && weights.len() != control_points.len())
                    || knots.windows(2).any(|pair| matches!(pair, [a, b] if a > b))
                    || !knots
                        .get(usize::from(*degree))
                        .zip(knots.get(control_points.len()))
                        .is_some_and(|(start, end)| start < end)
                {
                    return Err(Error::Encode(
                        "DXF spline has unsupported degree, knots or weights".into(),
                    ));
                }
                tag(out, 100, "AcDbSpline");
                tag(out, 70, if weights.is_empty() { 8 } else { 12 });
                tag(out, 71, degree);
                tag(out, 72, knots.len());
                tag(out, 73, control_points.len());
                tag(out, 74, 0);
                for knot in knots {
                    number(out, 40, *knot)?;
                }
                for weight in weights {
                    positive(*weight)?;
                    number(out, 41, *weight)?;
                }
                for p in control_points {
                    point(out, 10, *p)?;
                }
            }
            Self::Text {
                position,
                height,
                rotation,
                text,
            } => {
                positive(*height)?;
                let unwrapped_width = *height * text.chars().count() as f64 * 2.0;
                let text = escaped(text);
                tag(
                    out,
                    100,
                    if kind == "MTEXT" {
                        "AcDbMText"
                    } else {
                        "AcDbText"
                    },
                );
                point(out, 10, *position)?;
                number(out, 40, *height)?;
                if kind == "MTEXT" {
                    number(out, 41, unwrapped_width)?;
                    tag(out, 71, 7);
                    if !rotation.is_finite() {
                        return Err(Error::Encode("DXF text rotation must be finite".into()));
                    }
                    point(
                        out,
                        11,
                        [rotation.to_radians().cos(), rotation.to_radians().sin()],
                    )?;
                    let mut chunks = text.as_bytes().chunks(250).peekable();
                    while let Some(chunk) = chunks.next() {
                        let value = std::str::from_utf8(chunk)
                            .map_err(|err| Error::Encode(err.to_string()))?;
                        tag(out, if chunks.peek().is_some() { 3 } else { 1 }, value);
                    }
                } else {
                    tag(out, 1, text);
                    number(out, 50, *rotation)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_entities_and_refuses_unbounded_or_nonfinite_input() {
        let mut document = Document::default();
        document.layers.insert("VIEW_1".into());
        document.entities = vec![
            (
                "VIEW_1".into(),
                Entity::Line {
                    start: [10.0, 20.0],
                    end: [40.0, 30.0],
                },
            ),
            (
                "VIEW_1".into(),
                Entity::Arc {
                    center: [20.0, 25.0],
                    radius: 5.0,
                    start_degrees: 0.0,
                    end_degrees: 90.0,
                },
            ),
            (
                "VIEW_1".into(),
                Entity::Circle {
                    center: [20.0, 25.0],
                    radius: 2.0,
                },
            ),
            (
                "VIEW_1".into(),
                Entity::Polyline {
                    points: vec![[10.0, 20.0], [40.0, 30.0]],
                    closed: false,
                },
            ),
            (
                "VIEW_1".into(),
                Entity::Spline {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![[10.0, 20.0], [20.0, 30.0], [40.0, 20.0]],
                    weights: vec![1.0, 0.5, 1.0],
                },
            ),
            (
                "VIEW_1".into(),
                Entity::Text {
                    position: [10.0, 20.0],
                    height: 2.5,
                    rotation: 0.0,
                    text: "Café \\ %%d".into(),
                },
            ),
        ];
        document.entities.push((
            "VIEW_1".into(),
            Entity::Text {
                position: [10.0, 20.0],
                height: 2.5,
                rotation: 0.0,
                text: "Long text ".repeat(30),
            },
        ));
        let text = String::from_utf8(document.encode().unwrap()).unwrap();
        // An optional artifact lets development checks open these exact encoder bytes.
        if let Some(path) = std::env::var_os("DUTCHMAN_DXF_CHECK_OUTPUT") {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, &text).unwrap();
            println!("DXF_ARTIFACT_BEGIN\n{text}DXF_ARTIFACT_END");
        }
        let lines: Vec<_> = text.lines().collect();
        let (records, remainder) = lines.as_chunks::<2>();
        assert!(remainder.is_empty());
        let tags: Vec<_> = records
            .iter()
            .map(|[code, value]| (*code, *value))
            .collect();
        for kind in [
            "LINE",
            "ARC",
            "CIRCLE",
            "LWPOLYLINE",
            "SPLINE",
            "TEXT",
            "MTEXT",
        ] {
            assert_eq!(tags.iter().filter(|p| **p == ("0", kind)).count(), 1);
        }
        assert_eq!(tags.iter().filter(|p| **p == ("8", "VIEW_1")).count(), 7);
        assert!(text.contains("$INSUNITS\n70\n4\n"));
        let xs: Vec<f64> = tags
            .iter()
            .filter(|(c, _)| *c == "10")
            .map(|(_, v)| v.parse().unwrap())
            .collect();
        assert_eq!(xs.iter().copied().fold(f64::INFINITY, f64::min), 10.0);
        assert_eq!(xs.iter().copied().fold(f64::NEG_INFINITY, f64::max), 40.0);
        assert!(text.contains("Caf\\U+00E9"));
        let spline = document.entities.get(4).cloned().unwrap();
        if let Some((_, Entity::Spline { knots, .. })) = document.entities.get_mut(4) {
            *knots = vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        }
        assert!(document.encode().is_err());
        *document.entities.get_mut(4).unwrap() = spline;
        if let Some((_, Entity::Line { start, .. })) = document.entities.first_mut() {
            *start = [f64::NAN, 0.0];
        }
        assert!(document.encode().is_err());
    }
}
