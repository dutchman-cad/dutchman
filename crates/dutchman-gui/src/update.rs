//! Bounded, conditional release checks. Network work stays off the UI thread.
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use serde_json::Value;

mod package;
use package::{Install, Job, Phase};

const FEED: &str = "https://github.com/dutchman-cad/dutchman/releases.atom";
const API: &str = "https://api.github.com/repos/dutchman-cad/dutchman/releases/latest";
const RELEASES: &str = "https://github.com/dutchman-cad/dutchman/releases";
const MAX_BYTES: usize = 1_048_576;
const MAX_NOTES: usize = 8_000;
const HOUR: u64 = 3600;
const THREE_DAYS: u64 = 3 * 24 * HOUR;

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub notes: String,
    pub download: String,
}

fn version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let mut number = || {
        let s = parts.next()?;
        if s.is_empty()
            || !s.bytes().all(|b| b.is_ascii_digit())
            || (s.len() > 1 && s.starts_with('0'))
        {
            return None;
        }
        s.parse().ok()
    };
    let result = (number()?, number()?, number()?);
    parts.next().is_none().then_some(result)
}

fn plain_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars().take(MAX_BYTES) {
        match c {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
        if text.len() >= MAX_NOTES {
            break;
        }
    }
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .chars()
        .take(MAX_NOTES)
        .collect()
}

fn parse_feed(body: &[u8]) -> Result<Option<Release>, String> {
    Ok(feed_entries(body)?
        .into_iter()
        .max_by_key(|r| version(&r.tag)))
}

fn feed_entries(body: &[u8]) -> Result<Vec<Release>, String> {
    if body.len() > MAX_BYTES {
        return Err("Release feed is too large.".into());
    }
    let xml = std::str::from_utf8(body).map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            nodes_limit: 20_000,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    if !doc.root_element().has_tag_name("feed") {
        return Err("Release feed could not be read.".into());
    }
    let mut releases = Vec::new();
    for entry in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("entry"))
        .take(200)
    {
        let field = |name| {
            entry
                .children()
                .find(|n| n.has_tag_name(name))
                .and_then(|n| n.text())
                .unwrap_or("")
        };
        let tag = field("id").rsplit('/').next().unwrap_or("");
        if version(tag).is_some() {
            releases.push(Release {
                tag: tag.into(),
                notes: plain_text(field("content")),
                download: format!("{RELEASES}/tag/{tag}"),
            });
        }
    }
    Ok(releases)
}

enum Response {
    Unchanged,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

struct Checked {
    clear_release: bool,
    release: Option<Release>,
    etag: Option<String>,
}

fn check(
    fetch: &impl Fn(&str, Option<&str>) -> Result<Response, String>,
    known: &str,
    etag: Option<&str>,
) -> Result<Checked, String> {
    let mut feed_etag = None;
    let mut feed_release = None;
    match fetch(FEED, etag) {
        Ok(Response::Unchanged) => {
            return Ok(Checked {
                clear_release: false,
                release: None,
                etag: etag.map(str::to_owned),
            })
        }
        Ok(Response::Body { bytes, etag }) => {
            if let Ok(top) = parse_feed(&bytes) {
                feed_etag = etag;
                if top.as_ref().and_then(|r| version(&r.tag)) <= version(known) {
                    return Ok(Checked {
                        clear_release: false,
                        release: None,
                        etag: feed_etag,
                    });
                }
                feed_release = top;
            }
        }
        Err(_) => {}
    }
    let Response::Body { bytes, .. } = fetch(API, None)? else {
        return Err("Release details could not be read.".into());
    };
    if bytes.len() > MAX_BYTES {
        return Err("Release details are too large.".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let tag = value.get("tag_name").and_then(Value::as_str).unwrap_or("");
    if value.get("prerelease").and_then(Value::as_bool) == Some(true)
        || value.get("draft").and_then(Value::as_bool) == Some(true)
        || version(tag).is_none()
    {
        return Err("No stable release was found.".into());
    }
    // Do not memoize a feed tag until the API has caught up with it.
    if feed_release
        .as_ref()
        .is_some_and(|r| version(&r.tag) > version(tag))
    {
        return Err("Release details are not available yet.".into());
    }
    let notes = feed_release
        .filter(|r| r.tag == tag)
        .map(|r| r.notes)
        .unwrap_or_default();
    let appimage = std::env::current_exe()
        .ok()
        .is_some_and(|path| is_appimage(&path));
    let download = value
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(200)
        .find_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let url = asset.get("browser_download_url")?.as_str()?;
            (matches_asset(name, std::env::consts::OS, std::env::consts::ARCH, appimage)
                && url.starts_with("https://github.com/dutchman-cad/dutchman/releases/download/"))
            .then(|| url.to_owned())
        })
        .unwrap_or_else(|| format!("{RELEASES}/tag/{tag}"));
    Ok(Checked {
        clear_release: false,
        release: (version(tag) > version(known)).then(|| Release {
            tag: tag.into(),
            notes,
            download,
        }),
        etag: feed_etag,
    })
}

fn check_package(
    fetch: &impl Fn(&str, Option<&str>) -> Result<Response, String>,
    install: &Install,
    current: &str,
    known: &str,
    etag: Option<&str>,
) -> Result<Checked, String> {
    let source = install
        .source()
        .ok_or("No package metadata source was found.")?;
    let Response::Body { bytes, etag } = fetch(source, etag)? else {
        return Ok(Checked {
            clear_release: false,
            release: None,
            etag: etag.map(str::to_owned),
        });
    };
    let tag = package::parse_version(install, &bytes)?;
    if version(&tag) <= version(current) {
        return Ok(Checked {
            clear_release: true,
            release: None,
            etag,
        });
    }
    if tag == known {
        return Ok(Checked {
            clear_release: false,
            release: None,
            etag,
        });
    }
    let notes = match fetch(FEED, None) {
        Ok(Response::Body { bytes, .. }) => feed_entries(&bytes)
            .ok()
            .and_then(|entries| entries.into_iter().find(|entry| entry.tag == tag))
            .map(|entry| entry.notes)
            .unwrap_or_default(),
        _ => String::new(),
    };
    Ok(Checked {
        clear_release: false,
        release: Some(Release {
            download: format!("{RELEASES}/tag/{tag}"),
            tag,
            notes,
        }),
        etag,
    })
}

fn is_appimage(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.contains("/.mount_") || path.ends_with(".AppImage")
}

fn matches_asset(name: &str, os: &str, arch: &str, appimage: bool) -> bool {
    match os {
        "macos" => name.ends_with("-macos-universal.dmg"),
        "windows" => name.ends_with(&format!("-{arch}-pc-windows-msvc.zip")),
        "linux" if appimage => name.contains(arch) && name.ends_with(".AppImage"),
        "linux" => name.ends_with(&format!("-{arch}-unknown-linux-gnu.tar.gz")),
        _ => false,
    }
}

fn fetch(url: &str, etag: Option<&str>) -> Result<Response, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let mut request = agent.get(url).set(
        "User-Agent",
        concat!("Dutchman-update-check/", env!("CARGO_PKG_VERSION")),
    );
    if let Some(etag) = etag {
        request = request.set("If-None-Match", etag);
    }
    let response = request.call().map_err(|e| e.to_string())?;
    if response.status() == 304 {
        return Ok(Response::Unchanged);
    }
    let etag = response
        .header("ETag")
        .filter(|s| s.len() <= 1024)
        .map(str::to_owned);
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Release response is too large.".into());
    }
    Ok(Response::Body { bytes, etag })
}

fn permitted(release_build: bool, env: Option<&str>) -> bool {
    release_build && env != Some("0")
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub struct Updater {
    install: Install,
    job: Job,
    closing: bool,
    scoop_confirm: bool,
    scoop_failure: bool,
    automatic: bool,
    skipped: String,
    remind_at: u64,
    release: Option<Release>,
    etag: Option<String>,
    next: u64,
    rx: Option<Receiver<Result<Checked, String>>>,
    manual: bool,
    pub open: bool,
    settings: bool,
    status: String,
    allowed: bool,
}

impl Updater {
    pub fn new(settings: Option<&Value>) -> Self {
        let get = |key| settings.and_then(|v| v.get(key));
        let install = Install::current();
        let failure = package::scoop_startup(&install);
        let mut updater = Self {
            install,
            job: Job::new(),
            closing: false,
            scoop_confirm: false,
            scoop_failure: false,
            automatic: get("automatic").and_then(Value::as_bool).unwrap_or(true),
            skipped: get("skipped").and_then(Value::as_str).unwrap_or("").into(),
            remind_at: get("remind_at").and_then(Value::as_u64).unwrap_or(0),
            release: None,
            etag: None,
            next: 0,
            rx: None,
            manual: false,
            open: false,
            settings: false,
            status: String::new(),
            allowed: permitted(
                option_env!("DUTCHMAN_RELEASE_BUILD") == Some("1") && !cfg!(debug_assertions),
                std::env::var("DUTCHMAN_UPDATE_CHECK").ok().as_deref(),
            ),
        };
        if let Some(failure) = failure {
            updater.release = Some(Release {
                download: format!("{RELEASES}/tag/{}", failure.tag),
                tag: failure.tag.clone(),
                notes: String::new(),
            });
            updater
                .job
                .preview(failure.tag, Phase::Failed(failure.error), &failure.output);
            updater.open = true;
            updater.scoop_failure = true;
        }
        updater
    }
    pub fn newer_known(&self) -> bool {
        self.release.is_some()
    }
    pub fn saved(&self) -> Value {
        serde_json::json!({"automatic": self.automatic, "skipped": self.skipped, "remind_at": self.remind_at})
    }
    fn offered(&self, time: u64) -> bool {
        self.release
            .as_ref()
            .is_some_and(|r| r.tag != self.skipped && time >= self.remind_at)
    }
    fn start(&mut self, ctx: &egui::Context, manual: bool) {
        if !self.allowed || self.rx.is_some() || self.job.holds_release() {
            return;
        }
        let known = self
            .release
            .as_ref()
            .map(|r| r.tag.clone())
            .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
        let etag = self.etag.clone();
        let install = self.install.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.manual = manual;
        self.status = "Checking for updates…".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if install.source().is_some() {
                check_package(
                    &fetch,
                    &install,
                    &format!("v{}", env!("CARGO_PKG_VERSION")),
                    &known,
                    etag.as_deref(),
                )
            } else {
                check(&fetch, &known, etag.as_deref())
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn tick(&mut self, ctx: &egui::Context) {
        if self.job.busy() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.open = true;
            self.closing = true;
        }
        if self.job.snapshot().phase == Phase::Restarted {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.scoop_failure {
            if self.open {
                return;
            }
            self.scoop_failure = false;
        }
        if self.job.holds_release() {
            return;
        }
        if let Some(rx) = &self.rx {
            let result = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("The update check stopped. Try Check now again.".into()))
                }
            };
            if let Some(result) = result {
                self.rx = None;
                self.next = now().saturating_add(HOUR);
                match result {
                    Ok(checked) => {
                        self.etag = checked.etag;
                        if checked.clear_release {
                            self.release = None;
                        }
                        if let Some(release) = checked.release {
                            self.release = Some(release);
                        }
                        self.status = if self.release.is_some() {
                            "An update is available."
                        } else {
                            "Dutchman is up to date."
                        }
                        .into();
                        if self.manual && self.release.is_some() {
                            self.open = true;
                        }
                    }
                    Err(_) => {
                        self.status =
                            "Could not check for updates. Try Check now or retry in an hour.".into()
                    }
                }
            }
        }
        if self.allowed && self.automatic && now() >= self.next {
            self.start(ctx, false);
        }
        if self.allowed && self.automatic {
            ctx.request_repaint_after(Duration::from_secs(self.next.saturating_sub(now()).max(1)));
        }
    }
    pub fn header(&mut self, ui: &mut egui::Ui, narrow: bool) {
        if self.offered(now()) {
            let label = if narrow {
                "Update".into()
            } else {
                format!(
                    "Update to {}",
                    self.release
                        .as_ref()
                        .map(|r| r.tag.trim_start_matches('v'))
                        .unwrap_or("")
                )
            };
            if ui
                .add(egui::Button::new(label).corner_radius(egui::CornerRadius::same(16)))
                .clicked()
            {
                self.open = true;
            }
        }
        if ui.small_button("Settings").clicked() {
            self.settings = true;
        }
    }
    pub fn windows(&mut self, ctx: &egui::Context, can_restart: bool) {
        let mut settings = self.settings;
        egui::Window::new("Settings")
            .open(&mut settings)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.add_enabled(
                    self.allowed,
                    egui::Checkbox::new(&mut self.automatic, "Check for updates automatically"),
                );
                if ui
                    .add_enabled(
                        self.allowed && self.rx.is_none() && !self.job.holds_release(),
                        egui::Button::new("Check now"),
                    )
                    .clicked()
                {
                    self.start(ctx, true);
                }
                if !self.allowed {
                    ui.label(
                        "Update checks are off for source builds or when DUTCHMAN_UPDATE_CHECK=0.",
                    );
                }
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
            });
        self.settings = settings;
        let mut open = self.open;
        if let Some(release) = self.release.clone() {
            egui::Window::new("Dutchman update")
                // Under the Update pill, at the header's right.
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 72.0))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    // After a successful package update only the result and Restart apply.
                    let installed =
                        matches!(self.job.snapshot().phase, Phase::Installed | Phase::Restarted);
                    if installed {
                        ui.heading(format!("Dutchman {} is installed", self.job.tag.trim_start_matches('v')));
                        self.package_ui(ui, &release, can_restart);
                        return;
                    }
                    ui.heading(format!("Dutchman {} is available", release.tag.trim_start_matches('v')));
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            if release.notes.is_empty() {
                                ui.label("Release notes could not be read. Open the release page to read them.");
                                ui.hyperlink_to("Release page", format!("{RELEASES}/tag/{}", release.tag));
                            } else {
                                ui.label(notes_body(&release.notes, &release.tag));
                            }
                        });
                    self.package_ui(ui, &release, can_restart);
                    ui.horizontal(|ui| {
                        if self.install == Install::Direct
                            && ui
                                .add(egui::Button::new("Download").fill(ui.visuals().selection.bg_fill))
                                .clicked()
                        {
                            ctx.open_url(egui::OpenUrl::new_tab(&release.download));
                        }
                        if ui.add_enabled(!self.job.busy(), egui::Button::new("Skip this version")).clicked() {
                            self.skipped = release.tag.clone();
                            self.open = false;
                        }
                        if ui.add_enabled(!self.job.busy(), egui::Button::new("Remind me later")).on_hover_text("Hide updates for three days").clicked() {
                            self.remind_at = now().saturating_add(THREE_DAYS);
                            self.open = false;
                        }
                    });
                });
        }
        self.open = open && self.open;
    }
    fn package_ui(&mut self, ui: &mut egui::Ui, release: &Release, can_restart: bool) {
        let Some(command) = self.install.command() else {
            return;
        };
        let progress = self.job.snapshot();
        match &progress.phase {
            Phase::Idle => {
                if matches!(self.install, Install::Scoop(_)) {
                    if self.scoop_confirm {
                        ui.label("Dutchman closes, Scoop updates it, and Dutchman opens again.");
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(can_restart, egui::Button::new("Update and restart"))
                                .clicked()
                            {
                                self.skipped.clear();
                                self.remind_at = 0;
                                self.scoop_confirm = false;
                                self.job.handoff_scoop(
                                    self.install.clone(),
                                    release.tag.clone(),
                                    ui.ctx(),
                                );
                            }
                            if ui.button("Cancel").clicked() {
                                self.scoop_confirm = false;
                            }
                        });
                        if !can_restart {
                            ui.label("Finish the current conversion before updating.");
                        }
                    } else if ui.button("Update with Scoop").clicked() {
                        self.scoop_confirm = true;
                    }
                } else if ui.button(self.install.label()).clicked() {
                    self.skipped.clear();
                    self.remind_at = 0;
                    self.job
                        .start(self.install.clone(), release.tag.clone(), ui.ctx());
                }
            }
            Phase::Running | Phase::Restarting => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if progress.phase == Phase::Restarting {
                        "Restarting Dutchman…"
                    } else {
                        "Updating Dutchman…"
                    });
                });
                ui.label("You can close this update window; the update will keep running.");
                if self.closing {
                    ui.label("Finish updating before closing Dutchman.");
                }
            }
            Phase::Installed | Phase::Restarted => {
                ui.label("The update finished.");
                if !progress.tail.is_empty() {
                    egui::CollapsingHeader::new("Update output")
                        .id_salt("package-output-done")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.monospace(
                                progress.tail.iter().cloned().collect::<Vec<_>>().join("\n"),
                            );
                        });
                }
                if ui
                    .add_enabled(
                        can_restart,
                        egui::Button::new("Restart Dutchman").fill(ui.visuals().selection.bg_fill),
                    )
                    .clicked()
                {
                    self.job.restart(self.install.clone(), ui.ctx());
                }
                if !can_restart {
                    ui.label("Finish the current conversion before restarting.");
                }
                return;
            }
            Phase::Failed(error) => {
                ui.label(error);
            }
        }
        if !progress.tail.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("package-output")
                .max_height(190.0)
                .show(ui, |ui| {
                    ui.monospace(progress.tail.iter().cloned().collect::<Vec<_>>().join("\n"));
                });
        }
        // The command is an alternative before an update starts and the fallback after a
        // failure; while one runs it would only invite running it twice.
        if matches!(progress.phase, Phase::Running | Phase::Restarting) {
            return;
        }
        ui.separator();
        ui.label("Or run this command yourself:");
        if matches!(self.install, Install::Scoop(_)) {
            ui.label("Close Dutchman before running this command in PowerShell.");
        }
        ui.code(command);
        if ui.button("Copy command").clicked() {
            ui.ctx().copy_text(command.into());
        }
    }

    /// Supplies a Homebrew job without starting any processes, for GPU screenshots.
    pub fn preview_homebrew(&mut self, installed: bool) {
        self.install = Install::Cask("/opt/homebrew".into());
        self.open = true;
        let output = "Running /opt/homebrew/bin/brew update\nAlready up-to-date.\nRunning /opt/homebrew/bin/brew upgrade --cask dutchman-cad/tap/dutchman-convert\n==> Downloading Dutchman Convert\n==> Upgrading dutchman-convert\n==> Moving App 'Dutchman Convert.app' to '/Applications/Dutchman Convert.app'";
        self.job.preview(
            self.release
                .as_ref()
                .map(|r| r.tag.clone())
                .unwrap_or_default(),
            if installed {
                Phase::Installed
            } else {
                Phase::Running
            },
            output,
        );
    }
    /// Supplies the Scoop confirmation without running any package-manager processes.
    pub fn preview_scoop(&mut self) {
        self.install = Install::Scoop("C:/Users/Owner/scoop/shims".into());
        self.job = Job::new();
        self.scoop_confirm = true;
        self.open = true;
    }
    /// Supplies a release without networking for UI previews and GPU screenshots.
    pub fn preview(&mut self, release: Release) {
        self.allowed = false;
        self.install = Install::Direct;
        self.release = Some(release);
        self.skipped.clear();
        self.remind_at = 0;
    }
}

#[cfg(test)]
mod tests;

/// Release notes without a first line that only repeats the version the heading already names.
fn notes_body<'a>(notes: &'a str, tag: &str) -> &'a str {
    let version = tag.trim_start_matches('v');
    let trimmed = notes.trim_start();
    match trimmed.split_once('\n') {
        Some((first, rest))
            if first
                .trim()
                .trim_start_matches("Dutchman")
                .trim()
                .trim_start_matches('v')
                == version =>
        {
            rest.trim_start()
        }
        None if trimmed
            .trim()
            .trim_start_matches("Dutchman")
            .trim()
            .trim_start_matches('v')
            == version =>
        {
            ""
        }
        _ => trimmed,
    }
}
