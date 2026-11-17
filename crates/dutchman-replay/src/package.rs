use dutchman_core::Error;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipWriter};

/// Compute a lower-case SHA-256 digest.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Write ordered entries with bounded total size and deterministic metadata.
pub fn write_package(out: &mut dyn Write, files: &[(&str, &[u8])]) -> Result<(), Error> {
    check_size(files.iter().map(|(_, bytes)| bytes.len()))?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());
    let mut names = std::collections::BTreeSet::new();
    for (name, bytes) in files {
        if name.is_empty()
            || name.chars().any(|c| c.is_control())
            || name.contains(['/', '\\', ':'])
            || matches!(*name, "." | "..")
            || !names.insert(*name)
        {
            return Err(Error::Encode(
                "a package entry needs a unique file name".into(),
            ));
        }
        zip.start_file(*name, options).map_err(encode_error)?;
        zip.write_all(bytes).map_err(encode_error)?;
    }
    let bytes = zip.finish().map_err(encode_error)?.into_inner();
    out.write_all(&bytes).map_err(encode_error)
}
fn encode_error(error: impl std::fmt::Display) -> Error {
    Error::Encode(format!("cannot write the script package: {error}"))
}

fn check_size(mut lengths: impl Iterator<Item = usize>) -> Result<(), Error> {
    let total = lengths.try_fold(0usize, |sum, size| sum.checked_add(size));
    if total.is_none_or(|n| n > 512 * 1024 * 1024) {
        return Err(Error::Encode(
            "the script package exceeds the size limit; convert fewer features or omit additional files".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn total_input_bytes_and_overflow_are_bounded() {
        assert!(super::check_size([512 * 1024 * 1024].into_iter()).is_ok());
        let error = super::check_size([512 * 1024 * 1024, 1].into_iter()).unwrap_err();
        assert!(error
            .to_string()
            .contains("convert fewer features or omit additional files"));
        assert!(super::check_size([usize::MAX, 1].into_iter()).is_err());
    }
}
