//! Bounded, conditional release checks. Network work stays off the UI thread.
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use serde_json::Value;

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
    let mut newest: Option<Release> = None;
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
        if let Some(v) = version(tag) {
            if newest
                .as_ref()
                .and_then(|r| version(&r.tag))
                .is_none_or(|old| v > old)
            {
                newest = Some(Release {
                    tag: tag.into(),
                    notes: plain_text(field("content")),
                    download: format!("{RELEASES}/tag/{tag}"),
                });
            }
        }
    }
    Ok(newest)
}

enum Response {
    Unchanged,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

struct Checked {
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
                release: None,
                etag: etag.map(str::to_owned),
            })
        }
        Ok(Response::Body { bytes, etag }) => {
            if let Ok(top) = parse_feed(&bytes) {
                feed_etag = etag;
                if top.as_ref().and_then(|r| version(&r.tag)) <= version(known) {
                    return Ok(Checked {
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
        release: (version(tag) > version(known)).then(|| Release {
            tag: tag.into(),
            notes,
            download,
        }),
        etag: feed_etag,
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

fn install_command(path: &Path) -> Option<&'static str> {
    let path = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if path.contains("/caskroom/") {
        Some("brew upgrade --cask dutchman")
    } else if path.contains("/cellar/dutchman/") || path.contains("/homebrew/opt/dutchman/") {
        Some("brew upgrade dutchman-cad/tap/dutchman")
    } else if path.contains("/scoop/apps/dutchman/") {
        Some("scoop update dutchman")
    } else {
        None
    }
}

pub struct Updater {
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
        Self {
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
        }
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
        if !self.allowed || self.rx.is_some() {
            return;
        }
        let known = self
            .release
            .as_ref()
            .map(|r| r.tag.clone())
            .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
        let etag = self.etag.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.manual = manual;
        self.status = "Checking for updates…".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check(&fetch, &known, etag.as_deref()));
            ctx.request_repaint();
        });
    }
    pub fn tick(&mut self, ctx: &egui::Context) {
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
    pub fn windows(&mut self, ctx: &egui::Context) {
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
                        self.allowed && self.rx.is_none(),
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
                    ui.heading(format!(
                        "Dutchman {} is available",
                        release.tag.trim_start_matches('v')
                    ));
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
                    let exe = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.canonicalize().ok().or(Some(p)));
                    if let Some(command) = exe.as_deref().and_then(install_command) {
                        ui.label("Run this command to update:");
                        ui.code(command);
                        if ui.button("Copy command").clicked() {
                            ctx.copy_text(command.into());
                        }
                    }
                    ui.horizontal(|ui| {
                        if install_command_for_current_exe().is_none()
                            && ui
                                .add(egui::Button::new("Download").fill(ui.visuals().selection.bg_fill))
                                .clicked()
                        {
                            ctx.open_url(egui::OpenUrl::new_tab(&release.download));
                        }
                        if ui.button("Skip this version").clicked() {
                            self.skipped = release.tag.clone();
                            self.open = false;
                        }
                        if ui.button("Remind me later").on_hover_text("Hide updates for three days").clicked() {
                            self.remind_at = now().saturating_add(THREE_DAYS);
                            self.open = false;
                        }
                    });
                });
        }
        self.open = open && self.open;
    }
    /// Supplies a release without networking for UI previews and GPU screenshots.
    pub fn preview(&mut self, release: Release) {
        self.allowed = false;
        self.release = Some(release);
        self.skipped.clear();
        self.remind_at = 0;
    }
}

#[cfg(test)]
mod tests;

/// The update command for how this copy was installed, when it was installed by a package manager.
fn install_command_for_current_exe() -> Option<&'static str> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok().or(Some(p)))?;
    install_command(&exe)
}

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
