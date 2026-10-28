//! Decoding native files through cadmpeg, on synthetic fixtures only.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use dutchman_core::{read, Error, Format, Report};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dutchman-decode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn decodes_a_fusion_unit_cube() {
    let mut report = Report::default();
    let model = read(
        Format::FusionArchive,
        &fixture("generated_unit_cube.f3d"),
        &mut report,
    )
    .unwrap();

    assert_eq!(model.name, "generated_unit_cube");
    assert_eq!(model.ir.model.bodies.len(), 1);
    assert_eq!(model.ir.model.faces.len(), 6);
    assert!(model.fidelity.is_some());
}

#[test]
fn decodes_an_inventor_unit_cube() {
    let mut report = Report::default();
    let model = read(
        Format::InventorPart,
        &fixture("synthetic_cube.ipt"),
        &mut report,
    )
    .unwrap();

    assert_eq!(model.name, "synthetic_cube");
    assert_eq!(model.source_file.as_deref(), Some("synthetic_cube.ipt"));
    assert_eq!(model.ir.model.bodies.len(), 1);
    assert_eq!(model.ir.model.faces.len(), 6);
}

#[test]
fn decodes_a_solidworks_unit_cube() {
    let mut report = Report::default();
    let model = read(
        Format::SolidWorksPart,
        &fixture("solidworks_cube.sldprt"),
        &mut report,
    )
    .unwrap();

    assert_eq!(model.name, "solidworks_cube");
    assert_eq!(model.source_file.as_deref(), Some("solidworks_cube.sldprt"));
    assert_eq!(model.ir.model.bodies.len(), 1);
    assert_eq!(model.ir.model.faces.len(), 6);
    assert!(
        model.ir.model.features.is_empty(),
        "the cube has no history"
    );
    assert!(model.fidelity.is_some(), "the native envelope is retained");
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("sldprt/source.dialect-unverified")),
        "cadmpeg's losses are kept: {:?}",
        report.losses()
    );
}

#[test]
fn a_solidworks_file_named_f3d_is_refused() {
    let bytes = std::fs::read(fixture("solidworks_cube.sldprt")).unwrap();
    let path = scratch("renamed.f3d", &bytes);
    let err = read(Format::FusionArchive, &path, &mut Report::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "cannot read this design: its contents are SolidWorks part (.sldprt), not Fusion archive \
         (.f3d); rename it to match"
    );
}

#[test]
fn a_fusion_file_named_ipt_is_refused() {
    let bytes = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let path = scratch("renamed.ipt", &bytes);
    let err = read(Format::InventorPart, &path, &mut Report::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "cannot read this design: its contents are Fusion archive (.f3d), not Inventor part \
         (.ipt); rename it to match"
    );
}

#[test]
fn a_file_without_geometry_is_an_error() {
    for (format, file) in [
        (Format::FusionArchive, "container_metadata_only.f3d"),
        (Format::SolidWorksPart, "container_metadata.sldprt"),
    ] {
        let mut report = Report::default();
        let err = read(format, &fixture(file), &mut report).unwrap_err();
        assert!(
            err.to_string().contains("no geometry could be read"),
            "{file}: {err}"
        );
    }
}

#[test]
fn hostile_inputs_are_errors_not_panics() {
    let cube = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let mut flipped = cube.clone();
    for byte in flipped.iter_mut().skip(64).step_by(97) {
        *byte ^= 0x5a;
    }
    let cases: [(&str, &[u8]); 5] = [
        ("empty.f3d", b""),
        ("text.f3d", b"ISO-10303-21;\nHEADER;\nENDSEC;\n"),
        ("noise.f3d", &[0xff; 4096]),
        ("truncated.f3d", cube.get(..cube.len() / 2).unwrap()),
        ("flipped.f3d", &flipped),
    ];
    for (name, bytes) in cases {
        let path = scratch(name, bytes);
        let mut report = Report::default();
        let result = read(Format::FusionArchive, &path, &mut report);
        assert!(
            matches!(result, Err(Error::Decode(_))),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn formats_without_a_decoder_are_not_supported_yet() {
    let path = scratch("assembly.sldasm", b"anything");
    let mut report = Report::default();
    let err = read(Format::SolidWorksAssembly, &path, &mut report).unwrap_err();
    assert!(matches!(
        err,
        Error::NotSupportedYet {
            format: Format::SolidWorksAssembly
        }
    ));
}

#[test]
fn recognises_all_format_extensions_and_shared_part_contents() {
    // Synthetic signatures only; no vendor files are needed to identify the formats.
    for (name, bytes, expected) in [
        ("x.f3d", b"PK\x03\x04".as_slice(), Format::FusionArchive),
        ("x.f3z", b"PK\x03\x04", Format::FusionMultiDocument),
        ("x.ipt", b"\xd0\xcf\x11\xe0", Format::InventorPart),
        ("x.iam", b"\xd0\xcf\x11\xe0", Format::InventorAssembly),
        ("x.sldprt", b"\xd0\xcf\x11\xe0", Format::SolidWorksPart),
        ("x.sldasm", b"\xd0\xcf\x11\xe0", Format::SolidWorksAssembly),
        ("x.CATPart", b"V5_CFV2", Format::CatiaPart),
        ("creo.prt", b"#UGC:2", Format::CreoPart),
        ("creo.prt.12", b"#UGC:1", Format::CreoPart),
        ("nx.prt", b"SPLMSSTR", Format::NxPart),
        ("nx.prt.3", b"SPLMSSTR", Format::NxPart),
        ("x.pvz", b"PK\x03\x04", Format::CreoView),
        ("x.3dm", b"3D Geometry File Format       80", Format::Rhino),
        ("x.sat", b"700 0 0 0\n", Format::Acis),
        ("x.sab", b"ACIS BinaryFile", Format::Acis),
        ("x.smt", b"700 0 0 0\n", Format::Acis),
        ("x.smb", b"ASM BinaryFile", Format::Acis),
        ("x.step", b"ISO-10303-21;", Format::Step),
        ("x.stp", b"ISO-10303-21;", Format::Step),
        (
            "x.igs",
            b"                                                                        S      1",
            Format::Iges,
        ),
        (
            "x.iges",
            b"                                                                        S      1",
            Format::Iges,
        ),
        ("x.FCStd", b"PK\x03\x04", Format::FreeCadDocument),
        ("unknown.prt", b"unknown", Format::UnclassifiedPart),
        ("unknown.prt.7", b"unknown", Format::UnclassifiedPart),
    ] {
        assert_eq!(
            Format::from_path(&scratch(name, bytes)),
            Some(expected),
            "{name}"
        );
        assert_eq!(
            Format::from_path(&scratch(&name.to_ascii_uppercase(), bytes)),
            Some(expected),
            "{name}"
        );
    }
    assert_eq!(Format::from_path(Path::new("x.txt")), None);
    assert_eq!(Format::from_path(Path::new("x.prt.3.7")), None);
}

#[test]
fn a_numbered_save_strips_one_suffix_and_uses_contents_for_its_program() {
    let path = scratch("bracket.prt.7", b"#UGC:2");
    let (part, revision) = Format::part_revision(&path).unwrap();
    assert_eq!(part.file_name().unwrap(), "bracket.prt");
    assert_eq!(revision, "7");
    assert_eq!(Format::source_extension(&path).as_deref(), Some("prt.7"));
    for (bytes, format, words) in [
        (
            b"#UGC:2".as_slice(),
            Format::CreoPart,
            "opens, no geometry yet",
        ),
        (
            b"SPLMSSTR".as_slice(),
            Format::NxPart,
            "opens, no geometry yet",
        ),
        (
            b"unknown".as_slice(),
            Format::UnclassifiedPart,
            "recognised, not read yet",
        ),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(Format::from_path(&path), Some(format));
        assert_eq!(
            format.status_for(&path),
            format!("{} .prt.7: {words}", format.family())
        );
    }
    assert!(Format::part_revision(Path::new("bracket.prt.3.7")).is_none());
    assert!(Format::part_revision(Path::new("bracket.prt.x")).is_none());
}

#[test]
fn conversion_requires_geometry_and_an_available_codec() {
    for format in Format::ALL {
        let geometry = !matches!(
            format,
            Format::CreoPart
                | Format::NxPart
                | Format::SolidWorksAssembly
                | Format::UnclassifiedPart
                | Format::Iges
        );
        assert_eq!(format.reads_geometry(), geometry, "{}", format.name());
        assert_eq!(
            format.can_convert(),
            geometry && format.codec_id().is_some()
        );
    }
}

#[test]
fn published_headlines_are_independent_of_codec_availability() {
    assert_eq!(
        Format::FusionArchive.headline_level().as_deref(),
        Some("L5")
    );
    assert_eq!(
        Format::FreeCadDocument.headline_level().as_deref(),
        Some("L5")
    );
    assert_eq!(Format::Iges.headline_level().as_deref(), Some("L9"));
    assert_eq!(Format::SolidWorksAssembly.headline_level(), None);
    assert_eq!(Format::UnclassifiedPart.headline_level(), None);
}

#[test]
fn inputs_without_geometry_are_refused_before_decoding() {
    for (name, bytes, format) in [
        ("low-creo.prt", b"#UGC:2".as_slice(), Format::CreoPart),
        ("low-nx.prt", b"SPLMSSTR".as_slice(), Format::NxPart),
    ] {
        let path = scratch(name, bytes);
        let err = read(format, &path, &mut Report::default()).unwrap_err();
        assert!(matches!(err, Error::NotSupportedYet { .. }));
        assert!(err.to_string().contains("opens, no geometry yet"));
    }
}

#[test]
fn iges_is_recognised_and_refused_before_opening_the_file() {
    for suffix in ["igs", "iges", "IGS", "IGES"] {
        let path = scratch(&format!("refused.{suffix}"), b"not IGES data");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Format::from_path(&path), Some(Format::Iges));
        assert_eq!(
            Format::Iges.status_for(&path),
            format!("IGES .{suffix}: open it in SolidCraft directly")
        );
        let err = read(Format::Iges, &path, &mut Report::default()).unwrap_err();
        assert!(matches!(
            err,
            Error::NotSupportedYet {
                format: Format::Iges
            }
        ));
        assert_eq!(
            err.to_string(),
            "Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import)."
        );
        assert_eq!(err.upstream(), None);
    }
    assert!(!Format::Iges.can_convert());
    assert!(!Format::supported_names().contains("IGES"));
}

#[test]
fn progress_counts_successful_documents_and_preserves_read_results() {
    use dutchman_core::{read_with_progress, PartSearch, Progress, Stage};
    use std::sync::Mutex;
    let events = Mutex::new(Vec::new());
    let callback = |event| events.lock().unwrap().push(event);
    let progress = Progress::new(&callback);
    let model = read_with_progress(
        Format::FusionArchive,
        &fixture("generated_unit_cube.f3d"),
        &PartSearch::default(),
        &mut Report::default(),
        Some(&progress),
    )
    .unwrap();
    assert_eq!(model.ir.model.bodies.len(), 1);
    let events = events.into_inner().unwrap();
    assert_eq!(events.last().unwrap().designs_read, 1);
    assert_eq!(events.last().unwrap().designs_total, Some(1));
    let decode = events
        .iter()
        .position(|event| event.stage == Stage::Decode)
        .unwrap();
    let project = events
        .iter()
        .position(|event| event.stage == Stage::Project)
        .unwrap();
    assert!(decode < project);
    assert!(events
        .iter()
        .take(project)
        .all(|event| event.designs_read == 0));

    let events = Mutex::new(Vec::new());
    let callback = |event| events.lock().unwrap().push(event);
    let progress = Progress::new(&callback);
    assert!(read_with_progress(
        Format::FusionArchive,
        &fixture("container_metadata_only.f3d"),
        &PartSearch::default(),
        &mut Report::default(),
        Some(&progress),
    )
    .is_err());
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .all(|event| event.designs_read == 0));
}
