//! Checks written designs in SolidCraft's own command-line tool, since Dutchman has no kernel
//! of its own: a written history against the original geometry (evaluating both in the same
//! SolidCraft makes any kernel difference cancel out), and geometry written as an import
//! against what the model holds.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The relative difference in volume at which a replay no longer matches the original, when
/// SolidCraft gives no area and box to bound its meshes' error by (see [`mesh_slack`]).
pub const VOLUME_TOLERANCE: f64 = 1e-4;

/// SolidCraft measures a body from a mesh whose chords keep within this fraction of the body's
/// size of its faces, and never closer than [`MESH_CHORD_FLOOR`].
const MESH_CHORD: f64 = 5e-5;

/// The closest, in millimetres, SolidCraft's measuring mesh keeps to a body's faces.
const MESH_CHORD_FLOOR: f64 = 1e-4;

/// The relative difference in area at which a replay no longer matches the original. A mesh
/// misses a curved face's area by about a third of its chord over the face's radius, and
/// SolidCraft keeps that chord within 3e-4 of the smallest radius: well inside this.
const AREA_TOLERANCE: f64 = 1e-3;

/// How long one SolidCraft evaluation may run before the check gives up.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// The environment variable that names the SolidCraft command-line tool.
pub const CLI_ENV: &str = "SOLIDCRAFT_CLI";

/// The SolidCraft command-line tool's file name.
const CLI_NAME: &str = "solidcraft-cli";

/// How often a running evaluation is polled.
const POLL: Duration = Duration::from_millis(20);

/// How a written history is checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verification {
    /// With the SolidCraft CLI named by `SOLIDCRAFT_CLI`, else the one on `PATH`, else the
    /// one an installed SolidCraft app carries (see [`find_cli`]).
    Auto,
    /// With this SolidCraft CLI.
    With(PathBuf),
    /// Not at all.
    Off,
}

/// What the check found.
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// The replayed bodies match the original's.
    Matches,
    /// The replay does not match; why.
    Differs(String),
    /// The check could not be made; why.
    Unverified(String),
}

/// The SolidCraft CLI `verification` checks with: for [`Verification::Auto`], the one
/// `SOLIDCRAFT_CLI` names, else `solidcraft-cli` on `PATH`, else the one inside an installed
/// SolidCraft app: on macOS `SolidCraft*.app` in `/Applications` or `~/Applications`, newest
/// version first; on Windows `SolidCraft\solidcraft-cli.exe` in Program Files.
pub fn find_cli(verification: &Verification) -> Result<PathBuf, String> {
    match verification {
        Verification::Off => Err("verification turned off".to_owned()),
        Verification::With(path) => Ok(path.clone()),
        Verification::Auto => std::env::var_os(CLI_ENV)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(on_path)
            .or_else(installed)
            .ok_or_else(|| {
                format!(
                    "no {CLI_NAME} found (install SolidCraft, set {CLI_ENV}, or add it to PATH)"
                )
            }),
    }
}

/// The oldest SolidCraft Dutchman's output is checked with, and opens in.
pub const OLDEST_SOLIDCRAFT: [u64; 3] = [0, 18, 2];

/// Why no SolidCraft CLI checks a design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoCli {
    /// None is found, or checking is turned off.
    NotFound(String),
    /// The one found is older than [`OLDEST_SOLIDCRAFT`].
    TooOld(String),
    /// macOS has not allowed the CLI to run yet.
    Blocked(String),
}

impl std::fmt::Display for NoCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoCli::NotFound(why) | NoCli::TooOld(why) | NoCli::Blocked(why) => f.write_str(why),
        }
    }
}

/// [`find_cli`], refused with the reason when the CLI found says it is older than
/// [`OLDEST_SOLIDCRAFT`] or macOS blocks it. One that does not say its version is used as it is.
pub fn usable_cli(verification: &Verification) -> Result<PathBuf, NoCli> {
    let cli = find_cli(verification).map_err(NoCli::NotFound)?;
    let Some(line) = version_of(&cli).map_err(NoCli::Blocked)? else {
        return Ok(cli);
    };
    match parse_version(&line) {
        Some(version) if version < OLDEST_SOLIDCRAFT => {
            let [major, minor, patch] = OLDEST_SOLIDCRAFT;
            Err(NoCli::TooOld(format!(
                "{line} at {} is older than SolidCraft {major}.{minor}.{patch}, the oldest that \
                 opens and checks Dutchman's designs; install a newer SolidCraft",
                cli.display()
            )))
        }
        _ => Ok(cli),
    }
}

/// The version a `--version` line gives (`solidcraft-cli 0.18.2` as `[0, 18, 2]`).
fn parse_version(line: &str) -> Option<[u64; 3]> {
    let word = line.split_whitespace().last()?.trim_start_matches('v');
    let mut parts = word.split('.').map(|part| {
        part.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u64>()
            .ok()
    });
    Some([
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ])
}

/// The CLI inside an installed SolidCraft app, where this system installs apps.
fn installed() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let mut folders = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            folders.push(Path::new(&home).join("Applications"));
        }
        in_app_bundles(&folders)
    } else if cfg!(windows) {
        ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .flat_map(|base| {
                let base = PathBuf::from(base);
                [
                    base.join("SolidCraft"),
                    base.join("Programs").join("SolidCraft"),
                ]
            })
            .map(|folder| folder.join(format!("{CLI_NAME}.exe")))
            .find(|cli| cli.is_file())
    } else {
        None
    }
}

/// The CLI in the newest `SolidCraft*.app` bundle in `folders` (`Contents/MacOS/solidcraft-cli`),
/// by the version its `Info.plist` gives, then by name.
fn in_app_bundles(folders: &[PathBuf]) -> Option<PathBuf> {
    let mut found: Vec<(Vec<u64>, PathBuf)> = folders
        .iter()
        .filter_map(|folder| std::fs::read_dir(folder).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|app| {
            app.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .is_some_and(|name| name.starts_with("solidcraft") && name.ends_with(".app"))
        })
        .filter_map(|app| {
            let cli = app.join("Contents/MacOS").join(CLI_NAME);
            cli.is_file().then(|| (bundle_version(&app), cli))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, cli)| cli)
}

/// The version an app bundle's `Info.plist` gives (`CFBundleShortVersionString`), as numbers
/// to compare; empty when it gives none.
fn bundle_version(app: &Path) -> Vec<u64> {
    let Ok(plist) = std::fs::read_to_string(app.join("Contents/Info.plist")) else {
        return Vec::new();
    };
    let Some(after) = plist
        .split_once("<key>CFBundleShortVersionString</key>")
        .map(|(_, after)| after)
    else {
        return Vec::new();
    };
    after
        .split_once("<string>")
        .and_then(|(_, rest)| rest.split_once("</string>"))
        .map(|(version, _)| {
            version
                .trim()
                .split(['.', '-'])
                .map_while(|part| part.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The version the SolidCraft CLI `verification` finds prints (`solidcraft-cli 0.18.2`),
/// when there is one and it answers within a few seconds.
pub fn solidcraft_version(verification: &Verification) -> Option<String> {
    version_of(&find_cli(verification).ok()?).ok().flatten()
}

/// How many times to try running a CLI the system says is busy being written.
const BUSY_TRIES: u32 = 50;

/// The first line `cli --version` prints, when it answers within a few seconds.
fn version_of(cli: &Path) -> Result<Option<String>, String> {
    let mut child = match spawn(cli, |command| {
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    }) {
        Ok(child) => child,
        Err(_) => return blocked_failure(cli).map_or(Ok(None), Err),
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    if let Some(why) = blocked_failure(cli) {
                        return Err(why);
                    }
                }
                break;
            }
            Ok(None) if started.elapsed() > Duration::from_secs(10) => {
                let _ = child.kill();
                let _ = child.wait();
                return blocked_failure(cli).map_or(Ok(None), Err);
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(_) => return blocked_failure(cli).map_or(Ok(None), Err),
        }
    }
    let line = (|| {
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut std::io::Read::take(child.stdout.take()?, 4096),
            &mut text,
        )
        .ok()?;
        let line = text.lines().next()?.trim();
        (!line.is_empty()).then(|| line.to_owned())
    })();
    Ok(line)
}

#[cfg(any(target_os = "macos", test))]
const MACOS_BLOCKED: &str = "macOS has not yet allowed SolidCraft to run: open SolidCraft once \
    from Applications, click Done when macOS stops it, then click Open Anyway in System \
    Settings > Privacy & Security, and try again.";

#[cfg(any(target_os = "macos", test))]
fn enclosing_app(cli: &Path) -> Option<&Path> {
    cli.parent()?.ancestors().find(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    })
}

#[cfg(any(target_os = "macos", test))]
fn quarantine_blocks(value: &str) -> bool {
    let Some((flags, _)) = value.trim().split_once(';') else {
        return false;
    };
    flags.len() == 4
        && flags.bytes().all(|byte| byte.is_ascii_hexdigit())
        && u16::from_str_radix(flags, 16).is_ok_and(|flags| flags & 0x0040 == 0)
}

fn blocked_failure(cli: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let resolved = std::fs::canonicalize(cli).ok();
        let app = enclosing_app(resolved.as_deref().unwrap_or(cli))?;
        let output = Command::new("/usr/bin/xattr")
            .arg("-p")
            .arg("com.apple.quarantine")
            .arg(app)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        (output.status.success()
            && std::str::from_utf8(&output.stdout).is_ok_and(quarantine_blocks))
        .then(|| MACOS_BLOCKED.to_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cli;
        None
    }
}

fn launch_failure(cli: &Path, err: std::io::Error) -> String {
    blocked_failure(cli).unwrap_or_else(|| format!("cannot run {}: {err}", cli.display()))
}

fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{CLI_NAME}{}", std::env::consts::EXE_SUFFIX)))
        .find(|candidate| candidate.is_file())
}

/// Compares `history` (the design as written) with `original` (the same design with the
/// history suppressed and the original geometry live).
/// Replays `history` and compares it with `original` (the same design with its original
/// geometry live), whose model has `solids` solid bodies. When SolidCraft opens the original
/// unsoundly (solids without a volume, bodies inside out), there is nothing sound to compare
/// with, and a replay that runs is kept unverified, saying why.
pub(crate) fn compare(
    cli: &Path,
    timeout: Duration,
    history: &[u8],
    original: &[u8],
    bodies: Expected,
) -> Outcome {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(err) => return Outcome::Unverified(format!("no scratch folder: {err}")),
    };
    let expected = match evaluate(cli, timeout, dir.path(), "original", original) {
        Ok(Evaluation::Done(eval)) => eval,
        Ok(Evaluation::Failed(why)) | Err(why) => {
            return Outcome::Unverified(format!("cannot evaluate the original geometry: {why}"))
        }
    };
    let replayed = match evaluate(cli, timeout, dir.path(), "history", history) {
        Ok(Evaluation::Done(eval)) => eval,
        Ok(Evaluation::Failed(why)) => {
            return Outcome::Differs(format!("SolidCraft cannot replay it: {why}"))
        }
        Err(why) => return Outcome::Unverified(why),
    };
    if let Some(error) = replayed.errors.first() {
        return Outcome::Differs(error.clone());
    }
    let unsound = volume_problems(&expected.volumes(), bodies);
    if !unsound.is_empty() {
        return Outcome::Unverified(format!(
            "the original geometry cannot be compared with: {}",
            unsound.join("; ")
        ));
    }
    match_bodies(&replayed.bodies, &expected.bodies)
}

/// What is wrong with a design's body `volumes` in SolidCraft, for a design with
/// `bodies.solids` solid bodies (0: not counted): bodies inside out, and solids without a volume.
///
/// A sheet body bounds no volume, but SolidCraft gives it one, of either sign, and does not
/// say which of its bodies are sheets. So as many bodies inside out as the design has sheets
/// are taken for them, and only the rest are reported.
fn volume_problems(volumes: &[f64], bodies: Expected) -> Vec<String> {
    let solids = bodies.solids;
    let mut problems = Vec::new();
    let inside_out = volumes
        .iter()
        .filter(|v| **v < -NO_VOLUME)
        .count()
        .saturating_sub(bodies.sheets);
    if inside_out > 0 {
        problems.push(format!(
            "SolidCraft reads {inside_out} bod{} inside out, with a negative volume",
            if inside_out == 1 { "y" } else { "ies" }
        ));
    }
    let with_volume = volumes.iter().filter(|v| **v > NO_VOLUME).count();
    if with_volume < solids {
        problems.push(format!(
            "SolidCraft gives {with_volume} of its {solids} solids a volume; the others open as \
             empty shells"
        ));
    }
    problems
}

/// How SolidCraft says a body it imports has a shell that is not closed.
const OPEN_BODY: &str = "imported as an open body";

/// What SolidCraft shows of geometry written as an import.
#[derive(Debug, PartialEq)]
pub(crate) enum Shown {
    /// Every face, with no warning.
    Whole,
    /// Less than the model holds, or with warnings; what.
    Short(String),
    /// The check could not be made; why.
    Unchecked(String),
}

/// Evaluates `design`, written as an import of a model with `faces` faces, and says whether
/// SolidCraft shows all of it.
/// What a design written as geometry holds, to compare with what SolidCraft shows of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Expected {
    /// Its faces; 0 when not counted.
    pub faces: usize,
    /// Its solid bodies, each of which should have a volume; 0 when not counted.
    pub solids: usize,
    /// Its sheet bodies: open by design, so neither a volume nor an open shell is a fault.
    pub sheets: usize,
}

/// A body volume, in cubic millimetres, below which SolidCraft gives the body none.
const NO_VOLUME: f64 = 1e-6;

pub(crate) fn check_import(
    cli: &Path,
    timeout: Duration,
    design: &[u8],
    expected: Expected,
) -> Shown {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(err) => return Shown::Unchecked(format!("no scratch folder: {err}")),
    };
    let eval = match evaluate(cli, timeout, dir.path(), "import", design) {
        Ok(Evaluation::Done(eval)) => eval,
        Ok(Evaluation::Failed(why)) => {
            return Shown::Short(format!("SolidCraft cannot open it: {why}"))
        }
        Err(why) => return Shown::Unchecked(why),
    };
    shown(&eval, expected)
}

fn shown(eval: &Eval, expected: Expected) -> Shown {
    let mut problems: Vec<String> = eval.errors.clone();
    let faces = expected.faces;
    match eval.faces {
        Some(shown) if shown < faces as u64 => {
            let missing = faces as u64 - shown;
            let (missing_s, are) = if missing == 1 {
                ("", "is")
            } else {
                ("s", "are")
            };
            problems.insert(
                0,
                format!(
                    "SolidCraft opens {shown} of the {faces} faces converted: {missing} \
                     face{missing_s} {are} missing there"
                ),
            );
        }
        _ => {}
    }
    problems.extend(volume_problems(&eval.volumes(), expected));
    // SolidCraft warns of every open shell; the design's sheets are open by design.
    let mut sheets = expected.sheets;
    problems.extend(eval.warnings.iter().filter_map(|warning| {
        if sheets > 0 && warning.contains(OPEN_BODY) {
            sheets -= 1;
            None
        } else {
            Some(warning.clone())
        }
    }));
    if problems.is_empty() {
        Shown::Whole
    } else {
        Shown::Short(problems.join("; "))
    }
}

/// Whether two sets of bodies match, regardless of body order: in volume, within what
/// SolidCraft's measuring meshes can miss, and in area.
fn match_bodies(replayed: &[Measured], expected: &[Measured]) -> Outcome {
    if replayed.len() != expected.len() {
        return Outcome::Differs(format!(
            "{} bodies replayed, {} expected",
            replayed.len(),
            expected.len()
        ));
    }
    let mut replayed = replayed.to_vec();
    let mut expected = expected.to_vec();
    replayed.sort_by(|a, b| a.volume.total_cmp(&b.volume));
    expected.sort_by(|a, b| a.volume.total_cmp(&b.volume));
    for (got, want) in replayed.iter().zip(&expected) {
        let (v, w) = (got.volume, want.volume);
        let allowed = (VOLUME_TOLERANCE * v.abs().max(w.abs())).max(mesh_slack(got, want));
        if !within((v - w).abs(), allowed) {
            return Outcome::Differs(format!(
                "a body's volume is {v:.6} mm³, the original's {w:.6} mm³"
            ));
        }
        if let (Some(a), Some(b)) = (got.area, want.area) {
            if !within((a - b).abs(), AREA_TOLERANCE * a.abs().max(b.abs())) {
                return Outcome::Differs(format!(
                    "a body's area is {a:.6} mm², the original's {b:.6} mm²"
                ));
            }
        }
    }
    Outcome::Matches
}

/// Whether `difference` is no more than `limit`; not when either is not a number.
fn within(difference: f64, limit: f64) -> bool {
    difference <= limit
}

/// The most two SolidCraft measurements of one shape can differ in volume: each mesh's chords
/// sag at most [`MESH_CHORD`] of the body's size from its faces, so it misses at most its area
/// times that. Zero when SolidCraft gives no area or box.
fn mesh_slack(a: &Measured, b: &Measured) -> f64 {
    match (a.area, a.size, b.area, b.size) {
        (Some(area_a), Some(size_a), Some(area_b), Some(size_b)) => {
            let chord = (MESH_CHORD * size_a.max(size_b)).max(MESH_CHORD_FLOOR);
            (area_a.abs() + area_b.abs()) * chord
        }
        _ => 0.0,
    }
}

/// One body as SolidCraft measured it, in millimetres.
#[derive(Clone, Debug, PartialEq)]
struct Measured {
    volume: f64,
    /// Its area, when SolidCraft gives it.
    area: Option<f64>,
    /// The diagonal of its box, when SolidCraft gives it.
    size: Option<f64>,
}

/// One design as SolidCraft evaluated it.
struct Eval {
    bodies: Vec<Measured>,
    errors: Vec<String>,
    /// The faces SolidCraft shows across every body, when it says.
    faces: Option<u64>,
    /// Warnings on timeline features, such as faces that could not be meshed.
    warnings: Vec<String>,
}

impl Eval {
    fn volumes(&self) -> Vec<f64> {
        self.bodies.iter().map(|body| body.volume).collect()
    }
}

enum Evaluation {
    Done(Eval),
    /// SolidCraft ran and refused the design.
    Failed(String),
}

/// Evaluates `design` with the CLI. `Err` when the CLI could not give an answer at all.
fn evaluate(
    cli: &Path,
    timeout: Duration,
    dir: &Path,
    name: &str,
    design: &[u8],
) -> Result<Evaluation, String> {
    let input = dir.join(format!("{name}.solidcraft"));
    let output = dir.join(format!("{name}.json"));
    std::fs::write(&input, design).map_err(|err| format!("cannot stage the design: {err}"))?;
    let mut child = Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| launch_failure(cli, err))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(blocked_failure(cli).unwrap_or_else(|| {
                    format!("{} timed out after {} s", cli.display(), timeout.as_secs())
                }));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(err) => {
                return Err(blocked_failure(cli)
                    .unwrap_or_else(|| format!("cannot wait for {}: {err}", cli.display())))
            }
        }
    };
    if !status.success() {
        if let Some(why) = blocked_failure(cli) {
            return Err(why);
        }
        return Ok(Evaluation::Failed(format!(
            "{CLI_NAME} exited with {status}"
        )));
    }
    let text = std::fs::read_to_string(&output)
        .map_err(|err| format!("{CLI_NAME} wrote no result: {err}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|err| format!("{CLI_NAME} result: {err}"))?;
    parse(&value).map(Evaluation::Done)
}

/// One edge of a body SolidCraft replayed, as `model.edges` gives it: its end vertices, the
/// point halfway along it and its length, in millimetres. It gives no carrier or tangent.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReplayedEdge {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub mid: [f64; 3],
    pub length: f64,
}

/// The bodies at one point of a replayed timeline, each by name with its edges.
pub(crate) type ReplayedBodies = Vec<(String, Vec<ReplayedEdge>)>;

/// How many bodies, and edges in all, one replayed step may report before it is refused.
const REPLAY_BODIES: usize = 1_000;
const REPLAY_EDGES: usize = 200_000;

/// Replays `design` in SolidCraft and reports the bodies and their edges with the timeline
/// rolled to just after each feature id in `after` (in that order): the bodies each later
/// feature starts from. Two runs of the CLI in all, however many steps, within `timeout`
/// together: one finds the bodies at every step, the next their edges. Each step is `Err`
/// when a feature before it fails there; the whole is `Err` when SolidCraft gives no answer.
pub(crate) fn replay_edges(
    cli: &Path,
    timeout: Duration,
    design: &[u8],
    after: &[u64],
) -> Result<Vec<Result<ReplayedBodies, String>>, String> {
    let deadline = Instant::now() + timeout;
    let dir = tempfile::tempdir().map_err(|err| format!("no scratch folder: {err}"))?;
    let input = dir.path().join("history.solidcraft");
    std::fs::write(&input, design).map_err(|err| format!("cannot stage the design: {err}"))?;
    let roll =
        |id: u64| serde_json::json!({"command": "timeline.roll_to", "params": {"feature": id}});
    let mut script = Vec::new();
    for id in after {
        script.push(roll(*id));
        script.push(serde_json::json!({"command": "document.inspect", "params": {}}));
    }
    let results = run_script(cli, deadline, dir.path(), "bodies", &input, &script)?;
    let mut steps = Vec::new();
    for (k, _) in after.iter().enumerate() {
        let inspect = results
            .get(k.saturating_mul(2).saturating_add(1))
            .ok_or_else(|| format!("{CLI_NAME} result is missing a step"))?;
        steps.push(step_bodies(inspect));
    }
    let mut script = Vec::new();
    for (id, step) in after.iter().zip(&steps) {
        script.push(roll(*id));
        if let Ok(bodies) = step {
            for (name, _) in bodies {
                script
                    .push(serde_json::json!({"command": "model.edges", "params": {"body": name}}));
            }
        }
    }
    let results = run_script(cli, deadline, dir.path(), "edges", &input, &script)?;
    let mut at = 0usize;
    for step in &mut steps {
        at = at.saturating_add(1);
        let Ok(bodies) = step else { continue };
        let mut total = 0usize;
        for (name, edges) in bodies.iter_mut() {
            let result = results
                .get(at)
                .ok_or_else(|| format!("{CLI_NAME} result is missing a step"))?;
            at = at.saturating_add(1);
            if result.get("body").and_then(Value::as_str) != Some(name.as_str()) {
                return Err(format!(
                    "{CLI_NAME} reported the edges of another body than {name:?}"
                ));
            }
            let rows = result
                .get("edges")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{CLI_NAME} result has no edges for {name:?}"))?;
            total = total.saturating_add(rows.len());
            if total > REPLAY_EDGES {
                return Err(format!(
                    "the replayed bodies have more than {REPLAY_EDGES} edges"
                ));
            }
            *edges = rows.iter().map(replayed_edge).collect::<Result<_, _>>()?;
        }
    }
    Ok(steps)
}

/// The bodies a `document.inspect` result holds, or the first feature still active that
/// failed.
fn step_bodies(inspect: &Value) -> Result<ReplayedBodies, String> {
    let timeline = inspect
        .get("timeline")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for entry in timeline {
        let active = entry.get("rolled_back").and_then(Value::as_bool) != Some(true)
            && entry.get("suppressed").and_then(Value::as_bool) != Some(true);
        if let (true, Some(error)) = (active, entry.get("error").and_then(Value::as_str)) {
            let name = entry.get("name").and_then(Value::as_str).unwrap_or("?");
            return Err(format!("feature \"{name}\" failed there: {error}"));
        }
    }
    let bodies = inspect
        .get("bodies")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{CLI_NAME} result has no bodies"))?;
    if bodies.len() > REPLAY_BODIES {
        return Err(format!("more than {REPLAY_BODIES} bodies"));
    }
    bodies
        .iter()
        .map(|body| {
            body.get("name")
                .and_then(Value::as_str)
                .map(|name| (name.to_owned(), Vec::new()))
                .ok_or_else(|| format!("{CLI_NAME} result has a body without a name"))
        })
        .collect()
}

fn replayed_edge(row: &Value) -> Result<ReplayedEdge, String> {
    edge_fields(row).ok_or_else(|| {
        format!("{CLI_NAME} result has an edge without its ends, midpoint or length")
    })
}

fn edge_fields(row: &Value) -> Option<ReplayedEdge> {
    let point = |key: &str| -> Option<[f64; 3]> {
        let values = row.get(key)?.as_array()?;
        if values.len() != 3 {
            return None;
        }
        let mut out = [0.0; 3];
        for (slot, value) in out.iter_mut().zip(values) {
            *slot = value.as_f64().filter(|v| v.is_finite())?;
        }
        Some(out)
    };
    Some(ReplayedEdge {
        start: point("start")?,
        end: point("end")?,
        mid: point("mid")?,
        length: row
            .get("length")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)?,
    })
}

/// Runs `script` on `design` with the CLI and returns its per-command results.
fn run_script(
    cli: &Path,
    deadline: Instant,
    dir: &Path,
    name: &str,
    design: &Path,
    script: &[Value],
) -> Result<Vec<Value>, String> {
    let path = dir.join(format!("{name}.json"));
    let output = dir.join(format!("{name}-out.json"));
    let text =
        serde_json::to_string(script).map_err(|err| format!("cannot write the script: {err}"))?;
    std::fs::write(&path, text).map_err(|err| format!("cannot stage the script: {err}"))?;
    let mut child = spawn(cli, |command| {
        command
            .arg("run")
            .arg(&path)
            .arg("--in")
            .arg(design)
            .arg("--results")
            .arg("--allow-errors")
            .arg("--json-out")
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    })
    .map_err(|err| launch_failure(cli, err))?;
    let status = wait(&mut child, cli, deadline)?;
    if !status.success() {
        return Err(
            blocked_failure(cli).unwrap_or_else(|| format!("{CLI_NAME} exited with {status}"))
        );
    }
    let text = std::fs::read_to_string(&output)
        .map_err(|err| format!("{CLI_NAME} wrote no result: {err}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|err| format!("{CLI_NAME} result: {err}"))?;
    match value {
        Value::Array(results) => Ok(results),
        _ => Err(format!("{CLI_NAME} result is not a list of results")),
    }
}

/// Starts the CLI, retrying while the system says it is busy being written.
fn spawn(cli: &Path, configure: impl Fn(&mut Command)) -> std::io::Result<std::process::Child> {
    let mut tries = 0;
    loop {
        let mut command = Command::new(cli);
        configure(&mut command);
        match command.spawn() {
            Err(err)
                if err.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < BUSY_TRIES =>
            {
                tries += 1;
                std::thread::sleep(POLL);
            }
            spawned => return spawned,
        }
    }
}

/// Waits for `child` until `deadline`, killing it then.
fn wait(
    child: &mut std::process::Child,
    cli: &Path,
    deadline: Instant,
) -> Result<std::process::ExitStatus, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    blocked_failure(cli).unwrap_or_else(|| format!("{} timed out", cli.display()))
                );
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(err) => {
                return Err(blocked_failure(cli)
                    .unwrap_or_else(|| format!("cannot wait for {}: {err}", cli.display())))
            }
        }
    }
}

/// The bodies, face count and timeline errors and warnings in a SolidCraft `eval` result.
fn parse(value: &Value) -> Result<Eval, String> {
    let bodies = value
        .get("bodies")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{CLI_NAME} result has no bodies"))?;
    let bodies = bodies
        .iter()
        .map(|body| {
            let volume = body
                .get("volume_mm3")
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("{CLI_NAME} result has a body without a volume"))?;
            let corner = |end: &str| -> Option<Vec<f64>> {
                let corner = body.pointer(&format!("/bbox/{end}"))?.as_array()?;
                let corner: Option<Vec<f64>> = corner.iter().map(Value::as_f64).collect();
                corner.filter(|corner| corner.len() == 3)
            };
            let size = corner("min").zip(corner("max")).map(|(min, max)| {
                min.iter()
                    .zip(&max)
                    .map(|(lo, hi)| (hi - lo) * (hi - lo))
                    .sum::<f64>()
                    .sqrt()
            });
            Ok(Measured {
                volume,
                area: body.get("area_mm2").and_then(Value::as_f64),
                size: size.filter(|size| size.is_finite()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let timeline = value
        .get("timeline")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let errors = timeline
        .iter()
        .filter_map(|entry| {
            let error = entry.get("error").and_then(Value::as_str)?;
            let name = entry.get("name").and_then(Value::as_str).unwrap_or("?");
            Some(format!("feature \"{name}\" failed: {error}"))
        })
        .collect();
    let warnings = timeline
        .iter()
        .filter_map(|entry| entry.get("warning").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let faces = value.pointer("/total/faces").and_then(Value::as_u64);
    Ok(Eval {
        bodies,
        errors,
        faces,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_enclosing_app_is_found() {
        for bundle in ["SolidCraft.app", "SolidCraft 0.20.APP", "Solid Craft.app"] {
            let app = Path::new("Applications").join(bundle);
            let cli = app.join("Contents/MacOS/solidcraft-cli");
            assert_eq!(enclosing_app(&cli), Some(app.as_path()));
        }
        assert_eq!(enclosing_app(Path::new("bin/solidcraft-cli")), None);
        assert_eq!(enclosing_app(Path::new("solidcraft-cli")), None);
        assert_eq!(
            enclosing_app(Path::new("Apps.app/Inner.app/bin/cli")),
            Some(Path::new("Apps.app/Inner.app"))
        );
        assert_eq!(
            enclosing_app(Path::new("SolidCraft.app.backup/bin/cli")),
            None
        );
    }

    #[test]
    fn only_quarantine_without_user_approval_blocks() {
        for value in ["0081;1234;Safari;uuid", "0083;1234;Safari;uuid"] {
            assert!(quarantine_blocks(value), "{value}");
        }
        for value in [
            "00c1;1234;Safari;uuid",
            "00C3;1234;Safari;uuid\n",
            "",
            "garbage",
            "zzzz;1234;Safari;uuid",
            "081;1234;Safari;uuid",
            "00081;1234;Safari;uuid",
            "+081;1234;Safari;uuid",
        ] {
            assert!(!quarantine_blocks(value), "{value}");
        }
        assert!(MACOS_BLOCKED.contains("Open Anyway in System Settings > Privacy & Security"));
        // One sentence, so the CLI's first-sentence notes keep the steps.
        assert!(!MACOS_BLOCKED.trim_end_matches('.').contains(". "));
    }

    pub(super) fn volumes_of(volumes: &[f64]) -> Vec<Measured> {
        volumes
            .iter()
            .map(|&volume| Measured {
                volume,
                area: None,
                size: None,
            })
            .collect()
    }

    fn body(volume: f64, area: f64, size: f64) -> Vec<Measured> {
        vec![Measured {
            volume,
            area: Some(area),
            size: Some(size),
        }]
    }

    #[test]
    fn volumes_match_in_any_order_within_tolerance() {
        assert_eq!(
            match_bodies(&volumes_of(&[2.0, 1.0]), &volumes_of(&[1.0, 2.00001])),
            Outcome::Matches
        );
    }

    #[test]
    fn a_different_volume_or_body_count_differs() {
        assert!(matches!(
            match_bodies(&volumes_of(&[1.0]), &volumes_of(&[1.1])),
            Outcome::Differs(_)
        ));
        assert!(matches!(
            match_bodies(&volumes_of(&[1.0]), &volumes_of(&[1.0, 1.0])),
            Outcome::Differs(_)
        ));
    }

    #[test]
    fn a_curved_body_matches_within_what_the_meshes_miss() {
        // A 60 mm ring 40 mm tall around a 40 mm bore (exactly 20000π mm³), revolved in
        // SolidCraft and read from the original's surfaces: the two meshes differ by 1.6e-4.
        assert_eq!(
            match_bodies(
                &body(62835.566016, 15707.98, 93.81),
                &body(62825.683952, 15707.33, 93.81)
            ),
            Outcome::Matches
        );
        // A replay 5 % larger differs far past that.
        assert!(matches!(
            match_bodies(
                &body(13995.7, 7012.0, 85.0),
                &body(13288.7, 6823.4, 85.0)
            ),
            Outcome::Differs(why) if why.contains("volume")
        ));
    }

    #[test]
    fn a_thin_plate_missing_a_hole_differs_in_area() {
        // 100 × 100 × 1 mm: the mesh bound allows about 285 mm³, more than an 18 mm hole's
        // 254 mm³, but the hole changes the area by 2.2 %.
        let hole = std::f64::consts::PI * 9.0 * 9.0;
        let plate = 2.0 * 100.0 * 100.0 + 4.0 * 100.0;
        assert!(matches!(
            match_bodies(
                &body(10_000.0, plate, 141.4),
                &body(10_000.0 - hole, plate - 2.0 * hole + std::f64::consts::PI * 18.0, 141.4)
            ),
            Outcome::Differs(why) if why.contains("area")
        ));
    }

    #[test]
    fn parses_an_eval_result() {
        let value = serde_json::json!({
            "bodies": [
                {"name": "Body1", "volume_mm3": 1000.0},
                {
                    "name": "Body2", "volume_mm3": 24.0, "area_mm2": 52.0,
                    "bbox": {"min": [0.0, 0.0, 0.0], "max": [2.0, 3.0, 6.0]}
                }
            ],
            "timeline": [
                {"name": "Sketch1", "error": null},
                {"name": "Extrude1", "error": "profile not closed"}
            ]
        });
        let eval = parse(&value).unwrap();
        assert_eq!(eval.volumes(), [1000.0, 24.0]);
        assert_eq!(eval.bodies[0].area, None);
        assert_eq!(eval.bodies[1].area, Some(52.0));
        assert_eq!(eval.bodies[1].size, Some(7.0));
        assert_eq!(
            eval.errors,
            ["feature \"Extrude1\" failed: profile not closed"]
        );
    }

    #[test]
    fn an_import_shows_whole_unless_faces_are_missing_or_warned_about() {
        let eval = |faces: u64, warning: Option<&str>| {
            parse(&serde_json::json!({
                "bodies": [{"volume_mm3": 1000.0}],
                "timeline": [{"name": "cube", "error": null, "warning": warning}],
                "total": {"faces": faces},
            }))
            .unwrap()
        };
        let six = Expected {
            faces: 6,
            solids: 1,
            sheets: 0,
        };
        assert_eq!(shown(&eval(6, None), six), Shown::Whole);
        assert_eq!(
            shown(
                &eval(3, Some("solid 'cube': 3 face(s) could not be meshed")),
                six
            ),
            Shown::Short(
                "SolidCraft opens 3 of the 6 faces converted: 3 faces are missing there; solid 'cube': 3 face(s) could not be meshed"
                    .to_owned()
            )
        );
        assert!(matches!(shown(&eval(6, Some("odd")), six), Shown::Short(_)));
    }

    #[test]
    fn solids_without_a_volume_or_inside_out_are_reported() {
        let eval = |volumes: &[f64]| Eval {
            bodies: volumes_of(volumes),
            errors: Vec::new(),
            faces: None,
            warnings: Vec::new(),
        };
        let two = Expected {
            faces: 0,
            solids: 2,
            sheets: 0,
        };
        assert_eq!(shown(&eval(&[1000.0, 8.0]), two), Shown::Whole);
        assert_eq!(
            shown(&eval(&[1000.0, 0.0, 0.0]), two),
            Shown::Short(
                "SolidCraft gives 1 of its 2 solids a volume; the others open as empty shells"
                    .to_owned()
            )
        );
        assert_eq!(
            shown(&eval(&[1000.0, -65.4]), Expected::default()),
            Shown::Short("SolidCraft reads 1 body inside out, with a negative volume".to_owned())
        );
    }

    #[test]
    fn verification_off_finds_no_cli() {
        assert!(find_cli(&Verification::Off).is_err());
        let path = PathBuf::from("/opt/sc");
        assert_eq!(find_cli(&Verification::With(path.clone())), Ok(path));
    }
}

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod cli_tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn an_unblocked_probe_keeps_its_version_even_on_failure() {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("solidcraft-cli");
        std::fs::write(&cli, "#!/bin/sh\necho 'solidcraft-cli 0.1.0'\nexit 3\n").unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            version_of(&cli),
            Ok(Some("solidcraft-cli 0.1.0".to_owned()))
        );
        assert!(matches!(
            usable_cli(&Verification::With(cli)),
            Err(NoCli::TooOld(_))
        ));
    }

    /// A stand-in SolidCraft CLI: `script` runs with `$design` (the design path) and must
    /// print the eval result, which is written to the `--json-out` file.
    #[test]
    fn the_newest_solidcraft_app_carries_the_cli() {
        let dir = tempfile::tempdir().unwrap();
        for (app, version) in [
            ("SolidCraft.app", Some("0.18.2")),
            ("SolidCraft 2.app", Some("0.18.10")),
            ("SolidCraft old.app", None),
            ("Other.app", Some("9.0")),
        ] {
            let macos = dir.path().join(app).join("Contents/MacOS");
            std::fs::create_dir_all(&macos).unwrap();
            std::fs::write(macos.join(CLI_NAME), b"").unwrap();
            if let Some(version) = version {
                std::fs::write(
                    dir.path().join(app).join("Contents/Info.plist"),
                    format!(
                        "<dict><key>CFBundleShortVersionString</key>\n<string>{version}</string></dict>"
                    ),
                )
                .unwrap();
            }
        }
        let cli = in_app_bundles(&[dir.path().to_path_buf(), dir.path().join("missing")]).unwrap();
        assert!(
            cli.ends_with("SolidCraft 2.app/Contents/MacOS/solidcraft-cli"),
            "{cli:?}"
        );
        assert_eq!(in_app_bundles(&[dir.path().join("missing")]), None);
    }

    fn fake_cli(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("solidcraft-cli");
        let body = format!("#!/bin/sh\ndesign=\"$2\"\nout=\"$4\"\n{{\n{script}\n}} > \"$out\"\n");
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn eval(volume: f64, error: &str) -> String {
        format!(
            "echo '{{\"bodies\":[{{\"volume_mm3\":{volume}}}],\"timeline\":[{{\"name\":\"Extrude1\",\"error\":{error}}}]}}'"
        )
    }

    fn by_design(original: &str, history: &str) -> String {
        format!("case \"$design\" in *original*) {original};; *) {history};; esac")
    }

    /// Held while a stand-in CLI is written and run. A script still open for writing when
    /// another test forks is inherited by that child, and running it then fails with "text file
    /// busy"; one at a time avoids that.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn run(script: &str, timeout: Duration) -> Outcome {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(dir.path(), script);
        compare(
            &cli,
            timeout,
            b"history",
            b"original",
            Expected {
                solids: 1,
                ..Expected::default()
            },
        )
    }

    #[test]
    fn a_sigkill_without_unapproved_quarantine_keeps_its_failure_message() {
        assert!(matches!(
            run("kill -KILL $$", DEFAULT_TIMEOUT),
            Outcome::Unverified(why) if why.contains("signal: 9")
        ));
    }

    #[test]
    fn an_original_that_opens_unsoundly_leaves_a_replayed_history_unverified() {
        // The original opens as an empty shell; the history replays to a solid.
        let shell = by_design(&eval(0.0, "null"), &eval(1000.0, "null"));
        assert_eq!(
            run(&shell, DEFAULT_TIMEOUT),
            Outcome::Unverified(
                "the original geometry cannot be compared with: SolidCraft gives 0 of its 1 \
                 solids a volume; the others open as empty shells"
                    .to_owned()
            )
        );
        let inside_out = by_design(&eval(-65.4, "null"), &eval(1000.0, "null"));
        assert!(matches!(
            run(&inside_out, DEFAULT_TIMEOUT),
            Outcome::Unverified(_)
        ));
        // A history that does not replay still differs.
        let failing = by_design(&eval(0.0, "null"), &eval(0.0, "\"profile not closed\""));
        assert!(matches!(
            run(&failing, DEFAULT_TIMEOUT),
            Outcome::Differs(_)
        ));
    }

    #[test]
    fn a_matching_replay_matches() {
        let script = by_design(&eval(1000.0, "null"), &eval(1000.0, "null"));
        assert_eq!(run(&script, DEFAULT_TIMEOUT), Outcome::Matches);
    }

    #[test]
    fn a_different_volume_differs() {
        let script = by_design(&eval(1000.0, "null"), &eval(900.0, "null"));
        assert!(matches!(run(&script, DEFAULT_TIMEOUT), Outcome::Differs(_)));
    }

    #[test]
    fn a_failing_feature_differs() {
        let script = by_design(&eval(1000.0, "null"), &eval(1000.0, "\"no profile\""));
        assert_eq!(
            run(&script, DEFAULT_TIMEOUT),
            Outcome::Differs("feature \"Extrude1\" failed: no profile".to_owned())
        );
    }

    #[test]
    fn a_history_solidcraft_refuses_differs() {
        let script = by_design(&eval(1000.0, "null"), "exit 3");
        assert!(matches!(run(&script, DEFAULT_TIMEOUT), Outcome::Differs(_)));
    }

    #[test]
    fn an_original_solidcraft_cannot_read_leaves_it_unverified() {
        let script = by_design("exit 3", &eval(1000.0, "null"));
        assert!(matches!(
            run(&script, DEFAULT_TIMEOUT),
            Outcome::Unverified(_)
        ));
    }

    #[test]
    fn a_slow_solidcraft_leaves_it_unverified() {
        let outcome = run("sleep 5", Duration::from_millis(200));
        assert!(
            matches!(&outcome, Outcome::Unverified(why) if why.contains("timed out")),
            "{outcome:?}"
        );
    }

    fn import_check(script: &str) -> Shown {
        let _turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(dir.path(), script);
        check_import(
            &cli,
            DEFAULT_TIMEOUT,
            b"design",
            Expected {
                faces: 6,
                solids: 0,
                sheets: 0,
            },
        )
    }

    #[test]
    fn an_import_solidcraft_shows_short_is_reported() {
        let whole =
            "echo '{\"bodies\":[{\"volume_mm3\":1000}],\"timeline\":[],\"total\":{\"faces\":6}}'";
        assert_eq!(import_check(whole), Shown::Whole);
        let short = "echo '{\"bodies\":[{\"volume_mm3\":3.3}],\"timeline\":[{\"name\":\"cube\",\"error\":null,\"warning\":\"3 face(s) could not be meshed\"}],\"total\":{\"faces\":3}}'";
        assert_eq!(
            import_check(short),
            Shown::Short(
                "SolidCraft opens 3 of the 6 faces converted: 3 faces are missing there; 3 face(s) could not be meshed".to_owned()
            )
        );
        assert!(matches!(import_check("exit 3"), Shown::Short(_)));
        assert!(matches!(import_check("echo nonsense"), Shown::Unchecked(_)));
    }

    #[test]
    fn a_missing_cli_leaves_it_unverified() {
        let outcome = compare(
            Path::new("/nonexistent/solidcraft-cli"),
            DEFAULT_TIMEOUT,
            b"h",
            b"o",
            Expected {
                solids: 1,
                ..Expected::default()
            },
        );
        assert!(matches!(outcome, Outcome::Unverified(_)));
    }
    #[test]
    fn versions_are_read_from_the_cli_line() {
        assert_eq!(parse_version("solidcraft-cli 0.19.0"), Some([0, 19, 0]));
        assert_eq!(parse_version("solidcraft-cli v0.18"), Some([0, 18, 0]));
        assert_eq!(parse_version("solidcraft-cli 1.2.3-beta"), Some([1, 2, 3]));
        assert_eq!(parse_version("no version here"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_solidcraft_too_old_is_refused_with_the_reason() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cli = |name: &str, version: &str| {
            let path = dir.path().join(name);
            std::fs::write(
                &path,
                format!("#!/bin/sh\necho 'solidcraft-cli {version}'\n"),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let old = cli("old", "0.17.0");
        match usable_cli(&Verification::With(old)) {
            Err(NoCli::TooOld(why)) => assert!(
                why.starts_with("solidcraft-cli 0.17.0 at ")
                    && why.ends_with(
                        "is older than SolidCraft 0.18.2, the oldest that opens \
                                      and checks Dutchman's designs; install a newer SolidCraft"
                    ),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
        let new = cli("new", "0.19.0");
        assert_eq!(usable_cli(&Verification::With(new.clone())), Ok(new));
        assert!(matches!(
            usable_cli(&Verification::Off),
            Err(NoCli::NotFound(_))
        ));
    }

    #[test]
    fn sheets_are_not_reported_as_inside_out_or_open() {
        let eval = |volumes: &[f64], warnings: &[&str]| Eval {
            bodies: super::tests::volumes_of(volumes),
            errors: Vec::new(),
            faces: None,
            warnings: warnings.iter().map(|w| (*w).to_owned()).collect(),
        };
        let open = "solid #6704: imported as an open body (the shell is not closed)";
        // A solid and a sheet disk, which SolidCraft gives a negative volume and calls open.
        let one_sheet = Expected {
            faces: 0,
            solids: 1,
            sheets: 1,
        };
        assert_eq!(
            shown(&eval(&[1000.0, -2629.0], &[open]), one_sheet),
            Shown::Whole
        );
        // A solid inside out, or a second open shell, is still reported.
        assert_eq!(
            shown(&eval(&[-1000.0, -2629.0], &[open, open]), one_sheet),
            Shown::Short(format!(
                "SolidCraft reads 1 body inside out, with a negative volume; SolidCraft gives 0 \
                 of its 1 solids a volume; the others open as empty shells; {open}"
            ))
        );
        // Without sheets, both are faults.
        assert!(matches!(
            shown(&eval(&[1000.0, -2629.0], &[open]), Expected::default()),
            Shown::Short(_)
        ));
    }
}
