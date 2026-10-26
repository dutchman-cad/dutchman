//! Uploaded files and bounded project zip extraction.

#[cfg(test)]
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub use dutchman_serve::upload::{safe_name, unique, unzip, ZipLimits};

/// Whether an uploaded file is a zip to unpack, by its name.
pub fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Reads at most `max` bytes of `reader`, for small files the server wrote itself.
pub fn read_small(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(max).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    const LIMITS: ZipLimits = ZipLimits {
        entries: 100,
        bytes: 1 << 20,
    };

    fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(data).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn names_lose_folders_and_unsafe_characters() {
        assert_eq!(safe_name("C:\\work\\part.f3d"), "part.f3d");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("a:b*c?.ipt"), "a_b_c_.ipt");
        assert_eq!(safe_name(".."), "upload");
        assert_eq!(safe_name(""), "upload");
        assert_eq!(safe_name("Bracket v2.f3d"), "Bracket v2.f3d");
    }

    #[test]
    fn unique_names_count_up() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("part.f3d");
        assert_eq!(unique(&first).unwrap(), first);
        File::create(&first).unwrap();
        assert_eq!(unique(&first).unwrap(), dir.path().join("part (1).f3d"));
    }

    #[test]
    fn a_zip_of_one_folder_unpacks_as_that_folder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("upload.zip");
        zip_of(
            &archive,
            &[
                ("proj/", b""),
                ("proj/sub/a.ipt", b"a"),
                ("proj/b.iam", b"b"),
            ],
        );
        unzip(&archive, dir.path(), &mut { LIMITS }).unwrap();
        assert!(dir.path().join("proj/sub/a.ipt").is_file());
        assert!(dir.path().join("proj/b.iam").is_file());
        assert!(!archive.exists());
    }

    #[test]
    fn loose_entries_unpack_into_a_folder_named_after_the_zip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("parts.zip");
        zip_of(&archive, &[("a.ipt", b"a"), ("b.ipt", b"b")]);
        unzip(&archive, dir.path(), &mut { LIMITS }).unwrap();
        assert!(dir.path().join("parts/a.ipt").is_file());
    }

    #[test]
    fn paths_outside_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("evil.zip");
        zip_of(&archive, &[("../evil.txt", b"x")]);
        let err = unzip(&archive, &dir.path().join("in"), &mut { LIMITS }).unwrap_err();
        assert!(err.contains("outside its own folder"), "{err}");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn too_many_entries_or_bytes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("many.zip");
        zip_of(&archive, &[("a", b"1"), ("b", b"2"), ("c", b"3")]);
        let few = ZipLimits {
            entries: 2,
            bytes: 1 << 20,
        };
        assert!(unzip(&archive, dir.path(), &mut { few })
            .unwrap_err()
            .contains("more than 2 entries"));

        let archive = dir.path().join("big.zip");
        zip_of(&archive, &[("big.bin", &[0u8; 4096])]);
        let small = ZipLimits {
            entries: 10,
            bytes: 1024,
        };
        assert!(unzip(&archive, dir.path(), &mut { small })
            .unwrap_err()
            .contains("unpacks to more than"));
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("fake.zip");
        fs::write(&archive, b"not a zip").unwrap();
        assert!(unzip(&archive, dir.path(), &mut { LIMITS })
            .unwrap_err()
            .contains("could not be opened as a zip file"));
    }
}
