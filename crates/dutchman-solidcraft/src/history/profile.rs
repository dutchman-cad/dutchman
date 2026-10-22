//! Which closed profiles of a sketch a feature uses, as a SolidCraft profile selection.

use serde_json::{json, Value};

use super::sketch::Mapped;
use super::Context;

/// The sketch a profile reference uses, and the SolidCraft profile selection for it.
pub(super) fn profile(profile: Option<&Value>, ctx: &Context) -> Result<(String, Value), String> {
    let profile = profile.ok_or("it has no profile")?;
    let kind = profile
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let value = profile.get("value").unwrap_or(&Value::Null);
    if kind == "sketch" {
        let sketch = value.as_str().ok_or("its profile sketch is malformed")?;
        return Ok((sketch.to_owned(), json!({"type": "all"})));
    }
    let sketch = value
        .get("sketch")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{kind} profiles are not mapped yet"))?;
    let mapped = ctx
        .sketches
        .get(sketch)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let selection = match kind {
        "sketch_profiles" => {
            let indices = value
                .get("profiles")
                .and_then(Value::as_array)
                .ok_or("its profile selection is malformed")?;
            let loops = indices
                .iter()
                .map(|index| source_loop(ctx, sketch, index, mapped))
                .collect::<Result<Vec<_>, _>>()?;
            let mut selected = Vec::new();
            for (index, ring) in indices.iter().zip(loops) {
                if needs_split(ctx, sketch, index, &ring) {
                    match split_loop(ctx, sketch, index, mapped) {
                        Ok(rings) => selected.extend(rings),
                        // Preserve the existing whole-sketch mapping when every source
                        // loop is taken, including arrangements we cannot split yet.
                        Err(_) if takes_every_loop(ctx, sketch, indices) => {
                            return Ok((sketch.to_owned(), json!({"type": "all"})));
                        }
                        Err(reason) => return Err(reason),
                    }
                } else {
                    selected.push(ring);
                }
            }
            curves(selected)?
        }
        "sketch_regions" => regions(value.get("regions"), ctx, sketch, mapped)?,
        "sketch_entities" => entities(value.get("entities"), mapped)?,
        // Native picks carry no neutral point or index. Infer only a single material
        // region, including a surrounding loop with holes under even-odd nesting.
        "sketch_selection" => {
            let count = profile_count(ctx, sketch);
            let source = ctx
                .row("sketches", sketch)
                .ok_or("its sketch has no profiles")?;
            let entities = sketch_entities(ctx, sketch);
            if count == 1 {
                json!({"type": "all"})
            } else if let Some(outer) = super::crossing::single_region(source, &entities) {
                let boundary = source_loop(ctx, sketch, &json!(outer), mapped)?;
                let holes = (0..count)
                    .filter(|index| *index != outer)
                    .map(|index| source_loop(ctx, sketch, &json!(index), mapped))
                    .collect::<Result<Vec<_>, _>>()?;
                json!({"type": "whole_boundaries", "profiles": [{"outer": boundary, "holes": holes}]})
            } else {
                return Err(format!(
                    "a profile cadmpeg does not resolve, in a sketch of {count} profiles, is not \
                     mapped yet"
                ));
            }
        }
        other => return Err(format!("{other} profiles are not mapped yet")),
    };
    Ok((sketch.to_owned(), selection))
}

/// Whether a loop runs along one of its curves more than once.
fn crossed(curves: &[String]) -> bool {
    curves
        .iter()
        .enumerate()
        .any(|(index, curve)| curves.iter().skip(index + 1).any(|other| other == curve))
}

/// How many profile loops the source sketch has.
fn profile_count(ctx: &Context, sketch: &str) -> usize {
    ctx.row("sketches", sketch)
        .and_then(|row| row.get("profiles"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn takes_every_loop(ctx: &Context, sketch: &str, indices: &[Value]) -> bool {
    (0..profile_count(ctx, sketch)).all(|wanted| {
        indices
            .iter()
            .any(|index| index.as_u64().and_then(|i| usize::try_from(i).ok()) == Some(wanted))
    })
}

fn needs_split(ctx: &Context, sketch: &str, index: &Value, ring: &[String]) -> bool {
    if crossed(ring) {
        return true;
    }
    source_members(ctx, sketch, index)
        .is_ok_and(|members| super::crossing::crossed_loop(members, &sketch_entities(ctx, sketch)))
}

/// Source entities belonging to this sketch.
fn sketch_entities<'a>(ctx: &'a Context, sketch: &str) -> Vec<&'a Value> {
    ctx.arena("sketch_entities")
        .iter()
        .filter(|entity| entity.get("sketch").and_then(Value::as_str) == Some(sketch))
        .collect()
}

fn split_loop(
    ctx: &Context,
    sketch: &str,
    index: &Value,
    mapped: &Mapped,
) -> Result<Vec<Vec<String>>, String> {
    let members = source_members(ctx, sketch, index)?;
    let source = sketch_entities(ctx, sketch);
    let sketch = ctx
        .row("sketches", sketch)
        .ok_or("its sketch has no profiles")?;
    let construction = super::crossing::crossing(sketch, &source);
    let entities: Vec<&Value> = source
        .into_iter()
        .filter(|entity| {
            !entity
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| construction.contains(id))
        })
        .collect();
    let rings = super::crossing::split(members, &entities)
        .ok_or("a profile crossed by its own sketch's curves is not mapped yet")?;
    mapped.boundaries(rings)
}

/// Profiles whose outer loops are these curve loops.
fn curves(loops: Vec<Vec<String>>) -> Result<Value, String> {
    if loops.is_empty() {
        return Err("it selects no profile".to_owned());
    }
    Ok(json!({"type": "curves", "loops": loops}))
}

/// Loop `index` of the source sketch's profile loops, as SolidCraft curve ids.
fn source_loop(
    ctx: &Context,
    sketch: &str,
    index: &Value,
    mapped: &Mapped,
) -> Result<Vec<String>, String> {
    source_members(ctx, sketch, index)?
        .iter()
        .map(|uses| curve_id(uses.get("entity"), mapped))
        .collect()
}

fn source_members<'a>(
    ctx: &'a Context,
    sketch: &str,
    index: &Value,
) -> Result<&'a [Value], String> {
    let loops = ctx
        .row("sketches", sketch)
        .and_then(|row| row.get("profiles"))
        .and_then(Value::as_array)
        .ok_or("its sketch has no profiles")?;
    index
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| loops.get(i))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| "it selects a sketch profile that does not exist".to_owned())
}

/// The SolidCraft curve id of a source entity.
fn curve_id(entity: Option<&Value>, mapped: &Mapped) -> Result<String, String> {
    entity
        .and_then(Value::as_str)
        .and_then(|entity| mapped.curve(entity))
        .map(|curve| curve.id.clone())
        .ok_or_else(|| "its profile uses a curve the sketch does not carry".to_owned())
}

/// Profile regions. Regions made of whole source loops become SolidCraft's exact whole
/// boundaries (outer loop and holes); when any region is bounded by pieces of curves, every
/// region is selected by the curves of its outer boundary, which SolidCraft also matches
/// against pieces.
fn regions(
    regions: Option<&Value>,
    ctx: &Context,
    sketch: &str,
    mapped: &Mapped,
) -> Result<Value, String> {
    let regions = regions
        .and_then(Value::as_array)
        .filter(|regions| !regions.is_empty())
        .ok_or("its profile regions are malformed")?;
    let mut whole = Vec::with_capacity(regions.len());
    let mut outers = Vec::with_capacity(regions.len());
    let mut trimmed = false;
    for region in regions {
        match region.get("region").and_then(Value::as_str) {
            Some("loops") => {
                let outer = source_loop(
                    ctx,
                    sketch,
                    region.get("outer").unwrap_or(&Value::Null),
                    mapped,
                )?;
                let holes = region
                    .get("holes")
                    .and_then(Value::as_array)
                    .map(|holes| {
                        holes
                            .iter()
                            .map(|hole| source_loop(ctx, sketch, hole, mapped))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let index = region.get("outer").unwrap_or(&Value::Null);
                if needs_split(ctx, sketch, index, &outer) {
                    if !holes.is_empty() {
                        return Err(
                            "a profile crossed by its own sketch's curves is not mapped yet"
                                .to_owned(),
                        );
                    }
                    trimmed = true;
                    outers.extend(split_loop(ctx, sketch, index, mapped)?);
                } else {
                    outers.push(outer.clone());
                    whole.push(json!({"outer": outer, "holes": holes}));
                }
            }
            Some("trimmed") => {
                trimmed = true;
                let mut outer: Vec<String> = Vec::new();
                for boundary in region
                    .get("outer_boundary")
                    .and_then(Value::as_array)
                    .ok_or("its trimmed region is malformed")?
                {
                    let id = curve_id(boundary.get("entity"), mapped)?;
                    if !outer.contains(&id) {
                        outer.push(id);
                    }
                }
                outers.push(outer);
            }
            _ => return Err("its profile regions are malformed".to_owned()),
        }
    }
    if trimmed {
        curves(outers)
    } else {
        Ok(json!({"type": "whole_boundaries", "profiles": whole}))
    }
}

/// Profiles bounded by the selected curves: each connected group of them is one loop.
fn entities(entities: Option<&Value>, mapped: &Mapped) -> Result<Value, String> {
    let entities = entities
        .and_then(Value::as_array)
        .ok_or("its profile entities are malformed")?;
    let mut groups: Vec<(Vec<usize>, Vec<String>)> = Vec::new();
    for entity in entities {
        let id = entity
            .as_str()
            .ok_or("its profile entities are malformed")?;
        let curve = mapped
            .curve(id)
            .ok_or("its profile uses a curve the sketch does not carry")?;
        let points: Vec<usize> = [curve.start, curve.end].into_iter().flatten().collect();
        // The curve joins the first group it touches; any other group it touches joins that
        // one too. Curves keep their selection order.
        let touching: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter(|(_, (group, _))| points.iter().any(|p| group.contains(p)))
            .map(|(index, _)| index)
            .collect();
        match touching.split_first() {
            None => groups.push((points, vec![curve.id.clone()])),
            Some((&first, others)) => {
                for &other in others.iter().rev() {
                    if other < groups.len() {
                        let (other_points, other_curves) = groups.remove(other);
                        if let Some(group) = groups.get_mut(first) {
                            group.0.extend(other_points);
                            group.1.extend(other_curves);
                        }
                    }
                }
                if let Some(group) = groups.get_mut(first) {
                    group.0.extend(points);
                    group.1.push(curve.id.clone());
                }
            }
        }
    }
    curves(groups.into_iter().map(|(_, curves)| curves).collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::history::sketch::{Curve, Shape};

    fn line(index: usize, a: usize, b: usize) -> Shape {
        Shape::Curve(Curve {
            index,
            id: format!("l{}", index + 1),
            kind: "line",
            start: Some(a),
            end: Some(b),
            center: None,
        })
    }

    /// Two separate triangles (curves l1–l3 and l4–l6) and a circle c1.
    fn mapped() -> Mapped {
        let mut shapes = BTreeMap::new();
        for (i, (a, b)) in [(1, 2), (2, 3), (3, 1), (4, 5), (5, 6), (6, 4)]
            .into_iter()
            .enumerate()
        {
            shapes.insert(format!("e{i}"), line(i, a, b));
        }
        shapes.insert(
            "circle".to_owned(),
            Shape::Curve(Curve {
                index: 6,
                id: "c1".to_owned(),
                kind: "circle",
                start: None,
                end: None,
                center: Some(7),
            }),
        );
        Mapped {
            feature: 1,
            normal: [0.0, 0.0, 1.0],
            origin: [0.0; 3],
            x_dir: [1.0, 0.0, 0.0],
            y_dir: [0.0, 1.0, 0.0],
            points: Vec::new(),
            shapes,
        }
    }

    fn model() -> Value {
        json!({"sketches": [{"id": "s", "profiles": [
            [{"entity": "e0"}, {"entity": "e1"}, {"entity": "e2"}],
            [{"entity": "circle"}],
        ]}]})
    }

    fn select(model: &Value, profile: Value) -> Result<Value, String> {
        let mut ctx = Context::new(model);
        ctx.sketches.insert("s".to_owned(), mapped());
        super::profile(Some(&profile), &ctx).map(|(_, selection)| selection)
    }

    #[test]
    fn whole_loop_regions_keep_their_holes() {
        let model = model();
        let selection = select(
            &model,
            json!({"kind": "sketch_regions", "value": {"sketch": "s",
            "regions": [{"region": "loops", "outer": 0, "holes": [1]}]}}),
        );
        assert_eq!(
            selection.unwrap(),
            json!({"type": "whole_boundaries", "profiles": [
                {"outer": ["l1", "l2", "l3"], "holes": [["c1"]]}
            ]})
        );
    }

    #[test]
    fn trimmed_regions_select_by_their_outer_curves() {
        let model = model();
        let boundary = |e: &str| json!({"entity": e, "parameter_range": [0.0, 1.0]});
        let selection = select(
            &model,
            json!({"kind": "sketch_regions", "value": {"sketch": "s",
            "regions": [{"region": "trimmed", "outer_boundary": [
                boundary("e0"), boundary("e1"), boundary("e2"), boundary("e0")]}]}}),
        );
        assert_eq!(
            selection.unwrap(),
            json!({"type": "curves", "loops": [["l1", "l2", "l3"]]})
        );
    }

    #[test]
    fn selected_entities_group_into_connected_loops() {
        let model = model();
        let selection = select(
            &model,
            json!({"kind": "sketch_entities", "value": {"sketch": "s",
            "entities": ["e0", "e3", "e1", "e4", "e2", "e5", "circle"]}}),
        )
        .unwrap();
        assert_eq!(
            selection,
            json!({"type": "curves", "loops": [["l1", "l2", "l3"], ["l4", "l5", "l6"], ["c1"]]})
        );
    }

    #[test]
    fn a_crossed_profile_without_solved_geometry_is_refused() {
        // The first triangle with a curve inside it, which its loop runs along both ways (as a
        // rectangle with its diagonals drawn comes from Fusion), and the circle.
        let model = json!({"sketches": [{"id": "s", "profiles": [
            [{"entity": "e0"}, {"entity": "e3"}, {"entity": "e1"},
             {"entity": "e3", "reversed": true}, {"entity": "e2"}],
            [{"entity": "circle"}],
        ]}]});
        let every = select(
            &model,
            json!({"kind": "sketch_profiles", "value": {"sketch": "s", "profiles": [1, 0]}}),
        );
        assert_eq!(every.unwrap(), json!({"type": "all"}));
        let some = select(
            &model,
            json!({"kind": "sketch_profiles", "value": {"sketch": "s", "profiles": [0]}}),
        );
        assert_eq!(
            some.unwrap_err(),
            "a profile crossed by its own sketch's curves is not mapped yet"
        );
        // A loop that uses each curve once is selected by its curves, as before.
        let plain = select(
            &model,
            json!({"kind": "sketch_profiles", "value": {"sketch": "s", "profiles": [1]}}),
        );
        assert_eq!(plain.unwrap(), json!({"type": "curves", "loops": [["c1"]]}));
    }

    #[test]
    fn a_selection_cadmpeg_keeps_native_is_the_profile_of_a_one_profile_sketch() {
        let one = json!({"sketches": [{"id": "s", "profiles": [
            [{"entity": "e0"}, {"entity": "e1"}, {"entity": "e2"}],
        ]}]});
        let native = json!({"kind": "sketch_selection",
            "value": {"sketch": "s", "selections": ["native#1"]}});
        assert_eq!(
            select(&one, native.clone()).unwrap(),
            json!({"type": "all"})
        );
        assert_eq!(
            select(&model(), native).unwrap_err(),
            "a profile cadmpeg does not resolve, in a sketch of 2 profiles, is not mapped yet"
        );
    }

    #[test]
    fn unknown_or_missing_references_stop_the_history() {
        let model = model();
        assert!(select(&model, json!({"kind": "native", "value": "x"})).is_err());
        assert!(select(
            &model,
            json!({"kind": "sketch_entities",
            "value": {"sketch": "s", "entities": ["nope"]}})
        )
        .is_err());
        assert!(select(
            &model,
            json!({"kind": "sketch_profiles",
            "value": {"sketch": "s", "profiles": [7]}})
        )
        .is_err());
    }
}
