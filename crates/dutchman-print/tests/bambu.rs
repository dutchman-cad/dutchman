//! LAN monitoring uses only this test's loopback broker and synthetic certificates.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
#[path = "helpers/bambu.rs"]
mod fake;
#[path = "helpers/slicer.rs"]
mod slicer_fixture;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use dutchman_print::{
    backend::{Backend, Options, PrinterState},
    config::Config,
    printers::Printers,
    safety::Safety,
};
use fake::Fake;
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread")]
async fn cli_prepare_control_refusals_and_confirmation() {
    let fake = Fake::new().await;
    let temp = tempfile::Builder::new()
        .prefix("bambu job= ")
        .tempdir()
        .unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let config = format!(
        r#"
[backend]
kind = "printers"
[slicer]
kind = "orcaslicer"
path = {:?}
datadir = {:?}
[[printers]]
kind = "bambu"
id = "bambu"
name = "Test Bambu"
model = "Synthetic"
host = "127.0.0.1"
port = {}
serial = "TESTSERIAL"
access_code = "test-access"
cert_sha256 = "{}"
printer_profile = "Machine"
print_profile = "Draft"
filament_profile = "PLA"
"#,
        env!("CARGO_BIN_EXE_dutchman-fake-slicer"),
        dir.join("presets").to_str().unwrap(),
        fake.port,
        dutchman_print::safety::hash(fake.cert.as_ref())
    );
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, &config).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    // Enrollment works before trust is configured and never sends MQTT credentials.
    let enrollment = config
        .lines()
        .filter(|line| !line.starts_with("cert_sha256"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&config_path, &enrollment).unwrap();
    for json in [false, true] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_dutchman-print"));
        command
            .arg("--config")
            .arg(&config_path)
            .args(["fingerprint", "bambu"]);
        if json {
            command.arg("--json");
        }
        let result = command.output().await.unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains(&dutchman_print::safety::hash(fake.cert.as_ref())));
        assert!(text.contains("cert_sha256"));
        assert!(!text.contains("test-access"));
        if json {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert!(!value["subject"].as_str().unwrap().is_empty());
            assert!(!value["issuer"].as_str().unwrap().is_empty());
            assert!(!value["not_before"].as_str().unwrap().is_empty());
            assert!(!value["not_after"].as_str().unwrap().is_empty());
        }
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), enrollment);
    }
    assert_eq!(fake.connections.load(Ordering::SeqCst), 2);
    assert_eq!(fake.credentials.load(Ordering::SeqCst), 0);
    assert_eq!(fake.requests.load(Ordering::SeqCst), 0);
    assert!(Config::load(&config_path).is_err());
    std::fs::write(&config_path, &config).unwrap();
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_dutchman-print"))
        .arg("--config")
        .arg(&config_path)
        .arg("--data-dir")
        .arg(dir.join("cli-data"))
        .args(["printers", "--json"])
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let reports: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(reports[0]["state"], "idle");
    assert!(!String::from_utf8_lossy(&result.stdout).contains("test-access"));
    Fake::wait(&fake.disconnects, 1).await;
    assert_eq!(fake.connections.load(Ordering::SeqCst), 3);

    let backend = Arc::new(Printers::new(toml::from_str::<Config>(&config).unwrap()).unwrap());
    let safety = Safety::new(backend.clone());
    assert_eq!(
        safety.printers().await.unwrap()[0].state,
        PrinterState::Idle
    );
    safety.clear("bambu").await.unwrap();
    // A terminal report followed by idle between polls still consumes clearance.
    fake.reports
        .send(json!({"print":{"gcode_state":"FINISH"}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    fake.reports
        .send(json!({"print":{"gcode_state":"IDLE"}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert_eq!(
        safety.printers().await.unwrap()[0].needs_clearing,
        Some(true)
    );
    safety.clear("bambu").await.unwrap();
    // A second completion without an intervening status poll must also remove clearance.
    fake.reports
        .send(json!({"print":{"gcode_state":"FAILED"}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    fake.reports
        .send(json!({"print":{"gcode_state":"IDLE"}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert_eq!(
        safety.printers().await.unwrap()[0].needs_clearing,
        Some(true)
    );
    for (category, name) in [
        ("machine", "Machine"),
        ("process", "Draft"),
        ("filament", "PLA"),
    ] {
        let path = dir.join("presets/user/default").join(category);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("preset.json"), json!({"name":name}).to_string()).unwrap();
    }
    std::fs::create_dir(dir.join("nested")).unwrap();
    // Exercise canonicalization on Linux as well, without Windows symlink privileges.
    let alias = temp.path().join("nested").join("..");
    let input = slicer_fixture::input(&alias, |input| {
        let folder = input.parent().unwrap();
        let path_text = |s: &str| folder.join(s).display().to_string();
        vec![
            format!("--datadir={}", path_text("presets")),
            format!(
                "--load-filaments={}",
                path_text("presets/user/default/filament/preset.json")
            ),
            format!(
                "--load-settings={};{}",
                path_text("presets/user/default/machine/preset.json"),
                path_text("presets/user/default/process/preset.json")
            ),
            "--slice=0".into(),
            "--export-3mf=sliced.3mf".into(),
            format!("--outputdir={}", folder.display()),
            input.to_str().unwrap().into(),
        ]
    });
    assert_ne!(input, input.canonicalize().unwrap());
    let estimate = safety
        .prepare("job", &input, "bambu", Options::default())
        .await
        .unwrap();
    assert_eq!(estimate.seconds, Some(123));
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(dir.join("sliced.3mf")).unwrap()).unwrap();
    assert!(archive.by_name("Metadata/plate_1.gcode").is_ok());
    for result in [
        safety.start("job").await,
        safety.cancel("job").await,
        safety.pause("bambu").await,
        safety.resume("bambu").await,
    ] {
        assert!(result.unwrap_err().to_string().contains("lan_control"));
    }
    let app = dutchman_print::router(backend, dutchman_print::ServerConfig::default());
    for (i, path) in [
        "/api/v1/jobs/job/start",
        "/api/v1/jobs/job/cancel",
        "/api/v1/printers/bambu/pause",
        "/api/v1/printers/bambu/resume",
    ]
    .into_iter()
    .enumerate()
    {
        let response = app
            .clone()
            .oneshot(Request::post(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("Plugin-Confirmation", format!("{i:064x}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("lan_control"));
    }
    assert_eq!(fake.requests.load(Ordering::SeqCst), 2);
    // Read all fixture metadata so the same helper stays warning-free in both test targets.
    assert!(!fake.pem.is_empty());
    assert_ne!(fake.port, 0);
    assert_eq!(fake.credentials.load(Ordering::SeqCst), 2);
}

use dutchman_print::config::Printer as PrinterConfig;
use dutchman_print::safety::hash;
#[path = "helpers/ftps.rs"]
mod ftps;
struct Control {
    fake: Fake,
    ftps: ftps::Fake,
    safety: Safety,
    backend: Arc<Printers>,
    dir: tempfile::TempDir,
    input: std::path::PathBuf,
}
impl Control {
    async fn new() -> Self {
        Self::with_size(false).await
    }
    async fn with_size(wrong_size: bool) -> Self {
        let fake = Fake::new().await;
        fake.ready.store(true, Ordering::SeqCst);
        let ftps = ftps::Fake::with_server(
            fake.server.clone(),
            fake.cert.to_vec(),
            fake.pem.clone(),
            false,
            wrong_size,
            0,
        )
        .await;
        let dir = tempfile::Builder::new()
            .prefix("bambu control= ")
            .tempdir()
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        for (category, name) in [
            ("machine", "Machine"),
            ("process", "Draft"),
            ("filament", "PLA"),
        ] {
            let folder = root.join("presets/user/default").join(category);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("preset.json"), json!({"name":name}).to_string()).unwrap();
        }
        let input = slicer_fixture::input(&root, |input| {
            let folder = input.parent().unwrap();
            vec![
                format!("--datadir={}", folder.join("presets").display()),
                format!(
                    "--load-filaments={}",
                    folder
                        .join("presets/user/default/filament/preset.json")
                        .display()
                ),
                format!(
                    "--load-settings={};{}",
                    folder
                        .join("presets/user/default/machine/preset.json")
                        .display(),
                    folder
                        .join("presets/user/default/process/preset.json")
                        .display()
                ),
                "--slice=0".into(),
                "--export-3mf=sliced.3mf".into(),
                format!("--outputdir={}", folder.display()),
                input.to_str().unwrap().into(),
            ]
        });
        let config = Config {
            backend: dutchman_print::config::Backend {
                kind: "printers".into(),
                slice: false,
            },
            slicer: Some(
                toml::from_str(&format!(
                    "kind = 'orcaslicer'\npath = {:?}\ndatadir = {:?}",
                    env!("CARGO_BIN_EXE_dutchman-fake-slicer"),
                    root.join("presets").to_str().unwrap()
                ))
                .unwrap(),
            ),
            slicers: Default::default(),
            printers: vec![dutchman_print::config::Printer {
                kind: Some("bambu".into()),
                id: "bambu".into(),
                name: "Test printer".into(),
                model: "Synthetic".into(),
                host: Some("127.0.0.1".into()),
                port: Some(fake.port),
                ftps_port: ftps.config.ftps_port,
                serial: Some("TESTSERIAL".into()),
                access_code: Some("test-access".into()),
                cert_sha256: Some(dutchman_print::safety::hash(fake.cert.as_ref())),
                lan_control: true,
                printer_profile: Some("Machine".into()),
                print_profile: Some("Draft".into()),
                filament_profile: Some("PLA".into()),
                ..Default::default()
            }],
        };
        let backend = Arc::new(
            Printers::new(config)
                .unwrap()
                .with_store(root.join("jobs.json")),
        );
        let safety = Safety::new(backend.clone());
        safety.printers().await.unwrap();
        safety
            .prepare("control", &input, "bambu", Options::default())
            .await
            .unwrap();
        Self {
            fake,
            ftps,
            safety,
            backend,
            dir,
            input,
        }
    }
    async fn clear_start(&self) -> dutchman_print::backend::Job {
        self.safety.clear("bambu").await.unwrap();
        self.safety.start("control").await.unwrap()
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn control_lifecycle_and_finish_consume_clearance() {
    use dutchman_print::backend::JobState;
    let c = Control::new().await;
    assert!(c
        .safety
        .start("control")
        .await
        .unwrap_err()
        .to_string()
        .contains("clear bed"));
    assert!(c.fake.commands.lock().await.is_empty());
    assert_eq!(c.clear_start().await.state, JobState::Printing);
    assert!(c.ftps.received.load(Ordering::SeqCst) > 0);
    assert_eq!(
        c.safety.pause("bambu").await.unwrap().state,
        JobState::Paused
    );
    assert_eq!(
        c.safety.resume("bambu").await.unwrap().state,
        JobState::Printing
    );
    let name = c.backend.job("control").await.unwrap();
    assert_eq!(name.state, JobState::Printing);
    let commands = c.fake.commands.lock().await.clone();
    for (index, command) in ["project_file", "pause", "resume"].iter().enumerate() {
        assert_eq!(commands[index]["print"]["command"], *command);
        assert!(commands[index]["print"]["sequence_id"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .is_ok());
    }
    let name = format!("dutchman-{}", dutchman_print::safety::hash(b"control"));
    let archive = std::fs::read(c.dir.path().join("sliced.3mf")).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
    let mut gcode = Vec::new();
    use std::io::Read as _;
    zip.by_name("Metadata/plate_1.gcode")
        .unwrap()
        .read_to_end(&mut gcode)
        .unwrap();
    use md5::{Digest as _, Md5};
    let checksum: String = Md5::digest(&gcode)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        commands[0],
        json!({"print":{"command":"project_file","sequence_id":commands[0]["print"]["sequence_id"],"param":"Metadata/plate_1.gcode","project_id":"0","profile_id":"0","task_id":"0","subtask_id":"0","subtask_name":name,"url":format!("file:///sdcard/{name}.gcode.3mf"),"md5":checksum,"timelapse":false,"bed_type":"auto","bed_leveling":false,"flow_cali":false,"vibration_cali":false,"layer_inspect":false,"use_ams":false,"ams_mapping":""}})
    );
    assert_eq!(commands[1]["print"].as_object().unwrap().len(), 3);
    assert_eq!(commands[1]["print"]["param"], "");
    c.fake
        .reports
        .send(json!({"print":{"gcode_state":"FINISH","print_error":42}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_ne!(c.safety.job("control").await.unwrap().state, JobState::Done);
    c.fake
        .reports
        .send(json!({"print":{"gcode_state":"FINISH","print_error":0}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(c.safety.job("control").await.unwrap().state, JobState::Done);
    assert_eq!(
        c.safety.printers().await.unwrap()[0].needs_clearing,
        Some(true)
    );
    assert!(c.safety.pause("bambu").await.is_err());
    c.fake
        .reports
        .send(json!({"print":{"gcode_state":"IDLE"}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c.safety.clear("bambu").await.unwrap();
    assert_eq!(
        c.safety.printers().await.unwrap()[0].needs_clearing,
        Some(false)
    );
}
#[tokio::test(flavor = "multi_thread")]
async fn stop_only_the_tracked_live_job() {
    use dutchman_print::backend::JobState;
    let c = Control::new().await;
    c.clear_start().await;
    assert_eq!(
        c.safety.cancel("control").await.unwrap().state,
        JobState::Cancelled
    );
    assert_eq!(
        c.safety.printers().await.unwrap()[0].needs_clearing,
        Some(true)
    );
    let count = c.fake.commands.lock().await.len();
    c.safety.cancel("control").await.unwrap();
    assert!(c.safety.resume("bambu").await.is_err());
    assert_eq!(c.fake.commands.lock().await.len(), count);
}
#[tokio::test(flavor = "multi_thread")]
async fn replacement_live_job_blocks_control() {
    let c = Control::new().await;
    c.clear_start().await;
    c.fake
        .reports
        .send(json!({"print":{"gcode_state":"RUNNING","subtask_name":"another job"}}))
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(c.safety.pause("bambu").await.is_err());
    assert!(c.safety.cancel("control").await.is_err());
    assert_eq!(c.fake.commands.lock().await.len(), 1);
}
#[tokio::test(flavor = "multi_thread")]
async fn authorization_rejects_start_and_never_repeats_it() {
    let c = Control::new().await;
    c.fake.authorization.store(true, Ordering::SeqCst);
    c.safety.clear("bambu").await.unwrap();
    let error = c.safety.start("control").await.unwrap_err().to_string();
    assert!(
        error.contains("rejected") && error.contains("authorization"),
        "{error}"
    );
    assert!(c
        .backend
        .start("control")
        .await
        .unwrap_err()
        .to_string()
        .contains("already attempted"));
    assert_eq!(c.fake.commands.lock().await.len(), 1);
    assert!(std::fs::read_to_string(c.dir.path().join("jobs.json"))
        .unwrap()
        .contains("\"attempted\":true"));
}
#[tokio::test(flavor = "multi_thread")]
async fn lost_start_reply_reconciles_without_resending() {
    use dutchman_print::backend::JobState;
    let c = Control::new().await;
    c.fake.lose_reply.store(true, Ordering::SeqCst);
    c.safety.clear("bambu").await.unwrap();
    let _ = c.safety.start("control").await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        c.safety.job("control").await.unwrap().state,
        JobState::Printing
    );
    assert!(c.backend.start("control").await.is_err());
    assert_eq!(c.fake.commands.lock().await.len(), 1);
}
#[tokio::test(flavor = "multi_thread")]
async fn reviewed_archive_must_not_change() {
    let c = Control::new().await;
    assert!(c.input.is_file());
    std::fs::write(c.dir.path().join("sliced.3mf"), b"changed").unwrap();
    c.safety.clear("bambu").await.unwrap();
    assert!(c
        .safety
        .start("control")
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
    assert!(c.fake.commands.lock().await.is_empty());
    assert_eq!(c.ftps.received.load(Ordering::SeqCst), 0);
}
#[tokio::test(flavor = "multi_thread")]
async fn delivery_success_without_state_change_is_not_confirmed() {
    use dutchman_print::backend::JobState;
    let c = Control::new().await;
    c.fake.no_state.store(true, Ordering::SeqCst);
    c.safety.clear("bambu").await.unwrap();
    let error = c.safety.start("control").await.unwrap_err().to_string();
    assert!(error.contains("not confirmed"), "{error}");
    assert_eq!(
        c.safety.job("control").await.unwrap().state,
        JobState::Ready
    );
    assert!(c.backend.start("control").await.is_err());
    assert_eq!(c.fake.commands.lock().await.len(), 1);
}
#[tokio::test(flavor = "multi_thread")]
async fn failed_upload_never_sends_start() {
    let c = Control::with_size(true).await;
    c.safety.clear("bambu").await.unwrap();
    assert!(c.safety.start("control").await.is_err());
    assert!(c.fake.commands.lock().await.is_empty());
}
#[tokio::test(flavor = "multi_thread")]
async fn authorization_rejects_live_control_without_claiming_transition() {
    use dutchman_print::backend::JobState;
    let c = Control::new().await;
    c.clear_start().await;
    c.fake.authorization.store(true, Ordering::SeqCst);
    let error = c.safety.pause("bambu").await.unwrap_err().to_string();
    assert!(error.contains("rejected"));
    assert_eq!(
        c.safety.job("control").await.unwrap().state,
        JobState::Printing
    );
}
#[tokio::test(flavor = "multi_thread")]
async fn multi_filament_prepare_names_every_missing_assignment() {
    let c = Control::new().await;
    std::fs::write(c.dir.path().join("mode"), "multi").unwrap();
    let error = c
        .safety
        .prepare("multi", &c.input, "bambu", Options::default())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("filament 1 (PLA)"), "{error}");
    assert!(error.contains("filament 2 (PETG)"), "{error}");
    assert!(c.fake.commands.lock().await.is_empty());
    assert_eq!(c.ftps.received.load(Ordering::SeqCst), 0);
}
#[tokio::test(flavor = "multi_thread")]
async fn another_execution_of_the_same_file_is_not_the_tracked_job() {
    let c = Control::new().await;
    c.clear_start().await;
    let command = c.fake.commands.lock().await[0].clone();
    c.fake.reports.send(json!({"print":{"gcode_state":"RUNNING","subtask_name":command["print"]["subtask_name"],"gcode_start_time":"99999999999999"}})).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(c.safety.pause("bambu").await.is_err());
    assert!(c.safety.cancel("control").await.is_err());
    assert_eq!(c.fake.commands.lock().await.len(), 1);
}
