//! Checks written designs in SolidCraft's own command-line tool, since Dutchman has no kernel
//! of its own: a written history against the original geometry (evaluating both in the same
//! SolidCraft makes any kernel difference cancel out), and geometry written as an import
//! against what the model holds.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The relative difference in volume at which a replay no longer matches the original.
pub const VOLUME_TOLERANCE: f64 = 1e-4;

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
    let cli = find_cli(verification).ok()?;
    let mut child = Command::new(cli)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    while child.try_wait().ok()?.is_none() {
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(POLL);
    }
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(child.stdout.take()?, 4096),
        &mut text,
    )
    .ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{CLI_NAME}{}", std::env::consts::EXE_SUFFIX)))
        .find(|candidate| candidate.is_file())
}

/// Compares `history` (the design as written) with `original` (the same design with the
/// history suppressed and the original geometry live).
pub(crate) fn compare(cli: &Path, timeout: Duration, history: &[u8], original: &[u8]) -> Outcome {
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
    match_volumes(&replayed.volumes, &expected.volumes)
}

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
pub(crate) fn check_import(cli: &Path, timeout: Duration, design: &[u8], faces: usize) -> Shown {
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
    shown(&eval, faces)
}

fn shown(eval: &Eval, faces: usize) -> Shown {
    let mut problems: Vec<String> = eval.errors.clone();
    match eval.faces {
        Some(shown) if shown < faces as u64 => {
            problems.insert(0, format!("SolidCraft shows {shown} of its {faces} faces"));
        }
        _ => {}
    }
    problems.extend(eval.warnings.iter().cloned());
    if problems.is_empty() {
        Shown::Whole
    } else {
        Shown::Short(problems.join("; "))
    }
}

/// Whether two sets of body volumes match, regardless of body order.
fn match_volumes(replayed: &[f64], expected: &[f64]) -> Outcome {
    if replayed.len() != expected.len() {
        return Outcome::Differs(format!(
            "{} bodies replayed, {} expected",
            replayed.len(),
            expected.len()
        ));
    }
    let mut replayed = replayed.to_vec();
    let mut expected = expected.to_vec();
    replayed.sort_by(f64::total_cmp);
    expected.sort_by(f64::total_cmp);
    for (got, want) in replayed.iter().zip(&expected) {
        let scale = got.abs().max(want.abs()).max(f64::MIN_POSITIVE);
        if (got - want).abs() / scale > VOLUME_TOLERANCE {
            return Outcome::Differs(format!(
                "a body's volume is {got:.6} mm³, the original's {want:.6} mm³"
            ));
        }
    }
    Outcome::Matches
}

/// One design as SolidCraft evaluated it.
struct Eval {
    volumes: Vec<f64>,
    errors: Vec<String>,
    /// The faces SolidCraft shows across every body, when it says.
    faces: Option<u64>,
    /// Warnings on timeline features, such as faces that could not be meshed.
    warnings: Vec<String>,
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
        .map_err(|err| format!("cannot run {}: {err}", cli.display()))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} timed out after {} s",
                    cli.display(),
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(err) => return Err(format!("cannot wait for {}: {err}", cli.display())),
        }
    };
    if !status.success() {
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

/// The body volumes, face count and timeline errors and warnings in a SolidCraft `eval` result.
fn parse(value: &Value) -> Result<Eval, String> {
    let bodies = value
        .get("bodies")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{CLI_NAME} result has no bodies"))?;
    let volumes = bodies
        .iter()
        .map(|body| {
            body.get("volume_mm3")
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("{CLI_NAME} result has a body without a volume"))
        })
        .collect::<Result<Vec<_>, _>>()?;
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
        volumes,
        errors,
        faces,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_match_in_any_order_within_tolerance() {
        assert_eq!(
            match_volumes(&[2.0, 1.0], &[1.0, 2.00001]),
            Outcome::Matches
        );
    }

    #[test]
    fn a_different_volume_or_body_count_differs() {
        assert!(matches!(match_volumes(&[1.0], &[1.1]), Outcome::Differs(_)));
        assert!(matches!(
            match_volumes(&[1.0], &[1.0, 1.0]),
            Outcome::Differs(_)
        ));
    }

    #[test]
    fn parses_an_eval_result() {
        let value = serde_json::json!({
            "bodies": [{"name": "Body1", "volume_mm3": 1000.0}],
            "timeline": [
                {"name": "Sketch1", "error": null},
                {"name": "Extrude1", "error": "profile not closed"}
            ]
        });
        let eval = parse(&value).unwrap();
        assert_eq!(eval.volumes, [1000.0]);
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
        assert_eq!(shown(&eval(6, None), 6), Shown::Whole);
        assert_eq!(
            shown(
                &eval(3, Some("solid 'cube': 3 face(s) could not be meshed")),
                6
            ),
            Shown::Short(
                "SolidCraft shows 3 of its 6 faces; solid 'cube': 3 face(s) could not be meshed"
                    .to_owned()
            )
        );
        assert!(matches!(shown(&eval(6, Some("odd")), 6), Shown::Short(_)));
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
        compare(&cli, timeout, b"history", b"original")
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
        check_import(&cli, DEFAULT_TIMEOUT, b"design", 6)
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
                "SolidCraft shows 3 of its 6 faces; 3 face(s) could not be meshed".to_owned()
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
        );
        assert!(matches!(outcome, Outcome::Unverified(_)));
    }
}
