//! Solid Edge `.par`, `.psm` and `.asm` files: detection and conversion through cadmpeg's
//! Solid Edge codec, on documents built in code.
//!
//! The builders below follow cadmpeg's own synthetic Solid Edge documents (cadmpeg,
//! `crates/cadmpeg-codec-solidedge/src/test_support.rs`, Apache-2.0): a compound file whose root
//! holds the entries that identify a Solid Edge document, and one Parasolid partition holding a
//! 2 mm cube. No Solid Edge file is used.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use dutchman_core::{read_bytes, Error, Format, Report, Severity};

const CFB_END: u32 = 0xffff_fffe;
const CFB_FREE: u32 = 0xffff_ffff;
const CFB_FAT: u32 = 0xffff_fffd;

/// Root storage classes of the three document kinds, as the little-endian GUID bytes.
const PART_CLASS: [u8; 16] = guid(
    0x23c5_2e80,
    0x4698,
    0x11ce,
    [0xb3, 0x07, 0x08, 0x00, 0x36, 0x3a, 0x1e, 0x02],
);
const SHEET_METAL_CLASS: [u8; 16] = guid(
    0xdd85_22e0,
    0x2375,
    0x11d0,
    [0xac, 0x05, 0x08, 0x00, 0x36, 0xfd, 0x18, 0x02],
);
const ASSEMBLY_CLASS: [u8; 16] = guid(
    0x00c6_bf00,
    0x483b,
    0x11ce,
    [0x95, 0x1a, 0x08, 0x00, 0x36, 0x01, 0xbe, 0x52],
);
/// A root class that no Solid Edge document kind uses.
const OTHER_CLASS: [u8; 16] = guid(1, 2, 3, [4, 5, 6, 7, 8, 9, 10, 11]);

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> [u8; 16] {
    let a = data1.to_le_bytes();
    let b = data2.to_le_bytes();
    let c = data3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], data4[0], data4[1], data4[2], data4[3],
        data4[4], data4[5], data4[6], data4[7],
    ]
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn name(entry: &mut [u8], value: &str) {
    for (i, unit) in value.encode_utf16().enumerate() {
        put_u16(entry, i * 2, unit);
    }
    put_u16(entry, 64, u16::try_from((value.len() + 1) * 2).unwrap());
}

fn entry(
    directory: &mut [u8],
    index: usize,
    value: &str,
    kind: u8,
    child: u32,
    sector: Option<usize>,
) {
    let bytes = &mut directory[index * 128..(index + 1) * 128];
    name(bytes, value);
    bytes[66] = kind;
    bytes[67] = 1;
    put_u32(bytes, 76, child);
    put_u32(
        bytes,
        116,
        sector.map_or(CFB_END, |value| u32::try_from(value).unwrap()),
    );
    bytes[120..128].copy_from_slice(&(if sector.is_some() { 4096u64 } else { 0 }).to_le_bytes());
}

/// A compound file whose root has the given class and holds `PSMroots` and `JSitesList`,
/// followed by one `.P_B` stream for each partition.
fn document(class: [u8; 16], partitions: &[Vec<u8>]) -> Vec<u8> {
    // Two directory sectors hold the root, three named entries and up to four streams.
    assert!(partitions.len() <= 4);
    let fat = 2 + 8 * partitions.len();
    let mut file = vec![0u8; (fat + 2) * 512];
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, CFB_END);
    put_u32(&mut file, 68, CFB_END);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, CFB_FREE);
    }
    put_u32(&mut file, 76, u32::try_from(fat).unwrap());
    let directory = &mut file[512..1536];
    for slot in directory.as_chunks_mut::<128>().0 {
        slot[68..80].fill(0xff);
    }
    entry(directory, 0, "Root Entry", 5, 3, None);
    directory[80..96].copy_from_slice(&class);
    entry(directory, 1, "PSMroots", 2, u32::MAX, None);
    entry(directory, 2, "JSitesList", 2, u32::MAX, None);
    // CFB orders names by UTF-16 length, then uppercase spelling. All three
    // root children are black, with PARASOLID between PSMroots and JSitesList.
    entry(directory, 3, "PARASOLID", 1, CFB_FREE, None);
    put_u32(directory, 384 + 68, 1);
    put_u32(directory, 384 + 72, 2);
    for (index, stream) in partitions.iter().enumerate() {
        assert!(stream.len() <= 4096);
        let slot = 4 + index;
        entry(
            directory,
            slot,
            &format!("STREAM{index}.P_B"),
            2,
            u32::MAX,
            Some(2 + 8 * index),
        );
    }
    // STREAM0.P_B through STREAM3.P_B are already in CFB name order. An empty
    // storage must have NO_STREAM as its child, never a pointer to a free slot.
    let slots: Vec<usize> = (4..4 + partitions.len()).collect();
    let red_depth = if slots.is_empty() {
        0
    } else {
        slots.len().ilog2()
    };
    let child = sibling_tree(directory, &slots, 0, red_depth);
    put_u32(directory, 384 + 76, child);
    for (index, stream) in partitions.iter().enumerate() {
        let start = (3 + 8 * index) * 512;
        file[start..start + stream.len()].copy_from_slice(stream);
    }
    let table = &mut file[(fat + 1) * 512..(fat + 2) * 512];
    table.fill(0xff);
    put_u32(table, 0, 1);
    put_u32(table, 4, CFB_END);
    for index in 0..partitions.len() {
        for sector in 2 + index * 8..10 + index * 8 {
            let next = if sector == 9 + index * 8 {
                CFB_END
            } else {
                u32::try_from(sector + 1).unwrap()
            };
            put_u32(table, sector * 4, next);
        }
    }
    put_u32(table, fat * 4, CFB_FAT);
    file
}

/// Build a balanced sibling tree, coloring its deepest leaves red so every
/// path to a missing child has the same black height.
fn sibling_tree(directory: &mut [u8], slots: &[usize], depth: u32, red_depth: u32) -> u32 {
    if slots.is_empty() {
        return CFB_FREE;
    }
    let middle = slots.len() / 2;
    let slot = slots[middle];
    let left = sibling_tree(directory, &slots[..middle], depth + 1, red_depth);
    let right = sibling_tree(directory, &slots[middle + 1..], depth + 1, red_depth);
    directory[slot * 128 + 67] = u8::from(depth == 0 || depth != red_depth);
    put_u32(directory, slot * 128 + 68, left);
    put_u32(directory, slot * 128 + 72, right);
    u32::try_from(slot).unwrap()
}

fn node(bytes: &mut Vec<u8>, kind: u16, index: u16) {
    bytes.extend_from_slice(&kind.to_be_bytes());
    pointer(bytes, index);
}

fn pointer(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn pointers(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        pointer(bytes, *value);
    }
}

fn integer(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn real(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn vector(bytes: &mut Vec<u8>, values: [f64; 3]) {
    for value in values {
        real(bytes, value);
    }
}

fn wire_id(base: u16, index: usize) -> u16 {
    base + u16::try_from(index).unwrap()
}

/// One Parasolid partition holding a solid cube of 2 mm edge: one body, six planar faces.
fn cube_partition() -> Vec<u8> {
    let description = b": TRANSMIT FILE (partition)";
    let schema = b"SCH_999_13006";
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(schema);
    bytes.extend_from_slice(&0u32.to_be_bytes());
    let coordinates = [
        [0.0, 0.0, 0.0],
        [0.002, 0.0, 0.0],
        [0.002, 0.002, 0.0],
        [0.0, 0.002, 0.0],
        [0.0, 0.0, 0.002],
        [0.002, 0.0, 0.002],
        [0.002, 0.002, 0.002],
        [0.0, 0.002, 0.002],
    ];
    let rings = [
        [0usize, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let edge_ends = [
        [0usize, 1],
        [1, 2],
        [2, 3],
        [3, 0],
        [4, 5],
        [5, 6],
        [6, 7],
        [7, 4],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];
    let normals = [
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
    ];
    let x_axes = [
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut incidences = Vec::new();
    for (face, ring) in rings.iter().enumerate() {
        for side in 0..4 {
            let ends = [ring[side], ring[(side + 1) % 4]];
            let edge = edge_ends
                .iter()
                .position(|pair| *pair == ends || *pair == [ends[1], ends[0]])
                .unwrap();
            incidences.push((face, side, edge, ends, edge_ends[edge] == ends));
        }
    }
    node(&mut bytes, 12, 2);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1; 6]);
    real(&mut bytes, 1.0e-8);
    real(&mut bytes, 1.0e-8);
    pointers(&mut bytes, &[1; 3]);
    bytes.push(0);
    pointer(&mut bytes, 1);
    bytes.extend_from_slice(&[1, 0]);
    pointer(&mut bytes, 3);
    pointers(&mut bytes, &[1; 6]);
    node(&mut bytes, 13, 3);
    integer(&mut bytes, 0);
    pointers(&mut bytes, &[1, 2, 1, 4, 1, 1, 1, 1]);
    for face in 0..6 {
        node(&mut bytes, 14, wire_id(4, face));
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(
            &mut bytes,
            &[
                if face == 5 { 1 } else { wire_id(5, face) },
                if face == 0 { 1 } else { wire_id(3, face) },
                wire_id(16, face),
                3,
                wire_id(10, face),
            ],
        );
        bytes.push(b'+');
        pointers(&mut bytes, &[1; 5]);
        node(&mut bytes, 50, wire_id(10, face));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, coordinates[rings[face][0]]);
        vector(&mut bytes, normals[face]);
        vector(&mut bytes, x_axes[face]);
        node(&mut bytes, 15, wire_id(16, face));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, wire_id(62, face * 4), wire_id(4, face), 1]);
    }
    for (vertex, position) in coordinates.iter().enumerate() {
        let point = 37 - u16::try_from(vertex).unwrap();
        node(&mut bytes, 18, wire_id(22, vertex));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 1, 1, 1, point]);
        real(&mut bytes, 0.0);
        pointer(&mut bytes, 2);
        node(&mut bytes, 29, point);
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1]);
        vector(&mut bytes, *position);
    }
    for (edge, [start, end]) in edge_ends.iter().copied().enumerate() {
        let fin = incidences
            .iter()
            .position(|incidence| incidence.2 == edge)
            .unwrap();
        node(&mut bytes, 16, wire_id(38, edge));
        integer(&mut bytes, 0);
        pointer(&mut bytes, 1);
        real(&mut bytes, 0.0);
        pointers(
            &mut bytes,
            &[wire_id(62, fin), 1, 1, wire_id(50, edge), 1, 1, 2],
        );
        node(&mut bytes, 30, wire_id(50, edge));
        integer(&mut bytes, 0);
        pointers(&mut bytes, &[1, 2, 1, 1, 1]);
        bytes.push(b'+');
        vector(&mut bytes, coordinates[start]);
        vector(
            &mut bytes,
            std::array::from_fn(|axis| (coordinates[end][axis] - coordinates[start][axis]) / 0.002),
        );
    }
    for (fin, (face, side, edge, ends, forward)) in incidences.iter().copied().enumerate() {
        let other = incidences
            .iter()
            .enumerate()
            .position(|(index, incidence)| index != fin && incidence.2 == edge)
            .unwrap();
        node(&mut bytes, 17, wire_id(62, fin));
        pointers(
            &mut bytes,
            &[
                1,
                wire_id(16, face),
                wire_id(62, face * 4 + (side + 1) % 4),
                wire_id(62, face * 4 + (side + 3) % 4),
                wire_id(22, ends[1]),
                wire_id(62, other),
                wire_id(38, edge),
                1,
                1,
            ],
        );
        bytes.push(if forward { b'+' } else { b'-' });
    }
    bytes.extend_from_slice(&[0, 1, 0, 1]);
    bytes
}

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dutchman-solid-edge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn cube_document(class: [u8; 16]) -> Vec<u8> {
    document(class, &[cube_partition()])
}

#[test]
fn the_three_document_kinds_are_recognised_by_suffix_and_route_to_the_solid_edge_codec() {
    let part = cube_document(PART_CLASS);
    for (file, expected, extension) in [
        ("x.par", Format::SolidEdgePart, "par"),
        ("x.psm", Format::SolidEdgeSheetMetal, "psm"),
        ("x.asm", Format::SolidEdgeAssembly, "asm"),
    ] {
        for spelling in [file.to_owned(), file.to_ascii_uppercase()] {
            assert_eq!(
                Format::from_path(&scratch(&spelling, &part)),
                Some(expected),
                "{spelling}"
            );
        }
        assert_eq!(expected.extension(), extension);
        assert_eq!(expected.codec_id(), Some("solidedge"));
        assert_eq!(expected.family(), "Solid Edge");
        assert!(expected.can_convert());
        assert!(expected.reads_geometry());
        assert!(!expected.is_drawing());
        assert!(Format::ALL.contains(&expected));
    }
    assert_eq!(
        Format::from_codec_id("solidedge"),
        Some(Format::SolidEdgePart)
    );
    assert_eq!(
        Format::from_path(Path::new("x.dft")),
        Some(Format::SolidEdgeDrawing)
    );
    assert!(Format::supported_names().contains("Solid Edge part (.par)"));
}

#[test]
fn a_part_converts_as_geometry_and_the_report_says_the_history_is_not_read() {
    for (file, format, class) in [
        ("cube.par", Format::SolidEdgePart, PART_CLASS),
        ("cube.psm", Format::SolidEdgeSheetMetal, SHEET_METAL_CLASS),
    ] {
        let bytes = cube_document(class);
        let path = scratch(file, &bytes);
        let mut report = Report::default();
        let model = read_bytes(format, &path, &bytes, &mut report).unwrap();
        assert_eq!(model.ir.geometry.bodies.len(), 1, "{file}");
        assert_eq!(model.ir.geometry.faces.len(), 6, "{file}");
        assert!(model.ir.model.features.is_empty(), "{file}");
        let note = report
            .losses()
            .iter()
            .find(|loss| loss.subject == "feature history")
            .unwrap();
        assert_eq!(note.severity, Severity::Degraded);
        assert!(
            note.detail.contains("feature history is not read yet"),
            "{}",
            note.detail
        );
    }
}

#[test]
fn an_assembly_with_a_body_says_placements_and_history_are_not_read() {
    let bytes = cube_document(ASSEMBLY_CLASS);
    let path = scratch("cube.asm", &bytes);
    let mut report = Report::default();
    let model = read_bytes(Format::SolidEdgeAssembly, &path, &bytes, &mut report).unwrap();
    assert_eq!(model.ir.geometry.bodies.len(), 1);
    assert!(model.assembly.is_none());
    let note = report
        .losses()
        .iter()
        .find(|loss| loss.subject == "feature history")
        .unwrap();
    assert!(
        note.detail
            .contains("component placements are not read yet"),
        "{}",
        note.detail
    );
}

#[test]
fn a_document_without_a_body_is_refused_with_what_to_do_next() {
    let bytes = document(ASSEMBLY_CLASS, &[]);
    let path = scratch("empty.asm", &bytes);
    let err = read_bytes(
        Format::SolidEdgeAssembly,
        &path,
        &bytes,
        &mut Report::default(),
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(!matches!(err, Error::NotSupportedYet { .. }), "{message}");
    assert!(message.contains("component placements"), "{message}");
    assert!(message.contains(".par"), "{message}");

    let bytes = document(PART_CLASS, &[]);
    let path = scratch("empty.par", &bytes);
    let err = read_bytes(Format::SolidEdgePart, &path, &bytes, &mut Report::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("feature history"), "{err}");
}

#[test]
fn documents_with_up_to_four_partition_streams_have_valid_sibling_trees() {
    for count in 1..=4 {
        let partitions = vec![cube_partition(); count];
        let bytes = document(PART_CLASS, &partitions);
        let model = read_bytes(
            Format::SolidEdgePart,
            Path::new("partitions.par"),
            &bytes,
            &mut Report::default(),
        )
        .unwrap();
        assert_eq!(model.ir.geometry.bodies.len(), count);
    }
}

#[test]
fn contents_decide_the_program_not_the_suffix() {
    // A Solid Edge document under another program's suffix is refused, and a root class that
    // no Solid Edge document uses is not Solid Edge.
    let bytes = cube_document(PART_CLASS);
    let wrong = read_bytes(
        Format::InventorPart,
        Path::new("cube.ipt"),
        &bytes,
        &mut Report::default(),
    );
    assert!(wrong.is_err());
    let other = cube_document(OTHER_CLASS);
    let wrong = read_bytes(
        Format::SolidEdgePart,
        Path::new("cube.par"),
        &other,
        &mut Report::default(),
    );
    assert!(wrong.is_err());
}
