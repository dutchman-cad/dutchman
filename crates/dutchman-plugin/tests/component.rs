#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
//! Run with `DUTCHMAN_PLUGIN_COMPONENT=/absolute/path/dutchman.wasm cargo test --release
//! -p dutchman-plugin --features host-tests --test component -- --nocapture`; a relative path
//! names a file in the workspace.
//! The component is required: this test never silently skips conversion. The linker holds
//! only the `craft:plugin` imports, as in SolidCraft: no WASI, so a component that needs any
//! fails to instantiate.
#[path = "../src/adapter.rs"]
mod adapter;
#[path = "../src/limits.rs"]
#[allow(dead_code)]
mod limits;
#[path = "../src/report.rs"]
#[allow(dead_code)]
mod loss_report;
#[path = "../src/notice.rs"]
#[allow(dead_code)]
mod notice;
#[path = "../src/stand_up.rs"]
mod stand_up;
use adapter::host::{
    add_to_linker, clock, control, design, evaluate, import_file, log, source, types,
    ImporterPlugin,
};
use dutchman_core::{Format, PartSearch, Report, Writer};
use dutchman_solidcraft::{
    CliEvaluator, SolidCraftEvaluator, SolidCraftVersion, SolidCraftWriter, Verification,
};
use limits::MAX_REPORT_BYTES;
use serde_json::{json, Value};
use std::{
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};
use types::{FieldFailure, PluginError};
use wasmtime::{
    component::{Component, Linker},
    Config, Engine, ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder,
};

/// The store's limits, remembering the largest linear memory the guest asked for.
struct Peak {
    limits: StoreLimits,
    bytes: usize,
}
impl ResourceLimiter for Peak {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.limits.memory_growing(current, desired, maximum)?;
        if allowed {
            self.bytes = self.bytes.max(desired);
        }
        Ok(allowed)
    }
    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.limits.table_growing(current, desired, maximum)
    }
}

struct Host {
    root: PathBuf,
    main: Vec<u8>,
    output: Vec<u8>,
    limits: Peak,
    read: usize,
    calls: usize,
    progress: Vec<f32>,
    fault: Option<Fault>,
    input_failed: bool,
    good_member_read: bool,
    staged: Vec<u8>,
    commands: Vec<Value>,
    evaluator: Option<CliEvaluator>,
}
fn failed(reason: impl ToString) -> PluginError {
    PluginError::CommandFailed(types::CommandFailure {
        command: "source.read".into(),
        reason: reason.to_string(),
    })
}
impl types::Host for Host {}
/// How the plugin words a check this harness's evaluate refused.
const NO_HOST_EVALUATION: &str =
    "Host evaluation: PluginError::PermissionDenied(\"evaluate: the test host has no SolidCraft\")";
// Ordinary fixture comparisons refuse evaluation; the placement regression uses
// a real CLI in private archives and replays the accumulated session commands.
impl evaluate::Host for Host {
    fn stage(&mut self, bytes: Vec<u8>) -> Result<(), PluginError> {
        self.staged.extend(bytes);
        Ok(())
    }
    fn run(&mut self, command: String, args: String) -> Result<String, PluginError> {
        let Some(evaluator) = &self.evaluator else {
            return Err(PluginError::PermissionDenied(
                "evaluate: the test host has no SolidCraft".into(),
            ));
        };
        self.commands.push(
            json!({"command":command,"params":serde_json::from_str::<Value>(&args).unwrap()}),
        );
        let mut results = evaluator
            .run(
                &self.staged,
                &self.commands,
                std::time::Duration::from_secs(120),
                "component-session",
            )
            .map_err(failed)?;
        Ok(serde_json::to_string(&results.pop().unwrap()).unwrap())
    }
    fn reset(&mut self) {
        self.staged.clear();
        self.commands.clear();
    }
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    ReadDenied,
    ReadLimit,
    SizeDenied,
    SizeLimit,
    ResolveDenied,
    ResolveLimit,
    Aggregate,
    Oversize,
    FolderDenied,
}
impl Host {
    fn input(&mut self, operation: &str, path: &str) -> Result<(), PluginError> {
        assert!(
            !self.input_failed,
            "input work continued after a fatal failure"
        );
        if !path.ends_with("P02a.FCStd") {
            return Ok(());
        }
        let fail = match self.fault {
            Some(Fault::ReadDenied | Fault::ReadLimit) => operation == "read",
            Some(Fault::SizeDenied | Fault::SizeLimit) => operation == "size",
            Some(Fault::ResolveDenied | Fault::ResolveLimit) => operation == "resolve",
            _ => false,
        };
        if fail {
            assert!(
                self.good_member_read,
                "the good member must have converted first"
            );
            self.input_failed = true;
            let message = format!("Original source.{operation} failure");
            return Err(match self.fault {
                Some(Fault::ReadLimit | Fault::SizeLimit | Fault::ResolveLimit) => {
                    PluginError::LimitExceeded(message)
                }
                _ => PluginError::PermissionDenied(message),
            });
        }
        Ok(())
    }
}
impl log::Host for Host {
    fn log(&mut self, level: log::Level, message: String) {
        eprintln!("plugin {level:?}: {message}");
    }
}
impl control::Host for Host {
    fn progress(&mut self, fraction: f32, _message: String) {
        assert!((0.0..=1.0).contains(&fraction));
        self.progress.push(fraction);
    }
    fn cancelled(&mut self) -> bool {
        false
    }
}
impl clock::Host for Host {
    // Without the clock permission the host answers 0.
    fn now(&mut self) -> u64 {
        0
    }
}
impl source::Host for Host {
    fn resolve(&mut self, path: String) -> Result<Option<String>, PluginError> {
        if matches!(self.fault, Some(Fault::FolderDenied)) && !path.is_empty() {
            return Err(PluginError::PermissionDenied(
                "input_folder was not granted".into(),
            ));
        }
        self.input("resolve", &path)?;
        if path.is_empty() {
            return Ok(Some(path));
        }
        if !safe(&path) {
            return Err(PluginError::PermissionDenied("Input folder only".into()));
        }
        let mut folder = self.root.clone();
        let mut names = Vec::new();
        for name in path.split('/') {
            let wanted = name.to_lowercase();
            let mut names_here = std::fs::read_dir(&folder)
                .map_err(failed)?
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect::<Vec<_>>();
            names_here.sort();
            let selected = names_here
                .iter()
                .find(|candidate| candidate.as_str() == name)
                .or_else(|| {
                    names_here
                        .iter()
                        .find(|candidate| candidate.to_lowercase() == wanted)
                });
            let Some(selected) = selected else {
                return Ok(None);
            };
            folder.push(selected);
            names.push(selected.clone());
            if !folder.exists() {
                return Ok(None);
            }
        }
        Ok(folder.is_file().then(|| names.join("/")))
    }
    fn size(&mut self, path: String) -> Result<Option<u64>, PluginError> {
        self.input("size", &path)?;
        if path.ends_with("P02a.FCStd") {
            if let Some(Fault::Aggregate | Fault::Oversize) = self.fault {
                assert!(self.good_member_read);
                self.input_failed = true;
                return Ok(Some(
                    (1 << 30) + u64::from(matches!(self.fault, Some(Fault::Oversize))),
                ));
            }
        }
        if path.is_empty() {
            return Ok(Some(self.main.len() as u64));
        }
        if !safe(&path) {
            return Err(PluginError::PermissionDenied("Input folder only".into()));
        }
        Ok(self
            .root
            .join(path)
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len()))
    }
    fn read(&mut self, path: String, offset: u64, len: u32) -> Result<Vec<u8>, PluginError> {
        self.input("read", &path)?;
        assert!(len as usize <= limits::CHUNK_BYTES);
        let member;
        let bytes = if path.is_empty() {
            &self.main
        } else {
            assert!(safe(&path));
            member = std::fs::read(self.root.join(&path)).map_err(failed)?;
            &member
        };
        let start =
            usize::try_from(offset).map_err(|e| PluginError::LimitExceeded(e.to_string()))?;
        let end = start.saturating_add(len as usize).min(bytes.len());
        let chunk = bytes.get(start..end).unwrap_or_default().to_vec();
        if path.ends_with("P01a.FCStd") && end == bytes.len() {
            self.good_member_read = true;
        }
        self.calls += 1;
        self.read += chunk.len();
        assert!(self.read <= 1 << 30);
        Ok(chunk)
    }
}
impl design::Host for Host {
    fn write(&mut self, bytes: Vec<u8>) -> Result<(), PluginError> {
        assert!(bytes.len() <= limits::CHUNK_BYTES);
        assert!(self.output.len() + bytes.len() <= limits::MAX_OUTPUT_BYTES);
        self.output.extend(bytes);
        Ok(())
    }
}
fn safe(path: &str) -> bool {
    !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && !matches!(p, "." | ".."))
}
fn host(root: &Path, main: Vec<u8>) -> Host {
    Host {
        root: root.into(),
        main,
        output: Vec::new(),
        limits: Peak {
            limits: StoreLimitsBuilder::new()
                .memory_size(2048 * 1024 * 1024)
                .build(),
            bytes: 0,
        },
        read: 0,
        calls: 0,
        progress: Vec::new(),
        fault: None,
        input_failed: false,
        good_member_read: false,
        staged: Vec::new(),
        commands: Vec::new(),
        evaluator: None,
    }
}
fn contents(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        result.push((file.name().unwrap().into_owned(), data));
    }
    result.sort();
    result
}
#[test]
fn component_matches_cli_pipeline_on_public_fixtures(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run_fixtures)?
        .join()
        .map_err(|_| "Component harness worker failed")?
}

fn run_fixtures() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::var("DUTCHMAN_PLUGIN_COMPONENT")
        .map_err(|_| "Set DUTCHMAN_PLUGIN_COMPONENT to the wasm32-unknown-unknown component")?;
    // A relative path names a file in the workspace.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .consume_fuel(true)
        .max_wasm_stack(16 * 1024 * 1024)
        .async_stack_size(18 * 1024 * 1024);
    let engine = Engine::new(&config)?;
    let started = Instant::now();
    let component = Component::from_file(&engine, &path)?;
    let compile_elapsed = started.elapsed();
    #[cfg(target_os = "linux")]
    if let Some(peak) = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmHWM:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        })
    {
        eprintln!("wasmtime_first_compile_peak_rss_kib={peak}");
    }
    eprintln!(
        "component_bytes={} first_compile_seconds={:.3}",
        std::fs::metadata(&path)?.len(),
        compile_elapsed.as_secs_f64()
    );
    let mut linker = Linker::new(&engine);
    add_to_linker(&mut linker)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dutchman-core/tests/fixtures");
    // A tiny synthetic archive advertises a large payload. Refuse from the index,
    // under a small fuel budget, before decoding or sending any design bytes.
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("synthetic", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"not native CAD")?;
    let mut bytes = zip.finish()?.into_inner();
    let directory = bytes
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .ok_or("Synthetic ZIP directory")?;
    bytes[directory + 24..directory + 28].copy_from_slice(&400_000_000u32.to_le_bytes());
    let size = bytes.len() as u64;
    let mut store = Store::new(&engine, host(&root, bytes));
    store.limiter(|host| &mut host.limits);
    store.set_fuel(20_000_000)?;
    let instance = ImporterPlugin::instantiate(&mut store, &component, &linker)?;
    let started = Instant::now();
    let refused = import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: "synthetic.f3d".into(),
            size,
        },
        "{}",
    )?;
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    match refused {
        Err(PluginError::LimitExceeded(reason)) => {
            assert!(
                reason.starts_with("This design is too large to open inside SolidCraft"),
                "{reason}"
            );
            assert!(reason.contains("https://github.com/dutchman-cad/dutchman/releases/latest"));
        }
        other => return Err(format!("Expected early size refusal, got {other:?}").into()),
    }
    assert!(store.data().output.is_empty());
    assert!(store.data().read < 4096);
    // Guest-side checks remain strict even when a caller bypasses a host form.
    for (entry, params, size, expected) in [
        ("convert", r#"{"extra":true}"#, 0, "invalid options"),
        ("other", "{}", 0, "invalid entry"),
        ("convert", "{}", 5, "invalid file"),
        ("convert", "{}", (1u64 << 30) + 1, "limit"),
    ] {
        let mut store = Store::new(&engine, host(&root, Vec::new()));
        store.limiter(|host| &mut host.limits);
        store.set_fuel(1_000_000)?;
        let instance = ImporterPlugin::instantiate(&mut store, &component, &linker)?;
        let result = import_file(
            &instance,
            &mut store,
            entry,
            &source::FileInfo {
                name: "cube.f3d".into(),
                size,
            },
            params,
        )?;
        let actual = match result {
            Err(PluginError::InvalidInput(FieldFailure { field, .. })) => {
                format!("invalid {field}")
            }
            Err(PluginError::LimitExceeded(_)) => "limit".into(),
            other => return Err(format!("Expected {expected}, got {other:?}").into()),
        };
        assert_eq!(actual, expected);
        assert!(store.data().output.is_empty());
    }

    // A synthetic malformed stream spans several reads; the public CAD files are small.
    let stream_len = 2 * limits::CHUNK_BYTES + 17;
    let mut store = Store::new(&engine, host(&root, vec![0; stream_len]));
    store.limiter(|host| &mut host.limits);
    store.set_fuel(50_000_000_000)?;
    let instance = ImporterPlugin::instantiate(&mut store, &component, &linker)?;
    let result = import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: "stream.f3d".into(),
            size: stream_len as u64,
        },
        "{}",
    )?;
    assert!(matches!(
        result,
        Err(PluginError::InvalidInput(FieldFailure { ref field, .. })) if field == "file"
    ));
    assert_eq!(store.data().calls, 4);
    assert_eq!(store.data().read, stream_len + 65_633);
    assert!(store.data().output.is_empty());
    drop(store);

    fatal_member_failures(&engine, &component, &linker, &root)?;
    stand_up_replays(&engine, &component, &linker, &root)?;
    hidden_repeated_bodies(&engine, &component, &linker, &root)?;
    folder_not_granted(&engine, &component, &linker, &root)?;
    let mut fixtures = Vec::new();
    collect_fixtures(&root, &mut fixtures)?;
    fixtures.sort();
    for source in fixtures {
        let fixture = source.strip_prefix(&root)?.display().to_string();
        let folder = source.parent().unwrap();
        let name = source.file_name().unwrap().to_str().unwrap();
        let bytes = std::fs::read(&source)?;
        let source_len = bytes.len();
        let mut store = Store::new(&engine, host(folder, bytes.clone()));
        store.limiter(|host| &mut host.limits);
        store.set_fuel(50_000_000_000)?;
        let instance = ImporterPlugin::instantiate(&mut store, &component, &linker)?;
        let result = import_file(
            &instance,
            &mut store,
            "convert",
            &source::FileInfo {
                name: name.into(),
                size: bytes.len() as u64,
            },
            "{}",
        )?;
        let mut expected = Vec::new();
        let mut cli_report = Report::default();
        let native = dutchman_core::read_in(
            Format::from_path(&source).unwrap(),
            &source,
            &PartSearch {
                threads: 1,
                project: Some(folder.into()),
                ..PartSearch::default()
            },
            &mut cli_report,
        )
        .and_then(|model| {
            SolidCraftWriter::new()
                .threads(1)
                .target_version(Some(SolidCraftVersion::default()))
                .verification(Verification::Off)
                .hide_unverified_history(true)
                .write(&model, &mut expected, &mut cli_report)
        });
        let report = match (native, result) {
            (Ok(()), Ok(report)) => report,
            (Err(_), Err(_)) => {
                assert!(store.data().output.is_empty());
                eprintln!("{fixture}: both front ends refused the fixture");
                continue;
            }
            (native, guest) => {
                return Err(format!("{fixture}: native={native:?}, guest={guest:?}").into())
            }
        };
        assert!(report.len() < 1 << 20);
        let report: serde_json::Value = serde_json::from_str(&report)?;
        let summary = report["summary"].as_str().unwrap_or_default();
        assert!(!summary.is_empty() && summary.len() <= 240);
        let items = report["items"].as_array().ok_or("items")?;
        assert!(items.len() <= 4096);
        for item in items {
            assert!(matches!(
                item["severity"].as_str(),
                Some("info" | "degraded" | "dropped")
            ));
            assert!(item["subject"].as_str().ok_or("subject")?.len() <= 256);
            let title = item["title"].as_str().ok_or("title")?;
            assert!(!title.trim().is_empty() && title.chars().count() <= 120);
            assert!(item["code"].as_str().ok_or("code")?.chars().count() <= 128);
            let message = item["message"].as_str().ok_or("message")?;
            assert!(!message.trim().is_empty() && message.len() <= 1024);
        }
        assert!(report
            .as_object()
            .ok_or("report")?
            .keys()
            .all(|key| matches!(key.as_str(), "summary" | "items" | "details")));
        let details = &report["details"];
        assert_eq!(details["schema"], "dutchman.plugin.report/2");
        let native_report = loss_report::readable(&cli_report);
        // The harness's evaluate refuses every check where the CLI had verification off: the
        // unchecked status, and the CLI's reason within the plugin's, which names the host's
        // refusal where the CLI says verification is off (and may say which check it was).
        let guest = &details["shape_check"];
        let native = &native_report["shape_check"];
        // The harness host cannot evaluate, so the plugin says "Not checked" whatever the CLI
        // (verification off) said, and keeps the CLI's reason in front of the host's refusal
        // (`unavailable` in dutchman-solidcraft's opening.rs). The CLI never claims a check.
        assert_ne!(
            native["result"], "Checked against the original shape",
            "{fixture}: the CLI ran with verification off"
        );
        assert_eq!(
            guest["result"], "Not checked",
            "{fixture}: verification must report the host's refusal as unchecked"
        );
        let message = |value: &serde_json::Value| value["reason"].as_str().map(str::to_owned);
        let guest_message =
            message(guest).map(|text| text.replace(NO_HOST_EVALUATION, "verification turned off"));
        // `unavailable` keeps the earlier reason only for a partly checked or flattened
        // design; "No editable history to check" gives way to the host's refusal.
        let keeps_reason = matches!(
            native["result"].as_str(),
            Some("Original shape kept without editable history" | "Partly checked")
        );
        match (guest_message, message(native)) {
            (Some(guest), Some(native)) if keeps_reason => assert!(
                guest.contains(&native),
                "{fixture}: verification must retain the CLI's reason: {guest} / {native}"
            ),
            (Some(_), _) if !keeps_reason => {}
            (guest, native) => assert_eq!(guest, native, "{fixture}: verification reason"),
        }
        assert_eq!(
            contents(&store.data().output),
            contents(&expected),
            "{fixture}: document and STEP bodies must match the CLI writer"
        );
        assert!(store.data().calls > 0);
        assert_eq!(store.data().progress.last(), Some(&1.0));
        eprintln!(
            "{fixture}: matched CLI document and body payloads ({} streamed reads): {summary}",
            store.data().calls
        );
        eprintln!(
            "peak_guest_memory fixture={fixture} input_bytes={} peak_mib={:.1}",
            source_len,
            store.data().limits.bytes as f64 / 1048576.0
        );
    }
    Ok(())
}

fn collect_fixtures(folder: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_fixtures(&entry.path(), files)?;
        } else if matches!(
            Format::from_path(&entry.path()),
            Some(
                Format::FusionArchive
                    | Format::FusionMultiDocument
                    | Format::InventorPart
                    | Format::InventorAssembly
                    | Format::SolidWorksPart
                    | Format::SolidWorksAssembly
                    | Format::FreeCadDocument
            )
        ) {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn fatal_member_failures(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = root.join("freecad-1.1.4-A01");
    let main = std::fs::read(root.join("A01a.FCStd"))?;
    for fault in [
        Fault::ReadDenied,
        Fault::ReadLimit,
        Fault::SizeDenied,
        Fault::SizeLimit,
        Fault::ResolveDenied,
        Fault::ResolveLimit,
        Fault::Aggregate,
        Fault::Oversize,
    ] {
        let mut host = host(&root, main.clone());
        host.fault = Some(fault);
        let mut store = Store::new(engine, host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(50_000_000_000)?;
        let instance = ImporterPlugin::instantiate(&mut store, component, linker)?;
        let result = import_file(
            &instance,
            &mut store,
            "convert",
            &source::FileInfo {
                name: "A01a.FCStd".into(),
                size: main.len() as u64,
            },
            "{}",
        )?;
        let expected = match fault {
            Fault::ReadDenied => PluginError::PermissionDenied(folder_access("Original source.read failure")),
            Fault::ReadLimit => PluginError::LimitExceeded("Original source.read failure".into()),
            Fault::SizeDenied => PluginError::PermissionDenied(folder_access("Original source.size failure")),
            Fault::SizeLimit => PluginError::LimitExceeded("Original source.size failure".into()),
            Fault::ResolveDenied => PluginError::PermissionDenied(folder_access("Original source.resolve failure")),
            Fault::ResolveLimit => PluginError::LimitExceeded("Original source.resolve failure".into()),
            Fault::Aggregate => PluginError::LimitExceeded("Input exceeds the 1 GiB conversion limit; choose a smaller CAD file".into()),
            Fault::Oversize => PluginError::LimitExceeded("P02a.FCStd: input exceeds the 1073741824-byte file limit; choose a smaller CAD file".into()),
            // Not in this loop; folder_not_granted covers it.
            Fault::FolderDenied => PluginError::PermissionDenied("input_folder was not granted".into()),
        };
        assert_eq!(
            format!("{:?}", result.unwrap_err()),
            format!("{expected:?}"),
            "{fault:?}"
        );
        assert!(store.data().good_member_read);
        assert!(
            store.data().output.is_empty(),
            "{fault:?}: partial output was written"
        );
        eprintln!("fatal_member_failure={fault:?}: original error, no output");
    }
    Ok(())
}

/// What the plugin says when SolidCraft refuses the read of the folder beside A01a.FCStd.
fn folder_access(host: &str) -> String {
    format!(
        "Dutchman needs read access to the folder that holds A01a.FCStd: the design refers to other files in that folder. It reads only that folder, never writes, and never looks elsewhere. Give it that access by approving \"read access to the input folder\" when you install Dutchman (Preferences > Plugins > Install plugin...), keep the parts beside the assembly, then open the file again. SolidCraft said: {host}"
    )
}

fn document(bytes: &[u8]) -> Value {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    serde_json::from_reader(zip.by_name("document.json").unwrap()).unwrap()
}
fn component_parents(doc: &Value) {
    for occurrence in doc["occurrences"].as_array().unwrap() {
        let parent = occurrence["parent"].as_u64().unwrap();
        assert!(
            parent == 0
                || doc["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["id"] == parent),
            "occurrence has no component parent: {occurrence}"
        );
    }
}
fn stand_up_replays(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = dutchman_solidcraft::find_cli(&Verification::Auto)?;
    let evaluator = CliEvaluator::new(cli);
    let main = std::fs::read(root.join("freecad-1.1.4-P04/P04a.FCStd"))?;
    let mut host = host(root, main.clone());
    host.evaluator = Some(evaluator.clone());
    let mut store = Store::new(engine, host);
    store.limiter(|host| &mut host.limits);
    store.set_fuel(50_000_000_000)?;
    let instance = ImporterPlugin::instantiate(&mut store, component, linker)?;
    let report = import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: "P04a.FCStd".into(),
            size: main.len() as u64,
        },
        r#"{"up_axis":"stand_up"}"#,
    )?
    .map_err(|e| format!("{e:?}"))?;
    let report: Value = serde_json::from_str(&report)?;
    let doc = document(&store.data().output);
    component_parents(&doc);
    assert!(
        doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "fillet"),
        "{report}"
    );
    let verified = report["details"]["shape_check"]["result"]
        == "Checked against the original shape"
        || report["details"]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["shape_check"]["result"] == "Checked against the original shape");
    assert!(verified, "fillet conversion was not verified: {report}");
    for feature in doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "fillet")
    {
        assert_ne!(
            feature["suppressed"], true,
            "mapped fillet was suppressed: {feature}"
        );
    }
    let replay = evaluator.evaluate(
        &store.data().output,
        std::time::Duration::from_secs(120),
        "stood-fillet",
    )?;
    assert!(
        replay["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|body| body["volume_mm3"].as_f64().unwrap() > 0.0),
        "{replay}"
    );
    for row in replay["timeline"].as_array().unwrap() {
        assert!(row["error"].is_null() || row["error"] == "", "{replay}");
    }
    let mut native_report = Report::default();
    let model = dutchman_core::read(
        Format::FreeCadDocument,
        &root.join("freecad-1.1.4-P04/P04a.FCStd"),
        &mut native_report,
    )?;
    let mut untransformed = Vec::new();
    SolidCraftWriter::new()
        .threads(1)
        .target_version(Some(SolidCraftVersion::default()))
        .evaluator(std::sync::Arc::new(evaluator.clone()))
        .hide_unverified_history(true)
        .write(&model, &mut untransformed, &mut native_report)?;
    let baseline = evaluator.evaluate(
        &untransformed,
        std::time::Duration::from_secs(120),
        "fillet-baseline",
    )?;
    let body = &baseline["bodies"][0];
    let stood_body = &replay["bodies"][0];
    for field in ["volume_mm3", "area_mm2"] {
        let expected = body[field].as_f64().unwrap();
        assert!(
            (stood_body[field].as_f64().unwrap() - expected).abs() < 1e-6 * expected.abs().max(1.0),
            "{field}: {replay} / {baseline}"
        );
    }
    for (side, other) in [("min", "max"), ("max", "min")] {
        let expected = [
            body["bbox"][side][0].as_f64().unwrap(),
            -body["bbox"][other][2].as_f64().unwrap(),
            body["bbox"][side][1].as_f64().unwrap(),
        ];
        for (axis, expected) in expected.into_iter().enumerate() {
            assert!(
                (stood_body["bbox"][side][axis].as_f64().unwrap() - expected).abs() < 1e-6,
                "Stand up bounds: {replay} / {baseline}"
            );
        }
    }
    eprintln!(
        "stand_up_fillet: mapped, verified, and replayed with rotated bounds: {}",
        report["summary"]
    );
    Ok(())
}

fn hidden_repeated_bodies(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Use the writer's actual repeated-part assembly layout, including its
    // component ownership and occurrence paths.
    let main = std::fs::read(root.join("placed_cubes.sldasm"))?;
    let mut store = Store::new(engine, host(root, main.clone()));
    store.limiter(|host| &mut host.limits);
    store.set_fuel(50_000_000_000)?;
    let instance = ImporterPlugin::instantiate(&mut store, component, linker)?;
    import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: "placed_cubes.sldasm".into(),
            size: main.len() as u64,
        },
        r#"{"editable_history":false}"#,
    )?
    .map_err(|e| format!("{e:?}"))?;
    let evaluator = CliEvaluator::new(dutchman_solidcraft::find_cli(&Verification::Auto)?);
    let original = &store.data().output;
    let measured = evaluator.evaluate(
        original,
        std::time::Duration::from_secs(120),
        "repeated-names",
    )?;
    let bodies = measured["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 2, "repeated assembly baseline: {measured}");
    let names: Vec<_> = bodies
        .iter()
        .map(|body| body["name"].as_str().unwrap().to_owned())
        .collect();
    let base = names.iter().find(|name| !name.ends_with(')')).unwrap();
    let qualified = names.iter().find(|name| *name != base).unwrap();
    assert!(qualified.starts_with(&format!("{base} (")), "{names:?}");
    let expected_qualified =
        qualified.replacen(&format!("{base} ("), &format!("{base} (Stand up:1/"), 1);
    let mut doc = document(original);
    doc["visibility"]["hidden_bodies"] = json!([base, qualified]);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in contents(original) {
        archive.start_file(&path, zip::write::SimpleFileOptions::default())?;
        archive.write_all(&if path == "document.json" {
            serde_json::to_vec(&doc)?
        } else {
            data
        })?;
    }
    let repeated = archive.finish()?.into_inner();
    let stood = stand_up::apply(&repeated)?;
    let doc = document(&stood);
    component_parents(&doc);
    let replay = evaluator.evaluate(
        &stood,
        std::time::Duration::from_secs(120),
        "hidden-repeated",
    )?;
    let names: Vec<_> = replay["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| body["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{replay}");
    let hidden = doc["visibility"]["hidden_bodies"].as_array().unwrap();
    assert_eq!(hidden, &vec![json!(base), json!(expected_qualified)]);
    for hidden in hidden {
        assert!(
            names.contains(&hidden.as_str().unwrap()),
            "hidden body has no evaluated world name: {hidden}: {replay}"
        );
    }
    let inspected = evaluator.run(
        &stood,
        &[json!({"command":"document.inspect", "params":{}})],
        std::time::Duration::from_secs(120),
        "hidden-visibility",
    )?;
    let bodies = inspected[0]["bodies"].as_array().unwrap();
    // document.inspect lists the shared component's local definition once;
    // eval above supplies the two occurrence-qualified world names.
    assert_eq!(bodies.len(), 1, "{inspected:?}");
    assert_eq!(
        bodies[0]["visible"], false,
        "hidden definition was displayed: {inspected:?}"
    );
    eprintln!(
        "stand_up_hidden_bodies: first and repeated placement names and visibility match SolidCraft evaluation"
    );
    Ok(())
}

/// An assembly whose input folder was not granted stops before reading anything, with
/// a message that names the grant and what it is for.
fn folder_not_granted(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let main = std::fs::read(root.join("placed_cubes.sldasm"))?;
    let mut host = host(root, main.clone());
    host.fault = Some(Fault::FolderDenied);
    let mut store = Store::new(engine, host);
    store.limiter(|host| &mut host.limits);
    store.set_fuel(50_000_000_000)?;
    let instance = ImporterPlugin::instantiate(&mut store, component, linker)?;
    let result = import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: "placed_cubes.sldasm".into(),
            size: main.len() as u64,
        },
        "{}",
    )?;
    let Err(PluginError::PermissionDenied(message)) = result else {
        return Err(format!("Expected a folder refusal, got {result:?}").into());
    };
    for needle in [
        "placed_cubes.sldasm",
        "read access to the input folder",
        "part files",
        "input_folder was not granted",
    ] {
        assert!(message.contains(needle), "{needle}: {message}");
    }
    assert_eq!(store.data().calls, 0, "nothing is read before the grant");
    assert!(store.data().output.is_empty());
    Ok(())
}
