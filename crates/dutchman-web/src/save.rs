//! Saving a job's designs in the server's output folder (`--output-dir`), such as a shared
//! network folder, where they stay after the job expires. Each design keeps its uploaded
//! folders and name; a name already taken gets a number, as a browser's downloads do, so a file
//! already in the folder is never replaced.

use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

/// Copies each converted file's design from `out_dir` into `folder`, and adds to its `output`
/// where it was saved (`saved`, relative to `folder`) or why it could not be (`save_error`).
pub fn save_all(report: &mut Value, out_dir: &Path, folder: &Path) {
    let Some(Value::Array(results)) = report.get_mut("results") else {
        return;
    };
    for result in results {
        if result.get("ok") != Some(&json!(true)) {
            continue;
        }
        let Some(output) = result.get_mut("output").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(path) = output
            .get("path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
        else {
            continue;
        };
        match save(&path, out_dir, folder) {
            Ok(saved) => output.insert("saved".to_owned(), json!(saved)),
            Err(err) => output.insert(
                "save_error".to_owned(),
                json!(format!("It could not be saved in the output folder: {err}")),
            ),
        };
    }
}

/// Copies `design`, a file under `out_dir`, to the same place under `folder`, numbering its
/// name when taken; returns where it went, relative to `folder`, with `/` between folders.
fn save(design: &Path, out_dir: &Path, folder: &Path) -> io::Result<String> {
    let relative = design
        .strip_prefix(out_dir)
        .map_err(|_| io::Error::other("the design is not in the job's folder"))?;
    if !relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::other("the design's name is not a plain path"));
    }
    let wanted = folder.join(relative);
    let parent = wanted
        .parent()
        .ok_or_else(|| io::Error::other("the design has no folder"))?;
    // An uploaded folder name must not lead through a link out of the output folder.
    no_links(folder, parent)?;
    std::fs::create_dir_all(parent)?;
    no_links(folder, parent)?;
    let stem = wanted
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = wanted
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 0..10_000 {
        let candidate = if n == 0 {
            wanted.clone()
        } else {
            wanted.with_file_name(format!("{stem} ({n}){extension}"))
        };
        let mut target = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(target) => target,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        };
        let copied = std::fs::File::open(design).and_then(|mut source| {
            io::copy(&mut source, &mut target)?;
            target.sync_all()
        });
        if let Err(err) = copied {
            drop(target);
            let _ = std::fs::remove_file(&candidate);
            return Err(err);
        }
        let saved = candidate
            .strip_prefix(folder)
            .map_err(|_| io::Error::other("the design left the output folder"))?;
        return Ok(saved
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"));
    }
    Err(io::Error::other("too many files with the same name"))
}

/// Fails when a folder from `folder` (exclusive) down to `inner` is a symbolic link.
fn no_links(folder: &Path, inner: &Path) -> io::Result<()> {
    let mut current = inner;
    while current != folder {
        if std::fs::symlink_metadata(current).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(io::Error::other(
                "a folder on the way is a symbolic link, which designs are not saved through",
            ));
        }
        current = current
            .parent()
            .ok_or_else(|| io::Error::other("the design left the output folder"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(out: &Path, names: &[&str]) -> Value {
        json!({"results": names.iter().map(|name| json!({
            "ok": true, "output": {"path": out.join(name).to_string_lossy()}
        })).collect::<Vec<_>>()})
    }

    #[test]
    fn designs_keep_their_folders_and_never_replace_a_file() {
        let job = tempfile::tempdir().unwrap();
        let share = tempfile::tempdir().unwrap();
        let out = job.path();
        std::fs::create_dir_all(out.join("proj")).unwrap();
        std::fs::write(out.join("proj/a.solidcraft"), "new a").unwrap();
        std::fs::write(out.join("b.solidcraft"), "new b").unwrap();
        std::fs::create_dir_all(share.path().join("proj")).unwrap();
        std::fs::write(share.path().join("proj/a.solidcraft"), "old a").unwrap();

        let mut saved = report(out, &["proj/a.solidcraft", "b.solidcraft"]);
        save_all(&mut saved, out, share.path());
        assert_eq!(
            saved["results"][0]["output"]["saved"],
            "proj/a (1).solidcraft"
        );
        assert_eq!(saved["results"][1]["output"]["saved"], "b.solidcraft");
        let read = |name: &str| std::fs::read_to_string(share.path().join(name)).unwrap();
        assert_eq!(read("proj/a.solidcraft"), "old a");
        assert_eq!(read("proj/a (1).solidcraft"), "new a");
        assert_eq!(read("b.solidcraft"), "new b");
    }

    #[test]
    fn failed_files_are_not_saved() {
        let job = tempfile::tempdir().unwrap();
        let share = tempfile::tempdir().unwrap();
        let mut saved = json!({"results": [{"ok": false, "error": {"message": "no"}}]});
        save_all(&mut saved, job.path(), share.path());
        assert!(saved["results"][0].get("output").is_none());
        assert_eq!(std::fs::read_dir(share.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_is_not_saved_through() {
        let job = tempfile::tempdir().unwrap();
        let share = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let out = job.path();
        std::fs::create_dir_all(out.join("proj")).unwrap();
        std::fs::write(out.join("proj/a.solidcraft"), "a").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), share.path().join("proj")).unwrap();

        let mut saved = report(out, &["proj/a.solidcraft"]);
        save_all(&mut saved, out, share.path());
        let error = saved["results"][0]["output"]["save_error"]
            .as_str()
            .unwrap();
        assert!(error.contains("symbolic link"), "{error}");
        assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }
}
