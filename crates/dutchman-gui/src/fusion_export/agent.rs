//! Finding the Fusion exporter: the agent file the add-in keeps in `~/.cad-export/agents/`.
//!
//! The add-in rewrites its file at most 10 s apart. A file updated less than 60 s ago means the
//! add-in is running. An older one means Fusion is either busy (its process is still there:
//! the heartbeat comes from a thread Fusion can hold up for minutes while it opens one large
//! design) or gone (the process is not). A stale heartbeat alone never means gone.
//!
//! Every `*.json` in `agents/` is read, as the protocol says; a file is used only when it is for
//! this exporter, speaks protocol 1 and names the jobs folder inside the same `.cad-export`.
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The `exporter` this code looks for.
pub const EXPORTER: &str = "fusion-bulk-export";
/// The job protocol this code speaks.
pub const PROTOCOL: i64 = 1;

/// The exporter an export job is handed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    /// Where jobs are written.
    pub jobs_dir: PathBuf,
    /// What this version of the add-in supports.
    pub features: Vec<String>,
    /// The add-in's version.
    pub version: String,
    /// Fusion's process id, for the heartbeat rule.
    pub pid: Option<u32>,
    /// The heartbeat, seconds since 1970.
    pub updated: Option<i64>,
    /// `None`, or what an export in progress was started from: `button` or `job`.
    pub busy: Option<String>,
    /// The protocol job running now.
    pub job: Option<String>,
    /// Fusion's version, when the file says.
    pub fusion_version: Option<String>,
}

/// What reading one agent file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// No file, or another exporter, or one that does not accept protocol 1.
    Absent,
    /// A file that cannot be trusted; the text says why in plain words.
    Invalid(String),
    Ready(Agent),
}

/// Whether the exporter is there, by the protocol's rule: a heartbeat under 60 s old is
/// running; an older one is checked against the process, never taken as gone by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Running,
    /// The heartbeat is old but Fusion's process lives: Fusion is busy, keep waiting.
    Busy,
    Gone,
}

/// What the agent folder says about the exporter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// No agent file for the exporter: Fusion is closed or the add-in is not switched on.
    Absent,
    /// An agent file that cannot be used; the text says why in plain words.
    Invalid(String),
    /// A stale file whose process is gone: Fusion crashed.
    Gone,
    /// The heartbeat is fresh.
    Running(Agent),
    /// The heartbeat is stale and Fusion is still running.
    Unresponsive(Agent),
}

impl Presence {
    /// The exporter, when it is there to take a job (running, or Fusion busy with something).
    pub fn agent(&self) -> Option<&Agent> {
        match self {
            Presence::Running(agent) | Presence::Unresponsive(agent) => Some(agent),
            _ => None,
        }
    }
}

pub const HEARTBEAT_SECS: i64 = 60;

/// Seconds since 1970.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// Whether a process with this id is running (asks the system's own tool). When the tool cannot
/// be run, a running Fusion is never called gone.
pub fn pid_alive_system(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut command;
    if cfg!(windows) {
        command = std::process::Command::new("tasklist");
        command.args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"]);
    } else {
        command = std::process::Command::new("kill");
        command.args(["-0", &pid.to_string()]);
    }
    command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    let Ok(output) = command.output() else {
        return true;
    };
    if cfg!(windows) {
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    } else {
        output.status.success()
    }
}

/// The agent's liveness at `now`; `pid_alive` is asked only when the heartbeat is old.
pub fn liveness(agent: &Agent, now: i64, pid_alive: &dyn Fn(u32) -> bool) -> Liveness {
    match agent.updated {
        Some(updated) if (now - updated).abs() < HEARTBEAT_SECS => Liveness::Running,
        _ => match agent.pid {
            Some(pid) if pid != 0 && pid_alive(pid) => Liveness::Busy,
            _ => Liveness::Gone,
        },
    }
}

/// The exporter in the agent folder of `cad_dir` (`~/.cad-export`) at `now`, with `pid_alive`
/// telling whether a process id is running. A running exporter beats a busy one, a busy one a
/// stale one; a file that cannot be used is reported only when nothing usable is there.
pub fn presence_in(cad_dir: &Path, now: i64, pid_alive: &dyn Fn(u32) -> bool) -> Presence {
    let Ok(entries) = std::fs::read_dir(cad_dir.join("agents")) else {
        return Presence::Absent;
    };
    let mut invalid: Option<String> = None;
    let mut gone = false;
    let mut best: Option<(u8, i64, Presence)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Writers rename a finished temporary file into place; never read the temporary one.
        if name.starts_with('.') || name.ends_with(".tmp") || !name.ends_with(".json") {
            continue;
        }
        match read(&entry.path()) {
            Found::Absent => {}
            Found::Invalid(why) => invalid = invalid.or(Some(why)),
            Found::Ready(agent) => {
                let updated = agent.updated.unwrap_or(i64::MIN);
                let (rank, presence) = match liveness(&agent, now, pid_alive) {
                    Liveness::Running => (2, Presence::Running(agent)),
                    Liveness::Busy => (1, Presence::Unresponsive(agent)),
                    Liveness::Gone => {
                        gone = true;
                        continue;
                    }
                };
                if best
                    .as_ref()
                    .is_none_or(|(r, u, _)| (rank, updated) > (*r, *u))
                {
                    best = Some((rank, updated, presence));
                }
            }
        }
    }
    match (best, invalid) {
        (Some((_, _, presence)), _) => presence,
        (None, Some(why)) => Presence::Invalid(why),
        (None, None) if gone => Presence::Gone,
        (None, None) => Presence::Absent,
    }
}

/// The exporter in this person's Fusion, if there is one.
pub fn find() -> Option<Agent> {
    let cad = cad_export_dir()?;
    presence_in(&cad, now_secs(), &pid_alive_system)
        .agent()
        .cloned()
}

/// Seconds since the Unix epoch of an ISO 8601 time with a UTC offset, such as
/// `2026-11-23T09:30:11.004000+00:00` (or `Z`; no offset means UTC). Fractions of a second
/// are dropped.
pub fn parse_time(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, zone) = match rest.rfind(['+', '-', 'Z']) {
        Some(split) => rest.split_at(split),
        None => (rest, "Z"),
    };
    let mut c = clock.split(':');
    let hour: i64 = c.next()?.parse().ok()?;
    let minute: i64 = c.next()?.parse().ok()?;
    let second: i64 = c.next().unwrap_or("0").split('.').next()?.parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let offset = if zone == "Z" {
        0
    } else {
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let (h, m) = zone[1..].split_once(':').unwrap_or((&zone[1..], "0"));
        let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
        sign * (h * 3600 + m * 60)
    };
    // Days from the civil date (proleptic Gregorian).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Biggest agent or status file read.
pub const SMALL_FILE: u64 = 1024 * 1024;

/// Reads a regular file of at most `cap` bytes. `Ok(None)` when it does not exist; an error
/// for a link, a folder, or a file over the cap, without reading it.
pub fn read_small(path: &Path, cap: u64) -> Result<Option<String>, String> {
    use std::io::Read;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(why) => return Err(format!("cannot read a file of the Fusion exporter: {why}")),
    };
    if !meta.is_file() {
        return Err("a file of the Fusion exporter is a link or not a plain file".into());
    }
    if meta.len() > cap {
        return Err("a file of the Fusion exporter is too large".into());
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(cap).read_to_string(&mut text))
        .map_err(|why| format!("cannot read a file of the Fusion exporter: {why}"))?;
    Ok(Some(text))
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || (cfg!(windows)
            && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase())
}

/// Reads the agent file (`<cad dir>/agents/fusion-bulk-export.json`). Its `jobs_dir` must be
/// `<cad dir>/jobs`: any other folder is refused, so a file there cannot send jobs, or
/// deletes, elsewhere.
pub fn read(path: &Path) -> Found {
    let text = match read_small(path, SMALL_FILE) {
        Ok(Some(text)) => text,
        Ok(None) => return Found::Absent,
        Err(why) => {
            return Found::Invalid(format!(
                "The Fusion exporter's agent file cannot be used: {why}."
            ))
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Found::Absent;
    };
    if value.get("exporter").and_then(Value::as_str) != Some(EXPORTER) {
        return Found::Absent;
    }
    let accepts_one = value
        .get("protocols")
        .and_then(Value::as_array)
        .map(|list| list.iter().any(|p| p.as_i64() == Some(PROTOCOL)))
        .unwrap_or_else(|| value.get("protocol").and_then(Value::as_i64) == Some(PROTOCOL));
    if !accepts_one {
        return Found::Absent;
    }
    let Some(jobs_dir) = value
        .get("jobs_dir")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return Found::Absent;
    };
    let expected = path
        .parent()
        .and_then(Path::parent)
        .map(|cad| cad.join("jobs"));
    if !expected.is_some_and(|expected| same_path(&jobs_dir, &expected)) {
        return Found::Invalid(
            "The Fusion exporter's agent file names a jobs folder outside your .cad-export folder, so Dutchman will not use it.".into(),
        );
    }
    let features = value
        .get("features")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|f| f.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty())
    };
    Found::Ready(Agent {
        jobs_dir,
        features,
        version: text("version").unwrap_or_default(),
        busy: busy_of(&value),
        job: text("job"),
        fusion_version: text("fusion_version"),
        pid: value
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok()),
        updated: value
            .get("updated")
            .and_then(Value::as_str)
            .and_then(parse_time),
    })
}

/// What the add-in is busy with: any value but `null` (or missing) means busy; a string says
/// what, anything else is kept as its JSON text.
fn busy_of(value: &Value) -> Option<String> {
    match value.get("busy")? {
        Value::Null => None,
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::String(_) => Some("busy".into()),
        other => Some(other.to_string()),
    }
}

/// The agent, when the file is there and can be used.
pub fn read_minimal(path: &Path) -> Option<Agent> {
    match read(path) {
        Found::Ready(agent) => Some(agent),
        _ => None,
    }
}

/// The exporter's agent file: `~/.cad-export/agents/fusion-bulk-export.json`.
pub fn agent_file(cad_export: &Path) -> PathBuf {
    cad_export.join("agents").join("fusion-bulk-export.json")
}

/// `~/.cad-export` of the person running this program (`%USERPROFILE%` on Windows).
pub fn cad_export_dir() -> Option<PathBuf> {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    }?;
    Some(PathBuf::from(home).join(".cad-export"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_795_000_000;

    fn stamp(secs: i64) -> String {
        // The inverse of `parse_time` for a UTC time, built from the same days arithmetic.
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.004000+00:00",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    }

    /// Writes an agent file into `<cad>/agents/<name>`, `age` seconds stale at `NOW`.
    fn file(cad: &Path, name: &str, age: i64, pid: u32, edit: impl Fn(&mut Value)) {
        let mut value = serde_json::json!({
            "protocol": 1, "protocols": [1], "agent": "fusion-bulk-export",
            "exporter": "fusion-bulk-export", "version": "0.3.5",
            "fusion_version": "2.0.21508", "fusion_running": true, "pid": pid,
            "started": stamp(NOW - 900), "updated": stamp(NOW - age), "busy": null,
            "job": null, "jobs_dir": cad.join("jobs").to_string_lossy(),
            "features": ["export", "cancel"]
        });
        edit(&mut value);
        std::fs::create_dir_all(cad.join("agents")).unwrap();
        std::fs::write(cad.join("agents").join(name), value.to_string()).unwrap();
    }

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn look(cad: &Path, alive: bool) -> Presence {
        presence_in(cad, NOW, &|_| alive)
    }

    #[test]
    fn times_parse_with_offsets() {
        assert_eq!(parse_time("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_time("1970-01-02T00:00:00Z"), Some(86_400));
        assert_eq!(parse_time("1970-01-01T01:00:00.5+01:00"), Some(0));
        assert_eq!(parse_time("1970-01-01T00:00:00-02:30"), Some(9000));
        assert_eq!(parse_time("1970-01-02T00:00:00"), Some(86_400));
        assert_eq!(
            parse_time("2026-11-23T09:30:11.004000+00:00"),
            Some(1_795_426_211)
        );
        assert_eq!(parse_time("yesterday"), None);
        assert_eq!(parse_time(&stamp(NOW)), Some(NOW));
    }

    #[test]
    fn a_fresh_heartbeat_is_running() {
        let d = dir();
        file(d.path(), "fusion-bulk-export.json", 5, 41237, |v| {
            v["busy"] = "job".into();
            v["job"] = "abc".into();
        });
        let Presence::Running(agent) = look(d.path(), false) else {
            panic!("fresh file should be running");
        };
        assert_eq!(agent.version, "0.3.5");
        assert_eq!(agent.busy.as_deref(), Some("job"));
        assert_eq!(agent.job.as_deref(), Some("abc"));
        assert_eq!(agent.jobs_dir, d.path().join("jobs"));
        assert_eq!(agent.features, ["export", "cancel"]);
        assert_eq!(agent.fusion_version.as_deref(), Some("2.0.21508"));
    }

    #[test]
    fn any_value_but_null_for_busy_means_busy() {
        let d = dir();
        for (value, expect) in [
            (serde_json::json!(true), Some("true")),
            (serde_json::json!(1), Some("1")),
            (serde_json::json!("job"), Some("job")),
            (serde_json::json!(""), Some("busy")),
            (serde_json::Value::Null, None),
        ] {
            file(d.path(), "fusion-bulk-export.json", 1, 1, |v| {
                v["busy"] = value.clone()
            });
            let Presence::Running(agent) = look(d.path(), true) else {
                panic!("running");
            };
            assert_eq!(agent.busy.as_deref(), expect, "{value}");
        }
    }

    #[test]
    fn a_stale_heartbeat_with_a_live_process_is_busy_not_gone() {
        let d = dir();
        file(d.path(), "fusion-bulk-export.json", 300, 41237, |_| {});
        let Presence::Unresponsive(agent) = look(d.path(), true) else {
            panic!("stale but alive is Fusion busy");
        };
        assert_eq!(agent.pid, Some(41237));
    }

    #[test]
    fn a_stale_heartbeat_with_a_dead_process_is_gone() {
        let d = dir();
        file(d.path(), "fusion-bulk-export.json", 300, 41237, |_| {});
        assert_eq!(look(d.path(), false), Presence::Gone);
    }

    #[test]
    fn an_unreadable_time_counts_as_stale() {
        let d = dir();
        file(d.path(), "fusion-bulk-export.json", 0, 7, |v| {
            v["updated"] = "soon".into()
        });
        assert!(matches!(look(d.path(), true), Presence::Unresponsive(_)));
        assert_eq!(look(d.path(), false), Presence::Gone);
    }

    #[test]
    fn other_exporters_and_protocols_are_skipped() {
        let d = dir();
        file(d.path(), "other.json", 1, 1, |v| {
            v["exporter"] = "other".into()
        });
        file(d.path(), "fusion-bulk-export.json", 1, 1, |v| {
            v["protocols"] = serde_json::json!([2])
        });
        assert_eq!(look(d.path(), true), Presence::Absent);
        std::fs::write(d.path().join("agents/broken.json"), "{").unwrap();
        assert_eq!(look(d.path(), true), Presence::Absent);
    }

    #[test]
    fn dot_files_and_tmp_files_are_ignored() {
        let d = dir();
        file(d.path(), ".fusion-bulk-export.json.x1.tmp", 1, 1, |_| {});
        file(d.path(), ".hidden.json", 1, 1, |_| {});
        file(d.path(), "fusion-bulk-export.json.tmp", 1, 1, |_| {});
        file(d.path(), "notes.txt", 1, 1, |_| {});
        assert_eq!(look(d.path(), true), Presence::Absent);
        assert_eq!(look(&d.path().join("missing"), true), Presence::Absent);
    }

    #[test]
    fn a_running_agent_wins_over_a_stale_one() {
        let d = dir();
        file(d.path(), "a.json", 600, 1, |_| {});
        file(d.path(), "b.json", 3, 2, |_| {});
        assert!(matches!(look(d.path(), false), Presence::Running(a) if a.pid == Some(2)));
    }

    #[test]
    fn a_file_that_cannot_be_used_is_reported_only_when_nothing_else_is_there() {
        let d = dir();
        file(d.path(), "a.json", 3, 2, |v| {
            v["jobs_dir"] = "/somewhere/else/jobs".into()
        });
        let Presence::Invalid(why) = look(d.path(), true) else {
            panic!("a jobs folder outside .cad-export is refused");
        };
        assert!(why.contains("outside your .cad-export"), "{why}");
        file(d.path(), "b.json", 3, 3, |_| {});
        assert!(matches!(look(d.path(), true), Presence::Running(a) if a.pid == Some(3)));
    }

    #[test]
    fn find_reads_the_home_folder_only() {
        assert!(cad_export_dir().is_none_or(|d| d.ends_with(".cad-export")));
    }

    #[cfg(unix)]
    #[test]
    fn pid_alive_system_tells_this_process_from_a_gone_one() {
        assert!(pid_alive_system(std::process::id()));
        assert!(!pid_alive_system(0));
    }
}
