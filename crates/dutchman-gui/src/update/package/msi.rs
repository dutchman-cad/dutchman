//! Verified per-user MSI downloads and a detached helper, as for Scoop.
use std::io::Read;
use std::path::{Path, PathBuf};

use super::{scoop::Failure, Install, Spec};
use crate::update::version;

const SCRIPT: &str = include_str!("msi-update.ps1");
const RESULT: &str = "msi-update-result.json";

#[cfg(windows)]
pub(super) fn registered_folder() -> Option<PathBuf> {
    // Explicit UTF-8 avoids reg.exe's OEM encoding losing non-ASCII user names.
    let output = Spec::new("powershell.exe", &[
        "-NoProfile", "-NonInteractive", "-Command",
        r#"[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); (Get-ItemProperty -LiteralPath 'HKCU:\Software\Dutchman CAD\Dutchman Convert' -Name InstallDir -ErrorAction Stop).InstallDir"#,
    ]).command().output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

pub(super) fn spawn(
    install: &Install,
    tag: &str,
    package: super::windows::Verified,
) -> Result<(), String> {
    let Install::Msi(installed) = install else {
        return Err("This copy is not installed with the Windows installer.".into());
    };
    let logs = dutchman::diagnostics::default_folder()?;
    let folder = package.folder;
    let msi = package.path;
    let helper = folder.path().join("msi-update.ps1");
    std::fs::write(&helper, SCRIPT).map_err(|e| e.to_string())?;
    let result = logs.join(RESULT);
    let log = logs.join("msi-update.log");
    std::fs::write(&log, "Windows update is starting.\n").map_err(|e| e.to_string())?;
    std::fs::write(
        &result,
        serde_json::json!({"tag": tag, "code": -1}).to_string(),
    )
    .map_err(|e| e.to_string())?;
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
        helper.into_os_string(),
        "-ParentProcessId".into(),
        std::process::id().to_string().into(),
        "-InstallerPath".into(),
        msi.into_os_string(),
        "-GuiPath".into(),
        installed.join("dutchman-gui.exe").into_os_string(),
        "-LogPath".into(),
        log.into_os_string(),
        "-ResultPath".into(),
        result.as_os_str().to_owned(),
        "-Tag".into(),
        tag.into(),
    ]);
    spec.detached = true;
    spec.command().spawn().map_err(|e| {
        let _ = std::fs::remove_file(&result);
        format!("Could not start the installer helper: {e}. Open the release page.")
    })?;
    // The helper owns cleanup after the parent exits.
    let _ = folder.keep();
    Ok(())
}

pub(super) fn startup() -> Option<Failure> {
    let folder: PathBuf = dutchman::diagnostics::default_folder().ok()?;
    read_result(&folder)
}

fn read_result(folder: &Path) -> Option<Failure> {
    let path = folder.join(RESULT);
    let file = std::fs::File::open(&path).ok()?;
    let mut bytes = Vec::new();
    let read = file.take(4097).read_to_end(&mut bytes);
    let _ = std::fs::remove_file(&path);
    let result: Option<serde_json::Value> = read
        .ok()
        .filter(|_| bytes.len() <= 4096)
        .and_then(|_| serde_json::from_slice(&bytes).ok());
    if result
        .as_ref()
        .and_then(|v| v.get("code"))
        .and_then(|v| v.as_i64())
        .is_some_and(|c| c == 0 || c == 3010)
    {
        return None;
    }
    Some(Failure {
        tag: result
            .as_ref()
            .and_then(|v| v.get("tag"))
            .and_then(|v| v.as_str())
            .filter(|tag| version(tag).is_some())
            .unwrap_or(concat!("v", env!("CARGO_PKG_VERSION")))
            .into(),
        error: format!(
            "The Windows update did not finish. Read {} and open the release page to retry.",
            folder.join("msi-update.log").display()
        ),
        output: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_consumes_results_and_keeps_the_installer_log() {
        let folder = tempfile::tempdir().unwrap();
        let result = folder.path().join(RESULT);
        let log = folder.path().join("msi-update.log");
        std::fs::write(&log, "Installer output").unwrap();
        assert!(read_result(folder.path()).is_none());
        for code in [0, 3010, 1603, -1] {
            std::fs::write(
                &result,
                serde_json::json!({"tag": "v1.2.3", "code": code}).to_string(),
            )
            .unwrap();
            let failure = read_result(folder.path());
            if code == 0 || code == 3010 {
                assert!(failure.is_none());
            } else {
                let failure = failure.unwrap();
                assert_eq!(failure.tag, "v1.2.3");
                assert!(failure.error.contains("msi-update.log"));
            }
            assert!(!result.exists());
            assert!(log.exists());
        }
        for bytes in [b"garbage".to_vec(), vec![b'x'; 4097]] {
            std::fs::write(&result, bytes).unwrap();
            assert!(read_result(folder.path()).is_some());
            assert!(!result.exists());
        }
    }
}
