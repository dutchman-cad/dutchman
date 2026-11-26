//! The job side of Export from Fusion against a fake add-in that follows the protocol.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::fake::{sample_inventory, Ending, FakeAddin, Service, FEATURES};
use super::inventory::Inventory;
use super::job::{self, Kind, Next, Update};
use super::{Env, Panel, Stage};

const FAST: Duration = Duration::from_millis(10);

fn setup() -> (tempfile::TempDir, FakeAddin) {
    let dir = tempfile::tempdir().unwrap();
    let addin = FakeAddin::new(&dir.path().join(".cad-export"));
    addin.announce(FEATURES, None);
    (dir, addin)
}

fn designs() -> Vec<String> {
    vec![
        "urn:adsk.wipprod:dm.lineage:a1".into(),
        "urn:adsk.wipprod:dm.lineage:a2".into(),
    ]
}

fn export_job(dir: &Path) -> (String, Value) {
    let id = job::new_id();
    let request = job::build_job(&id, Kind::Export, &dir.join("out"), &designs());
    (id, request)
}

#[test]
fn a_job_moves_from_queued_to_running_to_done_and_its_files_are_cleaned_up() {
    let (dir, addin) = setup();
    let (id, request) = export_job(dir.path());
    let sent = job::submit(&addin.agent(), Kind::Export, &request).unwrap();
    assert_eq!(sent, id);
    assert!(job::read_status(&addin.jobs_dir(), &id).is_none());
    let (sender, receiver) = channel();
    job::follow(
        addin.jobs_dir(),
        id.clone(),
        Default::default(),
        Default::default(),
        FAST,
        sender,
        || {},
    );
    let out = dir.path().join("out");
    assert!(addin.claim(&id));
    let next = || receiver.recv_timeout(Duration::from_secs(5)).unwrap().1;
    let Update::Status(queued) = next() else {
        panic!()
    };
    assert_eq!(queued.state, "queued");
    addin.status(
        &id,
        "running",
        None,
        "Exporting design 1 of 3.",
        (0, Some(3), Some(1)),
        0,
        0,
        &out,
    );
    let Update::Status(running) = next() else {
        panic!()
    };
    assert_eq!(
        (running.done, running.total, running.current_number),
        (0, Some(3), Some(1))
    );
    addin.status(
        &id,
        "done",
        None,
        "Exported 3 designs.",
        (3, Some(3), None),
        3,
        0,
        &out,
    );
    let Update::Final(done) = next() else {
        panic!()
    };
    assert_eq!(
        (done.state.as_str(), done.exported, done.failed),
        ("done", 3, 0)
    );
    // After a final state the job's three files are gone.
    for name in [
        format!("{id}.json"),
        format!("{id}.status.json"),
        format!("{id}.cancel"),
    ] {
        assert!(!addin.jobs_dir().join(name).exists());
    }
}

#[test]
fn the_job_is_written_as_the_spec_says_and_never_seen_half_written() {
    let (dir, addin) = setup();
    let (id, request) = export_job(dir.path());
    job::submit(&addin.agent(), Kind::Export, &request).unwrap();
    let written = addin.job(&id);
    assert_eq!(written["protocol"], 1);
    assert_eq!(written["action"], "export");
    assert_eq!(written["designs"], json!(designs()));
    assert_eq!(written["include_references"], true);
    assert_eq!(written["formats"], json!(["f3d", "step"]));
    assert_eq!(written["rebuilt_f3z"], true);
    assert_eq!(written["older_versions"], "move");
    assert!(written["requested"].as_str().unwrap().ends_with("+00:00"));
    // The temporary file is renamed away: only the final name is left.
    let names: Vec<String> = std::fs::read_dir(addin.jobs_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec![format!("{id}.json")]);
    assert_eq!(addin.unclaimed(), vec![id]);
}

#[test]
fn an_inventory_job_asks_for_the_list_and_nothing_else() {
    let (dir, _addin) = setup();
    let request = job::build_job("inv-1", Kind::Inventory, dir.path(), &[]);
    assert_eq!(request["action"], "inventory");
    assert!(request.get("designs").is_none());
    job::validate(&request).unwrap();
}

#[test]
fn an_invalid_job_is_refused_before_it_is_written() {
    let (dir, addin) = setup();
    let (_, good) = export_job(dir.path());
    let broken = |edit: &dyn Fn(&mut Value)| {
        let mut request = good.clone();
        edit(&mut request);
        request
    };
    for request in [
        broken(&|j| j["surprise"] = json!(1)),
        broken(&|j| j["id"] = json!("bad id/../x")),
        broken(&|j| j["id"] = json!("x.status")),
        broken(&|j| j["output_dir"] = json!("relative/path")),
        broken(&|j| j["designs"] = json!([])),
        broken(&|j| j["formats"] = json!(["step"])),
        broken(&|j| j["formats"] = json!(["f3d", "f3d"])),
        broken(&|j| j["protocol"] = json!(2)),
        broken(&|j| j["hubs"] = json!(["h"])),
        broken(&|j| j["older_versions"] = json!("shred")),
    ] {
        assert!(submit_ok(&addin, &request).is_err(), "{request}");
    }
    assert!(addin.unclaimed().is_empty());
    job::validate(&good).unwrap();
}

fn submit_ok(addin: &FakeAddin, request: &Value) -> Result<String, String> {
    job::submit(&addin.agent(), Kind::Export, request)
}

#[test]
fn an_invalid_job_that_got_through_is_answered_failed_with_invalid_job() {
    let (dir, addin) = setup();
    let id = "hand-written";
    std::fs::write(
        addin.jobs_dir().join(format!("{id}.json")),
        json!({"protocol": 1, "id": id, "requested": "x", "output_dir": dir.path(), "wat": 1})
            .to_string(),
    )
    .unwrap();
    assert!(!addin.claim(id));
    let status = job::read_status(&addin.jobs_dir(), id).unwrap();
    assert_eq!(
        (status.state.as_str(), status.reason.as_deref()),
        ("failed", Some("invalid_job"))
    );
    assert!(status.is_final());
}

#[test]
fn an_exporter_without_a_needed_feature_is_not_sent_the_job() {
    let (dir, addin) = setup();
    addin.announce(&["export", "cancel"], None);
    let (_, request) = export_job(dir.path());
    let why = job::submit(&addin.agent(), Kind::Export, &request).unwrap_err();
    assert!(why.contains("designs_by_id"), "{why}");
    assert!(addin.unclaimed().is_empty());
}

#[test]
fn every_reason_code_has_plain_words_and_an_action() {
    let table = [
        ("invalid_job", Next::Report),
        ("unsupported_protocol", Next::UpdateExporter),
        ("folder_busy", Next::TryAgain),
        ("output_unusable", Next::TryAgain),
        ("not_found", Next::ChooseAgain),
        ("not_signed_in", Next::TryAgain),
        ("failing", Next::TryAgain),
        ("disk_full", Next::TryAgain),
        ("output_missing", Next::TryAgain),
        ("output_unwritable", Next::TryAgain),
        ("memory", Next::TryAgain),
        ("account_changed", Next::TryAgain),
        ("interrupted", Next::TryAgain),
        ("cancelled", Next::Nothing),
        ("agent_gone", Next::TryAgain),
        ("no_answer", Next::TryAgain),
        ("error", Next::TryAgain),
        ("from_a_future_release", Next::TryAgain),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (reason, next) in table {
        let (text, got) = job::reason_info(reason, "OSError");
        assert_eq!(got, next, "{reason}");
        assert!(text.len() > 30 && !text.contains('_'), "{reason}: {text}");
        if reason == "error" || reason == "from_a_future_release" {
            assert!(text.contains("unexpected error (OSError)"), "{text}");
        } else {
            assert!(seen.insert(text), "{reason} repeats another message");
        }
    }
}

#[test]
fn a_queued_job_is_cancelled_without_running() {
    let (dir, addin) = setup();
    let (id, request) = export_job(dir.path());
    job::submit(&addin.agent(), Kind::Export, &request).unwrap();
    assert!(addin.claim(&id));
    let (sender, receiver) = channel();
    let stop = Arc::new(Default::default());
    job::follow(
        addin.jobs_dir(),
        id.clone(),
        Arc::clone(&stop),
        Default::default(),
        FAST,
        sender,
        || {},
    );
    let Update::Status(status) = receiver.recv_timeout(Duration::from_secs(5)).unwrap().1 else {
        panic!()
    };
    assert_eq!(status.state, "queued");
    stop.store(true, Ordering::SeqCst);
    let started = Instant::now();
    while !addin.cancel_requested(&id) {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(FAST);
    }
    // The add-in cancels a queued job without running it.
    addin.status(
        &id,
        "cancelled",
        Some("cancelled"),
        "Cancelled.",
        (0, None, None),
        0,
        0,
        &dir.path().join("out"),
    );
    let Update::Final(status) = receiver.recv_timeout(Duration::from_secs(5)).unwrap().1 else {
        panic!()
    };
    assert_eq!(status.state, "cancelled");
    assert!(
        !addin.cancel_requested(&id),
        "the .cancel file is cleaned up"
    );
}

fn panel_for(dir: &Path) -> (Panel, eframe::egui::Context) {
    let mut panel = Panel::new();
    panel.preview(Env::for_test(dir));
    panel.open = true;
    (panel, eframe::egui::Context::default())
}

fn wait_for(
    panel: &mut Panel,
    ctx: &eframe::egui::Context,
    what: &str,
    mut ready: impl FnMut(&Panel) -> bool,
) -> Option<std::path::PathBuf> {
    let started = Instant::now();
    let mut folder = None;
    loop {
        folder = folder.or(panel.poll(ctx));
        if ready(panel) {
            return folder;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what}"
        );
        std::thread::sleep(FAST);
    }
}

fn pick_all(panel: &mut Panel) {
    let ids = panel.inventory.as_ref().unwrap().matching_ids("");
    panel.picked.extend(ids);
}

#[test]
fn the_panel_lists_designs_exports_the_picked_ones_and_hands_the_folder_on() {
    let (dir, addin) = setup();
    let service = Service::spawn(addin.clone(), sample_inventory(false), Ending::done(), FAST);
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    assert_eq!(panel.inventory.as_ref().unwrap().count(), 4);
    pick_all(&mut panel);
    panel.export_picked();
    assert_eq!(panel.stage(), Stage::Exporting);
    let folder = wait_for(&mut panel, &ctx, "the export", |p| {
        p.running.is_none() && p.stage() != Stage::Exporting
    });
    let folder = folder.expect("a clean export is handed to the converter");
    assert_eq!(
        folder,
        job::export_folder(
            &dir.path().join("Dutchman Fusion exports/exports"),
            &panel.export_of.as_ref().unwrap().0
        )
    );
    assert!(folder.is_dir());
    assert_eq!(service.exports_seen.load(Ordering::SeqCst), 1);
    assert!(
        std::fs::read_dir(addin.jobs_dir())
            .unwrap()
            .next()
            .is_none(),
        "no job files left"
    );
    // The same picks are the same folder: asking again resumes.
    let again = job::export_folder(
        &dir.path().join("Dutchman Fusion exports/exports"),
        &panel.export_of.as_ref().unwrap().0,
    );
    assert_eq!(folder, again);
}

#[test]
fn done_with_failed_designs_offers_try_again_with_the_same_job_folder() {
    let (dir, addin) = setup();
    let mut ending = Ending::done();
    ending.failed = 2;
    let _service = Service::spawn(addin.clone(), sample_inventory(false), ending, FAST);
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    pick_all(&mut panel);
    panel.export_picked();
    let folder = wait_for(&mut panel, &ctx, "the end", |p| {
        p.stage() == Stage::Finished
    });
    assert!(
        folder.is_none(),
        "failed designs are not converted without a choice"
    );
    let ended = panel.ended.as_ref().unwrap();
    assert_eq!(
        (ended.status.state.as_str(), ended.status.failed),
        ("done", 2)
    );
    let first = panel.export_of.clone().unwrap().1;
    panel.start_export();
    assert_eq!(panel.stage(), Stage::Exporting);
    assert_eq!(panel.export_of.clone().unwrap().1, first);
    wait_for(&mut panel, &ctx, "the second end", |p| {
        p.stage() == Stage::Finished
    });
    // Convert what exported hands the folder on.
    assert_eq!(panel.convert_what_exported(), Some(first));
}

#[test]
fn every_failure_reason_ends_in_its_plain_message() {
    for reason in [
        "folder_busy",
        "not_found",
        "not_signed_in",
        "memory",
        "interrupted",
        "disk_full",
        "failing",
        "invalid_job",
        "unsupported_protocol",
    ] {
        let (dir, addin) = setup();
        let _service = Service::spawn(
            addin.clone(),
            sample_inventory(false),
            Ending::failed_with(reason),
            FAST,
        );
        let (mut panel, ctx) = panel_for(dir.path());
        wait_for(&mut panel, &ctx, "the list", |p| {
            p.stage() == Stage::Picking
        });
        pick_all(&mut panel);
        panel.export_picked();
        wait_for(&mut panel, &ctx, reason, |p| p.stage() == Stage::Finished);
        let status = &panel.ended.as_ref().unwrap().status;
        assert_eq!(
            (status.state.as_str(), status.reason.as_deref()),
            ("failed", Some(reason))
        );
    }
}

#[test]
fn stop_while_running_cancels_and_keeps_what_was_exported() {
    let (dir, addin) = setup();
    let service = Service::spawn(
        addin.clone(),
        sample_inventory(false),
        Ending::done(),
        Duration::from_millis(30),
    );
    service.hold_at.store(1, Ordering::SeqCst);
    service.hold.store(true, Ordering::SeqCst);
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    pick_all(&mut panel);
    panel.export_picked();
    wait_for(&mut panel, &ctx, "progress", |p| {
        p.running
            .as_ref()
            .and_then(|r| r.status.as_ref())
            .is_some_and(|s| s.state == "running")
    });
    panel.stop();
    assert!(panel.running.as_ref().unwrap().stopping);
    wait_for(&mut panel, &ctx, "cancelled", |p| {
        p.stage() == Stage::Finished
    });
    assert_eq!(panel.ended.as_ref().unwrap().status.state, "cancelled");
    assert!(std::fs::read_dir(addin.jobs_dir())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn the_panel_notices_an_exporter_that_appears_later() {
    let (dir, addin) = setup();
    std::fs::remove_file(addin.cad.join("agents/fusion-bulk-export.json")).unwrap();
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "a first look", |p| {
        p.last_agent_check.is_some()
    });
    assert_eq!(panel.stage(), Stage::NoAgent);
    addin.announce(FEATURES, None);
    let _service = Service::spawn(addin.clone(), sample_inventory(false), Ending::done(), FAST);
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    assert!(panel.error.is_none());
}

#[test]
fn the_list_of_designs_nests_by_hub_project_and_folder_and_searches() {
    let inventory = Inventory::from_value(&sample_inventory(true)).unwrap();
    assert_eq!(inventory.count(), 4, "the PDF is not a design");
    let hub = &inventory.hubs[0];
    assert_eq!(hub.name, "Konstruktionsabteilung Süd");
    assert_eq!(hub.projects.len(), 2);
    let gearbox = hub
        .projects
        .iter()
        .find(|p| p.name == "Getriebegehäuse Baureihe 7")
        .unwrap();
    assert_eq!(gearbox.root.designs.len(), 1);
    assert_eq!(
        gearbox.root.folders[0].name,
        "Gehäuseteile für Straßenfahrzeuge"
    );
    assert_eq!(gearbox.root.folders[0].designs.len(), 2);
    assert_eq!(inventory.matching_ids("").len(), 4);
    assert_eq!(inventory.matching_ids("lagerdeckel").len(), 1);
    assert_eq!(
        inventory.matching_ids("STRAßENFAHRZEUGE").len(),
        2,
        "a folder's name matches its designs"
    );
    assert_eq!(
        inventory.matching_ids("getriebegehäuse").len(),
        3,
        "a project's name matches its designs"
    );
    assert_eq!(inventory.matching_ids("konstruktionsabteilung").len(), 4);
    assert!(inventory.matching_ids("nothing like this").is_empty());
}

#[test]
fn an_inventory_of_an_unknown_schema_version_is_refused_in_plain_words() {
    let mut newer = sample_inventory(false);
    newer["schema_id"] = json!("cad-export-inventory/2");
    let why = Inventory::from_value(&newer).unwrap_err();
    assert!(
        why.contains("newer form") && why.contains("Update Dutchman"),
        "{why}"
    );
    newer["schema_id"] = json!("something-else/1");
    assert!(Inventory::from_value(&newer).is_err());
    // A file without the id is an older one with the same layout.
    let mut older = sample_inventory(false);
    older.as_object_mut().unwrap().remove("schema_id");
    assert_eq!(Inventory::from_value(&older).unwrap().count(), 4);
    // A stopped listing is flagged.
    let mut stopped = sample_inventory(false);
    stopped["stopped"] = json!(true);
    assert!(Inventory::from_value(&stopped).unwrap().stopped);
}

#[test]
fn the_inventory_must_have_hubs() {
    assert!(Inventory::from_value(&json!({"nope": 1})).is_err());
    assert_eq!(
        Inventory::from_value(&json!({"hubs": {}})).unwrap().count(),
        0
    );
}

#[test]
fn the_export_folder_depends_only_on_the_set_of_designs() {
    let base = Path::new("/base");
    let a = job::export_folder(base, &["x".into(), "y".into()]);
    assert_eq!(
        a,
        job::export_folder(base, &["y".into(), "x".into(), "x".into()])
    );
    assert_ne!(a, job::export_folder(base, &["x".into()]));
}

#[test]
fn ids_are_valid_and_distinct() {
    let ids: std::collections::BTreeSet<String> = (0..50).map(|_| job::new_id()).collect();
    assert_eq!(ids.len(), 50);
    assert!(ids.iter().all(|id| job::valid_id(id)));
}

#[test]
fn the_time_is_iso_8601_with_an_offset() {
    let now = job::iso_now();
    assert_eq!(now.len(), 25, "{now}");
    assert!(now.starts_with("20") && now.ends_with("+00:00"), "{now}");
}

// ---- the review's findings ----

use super::agent::{self, Found, Liveness};
use super::inventory;

fn jobs_left(addin: &FakeAddin) -> usize {
    std::fs::read_dir(addin.jobs_dir()).unwrap().count()
}

fn fail_job(addin: &FakeAddin, id: &str, reason: &str) {
    let out = addin.jobs_dir().join("out");
    assert!(addin.claim(id));
    addin.status(id, "failed", Some(reason), "x", (0, None, None), 0, 0, &out);
}

#[test]
fn a_failed_list_waits_for_try_again_and_never_asks_again_by_itself() {
    let (dir, addin) = setup();
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list job", |_| {
        !addin.unclaimed().is_empty()
    });
    let id = addin.unclaimed().remove(0);
    fail_job(&addin, &id, "not_signed_in");
    wait_for(&mut panel, &ctx, "the failure", |p| {
        p.stage() == Stage::ListFailed
    });
    assert!(panel.list_error.as_ref().unwrap().contains("not signed in"));
    assert_eq!(
        jobs_left(&addin),
        0,
        "the failed job's files are cleaned up"
    );
    // Many polls later nothing was asked again: the agent is still there.
    for _ in 0..60 {
        panel.poll(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(panel.stage(), Stage::ListFailed);
    assert_eq!(jobs_left(&addin), 0, "no new inventory job without a click");
    // The person's Try again asks once.
    panel.list_designs();
    assert_eq!(panel.stage(), Stage::Listing);
    assert_eq!(addin.unclaimed().len(), 1);
}

#[test]
fn stop_while_listing_does_not_relist() {
    let (dir, addin) = setup();
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list job", |_| {
        !addin.unclaimed().is_empty()
    });
    let id = addin.unclaimed().remove(0);
    assert!(addin.claim(&id));
    wait_for(&mut panel, &ctx, "queued", |p| {
        p.running.as_ref().is_some_and(|r| r.status.is_some())
    });
    panel.stop();
    wait_for(&mut panel, &ctx, "the cancel", |_| {
        addin.cancel_requested(&id)
    });
    let out = addin.jobs_dir().join("out");
    addin.status(
        &id,
        "cancelled",
        Some("cancelled"),
        "x",
        (0, None, None),
        0,
        0,
        &out,
    );
    wait_for(&mut panel, &ctx, "the end", |p| {
        p.stage() == Stage::ListFailed
    });
    assert!(panel.list_error.as_ref().unwrap().contains("stopped"));
    for _ in 0..40 {
        panel.poll(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(panel.stage(), Stage::ListFailed);
    assert_eq!(jobs_left(&addin), 0);
}

#[test]
fn a_jobs_folder_outside_cad_export_is_refused() {
    let (dir, addin) = setup();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let file = agent::agent_file(&addin.cad);
    let mut agent: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    agent["jobs_dir"] = json!(elsewhere.to_string_lossy());
    std::fs::write(&file, agent.to_string()).unwrap();
    assert!(matches!(agent::read(&file), Found::Invalid(_)));
    assert!(agent::read_minimal(&file).is_none());
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "a look", |p| p.last_agent_check.is_some());
    assert_eq!(panel.stage(), Stage::NoAgent);
    assert!(panel
        .agent_problem
        .as_ref()
        .unwrap()
        .contains("outside your .cad-export"));
    assert_eq!(
        std::fs::read_dir(&elsewhere).unwrap().count(),
        0,
        "nothing was written there"
    );
    // A relative or parent-pointing jobs_dir is no better.
    agent["jobs_dir"] = json!(addin.cad.join("jobs/../../elsewhere").to_string_lossy());
    std::fs::write(&file, agent.to_string()).unwrap();
    assert!(matches!(agent::read(&file), Found::Invalid(_)));
}

#[cfg(unix)]
#[test]
fn links_and_oversized_files_are_not_read() {
    let (dir, addin) = setup();
    let secret = dir.path().join("secret.json");
    std::fs::write(&secret, "{}").unwrap();
    // A linked agent file.
    let file = agent::agent_file(&addin.cad);
    let real = dir.path().join("real-agent.json");
    std::fs::rename(&file, &real).unwrap();
    std::os::unix::fs::symlink(&real, &file).unwrap();
    assert!(matches!(agent::read(&file), Found::Invalid(_)));
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&real, &file).unwrap();
    // A linked and an oversized status file.
    let status = addin.jobs_dir().join("a.status.json");
    std::os::unix::fs::symlink(&secret, &status).unwrap();
    assert!(job::read_status(&addin.jobs_dir(), "a").is_none());
    std::fs::remove_file(&status).unwrap();
    let big = vec![b' '; agent::SMALL_FILE as usize + 1];
    std::fs::write(&status, &big).unwrap();
    assert!(job::read_status(&addin.jobs_dir(), "a").is_none());
    assert!(agent::read_small(&status, agent::SMALL_FILE).is_err());
    // A linked inventory.
    let listing = dir.path().join("inventory.json");
    std::os::unix::fs::symlink(&secret, &listing).unwrap();
    assert!(Inventory::read(&listing).is_err());
}

#[test]
fn an_oversized_inventory_is_refused_without_being_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inventory.json");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(inventory::MAX_FILE + 1).unwrap();
    let why = Inventory::read(&path).unwrap_err();
    assert!(why.contains("too large"), "{why}");
    assert!(Inventory::read(&dir.path().join("none.json")).is_err());
}

#[test]
fn the_list_is_the_file_in_dutchmans_folder_not_the_one_the_status_names() {
    let (dir, addin) = setup();
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list job", |_| {
        !addin.unclaimed().is_empty()
    });
    let id = addin.unclaimed().remove(0);
    assert!(addin.claim(&id));
    // A hostile status names another file as the inventory.
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        elsewhere.join("inventory.json"),
        sample_inventory(true).to_string(),
    )
    .unwrap();
    let ours = PathBuf::from(addin.job(&id)["output_dir"].as_str().unwrap());
    std::fs::create_dir_all(&ours).unwrap();
    std::fs::write(
        ours.join("inventory.json"),
        sample_inventory(false).to_string(),
    )
    .unwrap();
    addin.status(&id, "done", None, "x", (1, Some(1), None), 0, 0, &elsewhere);
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    assert_eq!(
        panel.inventory.as_ref().unwrap().hubs[0].name,
        "Acme Engineering"
    );
}

use std::path::PathBuf;

#[test]
fn a_deep_path_or_too_many_designs_get_a_plain_message() {
    let mut deep = sample_inventory(false);
    let path: Vec<String> = (0..=inventory::MAX_DEPTH)
        .map(|n| format!("f{n}"))
        .collect();
    deep["hubs"]["hub-1"]["projects"]["project-1"]["files"][0]["path"] = json!(path);
    let why = Inventory::from_value(&deep).unwrap_err();
    assert!(why.contains("folders deep"), "{why}");
    // Exactly at the limit still works, and does not recurse further than the path.
    let ok: Vec<String> = (0..inventory::MAX_DEPTH).map(|n| format!("f{n}")).collect();
    deep["hubs"]["hub-1"]["projects"]["project-1"]["files"][0]["path"] = json!(ok);
    assert_eq!(Inventory::from_value(&deep).unwrap().count(), 4);
    // A path of a million names is cut at the limit, not collected.
    let huge: Vec<String> = (0..1_000_000).map(|n| n.to_string()).collect();
    deep["hubs"]["hub-1"]["projects"]["project-1"]["files"][0]["path"] = json!(huge);
    assert!(Inventory::from_value(&deep).is_err());
    let files: Vec<Value> = (0..=inventory::MAX_DESIGNS)
        .map(|n| json!({"id": format!("urn:{n}"), "name": "d", "path": [], "extension": "f3d", "version": 1}))
        .collect();
    let many =
        json!({"hubs": {"h": {"name": "H", "projects": {"p": {"name": "P", "files": files}}}}});
    let why = Inventory::from_value(&many).unwrap_err();
    assert!(why.contains("more than 100000 designs"), "{why}");
}

#[test]
fn the_heartbeat_rule_never_takes_a_stale_heartbeat_alone_as_gone() {
    let agent = |updated: Option<i64>, pid: Option<u32>| agent::Agent {
        jobs_dir: PathBuf::new(),
        features: vec![],
        version: String::new(),
        pid,
        updated,
        busy: None,
        job: None,
        fusion_version: None,
    };
    let now = 1_000_000;
    assert_eq!(
        agent::liveness(&agent(Some(now - 59), Some(1)), now, &|_| false),
        Liveness::Running
    );
    assert_eq!(
        agent::liveness(&agent(Some(now - 61), Some(1)), now, &|_| true),
        Liveness::Busy
    );
    assert_eq!(
        agent::liveness(&agent(Some(now - 61), Some(1)), now, &|_| false),
        Liveness::Gone
    );
    assert_eq!(
        agent::liveness(&agent(None, None), now, &|_| true),
        Liveness::Gone
    );
    assert_eq!(agent::parse_time("1970-01-02T00:00:00+00:00"), Some(86_400));
    assert_eq!(agent::parse_time("1970-01-01T01:00:00+01:00"), Some(0));
    assert_eq!(
        agent::parse_time("2026-11-23T09:30:11.004000+00:00"),
        Some(1_795_426_211)
    );
    assert_eq!(
        agent::parse_time(&job::iso_at(1_795_426_211)),
        Some(1_795_426_211)
    );
    assert_eq!(agent::parse_time("nonsense"), None);
}

/// A panel with an export running against an exporter that never answers.
fn waiting_export(
    dir: &Path,
    addin: &FakeAddin,
    edit: impl Fn(&mut Env),
) -> (Panel, eframe::egui::Context) {
    let _ = addin;
    let mut env = Env::for_test(dir);
    edit(&mut env);
    let mut panel = Panel::new();
    panel.preview(env);
    panel.open = true;
    let ctx = eframe::egui::Context::default();
    // Skip the list: pretend it was made.
    panel.inventory = Some(Inventory::from_value(&sample_inventory(false)).unwrap());
    panel.stage = Stage::Picking;
    pick_all(&mut panel);
    panel.export_picked();
    assert_eq!(panel.stage(), Stage::Exporting);
    (panel, ctx)
}

#[test]
fn an_exporter_that_is_gone_for_a_minute_ends_the_wait_with_a_plain_message() {
    let (dir, addin) = setup();
    let (mut panel, ctx) = waiting_export(dir.path(), &addin, |env| {
        env.give_up = Duration::from_millis(150);
        env.pid_alive = |_| false;
    });
    assert_eq!(jobs_left(&addin), 1);
    // Fusion closes: its agent file goes away.
    std::fs::remove_file(agent::agent_file(&addin.cad)).unwrap();
    wait_for(&mut panel, &ctx, "giving up", |p| {
        p.stage() == Stage::Finished
    });
    let status = &panel.ended.as_ref().unwrap().status;
    assert_eq!(status.reason.as_deref(), Some("agent_gone"));
    assert!(panel.running.is_none());
    assert_eq!(
        jobs_left(&addin),
        0,
        "a job nobody took is withdrawn so it cannot start later"
    );
    assert!(
        panel.take_notice().is_none(),
        "the window is open: no notice needed"
    );
}

#[test]
fn a_stale_heartbeat_with_a_living_fusion_keeps_waiting() {
    let (dir, addin) = setup();
    addin.announce_at(FEATURES, None, &job::iso_at(agent::now_secs() - 600), 4242);
    let (mut panel, ctx) = waiting_export(dir.path(), &addin, |env| {
        env.give_up = Duration::from_millis(100);
        env.pid_alive = |_| true;
    });
    for _ in 0..40 {
        panel.poll(&ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(panel.stage(), Stage::Exporting, "Fusion is busy, not gone");
}

#[test]
fn a_job_that_never_gets_a_status_is_given_up_after_the_wait() {
    let (dir, addin) = setup();
    let (mut panel, ctx) = waiting_export(dir.path(), &addin, |env| {
        env.no_answer = Duration::from_millis(150);
    });
    wait_for(&mut panel, &ctx, "giving up", |p| {
        p.stage() == Stage::Finished
    });
    assert_eq!(
        panel.ended.as_ref().unwrap().status.reason.as_deref(),
        Some("no_answer")
    );
    assert_eq!(jobs_left(&addin), 0);
}

#[test]
fn stop_waiting_leaves_the_stage_and_withdraws_an_unclaimed_job() {
    let (dir, addin) = setup();
    let (mut panel, ctx) = waiting_export(dir.path(), &addin, |_| {});
    panel.stop();
    assert!(
        panel.running.as_ref().unwrap().stopping,
        "Stop alone waits for the exporter"
    );
    panel.stop_waiting();
    assert_eq!(panel.stage(), Stage::Picking);
    assert!(panel.running.is_none());
    assert_eq!(jobs_left(&addin), 0);
    assert!(panel.error.as_ref().unwrap().0.contains("Stopped waiting"));
    // The same while listing.
    let (dir, _addin) = setup();
    let (mut panel, ctx2) = panel_for(dir.path());
    wait_for(&mut panel, &ctx2, "the list job", |p| p.running.is_some());
    panel.stop_waiting();
    assert_eq!(panel.stage(), Stage::ListFailed);
    let _ = ctx;
}

#[test]
fn fusion_closing_while_the_picker_is_open_is_noticed_before_an_export() {
    let (dir, addin) = setup();
    let service = Service::spawn(addin.clone(), sample_inventory(false), Ending::done(), FAST);
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    drop(service);
    std::fs::remove_file(agent::agent_file(&addin.cad)).unwrap();
    pick_all(&mut panel);
    panel.export_picked();
    assert_eq!(panel.stage(), Stage::Picking);
    assert!(panel.error.as_ref().unwrap().0.contains("not running"));
    assert!(panel.running.is_none());
    assert_eq!(jobs_left(&addin), 0, "no job was written");
}

#[test]
fn an_export_ending_while_the_window_is_closed_leaves_a_notice() {
    let (dir, addin) = setup();
    let mut ending = Ending::done();
    ending.failed = 1;
    let _service = Service::spawn(addin.clone(), sample_inventory(false), ending, FAST);
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    pick_all(&mut panel);
    panel.export_picked();
    panel.open = false;
    wait_for(&mut panel, &ctx, "the end", |p| {
        p.stage() == Stage::Finished
    });
    assert!(panel.take_notice().unwrap().contains("ended with problems"));
    assert!(panel.take_notice().is_none());
}

#[test]
fn the_search_result_is_kept_until_the_text_changes() {
    let (dir, addin) = setup();
    let _service = Service::spawn(addin.clone(), sample_inventory(false), Ending::done(), FAST);
    let (mut panel, ctx) = panel_for(dir.path());
    wait_for(&mut panel, &ctx, "the list", |p| {
        p.stage() == Stage::Picking
    });
    // The window draws it; here the cache is exercised through the same fields.
    assert!(panel.shown_query.is_none());
    panel.query = "seal".into();
    let inventory = panel.inventory.clone().unwrap();
    panel.shown = inventory.matching_ids(&panel.query);
    panel.shown_query = Some(panel.query.clone());
    assert_eq!(panel.shown.len(), 1);
}

// The hand-off between the find-and-install part and the job side, in the one window.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

/// The window of a panel, polled like the app does each frame.
fn window_of(panel: Panel) -> Harness<'static, Panel> {
    Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 700.0))
        .build_state(
            |ctx, panel: &mut Panel| {
                panel.poll(ctx);
                let _ = panel.window(ctx, 0.0);
            },
            panel,
        )
}

/// Steps the window until `ready` holds.
fn step_until(h: &mut Harness<'static, Panel>, what: &str, ready: impl Fn(&Panel) -> bool) {
    let started = Instant::now();
    while !ready(h.state()) {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what}"
        );
        std::thread::sleep(FAST);
        h.step();
    }
    h.run_steps(2);
}

#[test]
fn without_an_exporter_the_window_offers_the_install_and_goes_on_when_it_appears() {
    let (dir, addin) = setup();
    std::fs::remove_file(agent::agent_file(&addin.cad)).unwrap();
    let (mut panel, _) = panel_for(dir.path());
    panel.open = true;
    let mut h = window_of(panel);
    step_until(&mut h, "a first look", |p| p.last_agent_check.is_some());
    assert_eq!(h.state().stage(), Stage::NoAgent);
    h.get_by_label("Install the Fusion exporter");
    h.get_by_label("The Fusion exporter was not found");
    assert_eq!(jobs_left(&addin), 0, "nothing is asked of Fusion yet");
    // Fusion starts with the exporter on: the window carries on with the list by itself.
    addin.announce(FEATURES, None);
    let _service = Service::spawn(addin.clone(), sample_inventory(false), Ending::done(), FAST);
    step_until(&mut h, "the list", |p| p.stage() == Stage::Picking);
    assert!(h.query_by_label("Install the Fusion exporter").is_none());
    assert!(h
        .query_by_label("The Fusion exporter was not found")
        .is_none());
    h.get_by_label("Search");
}

#[test]
fn fusion_busy_is_waited_for_and_no_job_is_written_until_it_answers() {
    let (dir, addin) = setup();
    addin.announce_at(
        FEATURES,
        None,
        &job::iso_at(agent::now_secs() - 600),
        std::process::id(),
    );
    let (mut panel, _) = panel_for(dir.path());
    panel.open = true;
    let mut h = window_of(panel);
    step_until(&mut h, "a first look", |p| p.last_agent_check.is_some());
    h.run_steps(3);
    assert_eq!(h.state().stage(), Stage::NoAgent);
    h.get_by_label("Fusion is busy");
    assert!(h.query_by_label("Install the Fusion exporter").is_none());
    assert_eq!(jobs_left(&addin), 0);
    // The heartbeat comes back.
    addin.announce(FEATURES, None);
    let _service = Service::spawn(addin.clone(), sample_inventory(false), Ending::done(), FAST);
    step_until(&mut h, "the list", |p| p.stage() == Stage::Picking);
}

#[test]
fn an_agent_file_that_cannot_be_used_is_said_in_the_window_beside_the_install() {
    let (dir, addin) = setup();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let file = agent::agent_file(&addin.cad);
    let mut agent_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    agent_json["jobs_dir"] = json!(elsewhere.to_string_lossy());
    std::fs::write(&file, agent_json.to_string()).unwrap();
    let (mut panel, _) = panel_for(dir.path());
    panel.open = true;
    let mut h = window_of(panel);
    step_until(&mut h, "a first look", |p| p.agent_problem.is_some());
    h.get_by_label_contains("outside your .cad-export");
    h.get_by_label("Install the Fusion exporter");
    assert_eq!(h.state().stage(), Stage::NoAgent);
}

#[test]
fn a_missing_download_in_the_window_says_so_plainly_and_nothing_hangs() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 2048];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let _ = std::io::Write::write_all(
                &mut stream,
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
            );
        }
    });
    let (dir, addin) = setup();
    std::fs::remove_file(agent::agent_file(&addin.cad)).unwrap();
    let addins = tempfile::tempdir().unwrap();
    let (mut panel, _) = panel_for(dir.path());
    panel.open = true;
    panel
        .installer_mut()
        .set_install_options(super::install::Options {
            version: "0.4.0".into(),
            zip_url: format!("http://127.0.0.1:{port}/FusionBulkExport-0.4.0.zip"),
            sha256: "00".repeat(32),
            addins: addins.path().to_path_buf(),
            connect_timeout: Duration::from_secs(2),
            stall_timeout: Duration::from_secs(2),
            deadline: Duration::from_secs(20),
            allow_http: true,
            mode: super::install::Mode::Install,
        });
    let mut h = window_of(panel);
    step_until(&mut h, "a first look", |p| p.last_agent_check.is_some());
    h.get_by_label("Install the Fusion exporter").click();
    step_until(&mut h, "the answer", |p| {
        !matches!(
            p.installer.state(),
            super::installer::Install::Idle | super::installer::Install::Running(_)
        )
    });
    h.get_by_label_contains("not published yet");
    h.get_by_label_contains("project page");
    h.get_by_label("Install the Fusion exporter");
    assert_eq!(h.state().stage(), Stage::NoAgent);
    assert_eq!(std::fs::read_dir(addins.path()).unwrap().count(), 0);
}
