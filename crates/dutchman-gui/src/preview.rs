//! Optional snapshots. Disk reads and the CLI stay off the UI thread.
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
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
// Read ownership and pixels from the same bounded buffer, so replacing the path
// after the ownership check can never make us display an unrelated image.
enum Cache {
    Missing,
    Owned { identity: String, bytes: Vec<u8> },
    Foreign,
}
const SOFTWARE: &str = concat!("Dutchman preview ", env!("CARGO_PKG_VERSION"));
const IDENTITY: &str = "Dutchman source";
fn source_identity(source: &Path, design: &Path) -> Option<String> {
    let stamp = |path| {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(format!("{}:{}", meta.len(), modified.as_nanos()))
    };
    Some(format!(
        "design={};source={}",
        stamp(design)?,
        stamp(source)?
    ))
}
fn read_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_PNG {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PNG + 1).read_to_end(&mut bytes).ok()?;
    (u64::try_from(bytes.len()).ok()? <= MAX_PNG).then_some(bytes)
}
fn cache_state(path: &Path) -> Cache {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Cache::Missing,
        Ok(meta) if meta.is_file() => (),
        _ => return Cache::Foreign,
    }
    let Some(bytes) = read_bytes(path) else {
        return Cache::Foreign;
    };
    let mut decoder = png::Decoder::new(Cursor::new(&bytes));
    decoder.set_limits(png::Limits {
        bytes: 16 * 1024 * 1024,
    });
    let Ok(reader) = decoder.read_info() else {
        return Cache::Foreign;
    };
    let texts = &reader.info().uncompressed_latin1_text;
    if !texts
        .iter()
        .any(|text| text.keyword == "Software" && text.text.starts_with("Dutchman preview "))
    {
        return Cache::Foreign;
    }
    let Some(identity) = texts
        .iter()
        .find(|text| text.keyword == IDENTITY)
        .map(|text| text.text.clone())
    else {
        return Cache::Foreign;
    };
    drop(reader);
    Cache::Owned { identity, bytes }
}
fn read_image(path: &Path) -> Option<egui::ColorImage> {
    decode_image(&read_bytes(path)?)
}
fn decode_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
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
    let identity = source_identity(source, design)?;
    match cache_state(&cache) {
        Cache::Foreign => return None,
        Cache::Owned {
            identity: saved,
            bytes,
        } if saved == identity && alive() => {
            if let Some(image) = decode_image(&bytes) {
                return Some(image);
            }
        }
        _ => (),
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
    if source_identity(source, design)? != identity || !alive() {
        return None;
    }
    save_cache(&cache, &identity, &image, alive).then_some(image)
}

fn save_cache(
    cache: &Path,
    identity: &str,
    image: &egui::ColorImage,
    alive: &impl Fn() -> bool,
) -> bool {
    let Some(parent) = cache.parent() else {
        return false;
    };
    let Ok(mut temp) = tempfile::Builder::new().suffix(".png").tempfile_in(parent) else {
        return false;
    };
    let mut encode = || -> Option<()> {
        let mut encoder = png::Encoder::new(
            temp.as_file_mut(),
            u32::try_from(image.width()).ok()?,
            u32::try_from(image.height()).ok()?,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("Software".to_owned(), SOFTWARE.to_owned())
            .ok()?;
        encoder
            .add_text_chunk(IDENTITY.to_owned(), identity.to_owned())
            .ok()?;
        let mut writer = encoder.write_header().ok()?;
        let rgba: Vec<_> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect();
        writer.write_image_data(&rgba).ok()?;
        writer.finish().ok()?;
        Some(())
    };
    if encode().is_none() || !alive() {
        return false;
    }
    // Check immediately before publishing, and use no-clobber persistence as
    // well: a foreign file appearing after this check must survive too. We never
    // unlink an occupied path, even an owned stale cache; fresh pixels still
    // appear in this run, and deleting that owned PNG enables caching again.
    match cache_state(cache) {
        Cache::Foreign => false,
        Cache::Owned { .. } => true,
        Cache::Missing => match temp.persist_noclobber(cache) {
            Ok(_) => true,
            Err(_) => !matches!(cache_state(cache), Cache::Foreign),
        },
    }
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
    fn png(path: &Path) {
        image::RgbaImage::from_pixel(4, 3, image::Rgba([40, 90, 120, 255]))
            .save(path)
            .unwrap();
    }
    #[test]
    fn cache_requires_ownership_current_source_and_bounded_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("part.sc");
        let step = dir.path().join("part.step");
        std::fs::write(&design, "design").unwrap();
        std::fs::write(&step, "step").unwrap();
        for original in [false, true] {
            let source = if original { &step } else { &design };
            let cache = cache_path(&design, original);
            let raw = dir.path().join("render.png");
            png(&raw);
            let image = read_image(&raw).unwrap();
            let identity = source_identity(source, &design).unwrap();
            assert!(save_cache(&cache, &identity, &image, &|| true));
            let Cache::Owned {
                identity: saved, ..
            } = cache_state(&cache)
            else {
                panic!("unmarked cache")
            };
            assert_eq!(saved, identity);
            let image = snapshot(
                Path::new("missing-cli"),
                source,
                &design,
                original,
                &|| true,
                LIMIT,
            )
            .unwrap();
            assert_eq!(image.size, [4, 3]);
            let before = std::fs::read(&cache).unwrap();
            // Freshness tracks the design for both variants, and STEP for the original.
            std::fs::write(source, "changed source with different size").unwrap();
            assert!(snapshot(
                Path::new("missing-cli"),
                source,
                &design,
                original,
                &|| true,
                LIMIT
            )
            .is_none());
            assert_eq!(std::fs::read(&cache).unwrap(), before);
            if original {
                assert!(save_cache(
                    &cache,
                    &source_identity(source, &design).unwrap(),
                    &image,
                    &|| true
                ));
                // Occupied paths are never removed or overwritten, including owned stale caches.
                assert_eq!(std::fs::read(&cache).unwrap(), before);
            }
        }
        let oversized = dir.path().join("large.png");
        image::RgbaImage::new(1921, 1).save(&oversized).unwrap();
        assert!(read_image(&oversized).is_none());
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_PNG + 1)
            .unwrap();
        assert!(read_image(&oversized).is_none());
    }
    #[test]
    fn unowned_pngs_are_never_displayed_changed_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("part.sc");
        std::fs::write(&design, "design").unwrap();
        for original in [false, true] {
            for newer in [false, true] {
                let cache = cache_path(&design, original);
                png(&cache);
                let modified = std::time::SystemTime::now();
                let modified = if newer {
                    modified + Duration::from_secs(60)
                } else {
                    modified - Duration::from_secs(60)
                };
                std::fs::File::options()
                    .write(true)
                    .open(&cache)
                    .unwrap()
                    .set_times(std::fs::FileTimes::new().set_modified(modified))
                    .unwrap();
                let modified = std::fs::metadata(&cache).unwrap().modified().unwrap();
                let before = std::fs::read(&cache).unwrap();
                assert!(matches!(cache_state(&cache), Cache::Foreign));
                // Fresh/repeated requests, render failure, timeout, and Stop all skip collisions.
                for alive in [true, true, false] {
                    assert!(snapshot(
                        Path::new("missing-cli"),
                        &design,
                        &design,
                        original,
                        &|| alive,
                        Duration::ZERO
                    )
                    .is_none());
                    assert_eq!(std::fs::read(&cache).unwrap(), before);
                    assert_eq!(
                        std::fs::metadata(&cache).unwrap().modified().unwrap(),
                        modified
                    );
                }
            }
        }
    }
    #[test]
    fn collision_during_encoding_is_not_published_or_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("render.png");
        png(&raw);
        let image = read_image(&raw).unwrap();
        let before = std::fs::read(&raw).unwrap();
        for original in [false, true] {
            let cache = cache_path(&dir.path().join("part.sc"), original);
            assert!(!save_cache(
                &cache,
                "design=1:1;source=1:1",
                &image,
                &|| {
                    std::fs::copy(&raw, &cache).unwrap();
                    true
                }
            ));
            assert_eq!(std::fs::read(&cache).unwrap(), before);
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
    #[cfg(unix)]
    fn fake_cli(dir: &Path, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let cli = dir.join("cli");
        std::fs::write(&cli, format!("#!/bin/sh\nroot=$(dirname \"$0\")\n{script}")).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        cli
    }
    #[cfg(unix)]
    #[test]
    fn fresh_render_rerender_failure_and_stop_preserve_foreign_pngs() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("part.sc");
        std::fs::write(&design, "design").unwrap();
        let raw = dir.path().join("source.png");
        png(&raw);
        let foreign = std::fs::read(&raw).unwrap();
        for original in [false, true] {
            let cache = cache_path(&design, original);
            let cli = fake_cli(dir.path(), "while [ \"$#\" -gt 0 ]; do\n if [ \"$1\" = --out ]; then shift; output=$1; fi\n shift\ndone\ncp \"$root/source.png\" \"$output\"\n");
            assert!(snapshot(&cli, &design, &design, original, &|| true, LIMIT).is_some());
            assert!(matches!(cache_state(&cache), Cache::Owned { .. }));
            std::fs::write(
                &design,
                if original {
                    "second changed design"
                } else {
                    "changed design"
                },
            )
            .unwrap();
            assert!(snapshot(&cli, &design, &design, original, &|| true, LIMIT).is_some());
            // A user replaces an owned cache with their own PNG before another request.
            std::fs::write(&cache, &foreign).unwrap();
            assert!(snapshot(&cli, &design, &design, original, &|| true, LIMIT).is_none());
            let cli = fake_cli(dir.path(), "exit 1\n");
            assert!(snapshot(&cli, &design, &design, original, &|| true, LIMIT).is_none());
            let cli = fake_cli(dir.path(), "exec sleep 30\n");
            assert!(snapshot(&cli, &design, &design, original, &|| false, LIMIT).is_none());
            assert_eq!(std::fs::read(&cache).unwrap(), foreign);
            // The user creates their PNG while a successful renderer is running.
            std::fs::remove_file(&cache).unwrap();
            let name = cache.file_name().unwrap().to_str().unwrap();
            let cli = fake_cli(dir.path(), &format!("while [ \"$#\" -gt 0 ]; do\n if [ \"$1\" = --out ]; then shift; output=$1; fi\n shift\ndone\ncp \"$root/source.png\" \"$root/{name}\"\ncp \"$root/source.png\" \"$output\"\n"));
            assert!(snapshot(&cli, &design, &design, original, &|| true, LIMIT).is_none());
            assert_eq!(std::fs::read(&cache).unwrap(), foreign);
            // Stop during an active render also leaves a newly created user PNG.
            std::fs::remove_file(&cache).unwrap();
            let started = dir.path().join("started");
            let _ = std::fs::remove_file(&started);
            let cli = fake_cli(dir.path(), "touch \"$root/started\"\nexec sleep 30\n");
            assert!(snapshot(
                &cli,
                &design,
                &design,
                original,
                &|| {
                    if started.exists() {
                        std::fs::write(&cache, &foreign).unwrap();
                        false
                    } else {
                        true
                    }
                },
                LIMIT
            )
            .is_none());
            assert_eq!(std::fs::read(&cache).unwrap(), foreign);
        }
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
