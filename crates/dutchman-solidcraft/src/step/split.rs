//! Unshare excess edge uses in an encoded STEP sheet without changing geometry.
use std::collections::BTreeMap;
use std::ops::Range;

/// Keep the first two oriented uses of an edge, and give each later use an identical
/// EDGE_CURVE of its own. Only reference tokens and the new records change.
pub(crate) fn split_sheet_edges(text: &str) -> Option<(String, usize)> {
    if text.len() > super::MAX_STEP_BYTES {
        return None;
    }
    let mut curves = BTreeMap::new();
    let mut oriented = Vec::new();
    let mut maximum = 0_u64;
    let mut end = None;
    let mut count = 0_usize;
    for range in statements(text)? {
        let raw = text.get(range.clone())?;
        let mut statement = raw.trim();
        while let Some(comment) = statement.strip_prefix("/*") {
            statement = comment.split_once("*/")?.1.trim_start();
        }
        if statement.eq_ignore_ascii_case("ENDSEC") {
            end = Some(range.start);
        }
        let Some(entity) = statement.strip_prefix('#') else {
            continue;
        };
        let (id, definition) = entity.split_once('=')?;
        maximum = maximum.max(id.trim().parse().ok()?);
        count += 1;
        if count > 1_000_000 {
            return None;
        }
        let definition = definition.trim();
        let refs = references(definition)?;
        for (_, reference) in &refs {
            maximum = maximum.max(*reference);
        }
        let kind = definition.split_once('(')?.0.trim();
        if kind.eq_ignore_ascii_case("EDGE_CURVE") {
            curves.insert(id.trim().parse::<u64>().ok()?, definition);
        } else if kind.eq_ignore_ascii_case("ORIENTED_EDGE") {
            let [(reference, edge)] = refs.as_slice() else {
                return None;
            };
            let offset = range
                .start
                .checked_add(raw.trim_end().len())?
                .checked_sub(definition.len())?;
            oriented.push((
                offset.checked_add(reference.start)?..offset.checked_add(reference.end)?,
                *edge,
            ));
        }
    }
    let mut uses = BTreeMap::new();
    let mut replacements = Vec::new();
    let mut added = String::new();
    for (reference, edge) in oriented {
        let used = uses.entry(edge).or_insert(0_usize);
        *used += 1;
        if *used <= 2 {
            continue;
        }
        let definition = curves.get(&edge)?;
        maximum = maximum.checked_add(1)?;
        replacements.push((reference, format!("#{maximum}")));
        added.push_str(&format!("\n#{maximum} = {definition};"));
        if added.len().checked_add(text.len())? > super::MAX_STEP_BYTES {
            return None;
        }
    }
    if replacements.is_empty() {
        return None;
    }
    let split = replacements.len();
    added.push('\n');
    replacements.push((end?..end?, added));
    let mut result = String::new();
    let mut at = 0;
    for (range, replacement) in replacements {
        result.push_str(text.get(at..range.start)?);
        result.push_str(&replacement);
        at = range.end;
        if result.len() > super::MAX_STEP_BYTES {
            return None;
        }
    }
    result.push_str(text.get(at..)?);
    (result.len() <= super::MAX_STEP_BYTES).then_some((result, split))
}

/// STEP strings may contain semicolons and doubled quotes. Comments are not records.
fn statements(text: &str) -> Option<Vec<Range<usize>>> {
    let mut spans = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut comment = false;
    let mut star = false;
    for (at, ch) in text.char_indices() {
        if comment {
            if star && ch == '/' {
                comment = false;
            }
            star = ch == '*';
        } else if ch == '\'' {
            quoted = !quoted;
        } else if !quoted && text.get(at..)?.starts_with("/*") {
            comment = true;
            star = false;
        } else if !quoted && ch == ';' {
            spans.push(start..at);
            start = at.checked_add(1)?;
            if spans.len() > 1_000_100 {
                return None;
            }
        }
    }
    (!quoted && !comment).then_some(spans)
}

fn references(text: &str) -> Option<Vec<(Range<usize>, u64)>> {
    let mut refs = Vec::new();
    let mut quoted = false;
    for (at, ch) in text.char_indices() {
        if ch == '\'' {
            quoted = !quoted;
        }
        if quoted || ch != '#' {
            continue;
        }
        let digits = text
            .get(at.checked_add(1)?..)?
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let end = at.checked_add(1)?.checked_add(digits)?;
        if refs.len() >= 1_000_000 {
            return None;
        }
        refs.push((at..end, text.get(at.checked_add(1)?..end)?.parse().ok()?));
    }
    Some(refs)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    #[test]
    fn only_the_third_edge_reference_changes_and_carriers_are_identical() {
        let text = "ISO-10303-21;\nDATA;\n#1=EDGE_CURVE('name;#9',#10,#11,#12,.T.);\n#2=ORIENTED_EDGE('',*,*,#1,.T.);\n#3=ORIENTED_EDGE('',*,*,#1,.F.);\n#4=ORIENTED_EDGE('',*,*,#1,.T.);\nENDSEC;\nEND-ISO-10303-21;\n";
        let (repaired, split) = split_sheet_edges(text).unwrap();
        assert_eq!(split, 1);
        assert!(repaired.contains("#2=ORIENTED_EDGE('',*,*,#1,.T.);"));
        assert!(repaired.contains("#3=ORIENTED_EDGE('',*,*,#1,.F.);"));
        assert!(repaired.contains("#4=ORIENTED_EDGE('',*,*,#13,.T.);"));
        assert!(repaired.contains("#13 = EDGE_CURVE('name;#9',#10,#11,#12,.T.);"));
        assert!(split_sheet_edges(&repaired).is_none());
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod kernel_tests {
    use super::*;
    use crate::SolidCraftEvaluator;

    /// Three triangular pages sharing one straight edge; all coordinates are synthetic.
    fn three_face_sheet() -> String {
        struct Step {
            next: u64,
            text: String,
        }
        impl Step {
            fn add(&mut self, body: String) -> u64 {
                let id = self.next;
                self.next += 1;
                self.text.push_str(&format!("#{id}={body};\n"));
                id
            }
            fn edge(
                &mut self,
                a: usize,
                b: usize,
                points: &[u64],
                vertices: &[u64],
                xyz: &[[f64; 3]],
            ) -> u64 {
                let d: Vec<_> = xyz[b].iter().zip(xyz[a]).map(|(b, a)| b - a).collect();
                let direction =
                    self.add(format!("DIRECTION('',({:?},{:?},{:?}))", d[0], d[1], d[2]));
                let vector = self.add(format!("VECTOR('',#{direction},1.)"));
                let line = self.add(format!("LINE('',#{},#{vector})", points[a]));
                self.add(format!(
                    "EDGE_CURVE('',#{},#{},#{line},.T.)",
                    vertices[a], vertices[b]
                ))
            }
        }
        let mut step = Step {
            next: 1,
            text: String::new(),
        };
        let xyz = [
            [0., 0., 0.],
            [0., 0., 4.],
            [4., 0., 0.],
            [0., 4., 0.],
            [4., 4., 0.],
        ];
        let points: Vec<_> = xyz
            .iter()
            .map(|p| {
                step.add(format!(
                    "CARTESIAN_POINT('',({:?},{:?},{:?}))",
                    p[0], p[1], p[2]
                ))
            })
            .collect();
        let vertices: Vec<_> = points
            .iter()
            .map(|p| step.add(format!("VERTEX_POINT('',#{p})")))
            .collect();
        let shared = step.edge(0, 1, &points, &vertices, &xyz);
        let mut faces = Vec::new();
        for (triangle, normal, reference) in [
            ([0, 1, 2], [0., 1., 0.], [1., 0., 0.]),
            ([1, 0, 3], [1., 0., 0.], [0., 1., 0.]),
            ([0, 1, 4], [-1., 1., 0.], [1., 1., 0.]),
        ] {
            let a = step.add(format!(
                "ORIENTED_EDGE('',*,*,#{shared},{})",
                if triangle[0] == 0 { ".T." } else { ".F." }
            ));
            let bc = step.edge(triangle[1], triangle[2], &points, &vertices, &xyz);
            let b = step.add(format!("ORIENTED_EDGE('',*,*,#{bc},.T.)"));
            let ca = step.edge(triangle[2], triangle[0], &points, &vertices, &xyz);
            let c = step.add(format!("ORIENTED_EDGE('',*,*,#{ca},.T.)"));
            let edges = step.add(format!("EDGE_LOOP('',(#{a},#{b},#{c}))"));
            let bound = step.add(format!("FACE_OUTER_BOUND('',#{edges},.T.)"));
            let normal = step.add(format!(
                "DIRECTION('',({:?},{:?},{:?}))",
                normal[0], normal[1], normal[2]
            ));
            let reference = step.add(format!(
                "DIRECTION('',({:?},{:?},{:?}))",
                reference[0], reference[1], reference[2]
            ));
            let frame = step.add(format!(
                "AXIS2_PLACEMENT_3D('',#{},#{normal},#{reference})",
                points[0]
            ));
            let plane = step.add(format!("PLANE('',#{frame})"));
            faces.push(step.add(format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
        }
        let shell = step.add(format!(
            "OPEN_SHELL('',(#{},#{},#{}))",
            faces[0], faces[1], faces[2]
        ));
        let model = step.add(format!(
            "SHELL_BASED_SURFACE_MODEL('Synthetic sheet',(#{shell}))"
        ));
        let mm = step.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".to_owned());
        let angle = step.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".to_owned());
        let solid_angle =
            step.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".to_owned());
        let context = step.add(format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{angle},#{solid_angle}))REPRESENTATION_CONTEXT('',''))"));
        let representation = step.add(format!("SHAPE_REPRESENTATION('',(#{model}),#{context})"));
        let app = step.add("APPLICATION_CONTEXT('automotive_design')".to_owned());
        let product_context = step.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
        let product = step.add(format!("PRODUCT('Sheet','Sheet','',(#{product_context}))"));
        let formation = step.add(format!(
            "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#{product},.NOT_KNOWN.)"
        ));
        let definition_context = step.add(format!(
            "PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"
        ));
        let definition = step.add(format!(
            "PRODUCT_DEFINITION('design','',#{formation},#{definition_context})"
        ));
        let shape = step.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
        step.add(format!(
            "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"
        ));
        format!("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Synthetic sheet'),'2;1');\nFILE_NAME('sheet.step','',(''),(''),'','','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",step.text)
    }

    #[test]
    fn three_faces_sharing_an_edge_import_as_one_open_body_after_minimal_split() {
        let source = three_face_sheet();
        let (repaired, split) = split_sheet_edges(&source).unwrap();
        assert_eq!(split, 1);
        assert_eq!(repaired.matches("ADVANCED_FACE(").count(), 3);
        assert!(split_sheet_edges(&repaired).is_none());
        let Some(cli) = std::env::var_os("DUTCHMAN_TEST_SHEET_CLI")
            .or_else(|| std::env::var_os(crate::CLI_ENV))
        else {
            return;
        };
        let container = crate::container::Container::new(repaired.as_bytes()).unwrap();
        let blob = container.step_blob().to_owned();
        let bytes = container.finish(&serde_json::json!({"format":crate::FORMAT,"name":"Synthetic sheet","units":"mm","next_id":2,"params":[],"features":[{
            "id":1,"type":"import","name":"Sheet","file":"synthetic.step","step":"","step_blob":blob,"suppressed":false
        }]})).unwrap();
        let result = crate::CliEvaluator::new(std::path::PathBuf::from(cli))
            .evaluate(&bytes, std::time::Duration::from_secs(30), "sheet split")
            .unwrap();
        assert!(result.get("evaluation_failed").is_none(), "{result}");
        let bodies = result["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 1, "{result}");
        assert!(
            bodies[0]
                .get("volume_mm3")
                .is_none_or(serde_json::Value::is_null),
            "{result}"
        );
        assert_eq!(result["total"]["faces"], 3, "{result}");
        assert!(
            result["timeline"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row.get("error").is_none_or(serde_json::Value::is_null)),
            "{result}"
        );
    }
}
