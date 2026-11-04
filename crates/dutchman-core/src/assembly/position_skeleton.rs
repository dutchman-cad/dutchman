//! A bounded, geometry-free occurrence tree for testing positions before layout sharing.
//! Joint interpretation and solving stay in the existing assembly implementation.

use super::*;

pub(super) struct Skeleton<'a> {
    pub assembly: Assembly,
    pub paths: BTreeMap<&'a str, Located>,
}

impl<'a> Skeleton<'a> {
    pub fn build(
        document: &'a Value,
        positions: &BTreeMap<String, Placement3x4>,
    ) -> Result<Self, String> {
        let rows = document
            .pointer("/model/occurrences")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        if rows.len() > MAX_PLACEMENTS {
            return Err(format!("it places over {MAX_PLACEMENTS} components"));
        }
        let mut children: BTreeMap<Option<&str>, Vec<&Value>> = BTreeMap::new();
        for row in rows {
            let parent = row.pointer("/parent/occurrence").and_then(Value::as_str);
            children.entry(parent).or_default().push(row);
        }
        for children in children.values_mut() {
            children.sort_by_key(|row| {
                (
                    row.get("ordinal").and_then(Value::as_u64),
                    row.get("id").and_then(Value::as_str),
                )
            });
        }
        let mut paths = BTreeMap::new();
        let bodies: BTreeSet<_> = document
            .pointer("/model/product_definitions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|row| {
                row.get("bodies")
                    .and_then(Value::as_array)
                    .is_some_and(|b| !b.is_empty())
            })
            .filter_map(|row| row.get("id").and_then(Value::as_str))
            .collect();
        let mut assembly = level(
            None,
            &children,
            positions,
            &bodies,
            &mut paths,
            &mut Vec::new(),
            0,
        )?;
        if paths.len() != rows.len() {
            return Err(
                "some positioned occurrences have no unique parent or form a cycle".to_owned(),
            );
        }
        let occurrences: Vec<_> = rows.iter().collect();
        let worlds: BTreeMap<&str, Placement3x4> =
            positions.iter().map(|(id, p)| (id.as_str(), *p)).collect();
        let (joints, _) = joints(
            document
                .pointer("/model/assembly_joints")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            &paths,
            &worlds,
            &inserted_designs(&occurrences),
        );
        assembly.joints = joints;
        assembly.held = paths
            .iter()
            .filter(|(id, _)| id.contains("occurrence#root-"))
            .map(|(_, p)| p.path.clone())
            .collect();
        Ok(Self { assembly, paths })
    }
    pub fn solve(&mut self, posed: &BTreeSet<String>) {
        let paths = self
            .paths
            .iter()
            .filter(|(id, _)| posed.contains(**id))
            .map(|(_, p)| p.path.clone())
            .collect();
        let _ = place_by_joints(&mut self.assembly, &paths);
    }
    pub fn worlds(&self) -> BTreeMap<String, Placement3x4> {
        self.paths
            .iter()
            .filter_map(|(id, p)| Some(((*id).to_owned(), world_at(&self.assembly, &p.path)?)))
            .collect()
    }
}
#[allow(clippy::too_many_arguments)]
fn level<'a>(
    parent: Option<&str>,
    children: &BTreeMap<Option<&str>, Vec<&'a Value>>,
    positions: &BTreeMap<String, Placement3x4>,
    bodies: &BTreeSet<&str>,
    paths: &mut BTreeMap<&'a str, Located>,
    path: &mut Vec<usize>,
    depth: usize,
) -> Result<Assembly, String> {
    if depth > MAX_DEPTH {
        return Err("part positions nest too deeply".to_owned());
    }
    let mut assembly = Assembly {
        own_part: None,
        parts: Vec::new(),
        definitions: Vec::new(),
        identities: Vec::new(),
        placements: Vec::new(),
        joints: Vec::new(),
        left_out: Vec::new(),
        held: Vec::new(),
        joint_notes: Vec::new(),
        stored_placements: Vec::new(),
    };
    let outer = parent
        .and_then(|id| positions.get(id))
        .copied()
        .unwrap_or(IDENTITY);
    for row in children.get(&parent).into_iter().flatten() {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .ok_or("a placed occurrence has no identity")?;
        let at = positions
            .get(id)
            .ok_or("a placed occurrence has no position")?;
        let index = assembly.placements.len();
        path.push(index);
        paths.insert(
            id,
            Located {
                path: path.clone(),
                own: path.clone(),
                shared: None,
            },
        );
        let inner = if children.contains_key(&Some(id)) {
            let mut inner = level(
                Some(id),
                children,
                positions,
                bodies,
                paths,
                path,
                depth + 1,
            )?;
            if definition_key(row).is_some_and(|id| bodies.contains(id)) {
                let own = inner.parts.len();
                inner.parts.push(Model {
                    ir: cadmpeg_ir::CadIr::empty(),
                    fidelity: None,
                    name: String::new(),
                    source_file: None,
                    assembly: None,
                    source_step: None,
                });
                inner.placements.push(Placement {
                    hidden: false,
                    name: None,
                    part: own,
                    transform: IDENTITY,
                });
                inner.own_part = Some(own);
            }
            Some(inner)
        } else {
            None
        };
        path.pop();
        assembly.parts.push(Model {
            ir: cadmpeg_ir::CadIr::empty(),
            fidelity: None,
            name: String::new(),
            source_file: None,
            assembly: inner,
            source_step: None,
        });
        assembly.placements.push(Placement {
            hidden: false,
            name: None,
            part: index,
            transform: compose(&inverse(&outer), at),
        });
    }
    Ok(assembly)
}
