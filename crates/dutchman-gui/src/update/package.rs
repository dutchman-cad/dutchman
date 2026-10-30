//! Package identity, version metadata, and bounded background package-manager jobs.
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use super::{version, MAX_BYTES};

mod scoop;

pub(super) use scoop::startup as scoop_startup;

const TIMEOUT: Duration = Duration::from_secs(15 * 60);
const TAIL_LINES: usize = 12;
const LINE_BYTES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    Direct,
    Cask(PathBuf),
    Formula(PathBuf),
    Scoop(PathBuf),
}

impl Install {
    pub fn detect(exe: &Path) -> Self {
        let path = exe.to_string_lossy().replace('\\', "/");
        let lower = path.to_ascii_lowercase();
        for (marker, cask) in [
            ("/caskroom/dutchman-convert/", true),
            ("/cellar/dutchman/", false),
            ("/opt/dutchman/", false),
        ] {
            if let Some(index) = lower.find(marker) {
                let prefix = PathBuf::from(path.get(..index).unwrap_or("/"));
                return if cask {
                    Self::Cask(prefix)
                } else {
                    Self::Formula(prefix)
                };
            }
        }
        if let Some(index) = lower.find("/scoop/apps/dutchman/") {
            let root = path.get(..index).unwrap_or("");
            return Self::Scoop(PathBuf::from(root).join("scoop/shims"));
        }
        Self::Direct
    }

    pub fn current() -> Self {
        let Some(exe) = std::env::current_exe()
            .ok()
            .map(|p| p.canonicalize().unwrap_or(p))
        else {
            return Self::Direct;
        };
        let install = Self::detect(&exe);
        // Homebrew moves a cask's bundle into /Applications, unlike a formula's symlink.
        if install == Self::Direct && exe.starts_with("/Applications/Dutchman Convert.app") {
            for prefix in ["/opt/homebrew", "/usr/local"] {
                if Path::new(prefix).join("Caskroom/dutchman-convert").is_dir() {
                    return Self::Cask(prefix.into());
                }
            }
        }
        install
    }

    pub fn source(&self) -> Option<&'static str> {
        match self {
            Self::Cask(_) => Some("https://raw.githubusercontent.com/dutchman-cad/homebrew-tap/main/Casks/dutchman-convert.rb"),
            Self::Formula(_) => Some("https://raw.githubusercontent.com/dutchman-cad/homebrew-tap/main/Formula/dutchman.rb"),
            Self::Scoop(_) => Some("https://raw.githubusercontent.com/dutchman-cad/scoop-bucket/main/bucket/dutchman.json"),
            Self::Direct => None,
        }
    }

    pub fn command(&self) -> Option<&'static str> {
        match self {
            Self::Cask(_) => {
                Some("brew update && brew upgrade --cask dutchman-cad/tap/dutchman-convert")
            }
            Self::Formula(_) => Some("brew update && brew upgrade dutchman-cad/tap/dutchman"),
            Self::Scoop(_) => Some("scoop update; if ($?) { scoop update dutchman }"),
            Self::Direct => None,
        }
    }

    pub fn label(&self) -> &'static str {
        if matches!(self, Self::Scoop(_)) {
            "Update with Scoop"
        } else {
            "Update with Homebrew"
        }
    }

    fn restart(&self) -> Result<Spec, String> {
        match self {
            Self::Cask(_) => Ok(Spec::new(
                "/usr/bin/open",
                &["-n", "/Applications/Dutchman Convert.app"],
            )),
            Self::Formula(prefix) => Ok(Spec::new(prefix.join("bin/dutchman-gui"), &[])),
            Self::Scoop(shims) => Ok(Spec::new(shims.join("dutchman-gui.exe"), &[])),
            Self::Direct => {
                Err("This copy uses a direct download. Download and open the new version.".into())
            }
        }
    }
}

pub fn parse_version(install: &Install, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Package metadata is too large.".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Package metadata could not be read.")?;
    let candidate = if matches!(install, Install::Scoop(_)) {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|_| "Package metadata could not be read.")?;
        value
            .get("version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned()
    } else {
        let quoted = |field: &str| -> Option<&str> {
            text.lines().find_map(|line| {
                let rest = line.trim().strip_prefix(field)?;
                if !rest.starts_with(char::is_whitespace) {
                    return None;
                }
                let rest = rest.trim_start();
                let quote = rest.chars().next()?;
                if quote != '"' && quote != '\'' {
                    return None;
                }
                rest.strip_prefix(quote)?
                    .split_once(quote)
                    .map(|(value, _)| value)
            })
        };
        if let Some(explicit) = quoted("version") {
            explicit.to_owned()
        } else if matches!(install, Install::Formula(_)) {
            quoted("url")
                .and_then(|url| {
                    url.split('/').find_map(|segment| {
                        let tag = segment
                            .strip_suffix(".tar.gz")
                            .or_else(|| segment.strip_suffix(".zip"))
                            .unwrap_or(segment);
                        version(tag).map(|_| tag.trim_start_matches('v'))
                    })
                })
                .unwrap_or("")
                .to_owned()
        } else {
            String::new()
        }
    };
    let tag = format!("v{candidate}");
    version(&tag)
        .map(|_| tag)
        .ok_or_else(|| "No stable package version was found. Try Check now later.".into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Spec {
    program: PathBuf,
    args: Vec<OsString>,
    no_auto_update: bool,
    detached: bool,
}
impl Spec {
    fn new(program: impl Into<PathBuf>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
            no_auto_update: false,
            detached: false,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).stdin(Stdio::null());
        if self.detached {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            command.creation_flags(
                CREATE_NO_WINDOW | if self.detached { DETACHED_PROCESS } else { 0 },
            );
        }
        if self.no_auto_update {
            command.env("HOMEBREW_NO_AUTO_UPDATE", "1");
        }
        command
    }
}

fn find_program(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    extra
        .iter()
        .cloned()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .take(256)
                .map(|dir| dir.join(name)),
        )
        .find(|path| path.is_file())
}

fn find_scoop(shims: &Path) -> Result<PathBuf, String> {
    let user_shims = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|p| p.join("scoop/shims"))
        .unwrap_or_else(|| shims.to_path_buf());
    find_program(
        "scoop.exe",
        &[user_shims.join("scoop.exe"), shims.join("scoop.exe")],
    )
    .or_else(|| {
        find_program(
            "scoop.ps1",
            &[user_shims.join("scoop.ps1"), shims.join("scoop.ps1")],
        )
    })
    .ok_or_else(|| "Scoop was not found. Copy the command and run it in PowerShell.".into())
}

fn plan(install: &Install) -> Result<Vec<Spec>, String> {
    match install {
        Install::Cask(_) | Install::Formula(_) => {
            // macOS allows Homebrew to replace the running app; Scoop on Windows needs a hand-off.
            let brew = find_program(
                "brew",
                &[
                    "/opt/homebrew/bin/brew".into(),
                    "/usr/local/bin/brew".into(),
                ],
            )
            .ok_or("Homebrew was not found. Copy the command and run it in a terminal.")?;
            let mut upgrade = Spec::new(
                &brew,
                if matches!(install, Install::Cask(_)) {
                    &["upgrade", "--cask", "dutchman-cad/tap/dutchman-convert"]
                } else {
                    &["upgrade", "dutchman-cad/tap/dutchman"]
                },
            );
            upgrade.no_auto_update = true;
            Ok(vec![Spec::new(brew, &["update"]), upgrade])
        }
        Install::Scoop(_) => {
            Err("Scoop needs Dutchman to close before updating. Choose Update and restart.".into())
        }
        Install::Direct => Err("Use Download to update this copy.".into()),
    }
}

type Output = Arc<dyn Fn(String) + Send + Sync>;

fn pipe_output(
    mut reader: impl Read + Send + 'static,
    output: Output,
) -> Result<std::thread::JoinHandle<Result<(), String>>, String> {
    std::thread::Builder::new()
        .name("package-output".into())
        .spawn(move || {
            let mut buffer = [0; 1024];
            let mut line = Vec::new();
            loop {
                let count = reader
                    .read(&mut buffer)
                    .map_err(|error| format!("Could not read package-manager output: {error}"))?;
                if count == 0 {
                    break;
                }
                for byte in buffer.iter().take(count) {
                    if (*byte == b'\n' || *byte == b'\r' || line.len() >= LINE_BYTES)
                        && !line.is_empty()
                    {
                        output(String::from_utf8_lossy(&line).into_owned());
                        line.clear();
                    }
                    if *byte != b'\n' && *byte != b'\r' {
                        line.push(*byte);
                    }
                }
            }
            if !line.is_empty() {
                output(String::from_utf8_lossy(&line).into_owned());
            }
            Ok(())
        })
        .map_err(|e| format!("Could not capture package-manager output: {e}"))
}

fn run_command(spec: &Spec, deadline: Instant, output: Output) -> Result<(), String> {
    let mut child = spec
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", spec.program.display()))?;
    let readers = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or("Could not capture package-manager stdout.")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Could not capture package-manager stderr.")?;
        Ok::<_, String>(vec![
            pipe_output(stdout, output.clone())?,
            pipe_output(stderr, output)?,
        ])
    })();
    let readers = match readers {
        Ok(readers) => readers,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "The update timed out after 15 minutes. Copy the command to finish in a terminal."
                    .into(),
            );
        }
        match child.try_wait() {
            Ok(Some(status)) if readers.iter().all(|reader| reader.is_finished()) => {
                for reader in readers {
                    reader.join().map_err(|_| {
                        "The package output worker stopped. Copy the command to retry."
                    })??;
                }
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} exited with {status}. Copy the command to retry in a terminal.",
                        spec.program.display()
                    ))
                };
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not wait for the package manager: {error}"));
            }
        }
    }
}

fn run_plan(
    specs: &[Spec],
    mut runner: impl FnMut(&Spec, Instant, Output) -> Result<(), String>,
    output: Output,
) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    for spec in specs {
        output(format!(
            "Running {} {}",
            spec.program.display(),
            spec.args
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        ));
        runner(spec, deadline, output.clone())?;
    }
    Ok(())
}

fn execute(
    progress: &Mutex<Progress>,
    specs: Result<Vec<Spec>, String>,
    runner: impl FnMut(&Spec, Instant, Output) -> Result<(), String>,
    output: Output,
) {
    let result = specs.and_then(|specs| run_plan(&specs, runner, output));
    if let Ok(mut p) = progress.lock() {
        p.finish(result);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    Installed,
    Failed(String),
    Restarting,
    Restarted,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub tail: VecDeque<String>,
}
impl Progress {
    fn line(&mut self, line: String) {
        for line in line.lines() {
            self.tail.push_back(
                line.chars()
                    .take(LINE_BYTES)
                    .filter(|c| !c.is_control() || *c == '\t')
                    .collect(),
            );
            while self.tail.len() > TAIL_LINES {
                self.tail.pop_front();
            }
        }
    }
    fn finish(&mut self, result: Result<(), String>) {
        self.phase = match result {
            Ok(()) => Phase::Installed,
            Err(error) => Phase::Failed(error),
        };
    }
}

pub struct Job {
    progress: Arc<Mutex<Progress>>,
    pub tag: String,
}
impl Default for Job {
    fn default() -> Self {
        Self::new()
    }
}
impl Job {
    pub fn new() -> Self {
        Self {
            progress: Arc::new(Mutex::new(Progress {
                phase: Phase::Idle,
                tail: VecDeque::new(),
            })),
            tag: String::new(),
        }
    }
    pub fn snapshot(&self) -> Progress {
        self.progress
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|_| Progress {
                phase: Phase::Failed(
                    "The update worker stopped. Copy the command to retry.".into(),
                ),
                tail: VecDeque::new(),
            })
    }
    pub fn busy(&self) -> bool {
        matches!(self.snapshot().phase, Phase::Running | Phase::Restarting)
    }
    pub fn holds_release(&self) -> bool {
        matches!(
            self.snapshot().phase,
            Phase::Running | Phase::Installed | Phase::Restarting | Phase::Restarted
        )
    }
    pub fn start(&mut self, install: Install, tag: String, ctx: &egui::Context) {
        if !matches!(self.snapshot().phase, Phase::Idle) {
            return;
        }
        self.tag = tag;
        self.set_phase(Phase::Running);
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        let failed = self.progress.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("package-update".into())
            .spawn(move || {
                let state = progress.clone();
                let repaint = ctx.clone();
                let output: Output = Arc::new(move |line| {
                    if let Ok(mut p) = state.lock() {
                        p.line(line);
                    }
                    repaint.request_repaint();
                });
                execute(&progress, plan(&install), run_command, output);
                ctx.request_repaint();
            })
        {
            if let Ok(mut p) = failed.lock() {
                p.finish(Err(format!("Could not start the update worker: {error}")));
            }
        }
    }
    pub fn handoff_scoop(&mut self, install: Install, tag: String, ctx: &egui::Context) {
        if self.snapshot().phase != Phase::Idle {
            return;
        }
        self.tag = tag.clone();
        self.set_phase(Phase::Restarting);
        let progress = self.progress.clone();
        let failed = self.progress.clone();
        let ctx = ctx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("scoop-handoff".into())
            .spawn(move || {
                let result = scoop::spawn(&install, &tag);
                if let Ok(mut p) = progress.lock() {
                    p.phase = match result {
                        Ok(()) => Phase::Restarted,
                        Err(error) => Phase::Failed(error),
                    };
                }
                ctx.request_repaint();
            })
        {
            if let Ok(mut p) = failed.lock() {
                p.phase = Phase::Failed(format!(
                    "Could not start the Scoop hand-off: {error}. Copy the command to retry."
                ));
            }
        }
    }
    fn set_phase(&self, phase: Phase) {
        if let Ok(mut p) = self.progress.lock() {
            p.phase = phase;
        }
    }
    pub fn restart(&self, install: Install, ctx: &egui::Context) {
        if self.snapshot().phase != Phase::Installed {
            return;
        }
        self.set_phase(Phase::Restarting);
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        let failed = self.progress.clone();
        if let Err(error) = std::thread::Builder::new().name("package-restart".into()).spawn(move || {
            let result = install.restart().and_then(|spec| {
                if matches!(install, Install::Cask(_)) { run_command(&spec, Instant::now() + TIMEOUT, Arc::new(|_| {})) }
                else { spec.command().stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()).map_err(|e| format!("Could not restart Dutchman: {e}. Open it from its installed location.")) }
            });
            if let Ok(mut p) = progress.lock() { p.phase = match result { Ok(()) => Phase::Restarted, Err(e) => Phase::Failed(e) }; }
            ctx.request_repaint();
        }) { if let Ok(mut p) = failed.lock() { p.phase = Phase::Failed(format!("Could not start the restart worker: {error}")); } }
    }
    pub fn preview(&mut self, tag: String, phase: Phase, output: &str) {
        self.tag = tag;
        if let Ok(mut p) = self.progress.lock() {
            p.phase = phase;
            p.tail.clear();
            p.line(output.into());
        }
    }
}

#[cfg(test)]
mod tests;
