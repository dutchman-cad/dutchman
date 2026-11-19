//! Updating a Linux AppImage in place: the new file is downloaded and checked like the other
//! release packages (windows.rs), written beside the running one and renamed over it. Nothing
//! inside the AppImage's read-only mount is touched, and the old file stays until the rename.
#[cfg(any(target_os = "linux", test))]
use std::ffi::OsStr;
use std::path::Path;
#[cfg(any(target_os = "linux", test))]
use std::path::PathBuf;
use std::process::Stdio;

use super::{Install, Spec};
use crate::update::{version, RELEASES};

/// Starts the new file once this process has exited (the window closes when the restart is
/// reported); `$0` is the AppImage's path.
const WAIT_THEN_START: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$0""#;

/// The AppImage this process runs from, when `APPIMAGE` names an existing file (only Linux
/// runs AppImages, so only Linux asks).
#[cfg(any(target_os = "linux", test))]
pub(super) fn detect(env: Option<&OsStr>) -> Install {
    match env.map(PathBuf::from) {
        Some(path) if path.is_absolute() && path.is_file() => Install::AppImage(path),
        _ => Install::Direct,
    }
}

/// The release's AppImage for a processor, as the release workflow names it.
pub(super) fn asset_name(tag: &str, arch: &str) -> Result<String, String> {
    version(tag).ok_or("No stable AppImage version was found. Open the release page.")?;
    if !matches!(arch, "x86_64" | "aarch64") {
        return Err(format!(
            "There is no AppImage for {arch}. Open the release page."
        ));
    }
    Ok(format!(
        "dutchman-{}-{arch}.AppImage",
        tag.trim_start_matches('v')
    ))
}

pub(super) fn asset_url(tag: &str, arch: &str) -> Result<String, String> {
    Ok(format!(
        "{RELEASES}/download/{tag}/{}",
        asset_name(tag, arch)?
    ))
}

/// Writes the verified download beside `target`, makes it executable and renames it over
/// `target`. Any failure leaves `target` as it was.
pub(super) fn replace(downloaded: &Path, target: &Path) -> Result<(), String> {
    let folder = target
        .parent()
        .ok_or("The AppImage has no folder. Open the release page for a manual update.")?;
    let mut stage = tempfile::Builder::new()
        .prefix(".dutchman-update-")
        .tempfile_in(folder)
        .map_err(|e| {
            format!(
                "Could not write beside the AppImage ({}): {e}. Open the release page to download the new AppImage.",
                folder.display()
            )
        })?;
    let mut source = std::fs::File::open(downloaded).map_err(|e| e.to_string())?;
    std::io::copy(&mut source, stage.as_file_mut()).map_err(|e| e.to_string())?;
    stage.as_file().sync_all().map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        stage
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    stage.persist(target).map_err(|e| {
        format!(
            "Could not replace the AppImage: {}. Your copy is unchanged. Open the release page to download the new AppImage.",
            e.error
        )
    })?;
    Ok(())
}

pub(super) fn restart(install: &Install) -> Result<(), String> {
    let Install::AppImage(path) = install else {
        return Err("This copy is not an AppImage.".into());
    };
    let mut spec = Spec::new("/bin/sh", &["-c", WAIT_THEN_START]);
    spec.args.extend([
        path.clone().into_os_string(),
        std::process::id().to_string().into(),
    ]);
    spec.detached = true;
    spec.command()
        .env_remove("ARGV0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| {
            format!("Could not restart Dutchman: {e}. Close it and open the updated AppImage.")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_asset_is_the_release_appimage_for_the_processor() {
        assert_eq!(
            asset_name("v1.2.3", "x86_64").unwrap(),
            "dutchman-1.2.3-x86_64.AppImage"
        );
        assert_eq!(
            asset_url("v1.2.3", "aarch64").unwrap(),
            format!("{RELEASES}/download/v1.2.3/dutchman-1.2.3-aarch64.AppImage")
        );
        assert!(asset_name("v1.2.3", "riscv64").is_err());
        assert!(asset_name("1.2.3", "x86_64").is_err());
        assert!(asset_name("v1.2.3-rc.1", "x86_64").is_err());
    }

    #[test]
    fn detection_needs_an_existing_absolute_file() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Dutchman.AppImage");
        assert_eq!(detect(None), Install::Direct);
        assert_eq!(detect(Some(file.as_os_str())), Install::Direct);
        std::fs::write(&file, b"old").unwrap();
        assert_eq!(detect(Some(file.as_os_str())), Install::AppImage(file));
        assert_eq!(
            detect(Some(OsStr::new("relative.AppImage"))),
            Install::Direct
        );
    }

    #[test]
    fn replacing_swaps_the_file_and_leaves_nothing_behind() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Dutchman.AppImage");
        let downloaded = folder.path().join("download");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&downloaded, b"new").unwrap();
        replace(&downloaded, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        let names: Vec<_> = std::fs::read_dir(folder.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn a_failed_replace_keeps_the_old_file() {
        let folder = tempfile::tempdir().unwrap();
        let target = folder.path().join("Dutchman.AppImage");
        std::fs::write(&target, b"old").unwrap();
        // The download is missing, so nothing may replace the file.
        assert!(replace(&folder.path().join("missing"), &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        // A folder that does not exist cannot be written beside.
        let gone = folder.path().join("gone/Dutchman.AppImage");
        let error = replace(&target, &gone).unwrap_err();
        assert!(error.contains("release page"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }
}
