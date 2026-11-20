//! Publish complete companion files without replacing an edited or concurrent design.
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

use crate::Error;

fn stage(path: &Path, data: &[u8]) -> Result<NamedTempFile, Error> {
    let io = |source| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut temporary =
        NamedTempFile::new_in(path.parent().unwrap_or(Path::new("."))).map_err(io)?;
    temporary.write_all(data).map_err(io)?;
    Ok(temporary)
}

fn publish(temporary: NamedTempFile, path: &Path, data: &[u8]) -> Result<(), Error> {
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            // A concurrent publisher exposes only a complete file. Reuse its bytes,
            // but never overwrite a different companion, including a user's edits.
            let existing = std::fs::metadata(path).map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })?;
            if !existing.is_file() || existing.len() != data.len() as u64 {
                return Err(crate::companion_conflict(path));
            }
            match std::fs::read(path) {
                Ok(existing) if existing == data => Ok(()),
                Ok(_) => Err(crate::companion_conflict(path)),
                Err(source) => Err(Error::Io {
                    path: path.to_path_buf(),
                    source,
                }),
            }
        }
        Err(error) => Err(Error::Io {
            path: path.to_path_buf(),
            source: error.error,
        }),
    }
}

pub(super) fn write(path: &Path, data: &[u8]) -> Result<(), Error> {
    publish(stage(path, data)?, path, data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn concurrent_publishers_reuse_complete_identical_companions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("derived.sc");
        let data = vec![42; 1 << 20];
        let first = stage(&path, &data).unwrap();
        let second = stage(&path, &data).unwrap();
        assert!(
            !path.exists(),
            "staging must not expose a partial companion"
        );
        let barrier = Barrier::new(2);
        std::thread::scope(|threads| {
            for temporary in [first, second] {
                let (path, data, barrier) = (&path, &data, &barrier);
                threads.spawn(move || {
                    barrier.wait();
                    publish(temporary, path, data).unwrap();
                    assert_eq!(std::fs::read(path).unwrap(), *data);
                });
            }
        });
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn publication_preserves_a_companion_edited_after_staging() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("derived.sc");
        let temporary = stage(&path, b"original").unwrap();
        std::fs::write(&path, b"an edit!").unwrap();
        let error = publish(temporary, &path, b"original").unwrap_err();
        assert!(matches!(error, Error::Encode(_)));
        assert_eq!(std::fs::read(&path).unwrap(), b"an edit!");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
