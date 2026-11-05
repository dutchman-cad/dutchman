//! Bounded, conditional release checks. Network work stays off the UI thread.
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use serde_json::Value;

mod notes;
mod package;
use package::{Action, Install, Job, Phase};

const FEED: &str = "https://github.com/dutchman-cad/dutchman/releases.atom";
const API: &str = "https://api.github.com/repos/dutchman-cad/dutchman/releases/latest";
const RELEASES: &str = "https://github.com/dutchman-cad/dutchman/releases";
const MAX_BYTES: usize = 1_048_576;
const HOUR: u64 = 3600;
const THREE_DAYS: u64 = 3 * 24 * HOUR;

#[derive(Clone, Debug)]
pub struct Release {
    pub fixes_reports: Vec<String>,
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

// Release notes carry this small machine-readable comment through GitHub's Atom
// feed too, so package installations get the same fix notices without report queries.
fn report_fixes(value: &Value) -> Vec<String> {
    value
        .get("fixes_reports")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(10000)
        .filter_map(Value::as_str)
        .filter(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .map(str::to_owned)
        .collect()
}

fn notes_fixes(text: &str) -> Vec<String> {
    text.split_once("<!-- dutchman-release ")
        .and_then(|(_, rest)| rest.split_once(" -->"))
        .and_then(|(json, _)| serde_json::from_str::<Value>(json).ok())
        .map(|v| report_fixes(&v))
        .unwrap_or_default()
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
                fixes_reports: notes_fixes(field("content")),
                tag: tag.into(),
                notes: notes::html_to_markdown(field("content")),
                download: format!("{RELEASES}/tag/{tag}"),
            });
        }
    }
    Ok(releases)
}

pub(crate) enum Response {
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

/// Asks for the newest stable release.
///
/// The releases Atom feed is read first, so ordinary checks never touch the rate-limited API:
/// it names every release by tag, carries the notes, and the download names follow from the
/// tag. The API is asked only when the feed cannot be fetched or read. A `fresh` check (the
/// reader pressed Update or Check now) sends no ETag and compares with this build, not with
/// what an earlier check found, so a newer release always replaces a cached one and a release
/// no longer offered is dropped.
fn check(
    fetch: &impl Fn(&str, Option<&str>) -> Result<Response, String>,
    known: &str,
    etag: Option<&str>,
    fresh: bool,
) -> Result<Checked, String> {
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));
    let (known, etag) = if fresh {
        (current.as_str(), None)
    } else {
        (known, etag)
    };
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
                let newer = top
                    .filter(|r| version(&r.tag) > version(known))
                    .map(|mut release| {
                        release.download = download_url(&release.tag);
                        release
                    });
                return Ok(Checked {
                    clear_release: fresh && newer.is_none(),
                    release: newer,
                    etag,
                });
            }
        }
        Err(_) => {}
    }
    check_api(fetch, known, fresh)
}

/// The file this installation downloads from a release, or the release page when its name is not known.
fn download_url(tag: &str) -> String {
    let appimage = std::env::current_exe()
        .ok()
        .is_some_and(|path| is_appimage(&path));
    match asset_name(tag, std::env::consts::OS, std::env::consts::ARCH, appimage) {
        Some(name) => format!("{RELEASES}/download/{tag}/{name}"),
        None => format!("{RELEASES}/tag/{tag}"),
    }
}

/// The release file name for a platform, as the release workflow names it.
fn asset_name(tag: &str, os: &str, arch: &str, appimage: bool) -> Option<String> {
    let version = tag.strip_prefix('v')?;
    match os {
        "macos" => Some(format!("dutchman-{version}-macos-universal.dmg")),
        "windows" => Some(format!("dutchman-{version}-{arch}-pc-windows-msvc.zip")),
        "linux" if !appimage => Some(format!(
            "dutchman-{version}-{arch}-unknown-linux-gnu.tar.gz"
        )),
        _ => None,
    }
}

/// The fallback when the feed is unavailable: GitHub's API, one request.
fn check_api(
    fetch: &impl Fn(&str, Option<&str>) -> Result<Response, String>,
    known: &str,
    fresh: bool,
) -> Result<Checked, String> {
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
    let body = value.get("body").and_then(Value::as_str).unwrap_or("");
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
        .unwrap_or_else(|| download_url(tag));
    let newer = version(tag) > version(known);
    Ok(Checked {
        clear_release: fresh && !newer,
        release: newer.then(|| Release {
            fixes_reports: {
                let mut fixes = report_fixes(&value);
                fixes.extend(notes_fixes(body));
                fixes.sort();
                fixes.dedup();
                fixes
            },
            tag: tag.into(),
            notes: body.chars().take(notes::MAX_NOTES).collect(),
            download,
        }),
        etag: None,
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
    let feed_release = match fetch(FEED, None) {
        Ok(Response::Body { bytes, .. }) => feed_entries(&bytes)
            .ok()
            .and_then(|entries| entries.into_iter().find(|entry| entry.tag == tag)),
        _ => None,
    };
    Ok(Checked {
        clear_release: false,
        release: Some(Release {
            fixes_reports: feed_release
                .as_ref()
                .map(|r| r.fixes_reports.clone())
                .unwrap_or_default(),
            download: format!("{RELEASES}/tag/{tag}"),
            tag,
            notes: feed_release.map(|r| r.notes).unwrap_or_default(),
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

pub(crate) fn fetch(url: &str, etag: Option<&str>) -> Result<Response, String> {
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
/// Whether this build may reach the network on its own: a release build, unless
/// `DUTCHMAN_UPDATE_CHECK=0`.
pub(crate) fn network_allowed() -> bool {
    permitted(
        option_env!("DUTCHMAN_RELEASE_BUILD") == Some("1") && !cfg!(debug_assertions),
        std::env::var("DUTCHMAN_UPDATE_CHECK").ok().as_deref(),
    )
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// What the update window shows after Update is pressed, until a release is offered.
#[derive(Clone, Debug, PartialEq)]
enum Click {
    None,
    Checking,
    UpToDate,
    Failed(String),
}

/// Why a check failed, in words for people.
fn failure_text(error: &str) -> String {
    let e = error.to_ascii_lowercase();
    if e.contains("429") || e.contains("403") || e.contains("rate limit") {
        "GitHub is limiting requests right now. Try again in a few minutes.".into()
    } else if [
        "dns",
        "connection",
        "connect",
        "timed out",
        "timeout",
        "network",
        "resolve",
        "offline",
    ]
    .iter()
    .any(|word| e.contains(word))
    {
        "Could not reach GitHub. Check your internet connection and try again.".into()
    } else {
        "Could not check for updates. Try again, or open the releases page.".into()
    }
}

type Fetcher = std::sync::Arc<dyn Fn(&str, Option<&str>) -> Result<Response, String> + Send + Sync>;

pub struct Updater {
    pub report_ids: Vec<String>,
    install: Install,
    job: Job,
    closing: bool,
    scoop_confirm: bool,
    msi_needs_confirm: bool,
    msi_finish_confirmed: bool,
    windows_failure: bool,
    automatic: bool,
    skipped: String,
    remind_at: u64,
    release: Option<Release>,
    etag: Option<String>,
    next: u64,
    rx: Option<Receiver<Result<Checked, String>>>,
    /// The check in flight was asked for by the reader, so it ignores earlier results.
    fresh: bool,
    /// The reader asked from the update window, which then shows the outcome.
    window_check: bool,
    click: Click,
    fetcher: Fetcher,
    pub open: bool,
    settings: bool,
    /// Bumped while the window is closed, so each opening starts its notes at the top.
    notes_epoch: u32,
    status: String,
    allowed: bool,
}

impl Updater {
    pub fn new(settings: Option<&Value>) -> Self {
        let get = |key| settings.and_then(|v| v.get(key));
        let install = Install::current();
        let failure = package::windows_startup(&install);
        let mut updater = Self {
            report_ids: Vec::new(),
            install,
            job: Job::new(),
            closing: false,
            scoop_confirm: false,
            msi_needs_confirm: false,
            msi_finish_confirmed: false,
            windows_failure: false,
            automatic: get("automatic").and_then(Value::as_bool).unwrap_or(true),
            skipped: get("skipped").and_then(Value::as_str).unwrap_or("").into(),
            remind_at: get("remind_at").and_then(Value::as_u64).unwrap_or(0),
            release: None,
            etag: None,
            next: 0,
            rx: None,
            fresh: false,
            window_check: false,
            click: Click::None,
            fetcher: std::sync::Arc::new(fetch),
            open: false,
            settings: false,
            notes_epoch: 0,
            status: String::new(),
            allowed: network_allowed(),
        };
        if let Some(failure) = failure {
            updater.release = Some(Release {
                fixes_reports: Vec::new(),
                download: format!("{RELEASES}/tag/{}", failure.tag),
                tag: failure.tag.clone(),
                notes: String::new(),
            });
            updater
                .job
                .preview(failure.tag, Phase::Failed(failure.error), &failure.output);
            updater.open = true;
            updater.windows_failure = true;
        }
        updater
    }
    /// Whether the reader leaves automatic checks on.
    pub fn automatic(&self) -> bool {
        self.automatic
    }
    /// Do not start a conversion while an MSI download is preparing to close the app.
    pub fn blocks_conversions(&self) -> bool {
        (self.install.action() == Action::Msi && self.job.holds_release())
            || (self.install.action() == Action::Zip
                && matches!(
                    self.job.snapshot().phase,
                    Phase::Restarting | Phase::Restarted
                ))
    }
    pub fn windows_package(&self) -> bool {
        matches!(self.install.action(), Action::Msi | Action::Zip)
    }
    pub fn stopping_for_install(&self) -> bool {
        self.install.action() == Action::Msi
            && self.msi_finish_confirmed
            && self.job.snapshot().phase == Phase::Ready
    }
    fn msi_can_handoff(&mut self, can_restart: bool) -> bool {
        self.msi_needs_confirm |= !can_restart;
        can_restart && (!self.msi_needs_confirm || self.msi_finish_confirmed)
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
    /// Starts a check. A `fresh` one asks GitHub now and ignores every earlier result.
    fn start(&mut self, ctx: &egui::Context, fresh: bool) {
        if !self.allowed
            || self.job.holds_release()
            || (self.rx.is_some() && (self.fresh || !fresh))
        {
            return;
        }
        let current = format!("v{}", env!("CARGO_PKG_VERSION"));
        let known = match (&self.release, fresh) {
            (Some(r), false) => r.tag.clone(),
            _ => current.clone(),
        };
        let etag = if fresh { None } else { self.etag.clone() };
        let install = self.install.clone();
        let fetcher = self.fetcher.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.fresh = fresh;
        self.status = "Checking for updates…".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let fetch = |url: &str, etag: Option<&str>| fetcher(url, etag);
            let result = if install.source().is_some() {
                check_package(&fetch, &install, &current, &known, etag.as_deref())
            } else {
                check(&fetch, &known, etag.as_deref(), fresh)
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    /// The Update button: always asks GitHub for the latest release at this moment.
    fn press(&mut self, ctx: &egui::Context) {
        self.open = true;
        self.window_check = true;
        self.click = Click::Checking;
        self.start(ctx, true);
        if !(self.rx.is_some() && self.fresh) {
            // Nothing could be asked (checks are off, or an update is already installing).
            self.click = Click::None;
        }
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
        if self.install.action() == Action::Msi && self.job.snapshot().phase == Phase::Ready {
            self.open = true;
        }
        if self.windows_failure {
            if self.open {
                return;
            }
            self.windows_failure = false;
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
                let fresh = std::mem::take(&mut self.fresh);
                let window = std::mem::take(&mut self.window_check);
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
                        if fresh && self.release.is_some() {
                            self.open = true;
                        }
                        if window {
                            self.click = if self.release.is_some() {
                                Click::None
                            } else {
                                Click::UpToDate
                            };
                        }
                    }
                    Err(error) => {
                        let text = failure_text(&error);
                        self.status = text.clone();
                        if window {
                            self.click = Click::Failed(text);
                        }
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
                self.press(ui.ctx());
            }
        }
        if ui.small_button("Settings").clicked() {
            self.settings = true;
        }
    }
    /// `top` is the bottom edge of the header, so the update window opens below its controls.
    pub fn windows(&mut self, ctx: &egui::Context, can_restart: bool, top: f32) {
        if !self.open {
            self.notes_epoch = self.notes_epoch.wrapping_add(1);
        }
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
        if self.click != Click::None && self.open {
            let click = self.click.clone();
            let mut retry = false;
            egui::Window::new("Dutchman update")
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, top + 8.0))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| match &click {
                    Click::Checking => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking for updates…");
                        });
                    }
                    Click::UpToDate => {
                        ui.heading("Dutchman is up to date");
                        ui.label(format!(
                            "You have the newest version, {}.",
                            env!("CARGO_PKG_VERSION")
                        ));
                    }
                    Click::Failed(text) => {
                        ui.heading("Could not check for updates");
                        ui.label(text);
                        ui.horizontal(|ui| {
                            retry = ui
                                .add(
                                    egui::Button::new("Retry").fill(ui.visuals().selection.bg_fill),
                                )
                                .clicked();
                            ui.hyperlink_to("Releases page", RELEASES);
                        });
                    }
                    Click::None => {}
                });
            if retry {
                self.press(ctx);
            }
        } else if let Some(release) = self.release.clone() {
            egui::Window::new("Dutchman update")
                // Under the header, at its right.
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, top + 8.0))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    // After a successful package update only the result and Restart apply.
                    let installed = self.job.snapshot().phase == Phase::Installed
                        || (self.job.snapshot().phase == Phase::Restarted && self.install.action() != Action::Msi);
                    if installed {
                        ui.heading(format!("Dutchman {} is installed", self.job.tag.trim_start_matches('v')));
                        self.package_ui(ui, &release, can_restart);
                        return;
                    }
                    ui.heading(format!("Dutchman {} is available", release.tag.trim_start_matches('v')));
                    if let Some(notice) = dutchman::report_send::fixed_notice(&self.report_ids, &release.fixes_reports) { ui.label(notice); }
                    egui::ScrollArea::vertical()
                        .id_salt(("update-notes", release.tag.clone(), self.notes_epoch))
                        .max_height(320.0)
                        .show(ui, |ui| {
                            if release.notes.is_empty() {
                                ui.label("Release notes could not be read. Open the release page to read them.");
                                ui.hyperlink_to("Release page", format!("{RELEASES}/tag/{}", release.tag));
                            } else {
                                notes::show(ui, notes_body(&release.notes, &release.tag));
                            }
                        });
                    self.package_ui(ui, &release, can_restart);
                    ui.horizontal(|ui| {
                        if self.install.action() == Action::Download
                            && ui
                                .add(egui::Button::new("Download").fill(ui.visuals().selection.bg_fill))
                                .clicked()
                        {
                            let url = if cfg!(windows) { format!("{RELEASES}/tag/{}", release.tag) } else { release.download.clone() };
                            ctx.open_url(egui::OpenUrl::new_tab(url));
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
        if !self.open {
            self.click = Click::None;
        }
    }
    fn package_ui(&mut self, ui: &mut egui::Ui, release: &Release, can_restart: bool) {
        if matches!(self.install.action(), Action::Msi | Action::Zip) {
            self.windows_ui(ui, release, can_restart);
            return;
        }
        let Some(command) = self.install.command() else {
            return;
        };
        let progress = self.job.snapshot();
        match &progress.phase {
            Phase::Idle => {
                if self.install.action() == Action::Scoop {
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
                                self.job.handoff_windows(
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
                    } else if ui.button(self.install.label()).clicked() {
                        self.scoop_confirm = true;
                    }
                } else if ui.button(self.install.label()).clicked() {
                    self.skipped.clear();
                    self.remind_at = 0;
                    self.job
                        .start(self.install.clone(), release.tag.clone(), ui.ctx());
                }
            }
            Phase::Running | Phase::Ready | Phase::Restarting => {
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

    fn windows_ui(&mut self, ui: &mut egui::Ui, release: &Release, can_restart: bool) {
        match self.job.snapshot().phase {
            Phase::Idle => {
                if self.install.action() == Action::Msi {
                    ui.label("Download and verify the installer, then close Dutchman, install, and open it again.");
                } else {
                    ui.label("Download and verify the ZIP, replace the programs, then restart when you are ready.");
                }
                if ui.button(self.install.label()).clicked() {
                    self.msi_needs_confirm = false;
                    self.msi_finish_confirmed = false;
                    self.skipped.clear();
                    self.remind_at = 0;
                    self.job
                        .prepare_windows(self.install.clone(), release.tag.clone(), ui.ctx());
                }
            }
            Phase::Running | Phase::Restarting => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Preparing the Windows update…");
                });
            }
            Phase::Ready => {
                if self.msi_can_handoff(can_restart) {
                    self.job
                        .handoff_windows(self.install.clone(), release.tag.clone(), ui.ctx());
                } else {
                    if self.msi_finish_confirmed {
                        ui.label("Finishing the current files before closing and installing. New files will not start. Close any diagnosis window when it finishes.");
                    } else {
                        ui.label("The installer is verified. Finish the current files, then close Dutchman and install?");
                        if ui
                            .button(if can_restart {
                                "Close and install"
                            } else {
                                "Finish files and install"
                            })
                            .clicked()
                        {
                            self.msi_finish_confirmed = true;
                        }
                    }
                    if ui.button("Cancel install").clicked() {
                        self.msi_needs_confirm = false;
                        self.msi_finish_confirmed = false;
                        self.job = Job::new();
                    }
                }
            }
            Phase::Installed => {
                ui.label("The new programs are ready. Restart Dutchman to use them.");
                if ui
                    .add_enabled(can_restart, egui::Button::new("Restart now"))
                    .clicked()
                {
                    self.job.restart(self.install.clone(), ui.ctx());
                }
                if !can_restart {
                    ui.label("Finish the current conversion before restarting.");
                }
            }
            Phase::Restarted => {
                ui.label("Restarting Dutchman…");
            }
            Phase::Failed(error) => {
                ui.label(error);
                if ui.button("Try again").clicked() {
                    self.job = Job::new();
                }
            }
        }
        ui.hyperlink_to("Download", format!("{RELEASES}/tag/{}", release.tag));
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
    /// Offers the newest release in a releases feed without networking, for GPU screenshots.
    pub fn preview_feed(&mut self, atom: &[u8]) {
        if let Ok(Some(release)) = parse_feed(atom) {
            self.preview(release);
            self.open = true;
        }
    }
    /// Shows the update window as it is while the latest release is being asked for.
    pub fn preview_checking(&mut self) {
        self.allowed = false;
        self.open = true;
        self.click = Click::Checking;
    }
    /// Shows the update window after a failed check, with its Retry button.
    pub fn preview_failed(&mut self, error: &str) {
        self.allowed = false;
        self.open = true;
        self.click = Click::Failed(failure_text(error));
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
