//! Optional snapshots. Disk reads and the CLI stay off the UI thread.
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(20);
const MAX_PNG: u64 = 8 * 1024 * 1024;
struct Request {
    cli: PathBuf,
    design: PathBuf,
    step: Option<PathBuf>,
    input: PathBuf,
    generation: u64,
}
struct Ready {
    design: PathBuf,
    generation: u64,
    converted: Option<egui::ColorImage>,
    original: Option<egui::ColorImage>,
}
#[derive(Default)]
pub(crate) struct Images {
    pub converted: Option<egui::TextureHandle>,
    pub original: Option<egui::TextureHandle>,
}

pub(crate) struct Previews {
    requests: Option<mpsc::Sender<Request>>,
    ready: mpsc::Receiver<Ready>,
    generation: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
    requested: BTreeSet<PathBuf>,
    images: BTreeMap<PathBuf, Images>,
}
impl Default for Previews {
    fn default() -> Self {
        let (_, ready) = mpsc::channel();
        Self {
            requests: None,
            ready,
            generation: Arc::new(AtomicU64::new(0)),
            thread: None,
            requested: Default::default(),
            images: Default::default(),
        }
    }
}
impl Previews {
    pub fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.requested.clear();
        self.images.clear();
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(ready) = self.ready.try_recv() {
            if ready.generation != self.generation.load(Ordering::Relaxed) {
                continue;
            }
            let texture =
                |image| ctx.load_texture("Result preview", image, egui::TextureOptions::LINEAR);
            self.images.insert(
                ready.design,
                Images {
                    converted: ready.converted.map(texture),
                    original: ready.original.map(texture),
                },
            );
        }
    }
    pub fn request(
        &mut self,
        ctx: &egui::Context,
        cli: &Path,
        design: &Path,
        input: &Path,
        step: Option<&Path>,
    ) {
        if !design.extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("sc") || ext.eq_ignore_ascii_case("solidcraft")
        }) || !self.requested.insert(design.to_path_buf())
        {
            return;
        }
        if self.requests.is_none() {
            let (send, receive) = mpsc::channel::<Request>();
            let (done, ready) = mpsc::channel();
            self.ready = ready;
            let generation = self.generation.clone();
            let ctx = ctx.clone();
            self.thread = Some(std::thread::spawn(move || {
                while let Ok(request) = receive.recv() {
                    let alive = || generation.load(Ordering::Relaxed) == request.generation;
                    if !alive() {
                        continue;
                    }
                    let converted = snapshot(
                        &request.cli,
                        &request.design,
                        &request.design,
                        false,
                        &alive,
                        LIMIT,
                    );
                    let original = if alive() {
                        request
                            .step
                            .clone()
                            .or_else(|| dutchman::diagnose::step::matching(&request.input))
                            .as_ref()
                            .and_then(|step| {
                                snapshot(&request.cli, step, &request.design, true, &alive, LIMIT)
                            })
                    } else {
                        None
                    };
                    if alive() {
                        let _ = done.send(Ready {
                            design: request.design,
                            generation: request.generation,
                            converted,
                            original,
                        });
                        ctx.request_repaint();
                    }
                }
            }));
            self.requests = Some(send);
        }
        let request = Request {
            cli: cli.to_path_buf(),
            design: design.to_path_buf(),
            step: step.map(Path::to_path_buf),
            input: input.to_path_buf(),
            generation: self.generation.load(Ordering::Relaxed),
        };
        if let Some(sender) = &self.requests {
            let _ = sender.send(request);
        }
    }
    pub fn get(&self, design: &Path) -> Option<&Images> {
        self.images.get(design)
    }
}
impl Drop for Previews {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.requests.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn cache_path(design: &Path, original: bool) -> PathBuf {
    design.with_extension(if original {
        "step-preview.png"
    } else {
        "preview.png"
    })
}
fn cached(cache: &Path, source: &Path, design: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).ok()?.modified().ok();
    modified(cache)
        .zip(modified(source))
        .zip(modified(design))
        .is_some_and(|((cache, source), design)| cache >= source && cache >= design)
}
fn read_image(path: &Path) -> Option<egui::ColorImage> {
    if std::fs::metadata(path).ok()?.len() > MAX_PNG {
        return None;
    }
    let mut reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1920);
    limits.max_image_height = Some(1280);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.to_rgba8();
    let width = usize::try_from(image.width()).ok()?;
    let height = usize::try_from(image.height()).ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        image.as_raw(),
    ))
}
fn snapshot(
    cli: &Path,
    source: &Path,
    design: &Path,
    original: bool,
    alive: &impl Fn() -> bool,
    timeout: Duration,
) -> Option<egui::ColorImage> {
    let cache = cache_path(design, original);
    if cached(&cache, source, design) {
        if let Some(image) = read_image(&cache) {
            return Some(image);
        }
    }
    if !alive() || !source.is_file() {
        return None;
    }
    // A temporary neighbour keeps failed or cancelled snapshots out of the cache.
    let temp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile_in(design.parent()?)
        .ok()?;
    let mut command = snapshot_command(cli);
    command
        .args(["snapshot", "--in"])
        .arg(source)
        .arg("--out")
        .arg(temp.path())
        .args([
            "--width",
            "480",
            "--height",
            "320",
            "--view",
            "iso",
            "--no-grid",
            "--no-axes",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let started = Instant::now();
    loop {
        if !alive() || started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    if !alive() {
        return None;
    }
    let image = read_image(temp.path())?;
    // On Windows an existing destination must be removed before the rename.
    if cache.exists() {
        let _ = std::fs::remove_file(&cache);
    }
    let _ = temp.persist(&cache);
    Some(image)
}
fn snapshot_command(cli: &Path) -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("nice");
        command.args(["-n", "10"]).arg(cli);
        command
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(cli);
        command.creation_flags(0x0800_0000 | 0x0000_4000);
        command
    }
    #[cfg(not(any(unix, windows)))]
    {
        Command::new(cli)
    }
}

/// Optional images take no space before they arrive. Both use the same fixed iso camera.
pub(crate) fn show(ui: &mut egui::Ui, images: Option<&Images>) -> bool {
    let Some(images) = images.filter(|images| images.converted.is_some()) else {
        return false;
    };
    let mut clicked = false;
    let paired = images.original.is_some();
    let width = ((ui.available_width() - if paired { 8.0 } else { 0.0 })
        / if paired { 2.0 } else { 1.0 })
    .min(480.0);
    let height = (width * 2.0 / 3.0).min(if ui.ctx().content_rect().height() >= 950.0 {
        240.0
    } else {
        180.0
    });
    ui.horizontal(|ui| {
        for (label, image) in [
            ("Original (STEP)", images.original.as_ref()),
            ("Converted", images.converted.as_ref()),
        ] {
            if let Some(image) = image {
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height + if paired { 22.0 } else { 0.0 }),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if paired {
                            ui.label(label);
                        }
                        let response = ui.add(
                            egui::Image::new(image)
                                .fit_to_exact_size(egui::vec2(height * 1.5, height))
                                .sense(egui::Sense::click()),
                        );
                        let response = response.on_hover_text("Open in SolidCraft");
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Image,
                                true,
                                format!("{label} preview"),
                            )
                        });
                        clicked |= response.clicked();
                    },
                );
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_requires_current_design_and_decodable_bounded_image() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("part.sc");
        std::fs::write(&design, "design").unwrap();
        let cache = cache_path(&design, false);
        image::RgbaImage::from_pixel(4, 3, image::Rgba([40, 90, 120, 255]))
            .save(&cache)
            .unwrap();
        assert!(cached(&cache, &design, &design));
        let image = snapshot(
            Path::new("missing-cli"),
            &design,
            &design,
            false,
            &|| true,
            LIMIT,
        )
        .unwrap();
        assert_eq!(image.size, [4, 3]);
        let future = std::time::SystemTime::now() + Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&design)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(future))
            .unwrap();
        assert!(!cached(&cache, &design, &design));
        assert!(snapshot(
            Path::new("missing-cli"),
            &design,
            &design,
            false,
            &|| true,
            LIMIT
        )
        .is_none());
        std::fs::write(&cache, "not an image").unwrap();
        assert!(snapshot(
            Path::new("missing-cli"),
            &design,
            &design,
            false,
            &|| true,
            LIMIT
        )
        .is_none());
    }
    #[cfg(unix)]
    #[test]
    fn cancellation_and_timeout_kill_the_snapshot_and_leave_no_cache() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("part.sc");
        std::fs::write(&design, "design").unwrap();
        let cli = dir.path().join("cli");
        std::fs::write(&cli, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        assert!(snapshot(
            &cli,
            &design,
            &design,
            false,
            &|| true,
            Duration::from_millis(80)
        )
        .is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!cache_path(&design, false).exists());
        assert!(snapshot(&cli, &design, &design, false, &|| false, LIMIT).is_none());
        let mut previews = Previews::default();
        previews.request(&egui::Context::default(), &cli, &design, &design, None);
        std::thread::sleep(Duration::from_millis(100));
        let cancelled = Instant::now();
        previews.cancel();
        drop(previews);
        assert!(cancelled.elapsed() < Duration::from_secs(2));
        assert!(!cache_path(&design, false).exists());
    }
}
