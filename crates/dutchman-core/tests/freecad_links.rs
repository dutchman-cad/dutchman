//! CC0 Assembly workbench recipe A01, authored in FreeCAD 1.1.4.
#![cfg(feature = "freecad")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use std::path::{Path, PathBuf};

use dutchman_core::{read, read_in, Assembly, Format, Model, PartSearch, Report};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/freecad-1.1.4-A01")
}

fn leaves<'a>(model: &'a Model, x: f64, out: &mut Vec<(&'a Model, f64)>) {
    if let Some(assembly) = &model.assembly {
        for placement in &assembly.placements {
            let part = &assembly.parts[placement.part];
            leaves(part, x + placement.transform[0][3], out);
        }
    } else if !model.ir.model.bodies.is_empty() {
        out.push((model, x));
    }
}

fn grounds(assembly: &Assembly) -> usize {
    assembly
        .joints
        .iter()
        .filter(|joint| joint.a.is_empty() || joint.b.is_empty())
        .count()
        + assembly
            .parts
            .iter()
            .filter_map(|part| part.assembly.as_ref())
            .map(grounds)
            .sum::<usize>()
}

#[test]
fn external_body_links_keep_only_the_named_shapes_and_stored_placements() {
    let model = read(
        Format::FreeCadDocument,
        &fixtures().join("A01a.FCStd"),
        &mut Report::default(),
    )
    .unwrap();
    let assembly = model.assembly.as_ref().unwrap();
    assert_eq!(
        grounds(assembly),
        1,
        "grounding must use the existing rigid joint mapper"
    );
    let mut parts = Vec::new();
    leaves(&model, 0.0, &mut parts);
    assert_eq!(parts.len(), 2);
    parts.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(
        parts.iter().map(|(_, x)| *x).collect::<Vec<_>>(),
        [0.0, 70.0]
    );
    assert!(parts
        .iter()
        .all(|(part, _)| part.ir.model.bodies.len() == 1));
    // The box's vertex bounds from manifest.json; the second part has curved extrema,
    // whose bounds and volumes are checked through SolidCraft in the writer regression.
    let part = parts[0].0;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for vertex in &part.ir.model.vertices {
        let point = part
            .ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position()
            .get();
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    assert_eq!(low, [-10.0, -15.0, 0.0]);
    assert_eq!(high, [10.0, 15.0, 10.0]);
}

#[test]
fn a_missing_sibling_is_a_named_loss_and_other_components_still_convert() {
    let dir = tempfile::tempdir().unwrap();
    for file in ["A01a.FCStd", "P01a.FCStd"] {
        std::fs::copy(fixtures().join(file), dir.path().join(file)).unwrap();
    }
    let mut report = Report::default();
    let model = read_in(
        Format::FreeCadDocument,
        &dir.path().join("A01a.FCStd"),
        &PartSearch::project(dir.path()),
        &mut report,
    )
    .unwrap();
    let mut parts = Vec::new();
    leaves(&model, 0.0, &mut parts);
    assert_eq!(parts.len(), 1);
    let completeness = model.assembly.unwrap().completeness();
    assert!(completeness
        .left_out
        .iter()
        .any(|left| left.file.as_deref() == Some("P02a.FCStd")));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.to_string().contains("P02a.FCStd")));
}

#[test]
fn array_elements_are_placed_once_without_merging_coincident_parts() {
    let model = read(
        Format::FreeCadDocument,
        &fixtures().parent().unwrap().join("product_assembly.FCStd"),
        &mut Report::default(),
    )
    .unwrap();
    fn count(model: &Model) -> usize {
        model.assembly.as_ref().map_or(0, |assembly| {
            assembly
                .placements
                .iter()
                .filter(|placement| {
                    placement.name.as_deref() == Some("fcstd:native:object#PinArray")
                })
                .count()
                + assembly.parts.iter().map(count).sum::<usize>()
        })
    }
    assert_eq!(count(&model), 3);
    fn no_element(model: &Model) {
        if let Some(assembly) = &model.assembly {
            for placement in &assembly.placements {
                assert!(!placement
                    .name
                    .as_deref()
                    .unwrap_or("")
                    .contains("PinArray_i"));
            }
            for part in &assembly.parts {
                no_element(part);
            }
        }
    }
    no_element(&model);
}

fn copy_assembly_with_link(dir: &Path, old: &str, new: &str) {
    use std::io::{Read, Write};
    let input = std::fs::File::open(fixtures().join("A01a.FCStd")).unwrap();
    let mut archive = zip::ZipArchive::new(input).unwrap();
    let output = std::fs::File::create(dir.join("A01a.FCStd")).unwrap();
    let mut writer = zip::ZipWriter::new(output);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name == "Document.xml" {
            bytes = String::from_utf8(bytes)
                .unwrap()
                .replace(old, new)
                .into_bytes();
        }
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn external_link_cycles_are_reported_and_do_not_hide_the_other_part() {
    let dir = tempfile::tempdir().unwrap();
    copy_assembly_with_link(dir.path(), "file=\"P01a.FCStd\"", "file=\"A01a.FCStd\"");
    std::fs::copy(fixtures().join("P02a.FCStd"), dir.path().join("P02a.FCStd")).unwrap();
    let mut report = Report::default();
    let model = read_in(
        Format::FreeCadDocument,
        &dir.path().join("A01a.FCStd"),
        &PartSearch::project(dir.path()),
        &mut report,
    )
    .unwrap();
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.to_string().contains("places itself")));
    let mut parts = Vec::new();
    leaves(&model, 0.0, &mut parts);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].1, 70.0);
}

#[test]
fn external_links_use_search_folders_and_case_insensitive_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let project = dir.path().join("project");
    std::fs::create_dir(&library).unwrap();
    std::fs::create_dir(&project).unwrap();
    std::fs::copy(fixtures().join("A01a.FCStd"), project.join("A01a.FCStd")).unwrap();
    for file in ["P01a.FCStd", "P02a.FCStd"] {
        std::fs::copy(
            fixtures().join(file),
            library.join(file.to_ascii_lowercase()),
        )
        .unwrap();
    }
    let model = read_in(
        Format::FreeCadDocument,
        &project.join("A01a.FCStd"),
        &PartSearch {
            threads: 0,
            project: Some(project.clone()),
            also: vec![library],
        },
        &mut Report::default(),
    )
    .unwrap();
    let mut parts = Vec::new();
    leaves(&model, 0.0, &mut parts);
    assert_eq!(parts.len(), 2);
    assert!(model.assembly.unwrap().completeness().left_out.is_empty());
}

#[test]
fn repeated_external_targets_share_a_component_and_keep_both_occurrences() {
    let dir = tempfile::tempdir().unwrap();
    copy_assembly_with_link(dir.path(), "file=\"P02a.FCStd\"", "file=\"P01a.FCStd\"");
    std::fs::copy(fixtures().join("P01a.FCStd"), dir.path().join("P01a.FCStd")).unwrap();
    let model = read_in(
        Format::FreeCadDocument,
        &dir.path().join("A01a.FCStd"),
        &PartSearch::project(dir.path()),
        &mut Report::default(),
    )
    .unwrap();
    let top = model.assembly.as_ref().unwrap();
    let components = top.parts[top.placements[0].part].assembly.as_ref().unwrap();
    assert_eq!(components.placements.len(), 2);
    assert_eq!(components.parts.len(), 1);
    assert_eq!(components.placements[0].part, components.placements[1].part);
    let mut parts = Vec::new();
    leaves(&model, 0.0, &mut parts);
    assert_eq!(parts.len(), 2);
    assert_eq!(
        parts.iter().map(|(_, x)| *x).collect::<Vec<_>>(),
        [0.0, 70.0]
    );
}

#[test]
fn an_assembly_with_all_linked_files_missing_still_reports_each_omission() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(fixtures().join("A01a.FCStd"), dir.path().join("A01a.FCStd")).unwrap();
    let model = read_in(
        Format::FreeCadDocument,
        &dir.path().join("A01a.FCStd"),
        &PartSearch::project(dir.path()),
        &mut Report::default(),
    )
    .unwrap();
    let mut parts = Vec::new();
    leaves(&model, 0.0, &mut parts);
    assert!(parts.is_empty());
    let completeness = model.assembly.unwrap().completeness();
    assert_eq!(completeness.left_out.len(), 2);
    assert!(completeness.left_out.iter().all(|left| left.count == 1));
}
