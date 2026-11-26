//! Says plainly what is wrong with a Fusion archive (.f3d, .f3z) that holds no readable geometry,
//! and recovers the members a file that ends early still holds.
//!
//! A Fusion export can end early (the zip end record, written last, is missing) or leave out a
//! part: a design whose linked designs were not saved exports only its structure. cadmpeg then
//! reports a bare container failure or "no geometry"; this module looks at the zip itself to
//! name what is present and what is not.

use std::io::{Cursor, Read};

const LOCAL: [u8; 4] = *b"PK\x03\x04";
const BREPS: &str = "Breps.BlobParts/";
/// The Fusion design segment, where a design's components and features live.
const DESIGN: &str = "Design1/";
const RE_EXPORT: &str = "Re-export it from Fusion with File > Export > .f3d (with every linked \
design open and saved), or send the .f3z";

/// An archive rebuilt from the members a damaged file still holds whole.
pub(crate) struct Recovered {
    pub(crate) bytes: Vec<u8>,
    /// Members kept, directory entries not counted.
    pub(crate) kept: usize,
    /// Bytes at the end that belong to no whole member.
    pub(crate) lost_bytes: usize,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Whether the bytes open as a zip archive.
pub(crate) fn opens(bytes: &[u8]) -> bool {
    zip::ZipArchive::new(Cursor::new(bytes)).is_ok()
}

struct Entry {
    header: usize,
    name_len: usize,
    flags: u16,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    version: u16,
}

/// Walks the local member headers from the start of the file, which are written before the
/// directory at the end, and stops at the first member that is not whole.
fn whole_members(bytes: &[u8]) -> (Vec<Entry>, usize) {
    let mut entries = Vec::new();
    let mut at = 0usize;
    while bytes.get(at..at + 4) == Some(LOCAL.as_slice()) {
        let (Some(version), Some(flags), Some(method), Some(time), Some(date)) = (
            u16_at(bytes, at + 4),
            u16_at(bytes, at + 6),
            u16_at(bytes, at + 8),
            u16_at(bytes, at + 10),
            u16_at(bytes, at + 12),
        ) else {
            break;
        };
        let (Some(crc), Some(compressed), Some(size), Some(name_len), Some(extra_len)) = (
            u32_at(bytes, at + 14),
            u32_at(bytes, at + 18),
            u32_at(bytes, at + 22),
            u16_at(bytes, at + 26),
            u16_at(bytes, at + 28),
        ) else {
            break;
        };
        // Sizes written after the data, and zip64 sizes, cannot be read from the header alone.
        if flags & 0x8 != 0 && compressed == 0 || compressed == u32::MAX || size == u32::MAX {
            break;
        }
        let end = at + 30 + usize::from(name_len) + usize::from(extra_len) + compressed as usize;
        if end > bytes.len()
            || bytes
                .get(at + 30..at + 30 + usize::from(name_len))
                .is_none()
        {
            break;
        }
        entries.push(Entry {
            header: at,
            name_len: usize::from(name_len),
            flags: flags & !0x8,
            method,
            time,
            date,
            crc,
            compressed,
            size,
            version,
        });
        at = end;
    }
    (entries, at)
}

/// Rebuilds the archive's directory from the member headers, so a file whose end is missing
/// opens with the members that are whole. `None` when no member is whole or nothing is missing.
pub(crate) fn recover(bytes: &[u8]) -> Option<Recovered> {
    if opens(bytes) {
        return None;
    }
    let (entries, end) = whole_members(bytes);
    if entries.is_empty() {
        return None;
    }
    let mut out = bytes.get(..end)?.to_vec();
    let directory = out.len();
    for entry in &entries {
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&entry.version.to_le_bytes());
        out.extend_from_slice(&entry.flags.to_le_bytes());
        out.extend_from_slice(&entry.method.to_le_bytes());
        out.extend_from_slice(&entry.time.to_le_bytes());
        out.extend_from_slice(&entry.date.to_le_bytes());
        out.extend_from_slice(&entry.crc.to_le_bytes());
        out.extend_from_slice(&entry.compressed.to_le_bytes());
        out.extend_from_slice(&entry.size.to_le_bytes());
        out.extend_from_slice(&u16::try_from(entry.name_len).ok()?.to_le_bytes());
        out.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]);
        out.extend_from_slice(&u32::try_from(entry.header).ok()?.to_le_bytes());
        out.extend_from_slice(bytes.get(entry.header + 30..entry.header + 30 + entry.name_len)?);
    }
    let count = u16::try_from(entries.len()).ok()?;
    let size = u32::try_from(out.len() - directory).ok()?;
    out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&u32::try_from(directory).ok()?.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    if !opens(&out) {
        return None;
    }
    let kept = zip::ZipArchive::new(Cursor::new(out.as_slice()))
        .ok()?
        .file_names()
        .filter_map(Result::ok)
        .filter(|name| !name.ends_with('/'))
        .count();
    Some(Recovered {
        bytes: out,
        kept,
        lost_bytes: bytes.len() - end,
    })
}

/// What a damaged-but-recovered archive adds to the report.
pub(crate) fn recovered_note(recovered: &Recovered) -> String {
    format!(
        "this export is incomplete: the file ends early (its zip end record is missing, {} byte(s) \
         after the last whole part are not usable), so only the {} part(s) before that point were \
         read. {RE_EXPORT} for the rest",
        recovered.lost_bytes, recovered.kept
    )
}

/// Reads the member names of an archive that opens.
fn names(bytes: &[u8]) -> Option<Vec<String>> {
    let archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    // A name that cannot be read is not a part the diagnosis can count on.
    Some(
        archive
            .file_names()
            .filter_map(Result::ok)
            .map(std::borrow::Cow::into_owned)
            .collect(),
    )
}

/// The distinct `.f3d` / `.f3z` file names the archive's root manifest links to: the designs
/// a design built from other files refers to, which an export does not store. Counted, never named.
fn linked_designs(bytes: &[u8]) -> usize {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return 0;
    };
    let Ok(file) = archive.by_name("Manifest.dat") else {
        return 0;
    };
    let mut manifest = Vec::new();
    if file.take(1 << 20).read_to_end(&mut manifest).is_err() {
        return 0;
    }
    let mut found = std::collections::BTreeSet::new();
    // The strings are UTF-16 and follow 4-byte lengths and 1-byte text, so they may start on
    // either parity.
    for parity in 0..2 {
        let mut run = String::new();
        let (pairs, _) = manifest.get(parity..).unwrap_or_default().as_chunks::<2>();
        for pair in pairs.iter().chain(std::iter::once(&[0u8, 0u8])) {
            match pair {
                [byte @ 0x20..=0x7e, 0] => run.push(char::from(*byte)),
                _ => {
                    let lower = run.to_ascii_lowercase();
                    if run.len() > 6
                        && (run.contains('/') || run.contains('\\'))
                        && (lower.ends_with(".f3d") || lower.ends_with(".f3z"))
                    {
                        found.insert(run.clone());
                    }
                    run.clear();
                }
            }
        }
    }
    found.len()
}

/// Why an archive that opens holds no solid geometry, when it is missing the part that stores
/// it. `None` when the archive has a geometry part (so the cause is elsewhere), or is not a
/// Fusion archive at all.
pub(crate) fn missing_geometry(bytes: &[u8]) -> Option<String> {
    let names = names(bytes)?;
    let member = |part: &str| {
        names
            .iter()
            .any(|name| name.contains(part) && !name.ends_with('/'))
    };
    if !names.iter().any(|name| name == "Manifest.dat") && !member(DESIGN) {
        return None;
    }
    if member(BREPS) {
        return None;
    }
    let linked = linked_designs(bytes);
    let holds = match (member(DESIGN), linked) {
        (true, 0) => "the design's structure".to_owned(),
        (true, links) => format!(
            "the design's structure and references to {links} other design file(s), whose \
             contents it does not store"
        ),
        (false, _) => "only its manifest".to_owned(),
    };
    Some(format!(
        "This export is incomplete: it holds {holds} but no solid geometry (the Breps part is \
         missing from the file). {RE_EXPORT}."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in members {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn a_file_cut_before_its_end_record_keeps_the_whole_members() {
        let whole = archive(&[
            ("Manifest.dat", &[7; 300]),
            ("A/Design1/MetaStream.dat", &[9; 900]),
            ("A/Breps.BlobParts/BREP.1.smb", &[5; 700]),
        ]);
        assert!(opens(&whole));
        // The directory and end record are written last: cutting there leaves every member.
        let directory = whole.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
        let first_central = whole.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        assert!(first_central <= directory);
        let cut = &whole[..first_central];
        assert!(!opens(cut));
        let recovered = recover(cut).unwrap();
        assert_eq!(recovered.kept, 3);
        assert_eq!(recovered.lost_bytes, 0);
        let names = names(&recovered.bytes).unwrap();
        assert!(names.iter().any(|n| n.ends_with("BREP.1.smb")));
    }

    #[test]
    fn a_file_cut_inside_a_member_keeps_the_members_before_it() {
        let whole = archive(&[
            ("Manifest.dat", &[7; 300]),
            ("A/Design1/MetaStream.dat", &[9; 900]),
            (
                "A/Breps.BlobParts/BREP.1.smb",
                &(0..4000u32)
                    .map(|n| (n * 31 % 251) as u8)
                    .collect::<Vec<_>>(),
            ),
        ]);
        let third = whole
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == b"PK\x03\x04")
            .map(|(at, _)| at)
            .nth(2)
            .unwrap();
        let cut = &whole[..third + 200];
        let recovered = recover(cut).unwrap();
        assert_eq!(recovered.kept, 2);
        assert_eq!(recovered.lost_bytes, 200);
        assert!(missing_geometry(&recovered.bytes).is_some());
        let note = recovered_note(&recovered);
        assert!(note.contains("ends early") && note.contains("Re-export it from Fusion"));
    }

    #[test]
    fn nothing_is_recovered_from_a_file_that_is_not_a_zip_or_not_damaged() {
        assert!(recover(b"not a zip at all").is_none());
        assert!(recover(&archive(&[("Manifest.dat", &[1; 10])])).is_none());
        assert!(recover(b"PK\x03\x04 and then nothing").is_none());
    }

    #[test]
    fn a_design_without_its_geometry_part_names_what_it_holds() {
        // A text header of odd length puts the strings on an odd offset.
        let mut manifest = b"3-2-0".to_vec();
        manifest.extend(utf16("/Users/someone/Desktop/linked part one.f3d"));
        manifest.extend_from_slice(&[0, 0]);
        manifest.extend(utf16("C:\\work\\linked part two.f3d"));
        manifest.extend_from_slice(&[0, 0]);
        manifest.extend(utf16("/Users/someone/Desktop/linked part one.f3d"));
        manifest.extend_from_slice(&[0, 0]);
        let file = archive(&[
            ("Manifest.dat", &manifest),
            ("A/Design1/MetaStream.dat", &[1; 50]),
            ("A/Design1/BulkStream.dat", &[2; 50]),
        ]);
        let message = missing_geometry(&file).unwrap();
        assert!(
            message.starts_with("This export is incomplete"),
            "{message}"
        );
        assert!(message.contains("structure and references to 2 other design file(s)"));
        assert!(message.contains("Breps part is missing"));
        assert!(message.contains("File > Export > .f3d"));
        assert!(message.contains("or send the .f3z"));
        assert!(
            !message.contains("linked part"),
            "names stay out: {message}"
        );
    }

    #[test]
    fn an_archive_with_a_geometry_part_is_not_called_incomplete() {
        let file = archive(&[
            ("Manifest.dat", &[1; 20]),
            ("A/Breps.BlobParts/BREP.1.smb", &[2; 20]),
        ]);
        assert!(missing_geometry(&file).is_none());
        assert!(missing_geometry(b"PK\x03\x04").is_none());
        assert!(missing_geometry(&archive(&[("readme.txt", b"x")])).is_none());
    }
}
