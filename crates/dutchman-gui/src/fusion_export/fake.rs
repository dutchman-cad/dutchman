//! A stand-in for the Fusion exporter add-in, for tests and screenshots: it writes the agent
//! file and answers jobs with status files exactly as the protocol describes (atomically).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::agent::Agent;
use super::job::{atomic_write, iso_now, validate};

pub const FEATURES: &[&str] = &[
    "export",
    "inventory",
    "designs_by_id",
    "include_references",
    "hubs",
    "step",
    "step_only",
    "f3z_bundles",
    "tool_libraries",
    "older_versions",
    "cancel",
];

/// The add-in's side of `~/.cad-export`, rooted at `cad`.
#[derive(Clone)]
pub struct FakeAddin {
    pub cad: PathBuf,
}

/// What a fake export ends in.
#[derive(Clone, Debug)]
pub struct Ending {
    pub state: &'static str,
    pub reason: Option<&'static str>,
    pub failed: u64,
    pub message: String,
}

impl Ending {
    pub fn done() -> Self {
        Self {
            state: "done",
            reason: None,
            failed: 0,
            message: "Exported all designs.".into(),
        }
    }
    pub fn failed_with(reason: &'static str) -> Self {
        Self {
            state: "failed",
            reason: Some(reason),
            failed: 0,
            message: "Stopped.".into(),
        }
    }
}

impl FakeAddin {
    pub fn new(cad: &Path) -> Self {
        std::fs::create_dir_all(cad.join("agents")).unwrap();
        std::fs::create_dir_all(cad.join("jobs")).unwrap();
        Self {
            cad: cad.to_owned(),
        }
    }

    pub fn jobs_dir(&self) -> PathBuf {
        self.cad.join("jobs")
    }

    /// Writes the agent file with the given features.
    pub fn announce(&self, features: &[&str], busy: Option<&str>) {
        self.announce_at(features, busy, &iso_now(), std::process::id());
    }

    /// As `announce`, with the heartbeat time and the process id of Fusion given.
    pub fn announce_at(&self, features: &[&str], busy: Option<&str>, updated: &str, pid: u32) {
        let agent = json!({
            "protocol": 1, "protocols": [1], "agent": "fusion-bulk-export",
            "exporter": "fusion-bulk-export", "version": "0.3.5",
            "fusion_version": "2.0.21508", "fusion_running": true, "pid": pid,
            "started": iso_now(), "updated": updated, "busy": busy, "job": null,
            "jobs_dir": self.jobs_dir().to_string_lossy(), "features": features,
        });
        atomic_write(
            &self.cad.join("agents"),
            "fusion-bulk-export.json",
            &serde_json::to_vec_pretty(&agent).unwrap(),
        )
        .unwrap();
    }

    pub fn agent(&self) -> Agent {
        super::agent::read_minimal(&super::agent::agent_file(&self.cad)).unwrap()
    }

    /// Ids of job files nobody has claimed (no status file yet), oldest name first.
    pub fn unclaimed(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.jobs_dir())
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| {
                !name.starts_with('.')
                    && !name.ends_with(".tmp")
                    && name.ends_with(".json")
                    && !name.ends_with(".status.json")
            })
            .map(|name| name.trim_end_matches(".json").to_owned())
            .filter(|id| !self.jobs_dir().join(format!("{id}.status.json")).exists())
            .collect();
        ids.sort();
        ids
    }

    pub fn job(&self, id: &str) -> Value {
        serde_json::from_slice(&std::fs::read(self.jobs_dir().join(format!("{id}.json"))).unwrap())
            .unwrap()
    }

    /// Writes a complete status file.
    #[allow(clippy::too_many_arguments)]
    pub fn status(
        &self,
        id: &str,
        state: &str,
        reason: Option<&str>,
        message: &str,
        progress: (u64, Option<u64>, Option<u64>),
        exported: u64,
        failed: u64,
        output: &Path,
    ) {
        let finished = matches!(state, "done" | "failed" | "cancelled");
        let mut status = json!({
            "protocol": 1, "id": id, "agent": "fusion-bulk-export", "agent_version": "0.3.5",
            "agent_pid": std::process::id(), "state": state, "reason": reason, "message": message,
            "claimed": iso_now(), "started": iso_now(), "updated": iso_now(),
            "finished": if finished { json!(iso_now()) } else { Value::Null },
            "progress": {"done": progress.0, "total": progress.1, "current": null, "current_number": progress.2},
            "counts": {"exported": exported, "up_to_date": 0, "files": 0, "files_up_to_date": 0,
                       "skipped": 0, "unsupported": 0, "failed": failed},
            "output_dir": output.to_string_lossy(),
            "manifest": if state == "done" { json!(output.join("manifest.json").to_string_lossy()) } else { Value::Null },
            "result": null,
            "exit_code": if finished { json!(0) } else { Value::Null },
        });
        if self.job(id).get("action") == Some(&json!("inventory")) {
            status["inventory"] = if state == "done" {
                json!(output.join("inventory.json").to_string_lossy())
            } else {
                Value::Null
            };
        }
        atomic_write(
            &self.jobs_dir(),
            &format!("{id}.status.json"),
            &serde_json::to_vec_pretty(&status).unwrap(),
        )
        .unwrap();
    }

    /// Claims a job as the add-in does: an invalid one fails at once with `invalid_job`,
    /// naming the problem; a valid one is `queued`. Returns whether it was valid.
    pub fn claim(&self, id: &str) -> bool {
        let job = self.job(id);
        let output = job
            .get("output_dir")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_default();
        match validate(&job) {
            Ok(()) => {
                self.status(
                    id,
                    "queued",
                    None,
                    "Waiting for its turn.",
                    (0, None, None),
                    0,
                    0,
                    &output,
                );
                true
            }
            Err(why) => {
                self.status(
                    id,
                    "failed",
                    Some("invalid_job"),
                    &why,
                    (0, None, None),
                    0,
                    0,
                    &output,
                );
                false
            }
        }
    }

    pub fn cancel_requested(&self, id: &str) -> bool {
        self.jobs_dir().join(format!("{id}.cancel")).exists()
    }

    /// Writes `inventory.json` into the job's output folder and finishes the job.
    pub fn finish_inventory(&self, id: &str, inventory: &Value) {
        let output = PathBuf::from(self.job(id)["output_dir"].as_str().unwrap());
        std::fs::create_dir_all(&output).unwrap();
        atomic_write(
            &output,
            "inventory.json",
            &serde_json::to_vec(inventory).unwrap(),
        )
        .unwrap();
        self.status(
            id,
            "done",
            None,
            "Listed 1 hub.",
            (1, Some(1), None),
            0,
            0,
            &output,
        );
    }
}

/// A runnable fake: claims jobs as they appear, answers an inventory at once and walks an
/// export through its designs one by one.
pub struct Service {
    pub stop: Arc<AtomicBool>,
    /// While set, an export stays running after `hold_at` designs.
    pub hold: Arc<AtomicBool>,
    pub hold_at: Arc<AtomicUsize>,
    pub exports_seen: Arc<AtomicUsize>,
}

impl Service {
    pub fn spawn(addin: FakeAddin, inventory: Value, ending: Ending, step: Duration) -> Self {
        let service = Service {
            stop: Arc::new(AtomicBool::new(false)),
            hold: Arc::new(AtomicBool::new(false)),
            hold_at: Arc::new(AtomicUsize::new(usize::MAX)),
            exports_seen: Arc::new(AtomicUsize::new(0)),
        };
        let (stop, hold, hold_at, seen) = (
            service.stop.clone(),
            service.hold.clone(),
            service.hold_at.clone(),
            service.exports_seen.clone(),
        );
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                for id in addin.unclaimed() {
                    if !addin.claim(&id) {
                        continue;
                    }
                    let job = addin.job(&id);
                    let output = PathBuf::from(job["output_dir"].as_str().unwrap());
                    if job["action"] == "inventory" {
                        addin.finish_inventory(&id, &inventory);
                        continue;
                    }
                    seen.fetch_add(1, Ordering::SeqCst);
                    let total = job["designs"].as_array().map_or(0, Vec::len) as u64 + 1;
                    for n in 1..=total {
                        if addin.cancel_requested(&id) {
                            addin.status(
                                &id,
                                "cancelled",
                                Some("cancelled"),
                                "Cancelled.",
                                (n - 1, Some(total), None),
                                n - 1,
                                0,
                                &output,
                            );
                            break;
                        }
                        while hold.load(Ordering::SeqCst)
                            && n as usize > hold_at.load(Ordering::SeqCst)
                        {
                            if addin.cancel_requested(&id) || stop.load(Ordering::SeqCst) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        addin.status(
                            &id,
                            "running",
                            None,
                            &format!("Exporting design {n} of {total}."),
                            (n - 1, Some(total), Some(n)),
                            n - 1,
                            0,
                            &output,
                        );
                        std::thread::sleep(step);
                        if n == total {
                            std::fs::create_dir_all(&output).unwrap();
                            let message = ending.message.clone();
                            addin.status(
                                &id,
                                ending.state,
                                ending.reason,
                                &message,
                                (total, Some(total), None),
                                if ending.state == "done" {
                                    total - ending.failed.min(total)
                                } else {
                                    0
                                },
                                ending.failed,
                                &output,
                            );
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        service
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// A synthetic account: one hub, two projects, a folder tree, with long German names for
/// the screenshots that test umlauts and wrapping.
pub fn sample_inventory(german: bool) -> Value {
    let (hub, project_a, project_b, folder, names): (&str, &str, &str, &str, [&str; 4]) = if german
    {
        (
            "Konstruktionsabteilung Süd",
            "Getriebegehäuse Baureihe 7",
            "Vorrichtungen und Spannmittel Größe XL",
            "Gehäuseteile für Straßenfahrzeuge",
            [
                "Lagerdeckel mit Befestigungsflansch Rückseite",
                "Gehäusehälfte oben mit Ölführungsrippen",
                "Wellendichtring Aufnahme groß",
                "Zahnradpaket Übersetzung 3,5 zu 1",
            ],
        )
    } else {
        (
            "Acme Engineering",
            "Gearbox 7",
            "Fixtures",
            "Housings",
            ["Bearing cap", "Housing top", "Seal holder", "Gear set"],
        )
    };
    let file = |id: &str, name: &str, path: Vec<&str>, ext: &str| {
        json!({"id": format!("urn:adsk.wipprod:dm.lineage:{id}"), "name": name, "path": path,
               "folder_ids": path.iter().map(|_| Value::Null).collect::<Vec<_>>(), "extension": ext, "version": 3})
    };
    json!({
        "schema_id": "cad-export-inventory/1",
        "hubs": {"hub-1": {"name": hub, "projects": {
            "project-1": {"name": project_a, "files": [
                file("a1", names[0], vec![folder], "f3d"),
                file("a2", names[1], vec![folder], "f3d"),
                file("a3", names[2], vec![], "f3d"),
                file("a4", "Datasheet", vec![], "pdf"),
            ]},
            "project-2": {"name": project_b, "files": [
                file("b1", names[3], vec!["Jigs", "Drilling"], "f3d"),
            ]},
        }}},
        "unreadable": [], "stopped": false,
        "counts": {"hubs": 1, "projects": 2, "files": 5, "designs": 5,
                   "links_out_of_date": 0, "links_unreadable": 0, "unreadable": 0},
    })
}
