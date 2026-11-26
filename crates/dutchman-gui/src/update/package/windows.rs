//! Windows release downloads: verify first, then install or replace the portable programs.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Install, Spec};
use crate::update::{version, Timeouts, RELEASES};

/// Largest download accepted; release ZIPs and AppImages are about 170 MB.
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
/// Largest total unpacked from an update ZIP. The three release programs alone unpack to
/// about 575 MB, so this is separate from, and larger than, the download limit.
const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;
const GUIDES: [&str; 3] = ["AGENTS.md", "docs", "skills"];
const PROGRAMS: [&str; 3] = ["dutchman-gui.exe", "dutchman.exe", "dutchman-mcp.exe"];

pub(super) struct Verified {
    pub(super) folder: tempfile::TempDir,
    pub(super) path: PathBuf,
}

fn asset_url(tag: &str, extension: &str) -> Result<String, String> {
    version(tag).ok_or("No stable Windows package version was found. Open the release page.")?;
    Ok(format!(
        "{RELEASES}/download/{tag}/dutchman-{}-x86_64-pc-windows-msvc.{extension}",
        tag.trim_start_matches('v')
    ))
}

fn allowed_url(url: &str) -> bool {
    // Tests talk to a local server.
    #[cfg(test)]
    if url.starts_with("http://127.0.0.1:") {
        return true;
    }
    url.starts_with(&format!("{RELEASES}/download/"))
        || [
            "release-assets.githubusercontent.com",
            "objects.githubusercontent.com",
        ]
        .iter()
        .any(|host| url.starts_with(&format!("https://{host}/")))
}

/// Said whenever GitHub cannot be reached or stops answering; the installed copy is untouched.
pub(super) const UNREACHABLE: &str = "The update could not be downloaded: GitHub did not answer in time or the connection was blocked (a firewall or proxy may be in the way). Your installed Dutchman is unchanged. Use the manual download link to get the new version.";

/// Downloads up to this size are checksums and the like, not packages.
const SMALL_LIMIT: u64 = 64 * 1024;

/// Said when a read or write timed out or no connection could be made.
fn interrupted(detail: &dyn std::fmt::Display) -> String {
    format!("The update download was interrupted ({detail}). Your installed Dutchman is unchanged. Use the manual download link to get the new version.")
}

fn timed_out(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

pub(super) fn download(url: &str, limit: u64, out: &mut dyn Write) -> Result<(), String> {
    // The package has no total limit (a slow download that keeps moving must finish); small
    // files such as the checksum also get a deadline for the whole request.
    let timeouts = if limit <= SMALL_LIMIT {
        Timeouts::SMALL
    } else {
        Timeouts::DEFAULT
    };
    download_with(url, limit, out, timeouts)
}

pub(super) fn download_with(
    url: &str,
    limit: u64,
    out: &mut dyn Write,
    timeouts: Timeouts,
) -> Result<(), String> {
    let agent = crate::update::agent(timeouts, 0);
    let mut url = url.to_owned();
    for _ in 0..6 {
        if !allowed_url(&url) {
            return Err("The update download host is not allowed. Open the release page.".into());
        }
        let response = match agent
            .get(&url)
            .set(
                "User-Agent",
                concat!("Dutchman-update-check/", env!("CARGO_PKG_VERSION")),
            )
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(code, _)) => {
                return Err(format!(
                    "The update download was refused (HTTP {code}). Open the release page to download it manually."
                ))
            }
            Err(ureq::Error::Transport(t)) => {
                use std::error::Error;
                // Only a failed lookup or connection and a timeout look like a block; a reset or
                // a TLS error says what it was.
                let blocked = matches!(
                    t.kind(),
                    ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed
                ) || t
                    .source()
                    .and_then(|s| s.downcast_ref::<std::io::Error>())
                    .is_some_and(timed_out);
                return Err(if blocked {
                    UNREACHABLE.into()
                } else {
                    interrupted(&t)
                });
            }
        };
        if matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            url = response
                .header("Location")
                .ok_or("The update redirect has no location.")?
                .to_owned();
            continue;
        }
        if response.status() != 200 {
            return Err("The update download did not finish. Open the release page.".into());
        }
        let mut reader = response.into_reader().take(limit + 1);
        let mut buffer = [0; 64 * 1024];
        let mut count = 0u64;
        loop {
            // A read that fails is the network; a write that fails is the disk.
            let n = reader.read(&mut buffer).map_err(|e| {
                if timed_out(&e) {
                    UNREACHABLE.to_owned()
                } else {
                    interrupted(&e)
                }
            })?;
            if n == 0 {
                break;
            }
            out.write_all(
                buffer
                    .get(..n)
                    .ok_or("Could not read the update download.")?,
            )
            .map_err(|e| e.to_string())?;
            count += n as u64;
        }
        if count > limit {
            return Err("The update download is too large. Open the release page.".into());
        }
        return Ok(());
    }
    Err("The update download redirected too many times. Open the release page.".into())
}

fn checksum(bytes: &[u8], name: &str) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Could not read the update checksum.")?;
    let mut fields = text.split_whitespace();
    let hash = fields.next().unwrap_or("");
    if hash.len() != 64
        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        || fields.next() != Some(name)
        || fields.next().is_some()
    {
        return Err("Could not read the update checksum. Open the release page.".into());
    }
    Ok(hash.to_ascii_lowercase())
}

pub(super) fn prepare(
    install: &Install,
    tag: &str,
    fetch: &impl Fn(&str, u64, &mut dyn Write) -> Result<(), String>,
) -> Result<Option<Verified>, String> {
    prepare_in(
        install,
        tag,
        &dutchman::diagnostics::default_folder()?,
        fetch,
    )
}

fn prepare_in(
    install: &Install,
    tag: &str,
    logs: &Path,
    fetch: &impl Fn(&str, u64, &mut dyn Write) -> Result<(), String>,
) -> Result<Option<Verified>, String> {
    let url = match install {
        Install::Msi(_) => asset_url(tag, "msi")?,
        Install::Zip(_) => asset_url(tag, "zip")?,
        Install::AppImage(_) => super::appimage::asset_url(tag, std::env::consts::ARCH)?,
        _ => return Err("This copy does not use a release package.".into()),
    };
    let name = url
        .rsplit('/')
        .next()
        .ok_or("The update has no file name.")?;
    std::fs::create_dir_all(logs).map_err(|e| e.to_string())?;
    let folder = tempfile::Builder::new()
        .prefix("windows-update-")
        .tempdir_in(logs)
        .map_err(|e| e.to_string())?;
    let mut hash = Vec::new();
    fetch(&format!("{url}.sha256"), 4096, &mut hash)?;
    if hash.len() > 4096 {
        return Err("The update checksum is too large.".into());
    }
    let expected = checksum(&hash, name)?;
    let path = folder.path().join(name);
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    fetch(&url, MAX_PACKAGE, &mut file)?;
    drop(file);
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_PACKAGE {
        return Err("The update download is too large.".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(
            buffer
                .get(..n)
                .ok_or("Could not read the update download.")?,
        );
    }
    let actual: String = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    drop(file);
    if actual != expected {
        return Err("The update checksum does not match. The download was deleted; your installed copy is unchanged. Open the release page to retry.".into());
    }
    if let Install::AppImage(target) = install {
        super::appimage::replace(&path, target)?;
        Ok(None)
    } else if let Install::Zip(installed) = install {
        let stem = name
            .strip_suffix(".zip")
            .ok_or("The update is not a ZIP package.")?;
        apply_zip(&path, stem, installed)?;
        Ok(None)
    } else {
        Ok(Some(Verified { folder, path }))
    }
}

fn apply_zip(path: &Path, stem: &str, installed: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Could not open the update ZIP: {e}"))?;
    if archive.len() > 1000 {
        return Err("The update ZIP has too many entries.".into());
    }
    // Stage on the destination volume so every rename stays on one filesystem.
    let stage = tempfile::Builder::new()
        .prefix("dutchman-update-")
        .tempdir_in(installed)
        .map_err(|e| {
            format!(
                "Could not write beside Dutchman: {e}. Open the release page for a manual update."
            )
        })?;
    let mut remaining = MAX_UNPACKED;
    for name in PROGRAMS {
        if !installed.join(name).is_file() || installed.join(backup_name(name)).exists() {
            return Err(
                "Close other Dutchman copies and restart before updating this portable folder."
                    .into(),
            );
        }
        let mut entry = archive
            .by_name(&format!("{stem}/{name}"))
            .map_err(|_| format!("The update ZIP is missing {name}. Open the release page."))?;
        if entry.size() > remaining {
            return Err(
                "The update ZIP unpacks to more than Dutchman accepts. Open the release page."
                    .into(),
            );
        }
        if !entry.is_file()
            || entry.size() == 0
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err(
                "The update ZIP has an unsupported program entry. Open the release page.".into(),
            );
        }
        let expected = entry.size();
        let mut file = std::fs::File::create(stage.path().join(name)).map_err(|e| e.to_string())?;
        let size = std::io::copy(&mut entry.by_ref().take(remaining + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if size != expected {
            return Err(
                "The update ZIP program size does not match. Open the release page.".into(),
            );
        }
        remaining = remaining.saturating_sub(size);
    }
    // Refresh bundled guidance along with the programs, including older portable copies
    // that did not yet contain it. Only the files the package lists are replaced, one by
    // one, so a folder of the user's own (a `docs` they made, say) keeps everything else.
    let mut guides = [false; 3];
    let mut guide_files: Vec<String> = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        let Some(relative) = name.strip_prefix(&format!("{stem}/")) else {
            continue;
        };
        let selected = relative == "AGENTS.md"
            || relative.starts_with("docs/")
            || relative.starts_with("skills/");
        if !selected || entry.is_dir() {
            continue;
        }
        if relative.contains('\\')
            || Path::new(relative)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || entry.size() > remaining
        {
            return Err(
                "The update ZIP has an unsupported guide entry. Open the release page.".into(),
            );
        }
        for (present, name) in guides.iter_mut().zip(GUIDES) {
            if relative == name || relative.starts_with(&format!("{name}/")) {
                *present = true;
            }
        }
        let target = stage.path().join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let expected = entry.size();
        let mut file = std::fs::File::create(target).map_err(|e| e.to_string())?;
        let size = std::io::copy(&mut entry.by_ref().take(remaining + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if size != expected {
            return Err("The update ZIP guide size does not match.".into());
        }
        remaining = remaining.saturating_sub(size);
        guide_files.push(relative.to_owned());
    }
    if guides.iter().any(|v| *v) && !guides.iter().all(|v| *v) {
        return Err("The update ZIP guidance is incomplete. Open the release page.".into());
    }
    let mut targets: Vec<String> = PROGRAMS.iter().map(|n| (*n).to_owned()).collect();
    if guides.iter().all(|v| *v) {
        targets.extend(guide_files);
    }
    if installed.join(STATE).exists()
        || targets
            .iter()
            .any(|name| installed.join(backup_name(name)).exists())
    {
        return Err(
            "Close other Dutchman copies and restart before updating this portable folder.".into(),
        );
    }
    // Written before anything moves: the next start deletes exactly these backups and nothing else.
    let listed: Vec<String> = targets
        .iter()
        .filter(|name| installed.join(name).is_file())
        .map(|name| backup_name(name))
        .collect();
    std::fs::write(installed.join(STATE), listed.join("\n")).map_err(|e| e.to_string())?;
    let mut renamed: Vec<&str> = Vec::new();
    let mut replaced: Vec<&str> = Vec::new();
    let result = (|| {
        for name in &targets {
            if installed.join(name).is_file() {
                std::fs::rename(installed.join(name), installed.join(backup_name(name)))?;
                renamed.push(name);
            }
        }
        for name in &targets {
            let to = installed.join(name);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(stage.path().join(name), to)?;
            replaced.push(name);
        }
        Ok::<_, std::io::Error>(())
    })();
    if let Err(error) = result {
        for name in replaced {
            let _ = std::fs::remove_file(installed.join(name));
        }
        let mut restore_errors = Vec::new();
        for name in renamed.into_iter().rev() {
            if let Err(e) = std::fs::rename(installed.join(backup_name(name)), installed.join(name))
            {
                restore_errors.push(format!("{name}: {e}"));
            }
        }
        if restore_errors.is_empty() {
            let _ = std::fs::remove_file(installed.join(STATE));
        }
        return Err(format!("Could not replace the portable programs: {error}. {}Open the release page for a manual update.",
            if restore_errors.is_empty() { "The original programs were restored. ".into() } else { format!("Restore the .dutchman-old backups: {}. ", restore_errors.join("; ")) }));
    }
    Ok(())
}

/// Lists the backups of the last update, one relative path per line. Only these are ever
/// deleted, so a user's own `.old` files and folders are never touched.
const STATE: &str = ".dutchman-update-state";

/// The name a replaced file is parked under until the next start.
fn backup_name(name: &str) -> String {
    format!("{name}.dutchman-old")
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn cleanup(install: &Install) {
    let Install::Zip(folder) = install else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(folder.join(STATE)) else {
        return;
    };
    let mut kept = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let path = Path::new(line);
        let safe = line.ends_with(".dutchman-old")
            && path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)));
        if safe && path.components().count() > 0 {
            let full = folder.join(path);
            if full.is_file() && std::fs::remove_file(&full).is_err() {
                kept.push(line);
            }
        }
    }
    if kept.is_empty() {
        let _ = std::fs::remove_file(folder.join(STATE));
    } else {
        let _ = std::fs::write(folder.join(STATE), kept.join("\n"));
    }
}

pub(super) fn restart(install: &Install) -> Result<(), String> {
    let Install::Zip(installed) = install else {
        return Err("This copy is not a portable Windows installation.".into());
    };
    let folder = dutchman::diagnostics::default_folder()?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let helper = folder.join("zip-restart.ps1");
    std::fs::write(&helper, include_str!("zip-restart.ps1")).map_err(|e| e.to_string())?;
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
        "-GuiPath".into(),
        installed.join("dutchman-gui.exe").into_os_string(),
    ]);
    spec.detached = true;
    spec.command().spawn().map(|_| ()).map_err(|e| {
        format!("Could not restart Dutchman: {e}. Close it and open the updated copy.")
    })
}

#[cfg(test)]
mod tests;
