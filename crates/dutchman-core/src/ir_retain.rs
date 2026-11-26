//! Rows of a decoded document picked by identity, without writing the document out as JSON and
//! reading it back. The JSON round trip pulled every `Deserialize` implementation of the
//! reader into the SolidCraft plugin; selecting typed rows needs none of them.

use cadmpeg_ir::document::{Design, Model, Topology};
use cadmpeg_ir::schema::ArenaRecord;

fn pick<T: ArenaRecord + Clone>(rows: &[T], keep: &dyn Fn(&str) -> bool) -> Vec<T> {
    rows.iter()
        .filter(|row| keep(row.record_identity()))
        .cloned()
        .collect()
}

macro_rules! topology_arenas {
    ($($field:ident),* $(,)?) => {
        /// The rows of `source` whose identity `keep` accepts, arena by arena and in order.
        /// Naming every arena here is checked: a new arena in the reader stops this compiling.
        pub(crate) fn geometry(source: &Topology, keep: &dyn Fn(&str) -> bool) -> Topology {
            let Topology { $($field),* } = source;
            Topology { $($field: pick($field, keep)),* }
        }

        /// `topology` without the rows whose identity `keep` refuses.
        pub(crate) fn retain_geometry(topology: &mut Topology, keep: &dyn Fn(&str) -> bool) {
            $(topology.$field.retain(|row| keep(row.record_identity()));)*
        }
    };
}

topology_arenas!(
    bodies,
    regions,
    shells,
    faces,
    loops,
    coedges,
    edges,
    vertices,
    points,
    surfaces,
    curves,
    subds,
    pcurves,
    procedural_surfaces,
    procedural_curves,
);

macro_rules! design_arenas {
    ($($field:ident),* $(,)?) => {
        /// The design records of `source` whose identity `keep` accepts, and the names of the
        /// entities `keep` accepts. A design record shares the identity of the `model` record it
        /// restates, so a slice that keeps a model row keeps its design record. Naming every
        /// arena here is checked: a new arena in the reader stops this compiling.
        pub(crate) fn design(source: &Design, keep: &dyn Fn(&str) -> bool) -> Design {
            let Design { $($field),*, names } = source;
            Design {
                $($field: pick($field, keep)),*,
                names: names
                    .entries()
                    .iter()
                    .filter(|(entity, _)| keep(entity.id()))
                    .map(|(entity, entry)| (entity.clone(), entry.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into(),
            }
        }
    };
}

design_arenas!(
    scopes,
    containers,
    features,
    sketches,
    sketch_entities,
    sketch_constraints,
    spatial_sketches,
    spatial_sketch_entities,
    spatial_sketch_constraints,
    parameters,
    spreadsheets,
    sheet_metal_rules,
    programs,
    configurations,
    arrangements,
    publications,
    evaluations,
);

macro_rules! model_arenas {
    ($($field:ident),* $(,)?) => {
        /// The `model` home arenas [`model`] selects from, by name.
        #[cfg(test)]
        pub(crate) const MODEL_ARENAS: &[&str] = &[$(stringify!($field)),*];

        /// The rows of `source` whose identity `keep` accepts, arena by arena and in order,
        /// with the regeneration predecessor of each kept feature whose predecessor is kept.
        pub(crate) fn model(source: &Model, keep: &dyn Fn(&str) -> bool) -> Result<Model, String> {
            let mut selected = Model::default();
            $(selected.$field = pick(&source.$field, keep);)*
            let edges: Vec<_> = selected
                .features
                .iter()
                .filter_map(|feature| {
                    let parent = source.feature_regeneration_parent(&feature.id)?;
                    keep(parent.as_str()).then(|| (feature.id.clone(), parent.clone()))
                })
                .collect();
            for (child, parent) in edges {
                selected
                    .set_feature_regeneration_parent(&child, &parent)
                    .map_err(|err| err.to_string())?;
            }
            Ok(selected)
        }
    };
}

model_arenas!(
    assets,
    features,
    feature_input_topologies,
    feature_result_topologies,
    configurations,
    parameters,
    sketches,
    sketch_entities,
    sketch_constraints,
    spatial_sketches,
    spatial_sketch_entities,
    spatial_sketch_constraints,
    spreadsheets,
    product_definitions,
    occurrences,
    assembly_joints,
    drawings,
    semantic_annotations,
    presentation_documents,
    view_presentations,
    tessellations,
    appearances,
    appearance_bindings,
    attributes,
    pmi,
    presentation_layers,
    physical_materials,
    mass_properties,
    rigid_bodies,
    colliders,
    articulations,
    physics_scenes,
    solver_settings,
    declared_measures,
    point_clouds,
    history_bodies,
);

#[cfg(test)]
mod tests {
    use cadmpeg_ir::document::{ArenaHome, ArenaName};

    /// Every arena of the `model` home is named, so none is silently dropped from a slice.
    #[test]
    fn every_model_arena_is_selected() {
        let mut registered: Vec<&str> = ArenaName::ALL
            .iter()
            .filter(|arena| arena.home() == ArenaHome::Model)
            .map(|arena| arena.as_str())
            .collect();
        let mut named: Vec<&str> = super::MODEL_ARENAS.to_vec();
        registered.sort_unstable();
        named.sort_unstable();
        assert_eq!(registered, named);
    }
}
