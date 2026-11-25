//! Finding the Fusion exporter: the agent file the add-in keeps in `~/.cad-export/agents/`.
//!
//! The add-in rewrites its file at most 10 s apart. A file updated less than 60 s ago means the
//! add-in is running. An older one means Fusion is either busy (its process is still there:
//! the heartbeat comes from a thread Fusion can hold up for minutes while it opens one large
//! design) or gone (the process is not). A stale heartbeat alone never means gone.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// The `exporter` this code looks for.
pub const EXPORTER: &str = "fusion-bulk-export";
/// The job protocol this code speaks.
pub const PROTOCOL: u64 = 1;
/// A heartbeat younger than this many seconds means the add-in is running.
pub const FRESH_SECS: i64 = 60;
const MAX_FILE_BYTES: u64 = 1 << 20;

/// The exporter, as its agent file describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    /// Where jobs are written.
    pub jobs_dir: PathBuf,
    /// What this version of the add-in supports.
    pub features: Vec<String>,
    /// The add-in's version.
    pub version: String,
    /// `None`, or what an export in progress was started from: `button` or `job`.
    pub busy: Option<String>,
    /// The protocol job running now.
    pub job: Option<String>,
    /// Fusion's version, when the file says.
    pub fusion_version: Option<String>,
    /// Fusion's process id.
    pub pid: u32,
    /// The heartbeat is stale but Fusion's process is alive: Fusion is busy, keep waiting.
    pub fusion_busy: bool,
}

/// What the agent folder says about the exporter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// No agent file for the exporter: Fusion is closed or the add-in is not switched on.
    Absent,
    /// A stale file whose process is gone: Fusion crashed.
    Gone,
    /// The heartbeat is fresh.
    Running(Agent),
    /// The heartbeat is stale and Fusion is still running.
    Unresponsive(Agent),
}

/// The folder the add-in announces itself in.
pub fn agents_dir() -> Option<PathBuf> {
    home().map(|home| home.join(".cad-export").join("agents"))
}

fn home() -> Option<PathBuf> {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// The exporter running in this person's Fusion, if there is one: its heartbeat is fresh, or
/// stale with Fusion still alive (`fusion_busy`).
pub fn find() -> Option<Agent> {
    match presence() {
        Presence::Running(agent) | Presence::Unresponsive(agent) => Some(agent),
        Presence::Absent | Presence::Gone => None,
    }
}

/// [`find`], with the difference between the four cases kept.
pub fn presence() -> Presence {
    match agents_dir() {
        Some(dir) => presence_in(&dir, unix_now(), &process_alive),
        None => Presence::Absent,
    }
}

/// [`presence`] for the agent folder `dir`, at `now` (seconds since the Unix epoch), with
/// `alive` telling whether a process id is running.
pub fn presence_in(dir: &Path, now: i64, alive: &dyn Fn(u32) -> bool) -> Presence {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Presence::Absent;
    };
    let mut best: Option<(u8, i64, Presence)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Writers rename a finished temporary file into place; never read the temporary one.
        if name.starts_with('.') || name.ends_with(".tmp") || !name.ends_with(".json") {
            continue;
        }
        let Some((agent, updated)) = read_agent(&entry.path()) else {
            continue;
        };
        let fresh = updated.is_some_and(|updated| (now - updated).abs() < FRESH_SECS);
        let (rank, presence) = if fresh {
            (3, Presence::Running(agent))
        } else if alive(agent.pid) {
            let agent = Agent {
                fusion_busy: true,
                ..agent
            };
            (2, Presence::Unresponsive(agent))
        } else {
            (1, Presence::Gone)
        };
        let updated = updated.unwrap_or(i64::MIN);
        if best
            .as_ref()
            .is_none_or(|(r, u, _)| (rank, updated) > (*r, *u))
        {
            best = Some((rank, updated, presence));
        }
    }
    best.map_or(Presence::Absent, |(_, _, presence)| presence)
}

/// The exporter an agent file describes and when it last updated it, if the file is for the
/// exporter and speaks protocol 1.
fn read_agent(path: &Path) -> Option<(Agent, Option<i64>)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()?;
    if value.get("exporter")?.as_str()? != EXPORTER {
        return None;
    }
    if !value
        .get("protocols")?
        .as_array()?
        .iter()
        .any(|p| p.as_u64() == Some(PROTOCOL))
    {
        return None;
    }
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty())
    };
    let agent = Agent {
        jobs_dir: PathBuf::from(text("jobs_dir")?),
        features: value
            .get("features")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        version: text("version").unwrap_or_default(),
        busy: busy_of(&value),
        job: text("job"),
        fusion_version: text("fusion_version"),
        pid: value
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .unwrap_or(0),
        fusion_busy: false,
    };
    let updated = text("updated").and_then(|t| parse_time(&t));
    Some((agent, updated))
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

/// Seconds since the Unix epoch.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// Seconds since the Unix epoch of an ISO 8601 time with a UTC offset, such as
/// `2026-11-23T09:30:11.004000+00:00` (or `Z`). Fractions of a second are dropped.
pub fn parse_time(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let split = rest.rfind(['+', '-', 'Z'])?;
    let (clock, zone) = rest.split_at(split);
    let mut c = clock.split(':');
    let hour: i64 = c.next()?.parse().ok()?;
    let minute: i64 = c.next()?.parse().ok()?;
    let second: i64 = c.next()?.split('.').next()?.parse().ok()?;
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

/// Whether a process with this id is running.
pub fn process_alive(pid: u32) -> bool {
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
        // Cannot tell: never call a running Fusion gone.
        return true;
    };
    if cfg!(windows) {
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    } else {
        output.status.success()
    }
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

    fn file(dir: &Path, name: &str, age: i64, pid: u32, edit: impl Fn(&mut Value)) {
        let mut value = serde_json::json!({
            "protocol": 1, "protocols": [1], "agent": "fusion-bulk-export",
            "exporter": "fusion-bulk-export", "version": "0.3.5",
            "fusion_version": "2.0.21508", "fusion_running": true, "pid": pid,
            "started": stamp(NOW - 900), "updated": stamp(NOW - age), "busy": null,
            "job": null, "jobs_dir": "/Users/someone/.cad-export/jobs",
            "features": ["export", "cancel"]
        });
        edit(&mut value);
        std::fs::write(dir.join(name), value.to_string()).unwrap();
    }

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn look(dir: &Path, alive: bool) -> Presence {
        presence_in(dir, NOW, &|_| alive)
    }

    #[test]
    fn times_parse_with_offsets() {
        assert_eq!(parse_time("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_time("1970-01-02T00:00:00Z"), Some(86_400));
        assert_eq!(parse_time("1970-01-01T01:00:00.5+01:00"), Some(0));
        assert_eq!(parse_time("1970-01-01T00:00:00-02:30"), Some(9000));
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
        assert_eq!(
            agent.jobs_dir,
            PathBuf::from("/Users/someone/.cad-export/jobs")
        );
        assert_eq!(agent.features, ["export", "cancel"]);
        assert!(!agent.fusion_busy);
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
        assert!(agent.fusion_busy);
        assert_eq!(agent.pid, 41237);
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
        std::fs::write(d.path().join("broken.json"), "{").unwrap();
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
        assert!(matches!(look(d.path(), false), Presence::Running(a) if a.pid == 2));
    }

    #[test]
    fn find_reads_the_home_folder_only_through_presence() {
        // `find` is `presence` without the difference; both read ~/.cad-export/agents.
        assert!(agents_dir().is_none_or(
            |d| d.ends_with(".cad-export/agents") || d.ends_with(".cad-export\\agents")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn process_alive_tells_this_process_from_a_gone_one() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(0));
    }
}
