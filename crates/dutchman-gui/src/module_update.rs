//! Electronics updates own only the executable in the per-user modules folder.
#[cfg(test)]
use crate::update::package::replace;
use crate::update::package::{download, verify};
use eframe::egui;
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read, path::Path, sync::mpsc};

const RELEASES: &str = "https://github.com/dutchman-cad/dutchman-electronics/releases";
const LIMIT: u64 = 256 << 20;

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    file: String,
    sha256: String,
}
#[derive(Clone, Deserialize)]
struct Manifest {
    name: String,
    version: String,
    interface: u64,
    assets: BTreeMap<String, Asset>,
}
fn version(v: &str) -> Option<(u64, u64, u64)> {
    if !v
        .split('.')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let mut parts = v.split('.');
    let result = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(result)
}
fn target(os: &str, arch: &str, env: &str) -> Option<&'static str> {
    match (os, arch, env) {
        ("windows", "x86_64", "msvc") => Some("x86_64-pc-windows-msvc"),
        ("macos", "x86_64" | "aarch64", _) => Some("macos-universal"),
        ("linux", "x86_64", "gnu") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64", "gnu") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}
fn platform() -> Option<&'static str> {
    let env = if cfg!(target_env = "gnu") {
        "gnu"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else {
        ""
    };
    target(std::env::consts::OS, std::env::consts::ARCH, env)
}
impl Manifest {
    fn newer(&self, current: &str) -> Result<bool, String> {
        if self.name != "dutchman-electronics" || self.interface != dutchman_core::module::INTERFACE
        {
            return Err("This module update needs another version of Dutchman.".into());
        }
        let next =
            version(&self.version).ok_or("The module release version is not supported yet.")?;
        Ok(current.is_empty()
            || next
                > version(
                    current
                        .trim_start_matches('v')
                        .split(['-', '+'])
                        .next()
                        .unwrap_or(current),
                )
                .ok_or("The installed module version is not supported yet.")?)
    }
    fn asset(&self, target: &str) -> Result<&Asset, String> {
        let a = self
            .assets
            .get(target)
            .ok_or("no update for this platform")?;
        let ext = if target.contains("windows") {
            "zip"
        } else {
            "tar.gz"
        };
        if a.file != format!("dutchman-electronics-{}-{target}.{ext}", self.version)
            || a.sha256.len() != 64
            || !a.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("The module release asset is not supported yet.".into());
        }
        Ok(a)
    }
}
fn current() -> Result<(String, String), String> {
    let Some(p) = dutchman_core::module::find() else {
        return Ok((String::new(), "Electronics module: not found".into()));
    };
    let info = dutchman_core::module::info(&p, &Default::default())
        .map_err(|_| "The installed module could not be run. Check for an update to repair it.")?;
    let source = if dutchman_launcher::modules_folder().is_some_and(|d| p.starts_with(d)) {
        "your modules folder"
    } else {
        "included"
    };
    Ok((
        info.version.clone(),
        format!("Electronics module: v{} ({source})", info.version),
    ))
}
fn check() -> Result<Option<Manifest>, String> {
    let t = platform().ok_or("no update for this platform")?;
    let mut bytes = Vec::new();
    download(
        &format!("{RELEASES}/latest/download/release.json"),
        64 << 10,
        &mut bytes,
    )?;
    let m: Manifest = serde_json::from_slice(&bytes)
        .map_err(|_| "The module release manifest could not be read.")?;
    m.asset(t)?;
    offer(m, current(), t)
}
fn offer(
    m: Manifest,
    installed: Result<(String, String), String>,
    t: &str,
) -> Result<Option<Manifest>, String> {
    m.asset(t)?;
    let current = installed.map(|(v, _)| v).unwrap_or_default();
    Ok(m.newer(&current)?.then_some(m))
}
fn downloaded_info(path: &Path) -> Result<dutchman_core::module::Info, String> {
    dutchman_core::module::info(path, &Default::default()).map_err(|_| {
        "The downloaded module could not be run. Try installing the update again.".into()
    })
}
fn extract(archive: &Path, expected: &str, zip: bool) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    if zip {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        if zip.len() > 1024 {
            return Err("The module archive has too many entries.".into());
        }
        let entry = zip.by_name(expected).map_err(|e| e.to_string())?;
        if !entry.is_file() {
            return Err("The module program is missing.".into());
        }
        let mut bytes = Vec::new();
        entry
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("The module program is too large.".into());
        }
        return Ok(bytes);
    }
    // Read bounded tar records, never unpack paths or links onto disk.
    let mut tar = flate2::read::GzDecoder::new(file).take(LIMIT + 1);
    let mut total = 0u64;
    for _ in 0..1024 {
        let mut header = [0u8; 512];
        tar.read_exact(&mut header).map_err(|e| e.to_string())?;
        total = total
            .checked_add(512)
            .filter(|n| *n <= LIMIT)
            .ok_or("The module archive is too large.")?;
        if header.iter().all(|b| *b == 0) {
            return Err("The module program is missing.".into());
        }
        let field = |start, end| {
            header
                .get(start..end)
                .ok_or("The module archive could not be read.")
        };
        let name = std::str::from_utf8(field(0, 100)?)
            .map_err(|e| e.to_string())?
            .trim_end_matches('\0');
        let size = u64::from_str_radix(
            std::str::from_utf8(field(124, 136)?)
                .map_err(|e| e.to_string())?
                .trim_matches(['\0', ' ']),
            8,
        )
        .map_err(|e| e.to_string())?;
        let padded = size
            .checked_add(511)
            .map(|n| n / 512 * 512)
            .ok_or("The module archive is too large.")?;
        total = total
            .checked_add(padded)
            .filter(|n| *n <= LIMIT)
            .ok_or("The module archive is too large.")?;
        if name == expected {
            if !matches!(header.get(156), Some(0 | b'0')) {
                return Err("The module program must be a regular file.".into());
            }
            let mut bytes = Vec::new();
            tar.take(size)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 != size {
                return Err("The module download ended early.".into());
            }
            return Ok(bytes);
        }
        std::io::copy(&mut tar.by_ref().take(padded), &mut std::io::sink())
            .map_err(|e| e.to_string())?;
    }
    Err("The module archive has too many entries.".into())
}
fn install(m: &Manifest, folder: &Path) -> Result<(), String> {
    if !m.newer(&current().map(|(v, _)| v).unwrap_or_default())? {
        return Err("The electronics module is already up to date.".into());
    }
    let t = platform().ok_or("no update for this platform")?;
    install_with(m, folder, t, &download)
}
type Fetch<'a> = dyn Fn(&str, u64, &mut dyn std::io::Write) -> Result<(), String> + 'a;
fn install_with(m: &Manifest, folder: &Path, t: &str, fetch: &Fetch<'_>) -> Result<(), String> {
    m.newer("")?;
    let a = m.asset(t)?;
    let program = if t.contains("windows") {
        "dutchman-electronics.exe"
    } else {
        "dutchman-electronics"
    };
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|_| "The included program folder could not be found.")?;
    let bundled = exe
        .parent()
        .ok_or("The included program folder could not be found.")?
        .join(program);
    let target = owned_target(folder, program, &bundled)?;
    std::fs::create_dir_all(folder).map_err(|_| "Your modules folder could not be created.")?;
    let stage = download_stage(&std::env::temp_dir())?;
    let archive = stage.path().join("archive");
    let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    fetch(
        &format!("{RELEASES}/download/v{}/{}", m.version, a.file),
        LIMIT,
        &mut file,
    )?;
    drop(file);
    verify(&archive, &a.sha256.to_ascii_lowercase())?;
    let bytes = extract(
        &archive,
        &format!("dutchman-electronics-{}-{t}/{program}", m.version),
        t.contains("windows"),
    )?;
    let mut staged = tempfile::Builder::new()
        .prefix(".dutchman-module-")
        .suffix(std::env::consts::EXE_SUFFIX)
        .tempfile_in(folder)
        .map_err(|_| "The module update could not be prepared in your modules folder.")?;
    std::io::Write::write_all(&mut staged, &bytes)
        .map_err(|_| "The module update could not be written.")?;
    staged
        .as_file()
        .sync_all()
        .map_err(|_| "The module update could not be written.")?;
    // Close the writable handle before executing (ETXTBSY on Unix).
    let unpacked = staged.into_temp_path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&unpacked, std::fs::Permissions::from_mode(0o755))
            .map_err(|_| "The module update could not be prepared.")?;
    }
    let info = downloaded_info(&unpacked)?;
    if info.version != m.version || info.interface != dutchman_core::module::INTERFACE {
        return Err("The downloaded module does not match its release.".into());
    }
    unpacked.persist(&target).map_err(|_| {
        "The module could not be replaced. Close other programs using it and try again.".into()
    })
}
fn download_stage(root: &Path) -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("dutchman-download-")
        .tempdir_in(root)
        .map_err(|_| "The module download could not be prepared.".into())
}
fn future_path(path: &Path) -> Result<std::path::PathBuf, String> {
    if let Ok(path) = path.canonicalize() {
        return Ok(path);
    }
    let parent = path
        .parent()
        .ok_or("Your modules folder could not be resolved.")?;
    let name = path
        .file_name()
        .ok_or("Your modules folder could not be resolved.")?;
    Ok(future_path(parent)?.join(name))
}
fn owned_target(
    folder: &Path,
    program: &str,
    bundled: &Path,
) -> Result<std::path::PathBuf, String> {
    let folder = future_path(folder)?;
    let target = folder.join(program);
    let included_folder = bundled.parent().and_then(|p| future_path(p).ok());
    let same_program = target
        .canonicalize()
        .ok()
        .zip(bundled.canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if included_folder.as_ref() == Some(&folder) || same_program {
        return Err(
            "The module update would replace the included copy. Use a separate modules folder and try again."
                .into(),
        );
    }
    Ok(target)
}

enum Event {
    Status(String),
    Checked(Option<Manifest>),
    Installed { label: String, version: String },
}
#[derive(Default)]
pub(crate) struct Updater {
    label: String,
    status: String,
    offer: Option<Manifest>,
    rx: Option<mpsc::Receiver<Result<Event, String>>>,
}
impl Updater {
    #[cfg(feature = "test-support")]
    pub(crate) fn preview(&mut self, state: u8) {
        self.rx = None;
        self.label = format!(
            "Electronics module: v{} ({})",
            if state == 2 { "0.2.0" } else { "0.1.0" },
            if state == 2 {
                "your modules folder"
            } else {
                "included"
            }
        );
        self.status = if state == 1 {
            "Version 0.2.0 is available."
        } else if state == 2 {
            "Updated to v0.2.0."
        } else {
            "Electronics module is up to date."
        }
        .into();
        self.offer = (state == 1)
            .then(|| {
                serde_json::from_value(serde_json::json!({
                    "name":"dutchman-electronics","version":"0.2.0","interface":1,"assets":{}
                }))
                .ok()
            })
            .flatten();
    }

    fn start(&mut self, ctx: &egui::Context, install_offer: bool) {
        let offer = if install_offer {
            self.offer.take()
        } else {
            self.offer = None;
            None
        };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if let Some(m) = offer {
                dutchman_launcher::modules_folder()
                    .ok_or("Your modules folder could not be found.".into())
                    .and_then(|folder| install(&m, &folder))
                    .map(|()| Event::Installed {
                        label: format!("Electronics module: v{} (your modules folder)", m.version),
                        version: m.version.to_string(),
                    })
            } else {
                check().map(Event::Checked)
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn receive(&mut self) -> Option<Result<Event, String>> {
        match self.rx.as_ref()?.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                Some(Err("The update check stopped unexpectedly.".into()))
            }
        }
    }
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        if self.label.is_empty() && self.rx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.rx = Some(rx);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(current().map(|(_, label)| Event::Status(label)));
                ctx.request_repaint();
            });
        }
        if let Some(result) = self.receive() {
            self.rx = None;
            match result {
                Ok(Event::Status(label)) => self.label = label,
                Ok(Event::Installed { label, version }) => {
                    self.label = label;
                    self.offer = None;
                    self.status = format!("Updated to v{version}.");
                }
                Ok(Event::Checked(m)) => {
                    self.status = m.as_ref().map_or_else(
                        || "Electronics module is up to date.".into(),
                        |manifest| format!("Version {} is available.", manifest.version),
                    );
                    self.offer = m;
                }
                Err(e) => {
                    self.status = e
                        .lines()
                        .next()
                        .unwrap_or("The module update failed.")
                        .into();
                    if self.label.is_empty() {
                        self.label = "Electronics module: not found".into();
                    }
                }
            }
        }
        ui.label(&self.label);
        if ui
            .add_enabled(self.rx.is_none(), egui::Button::new("Check for update"))
            .clicked()
        {
            self.start(ui.ctx(), false);
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        if self.offer.is_some()
            && ui
                .add_enabled(
                    self.rx.is_none(),
                    egui::Button::new("Update electronics module"),
                )
                .clicked()
        {
            self.start(ui.ctx(), true);
        }
        if self.rx.is_some() {
            ui.spinner();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> Manifest {
        serde_json::from_value(serde_json::json!({
            "name":"dutchman-electronics","version":"0.2.0","interface":1,
            "assets":{"x86_64-unknown-linux-gnu":{
                "file":"dutchman-electronics-0.2.0-x86_64-unknown-linux-gnu.tar.gz",
                "sha256":"a".repeat(64)
            }}
        }))
        .unwrap()
    }
    #[test]
    fn disconnected_worker_stops_waiting() {
        let (tx, rx) = mpsc::channel();
        let mut updater = Updater {
            rx: Some(rx),
            ..Default::default()
        };
        assert!(updater.receive().is_none());
        drop(tx);
        assert!(
            matches!(updater.receive(), Some(Err(e)) if e == "The update check stopped unexpectedly.")
        );
        assert!(updater.rx.is_none());
    }
    #[test]
    fn failed_installed_info_offers_repair() {
        assert!(offer(
            manifest(),
            Err("private path".into()),
            "x86_64-unknown-linux-gnu"
        )
        .unwrap()
        .is_some());
    }
    #[test]
    fn installed_suffixes_are_accepted_but_release_suffixes_are_rejected() {
        let mut m = manifest();
        m.version = "0.4.0".into();
        for v in ["0.3.0-rc.1", "0.3.0+build", "v0.3.0"] {
            assert!(m.newer(v).unwrap());
        }
        for v in ["v0.4.0", "0.4.0-rc.1", "0.4.0+build", "+0.4.0", "0.4"] {
            m.version = v.into();
            assert!(m.newer("").is_err());
        }
    }
    #[test]
    fn preparation_and_info_errors_hide_paths() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("private-missing-program");
        assert_eq!(
            downloaded_info(&missing).unwrap_err(),
            "The downloaded module could not be run. Try installing the update again."
        );
        let file = dir.path().join("private-file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            download_stage(&file).unwrap_err(),
            "The module download could not be prepared."
        );
        let folder = file.join("modules");
        let err = install_with(
            &manifest(),
            &folder,
            "x86_64-unknown-linux-gnu",
            &|_, _, _| Err("Download must not start.".into()),
        )
        .unwrap_err();
        assert_eq!(err, "Your modules folder could not be created.");
        assert!(!folder.exists());
    }
    #[test]
    fn refused_update_does_not_create_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("not-created");
        let mut m = manifest();
        m.version = "v0.2.0".into();
        assert!(
            install_with(&m, &folder, "x86_64-unknown-linux-gnu", &|_, _, _| Err(
                "Download must not start.".into()
            ))
            .is_err()
        );
        assert!(!folder.exists());
        let bundled = folder.join("dutchman-electronics");
        assert!(owned_target(&folder, "dutchman-electronics", &bundled).is_err());
        assert!(!folder.exists());
    }
    #[test]
    fn manifest_compares_versions_and_interface() {
        let mut m = manifest();
        assert!(m.newer("0.1.0").unwrap());
        assert!(!m.newer("0.2.0").unwrap());
        assert!(!m.newer("0.3.0").unwrap());
        assert!(!m.newer("0.10.0").unwrap());
        m.interface = 2;
        assert!(m.newer("0.1.0").is_err());
    }
    #[test]
    fn assets_follow_the_manifest_and_platform() {
        for (os, arch, env, want) in [
            ("windows", "x86_64", "msvc", "x86_64-pc-windows-msvc"),
            ("macos", "aarch64", "", "macos-universal"),
            ("linux", "x86_64", "gnu", "x86_64-unknown-linux-gnu"),
            ("linux", "aarch64", "gnu", "aarch64-unknown-linux-gnu"),
        ] {
            assert_eq!(target(os, arch, env), Some(want));
        }
        assert_eq!(target("linux", "riscv64", "gnu"), None);
        assert_eq!(target("linux", "x86_64", "musl"), None);
        assert!(manifest()
            .asset("unknown")
            .unwrap_err()
            .contains("no update"));
        assert!(manifest().asset("x86_64-unknown-linux-gnu").is_ok());
    }
    #[test]
    fn checksum_failure_keeps_user_and_bundled_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive");
        let user = dir.path().join("dutchman-electronics");
        let bundled = dir.path().join("bundled");
        std::fs::write(&archive, b"download").unwrap();
        std::fs::write(&user, b"old user").unwrap();
        std::fs::write(&bundled, b"bundle").unwrap();
        let mut m = manifest();
        m.assets.get_mut("x86_64-unknown-linux-gnu").unwrap().sha256 = "0".repeat(64);
        let result = install_with(&m, dir.path(), "x86_64-unknown-linux-gnu", &|_, _, out| {
            out.write_all(b"download").map_err(|e| e.to_string())
        });
        assert!(result.unwrap_err().contains("checksum"));
        assert_eq!(std::fs::read(&user).unwrap(), b"old user");
        assert_eq!(std::fs::read(&bundled).unwrap(), b"bundle");
    }
    #[test]
    fn zip_reads_only_the_expected_program() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("module.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let path = "dutchman-electronics-0.2.0-x86_64-pc-windows-msvc/dutchman-electronics.exe";
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"synthetic program").unwrap();
        zip.finish().unwrap();
        assert_eq!(extract(&archive, path, true).unwrap(), b"synthetic program");
        assert!(extract(&archive, "unexpected.exe", true).is_err());
        assert!(!dir.path().join(path).exists());
    }
    #[test]
    fn atomic_replace_only_changes_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("new");
        let user = dir.path().join("dutchman-electronics");
        let bundled = dir.path().join("bundled");
        std::fs::write(&source, b"new module").unwrap();
        std::fs::write(&user, b"old module").unwrap();
        std::fs::write(&bundled, b"bundle").unwrap();
        replace(&source, &user).unwrap();
        assert_eq!(std::fs::read(&user).unwrap(), b"new module");
        assert_eq!(std::fs::read(&bundled).unwrap(), b"bundle");
        assert!(replace(&dir.path().join("missing"), &user).is_err());
        assert_eq!(std::fs::read(&user).unwrap(), b"new module");
    }
}

#[cfg(all(test, unix))]
mod install_tests {
    use super::*;
    use sha2::Digest;
    use std::io::Write;
    #[test]
    fn a_modules_link_cannot_replace_the_included_program() {
        let dir = tempfile::tempdir().unwrap();
        let included = dir.path().join("included");
        std::fs::create_dir(&included).unwrap();
        let program = included.join("dutchman-electronics");
        std::fs::write(&program, b"included module").unwrap();
        let user = dir.path().join("modules");
        std::os::unix::fs::symlink(&included, &user).unwrap();
        assert!(owned_target(&user, "dutchman-electronics", &program).is_err());
        assert_eq!(std::fs::read(&program).unwrap(), b"included module");
        let separate = dir.path().join("separate");
        std::fs::create_dir(&separate).unwrap();
        std::os::unix::fs::symlink(&program, separate.join("dutchman-electronics")).unwrap();
        assert!(owned_target(&separate, "dutchman-electronics", &program).is_err());
        assert_eq!(std::fs::read(&program).unwrap(), b"included module");
    }
    #[test]
    fn local_manifest_archive_installs_only_the_user_program() {
        let target = "x86_64-unknown-linux-gnu";
        let program =
            b"#!/bin/sh\ncase \"$0\" in */modules/.dutchman-module-*) ;; *) exit 8;; esac\necho '{\"name\":\"electronics\",\"version\":\"0.2.0\",\"interface\":1}'\n";
        let name = format!("dutchman-electronics-0.2.0-{target}/dutchman-electronics");
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:011o}\0", program.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].fill(b' ');
        let sum: u64 = header.iter().map(|b| u64::from(*b)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&header).unwrap();
        gzip.write_all(program).unwrap();
        gzip.write_all(&vec![0; 512 - program.len()]).unwrap();
        gzip.write_all(&[0; 1024]).unwrap();
        let archive = gzip.finish().unwrap();
        let sha: String = sha2::Sha256::digest(&archive)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let m: Manifest = serde_json::from_value(serde_json::json!({
            "name":"dutchman-electronics","version":"0.2.0","interface":1,
            "assets":{"x86_64-unknown-linux-gnu":{"file":format!("dutchman-electronics-0.2.0-{target}.tar.gz"),"sha256":sha}}
        })).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("included");
        std::fs::write(&bundle, b"bundle").unwrap();
        let user = dir.path().join("modules");
        let fetch = |url: &str, _limit, out: &mut dyn Write| {
            assert_eq!(
                url,
                format!("{RELEASES}/download/v0.2.0/dutchman-electronics-0.2.0-{target}.tar.gz")
            );
            out.write_all(&archive).map_err(|e| e.to_string())
        };
        install_with(&m, &user, target, &fetch).unwrap();
        assert_eq!(
            std::fs::read(user.join("dutchman-electronics")).unwrap(),
            program
        );
        assert_eq!(std::fs::read(&bundle).unwrap(), b"bundle");
        let fail = |_: &str, _, _: &mut dyn Write| Err("Offline. Try again later.".into());
        assert!(install_with(&m, &user, target, &fail).is_err());
        assert_eq!(
            std::fs::read(user.join("dutchman-electronics")).unwrap(),
            program
        );
        assert_eq!(std::fs::read(&bundle).unwrap(), b"bundle");
    }
}
