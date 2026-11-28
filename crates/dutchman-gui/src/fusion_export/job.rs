//! Job protocol 1, requester side: write a job file atomically, follow its status file,
//! cancel, clean up, and say in plain words what each reason code means.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::agent::Agent;

/// The name this program gives itself in a job (shown in the exporter's log).
pub const REQUESTER: &str = "Dutchman Convert";

/// What a job asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// List every hub, project and design into `inventory.json`.
    Inventory,
    /// Export the picked designs.
    Export,
}

impl Kind {
    /// The agent features a job of this kind uses; the job is not sent without them.
    pub fn required_features(self) -> &'static [&'static str] {
        match self {
            Kind::Inventory => &["inventory", "cancel"],
            Kind::Export => &[
                "export",
                "designs_by_id",
                "include_references",
                "step",
                "f3z_bundles",
                "older_versions",
                "cancel",
            ],
        }
    }
}

/// `2026-11-23T09:31:00+00:00` for now.
pub fn iso_now() -> String {
    iso_at(super::agent::now_secs())
}

/// The same for a time in seconds since 1970.
pub fn iso_at(secs: i64) -> String {
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+00:00",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A fresh job id: 32 hex digits in the usual 8-4-4-4-12 form.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut hash = Sha256::new();
    hash.update(nanos.to_le_bytes());
    hash.update(std::process::id().to_le_bytes());
    hash.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let digest: String = hash
        .finalize()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "{}-{}-{}-{}-{}",
        digest.get(0..8).unwrap_or_default(),
        digest.get(8..12).unwrap_or_default(),
        digest.get(12..16).unwrap_or_default(),
        digest.get(16..20).unwrap_or_default(),
        digest.get(20..32).unwrap_or_default()
    )
}

/// Writes `dir/name` atomically: a dot-prefixed temporary file in the same folder, flushed,
/// then renamed into place, so a reader never sees a part of it.
pub fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let temporary = dir.join(format!(
        ".{name}.{:x}{:x}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        std::fs::rename(&temporary, dir.join(name))
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// The folder of one picked set of designs, under `base`. The same designs always give the
/// same folder, so asking again resumes the earlier export.
pub fn export_folder(base: &Path, designs: &[String]) -> PathBuf {
    let mut sorted: Vec<&String> = designs.iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut hash = Sha256::new();
    for id in sorted {
        hash.update(id.as_bytes());
        hash.update([0]);
    }
    let name: String = hash
        .finalize()
        .iter()
        .take(5)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    base.join(format!("Export {name}"))
}

/// The job file for `kind`. `designs` is only used by an export.
pub fn build_job(id: &str, kind: Kind, output_dir: &Path, designs: &[String]) -> Value {
    let mut job = json!({
        "protocol": 1,
        "id": id,
        "requested": iso_now(),
        "requester": REQUESTER,
        "output_dir": output_dir.to_string_lossy(),
    });
    if let Some(fields) = job.as_object_mut() {
        match kind {
            Kind::Inventory => {
                fields.insert("action".into(), json!("inventory"));
            }
            Kind::Export => {
                fields.insert("action".into(), json!("export"));
                fields.insert("designs".into(), json!(designs));
                fields.insert("include_references".into(), json!(true));
                fields.insert("formats".into(), json!(["f3d", "step"]));
                fields.insert("rebuilt_f3z".into(), json!(true));
                fields.insert("older_versions".into(), json!("move"));
            }
        }
    }
    job
}

const FIELDS: &[&str] = &[
    "protocol",
    "id",
    "requested",
    "requester",
    "action",
    "output_dir",
    "designs",
    "include_references",
    "formats",
    "rebuilt_f3z",
    "older_versions",
    "tool_libraries",
    "designs_only",
    "max_designs",
    "memory_guard",
    "hub_timeout_seconds",
    "hubs",
    "projects",
    "folders",
];

/// A job id is 1 to 64 characters from `A-Z a-z 0-9 . _ -`, starts with a letter or digit
/// and does not end in `.status` or `.cancel`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !id.ends_with(".status")
        && !id.ends_with(".cancel")
}

/// Checks a job the way the exporter does, so an invalid job is never written.
pub fn validate(job: &Value) -> Result<(), String> {
    let fields: &Map<String, Value> = job.as_object().ok_or("a job is a JSON object")?;
    for key in fields.keys() {
        if !FIELDS.contains(&key.as_str()) && !key.starts_with("x-") {
            return Err(format!("unknown field {key}"));
        }
    }
    if fields.get("protocol").and_then(Value::as_i64) != Some(1) {
        return Err("protocol must be 1".into());
    }
    let id = fields.get("id").and_then(Value::as_str).unwrap_or_default();
    if !valid_id(id) {
        return Err("id is not a valid job id".into());
    }
    if !fields.get("requested").is_some_and(Value::is_string) {
        return Err("requested must be a time".into());
    }
    if fields.get("requester").is_some_and(|r| match r.as_str() {
        Some(text) => text.chars().count() > 200,
        None => true,
    }) {
        return Err("requester must be text of at most 200 characters".into());
    }
    let action = fields
        .get("action")
        .map_or(Some("export"), Value::as_str)
        .ok_or("action must be text")?;
    if !matches!(action, "export" | "inventory") {
        return Err("action must be export or inventory".into());
    }
    let output = fields
        .get("output_dir")
        .and_then(Value::as_str)
        .ok_or("output_dir is required")?;
    if !Path::new(output).is_absolute() {
        return Err("output_dir must be an absolute path".into());
    }
    let list = match fields.get("designs") {
        None => None,
        Some(Value::String(all)) if all == "all" => None,
        Some(Value::Array(ids)) => {
            if ids.is_empty() || !ids.iter().all(Value::is_string) {
                return Err("designs must be all or a list of design ids".into());
            }
            Some(ids)
        }
        Some(_) => return Err("designs must be all or a list of design ids".into()),
    };
    if action == "inventory" && list.is_some() {
        return Err("designs must be all for an inventory".into());
    }
    if list.is_some() {
        for key in ["hubs", "projects", "folders"] {
            if fields.contains_key(key) {
                return Err(format!("{key} cannot be used with a design list"));
            }
        }
    }
    let formats: Vec<&str> = match fields.get("formats") {
        None => vec!["f3d", "step"],
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().ok_or("formats must be text"))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("formats must be a list".into()),
    };
    if formats.is_empty()
        || formats.iter().any(|f| !matches!(*f, "f3d" | "step"))
        || formats
            .iter()
            .enumerate()
            .any(|(i, f)| formats.iter().take(i).any(|earlier| earlier == f))
    {
        return Err("formats must be f3d, step or both".into());
    }
    for key in [
        "include_references",
        "rebuilt_f3z",
        "designs_only",
        "memory_guard",
    ] {
        if fields.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(format!("{key} must be true or false"));
        }
    }
    if fields.get("rebuilt_f3z") == Some(&json!(true)) && !formats.contains(&"f3d") {
        return Err("rebuilt_f3z needs f3d in formats".into());
    }
    if let Some(older) = fields.get("older_versions") {
        if !matches!(older.as_str(), Some("move" | "keep" | "delete")) {
            return Err("older_versions must be move, keep or delete".into());
        }
    }
    Ok(())
}

/// Writes the job into the agent's jobs folder, after checking it and that the exporter
/// supports what it uses. Returns the job id.
pub fn submit(agent: &Agent, kind: Kind, job: &Value) -> Result<String, String> {
    if let Some(missing) = kind
        .required_features()
        .iter()
        .find(|feature| !agent.features.iter().any(|have| have == *feature))
    {
        return Err(format!(
            "This Fusion exporter is too old for this: it lacks {missing}. Update the Fusion exporter."
        ));
    }
    validate(job)?;
    let id = job
        .get("id")
        .and_then(Value::as_str)
        .ok_or("a job needs an id")?
        .to_owned();
    std::fs::create_dir_all(&agent.jobs_dir)
        .map_err(|why| format!("Cannot use the exporter's jobs folder: {why}"))?;
    let bytes = serde_json::to_vec_pretty(job).map_err(|why| why.to_string())?;
    atomic_write(&agent.jobs_dir, &format!("{id}.json"), &bytes)
        .map_err(|why| format!("Cannot write the export job: {why}"))?;
    Ok(id)
}

/// Asks the exporter to cancel a job: creates the empty `<id>.cancel`.
pub fn cancel(jobs_dir: &Path, id: &str) -> std::io::Result<()> {
    std::fs::File::create(jobs_dir.join(format!("{id}.cancel"))).map(|_| ())
}

/// Deletes the job's three files; the requester owns them once the job is final.
pub fn cleanup(jobs_dir: &Path, id: &str) {
    for name in [
        format!("{id}.json"),
        format!("{id}.status.json"),
        format!("{id}.cancel"),
    ] {
        let _ = std::fs::remove_file(jobs_dir.join(name));
    }
}

/// What the status file says, as far as this program uses it. Fields it does not know are
/// ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub reason: Option<String>,
    pub message: String,
    pub done: u64,
    pub total: Option<u64>,
    pub current_number: Option<u64>,
    pub exported: u64,
    pub up_to_date: u64,
    pub failed: u64,
    pub manifest: Option<PathBuf>,
    pub inventory: Option<PathBuf>,
}

/// The exporter's job protocol 1 writes `cancelled`; later versions write `canceled`. Both are
/// read as `canceled`.
fn american(word: &str) -> String {
    if word == "cancelled" {
        "canceled"
    } else {
        word
    }
    .to_owned()
}

impl Status {
    /// `done`, `failed` and `canceled` never change again.
    pub fn is_final(&self) -> bool {
        matches!(self.state.as_str(), "done" | "failed" | "canceled")
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let count = |parent: &str, key: &str| {
            value
                .get(parent)
                .and_then(|p| p.get(key))
                .and_then(Value::as_u64)
        };
        let path = |key: &str| value.get(key).and_then(Value::as_str).map(PathBuf::from);
        Some(Self {
            state: american(value.get("state")?.as_str()?),
            reason: value.get("reason").and_then(Value::as_str).map(american),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            done: count("progress", "done").unwrap_or(0),
            total: count("progress", "total"),
            current_number: count("progress", "current_number"),
            exported: count("counts", "exported").unwrap_or(0),
            up_to_date: count("counts", "up_to_date").unwrap_or(0),
            failed: count("counts", "failed").unwrap_or(0),
            manifest: path("manifest"),
            inventory: path("inventory"),
        })
    }
}

/// The job's status; `None` while the exporter has not claimed it yet.
pub fn read_status(jobs_dir: &Path, id: &str) -> Option<Status> {
    let text = super::agent::read_small(
        &jobs_dir.join(format!("{id}.status.json")),
        super::agent::SMALL_FILE,
    )
    .ok()??;
    Status::from_value(&serde_json::from_str(&text).ok()?)
}

/// What the person can do about a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing to do.
    Nothing,
    /// Fix the cause, then run the same job again (it resumes).
    TryAgain,
    /// The list is out of date: list the designs again and choose again.
    ChooseAgain,
    /// Install a newer Fusion exporter.
    UpdateExporter,
    /// A bug in Dutchman's request.
    Report,
}

/// A reason code in plain words, with what to do. A reason this program does not know is
/// treated as `error`.
pub fn reason_info(reason: &str, message: &str) -> (String, Next) {
    let (text, next): (&str, Next) = match reason {
        "invalid_job" => (
            "The Fusion exporter did not accept Dutchman's request. This is a fault in Dutchman: please report it.",
            Next::Report,
        ),
        "unsupported_protocol" => (
            "This Fusion exporter does not understand Dutchman's requests. Update the Fusion exporter, then try again.",
            Next::UpdateExporter,
        ),
        "folder_busy" => (
            "Another export is using this export folder. Wait until it finishes, then try again.",
            Next::TryAgain,
        ),
        "output_unusable" => (
            "Fusion cannot write to Dutchman's export folder. Check that the folder's drive is connected and that you may write to it, then try again.",
            Next::TryAgain,
        ),
        "not_found" => (
            "A design you chose is no longer in your Fusion account. List your designs again and choose again.",
            Next::ChooseAgain,
        ),
        "not_signed_in" => (
            "Fusion is not signed in to your account. Sign in to Fusion, then try again.",
            Next::TryAgain,
        ),
        "failing" => (
            "Five designs in a row failed, usually because Fusion went offline or signed out. Check Fusion's sign-in, then try again; the export carries on where it stopped.",
            Next::TryAgain,
        ),
        "disk_full" => (
            "The disk is full. Free some space, then try again; the export carries on where it stopped.",
            Next::TryAgain,
        ),
        "output_missing" => (
            "The export folder went away, perhaps a disconnected drive. Reconnect it, then try again.",
            Next::TryAgain,
        ),
        "output_unwritable" => (
            "The export folder cannot be written to. Check its permissions, then try again.",
            Next::TryAgain,
        ),
        "memory" => (
            "Fusion ran short of memory and stopped between designs. Close Fusion and open it again (save your work first), then try again; the export carries on where it stopped.",
            Next::TryAgain,
        ),
        "account_changed" => (
            "A design changed in your Fusion account while it was being exported. Try again.",
            Next::TryAgain,
        ),
        "interrupted" => (
            "Fusion closed or crashed during the export. Open Fusion and try again; the export carries on where it stopped. If it keeps stopping, one design may be the cause.",
            Next::TryAgain,
        ),
        "canceled" | "cancelled" => (
            "The export was stopped. Everything finished so far is kept.",
            Next::Nothing,
        ),
        // Found by Dutchman itself, not reported by the exporter.
        "agent_gone" => (
            "Fusion closed, or the Fusion exporter stopped, and did not come back within a minute. Open Fusion with the exporter switched on, then try again; the export carries on where it stopped.",
            Next::TryAgain,
        ),
        "no_answer" => (
            "The Fusion exporter did not take the request within two minutes. Check that Fusion is open with the exporter switched on, then try again.",
            Next::TryAgain,
        ),
        _ => {
            // The exporter's message names the error type, never a design or path.
            let text = if message.is_empty() {
                "The Fusion exporter stopped with an unexpected error. Try again; if it happens again, report it.".to_owned()
            } else {
                format!("The Fusion exporter stopped with an unexpected error ({message}). Try again; if it happens again, report this error.")
            };
            return (text, Next::TryAgain);
        }
    };
    (text.to_owned(), next)
}

/// A message from a job being followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// The newest status, sent when it changes.
    Status(Status),
    /// The job is final; its three files are already deleted.
    Final(Status),
    /// The exporter never claimed the job and nothing more will come.
    Lost(String),
}

/// Follows one job on its own thread: sends each new status, writes `.cancel` once when
/// `stop` is set, ends quietly when `abandon` is set, and after a final state deletes the job's files and sends [`Update::Final`].
pub fn follow(
    jobs_dir: PathBuf,
    id: String,
    stop: Arc<AtomicBool>,
    abandon: Arc<AtomicBool>,
    poll: Duration,
    sender: Sender<(String, Update)>,
    repaint: impl Fn() + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut last: Option<Status> = None;
        let mut cancelled = false;
        loop {
            if abandon.load(Ordering::SeqCst) {
                return;
            }
            if stop.load(Ordering::SeqCst) && !cancelled {
                cancelled = cancel(&jobs_dir, &id).is_ok();
            }
            if let Some(status) = read_status(&jobs_dir, &id) {
                if status.is_final() {
                    cleanup(&jobs_dir, &id);
                    let _ = sender.send((id.clone(), Update::Final(status)));
                    repaint();
                    return;
                }
                if last.as_ref() != Some(&status) {
                    let _ = sender.send((id.clone(), Update::Status(status.clone())));
                    repaint();
                    last = Some(status);
                }
            } else if !jobs_dir.join(format!("{id}.json")).exists() {
                // The job file went away before any answer: withdrawn by someone else.
                let _ = sender.send((
                    id.clone(),
                    Update::Lost(
                        "The export request disappeared before the Fusion exporter took it.".into(),
                    ),
                ));
                repaint();
                return;
            }
            std::thread::sleep(poll);
        }
    });
}
