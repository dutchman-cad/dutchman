//! Detached Scoop updates: Windows cannot replace the GUI executable while it is running.
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::{find_scoop, Install, Spec};
use crate::update::version;

const SCRIPT: &str = include_str!("scoop-update.ps1");
const LOG: &str = "scoop-update.log";
const MAX_LOG: u64 = 64 * 1024;

fn handoff(pid: u32, scoop: &Path, shims: &Path, folder: &Path, tag: &str) -> Spec {
    let mut spec = Spec::new(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ],
    );
    spec.args.extend([
        folder.join("scoop-update.ps1").into_os_string(),
        "-ParentProcessId".into(),
        pid.to_string().into(),
        "-ScoopPath".into(),
        scoop.as_os_str().to_owned(),
        "-GuiPath".into(),
        shims.join("dutchman-gui.exe").into_os_string(),
        "-LogPath".into(),
        folder.join(LOG).into_os_string(),
        "-Tag".into(),
        tag.into(),
    ]);
    spec.detached = true;
    spec
}

pub(super) fn spawn(install: &Install, tag: &str) -> Result<(), String> {
    let Install::Scoop(shims) = install else {
        return Err("This copy is not installed with Scoop.".into());
    };
    let scoop = find_scoop(shims)?;
    let folder = dutchman::diagnostics::default_folder()?;
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Could not create the update log folder: {e}"))?;
    std::fs::write(folder.join("scoop-update.ps1"), SCRIPT)
        .map_err(|e| format!("Could not write the Scoop update helper: {e}"))?;
    // Initialize before launch so an interrupted helper is reported on the next start too.
    std::fs::write(folder.join(LOG), "Scoop update is starting.\n")
        .map_err(|e| format!("Could not create the Scoop update log: {e}"))?;
    let result = handoff(std::process::id(), &scoop, shims, &folder, tag)
        .command()
        .spawn();
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(folder.join(LOG));
            Err(format!("Could not start the Scoop update helper: {e}. Copy the command to retry in PowerShell."))
        }
    }
}

pub struct Failure {
    pub tag: String,
    pub error: String,
    pub output: String,
}

fn parse_result(bytes: &[u8]) -> Option<Failure> {
    let output = String::from_utf8_lossy(bytes);
    let result = output.lines().next_back().and_then(|line| {
        let mut fields = line.trim_start_matches('\u{feff}').split_whitespace();
        if fields.next()? != "DUTCHMAN_SCOOP_RESULT" {
            return None;
        }
        let code = fields.next()?.parse::<i32>().ok()?;
        let tag = fields.next()?;
        if version(tag).is_none() || fields.next().is_some() {
            return None;
        }
        Some((code, tag.to_owned()))
    });
    if result.as_ref().is_some_and(|(code, _)| *code == 0) {
        return None;
    }
    let (tag, error) = match result {
        Some((code, tag)) => (
            tag,
            format!(
                "Scoop update exited with code {code}. Copy the command to retry in PowerShell."
            ),
        ),
        None => (
            format!("v{}", env!("CARGO_PKG_VERSION")),
            "Scoop did not report a completed update. Copy the command to retry in PowerShell."
                .into(),
        ),
    };
    Some(Failure {
        tag,
        error,
        output: output.into_owned(),
    })
}

fn read_result(path: &Path) -> Option<Failure> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            let _ = std::fs::remove_file(path);
            return Some(Failure {
                tag: format!("v{}", env!("CARGO_PKG_VERSION")),
                error: format!(
                    "Could not read the Scoop update log: {e}. Copy the command to retry."
                ),
                output: String::new(),
            });
        }
    };
    let mut bytes = Vec::new();
    let read = (|| {
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(MAX_LOG)))?;
        file.take(MAX_LOG).read_to_end(&mut bytes)
    })();
    // Drop the Windows file handle before deleting the one-shot result.
    let _ = std::fs::remove_file(path);
    match read {
        Ok(_) => parse_result(&bytes),
        Err(e) => Some(Failure {
            tag: format!("v{}", env!("CARGO_PKG_VERSION")),
            error: format!("Could not read the Scoop update log: {e}. Copy the command to retry."),
            output: String::new(),
        }),
    }
}

pub fn startup(install: &Install) -> Option<Failure> {
    if !matches!(install, Install::Scoop(_)) {
        return None;
    }
    let folder: PathBuf = dutchman::diagnostics::default_folder().ok()?;
    read_result(&folder.join(LOG))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn handoff_passes_pid_and_paths_as_separate_arguments() {
        let folder = Path::new("C:/Users/Owner Name/Dutchman/diagnostics");
        for scoop in ["C:/Scoop Path/scoop.ps1", "C:/Scoop Path/scoop.exe"] {
            let spec = handoff(
                1234,
                Path::new(scoop),
                Path::new("C:/Scoop Path/shims"),
                folder,
                "v0.2.6",
            );
            assert_eq!(spec.program, PathBuf::from("powershell.exe"));
            assert!(spec.detached);
            let mut expected: Vec<OsString> = [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]
            .map(OsString::from)
            .to_vec();
            expected.extend([
                folder.join("scoop-update.ps1").into_os_string(),
                "-ParentProcessId".into(),
                "1234".into(),
                "-ScoopPath".into(),
                scoop.into(),
                "-GuiPath".into(),
                Path::new("C:/Scoop Path/shims")
                    .join("dutchman-gui.exe")
                    .into_os_string(),
                "-LogPath".into(),
                folder.join(LOG).into_os_string(),
                "-Tag".into(),
                "v0.2.6".into(),
            ]);
            assert_eq!(spec.args, expected);
            assert!(SCRIPT.contains(
                "Wait-Process -Id $ParentProcessId -Timeout 120 -ErrorAction SilentlyContinue"
            ));
            assert!(SCRIPT.contains("-File $ScoopPath @arguments"));
        }
    }

    #[test]
    fn startup_log_is_consumed_for_success_failure_garbage_and_missing() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(LOG);
        assert!(read_result(&path).is_none());
        std::fs::write(&path, "\u{feff}output\nDUTCHMAN_SCOOP_RESULT 0 v0.2.6\n").unwrap();
        assert!(read_result(&path).is_none());
        assert!(!path.exists());
        std::fs::write(&path, "could not update\nDUTCHMAN_SCOOP_RESULT 1 v0.2.6\n").unwrap();
        let failure = read_result(&path).unwrap();
        assert_eq!(failure.tag, "v0.2.6");
        assert!(failure.error.contains("code 1"));
        assert!(failure.output.contains("could not update"));
        assert!(!path.exists());
        assert!(read_result(&path).is_none());
        for bytes in [
            b"garbage".as_slice(),
            b"\xff\xfe",
            b"DUTCHMAN_SCOOP_RESULT zero v0.2.6",
            b"DUTCHMAN_SCOOP_RESULT 0 nightly",
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(read_result(&path).unwrap().error.contains("did not report"));
            assert!(!path.exists());
        }
        let large = format!(
            "{}\nDUTCHMAN_SCOOP_RESULT 124 v0.2.6\n",
            "x".repeat(MAX_LOG as usize * 2)
        );
        std::fs::write(&path, large).unwrap();
        let failure = read_result(&path).unwrap();
        assert!(failure.output.len() <= MAX_LOG as usize);
        assert!(failure.error.contains("124"));
        assert!(!path.exists());
    }
}
