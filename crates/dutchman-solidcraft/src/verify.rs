//! Checks a written history against the original geometry by evaluating both in SolidCraft's
//! own command-line tool. Dutchman has no kernel of its own, and evaluating both in the same
//! SolidCraft makes any kernel difference cancel out.

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
    /// With the SolidCraft CLI named by `SOLIDCRAFT_CLI`, else the one on `PATH`.
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

/// The SolidCraft CLI to check with, if any.
pub(crate) fn find_cli(verification: &Verification) -> Result<PathBuf, String> {
    match verification {
        Verification::Off => Err("verification turned off".to_owned()),
        Verification::With(path) => Ok(path.clone()),
        Verification::Auto => std::env::var_os(CLI_ENV)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(on_path)
            .ok_or_else(|| format!("no {CLI_NAME} found (set {CLI_ENV} or add it to PATH)")),
    }
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

/// The body volumes and timeline errors in a SolidCraft `eval` result.
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
    let errors = value
        .get("timeline")
        .and_then(Value::as_array)
        .map(|timeline| {
            timeline
                .iter()
                .filter_map(|entry| {
                    let error = entry.get("error").and_then(Value::as_str)?;
                    let name = entry.get("name").and_then(Value::as_str).unwrap_or("?");
                    Some(format!("feature \"{name}\" failed: {error}"))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Eval { volumes, errors })
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

    fn run(script: &str, timeout: Duration) -> Outcome {
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
