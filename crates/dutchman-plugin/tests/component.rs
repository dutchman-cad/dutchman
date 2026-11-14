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
use adapter::host::{
    add_to_linker, clock, control, design, evaluate, import_file, log, source, types,
    ImporterPlugin,
};
use dutchman_core::{Format, PartSearch, Report, Writer};
use dutchman_solidcraft::{SolidCraftVersion, SolidCraftWriter, Verification};
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::Instant,
};
use types::{FieldFailure, PluginError};
use wasmtime::{
    component::{Component, Linker},
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder,
};

struct Host {
    root: PathBuf,
    main: Vec<u8>,
    output: Vec<u8>,
    limits: StoreLimits,
    read: usize,
    calls: usize,
    progress: Vec<f32>,
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
// No SolidCraft here: every check is unverified, as the CLI's output this is compared with.
impl evaluate::Host for Host {
    fn stage(&mut self, _bytes: Vec<u8>) -> Result<(), PluginError> {
        Ok(())
    }
    fn run(&mut self, _command: String, _args: String) -> Result<String, PluginError> {
        Err(PluginError::PermissionDenied(
            "evaluate: the test host has no SolidCraft".into(),
        ))
    }
    fn reset(&mut self) {}
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
        assert!(len as usize <= limits::CHUNK_BYTES);
        let member;
        let bytes = if path.is_empty() {
            &self.main
        } else {
            assert!(safe(&path));
            member = std::fs::read(self.root.join(path)).map_err(failed)?;
            &member
        };
        let start =
            usize::try_from(offset).map_err(|e| PluginError::LimitExceeded(e.to_string()))?;
        let end = start.saturating_add(len as usize).min(bytes.len());
        let chunk = bytes.get(start..end).unwrap_or_default().to_vec();
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
        limits: StoreLimitsBuilder::new()
            .memory_size(1024 * 1024 * 1024)
            .build(),
        read: 0,
        calls: 0,
        progress: Vec::new(),
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
    assert_eq!(store.data().calls, 3);
    assert_eq!(store.data().read, stream_len);
    assert!(store.data().output.is_empty());
    drop(store);

    let mut fixtures = Vec::new();
    collect_fixtures(&root, &mut fixtures)?;
    fixtures.sort();
    for source in fixtures {
        let fixture = source.strip_prefix(&root)?.display().to_string();
        let folder = source.parent().unwrap();
        let name = source.file_name().unwrap().to_str().unwrap();
        let bytes = std::fs::read(&source)?;
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
        assert_eq!(details["schema"], "dutchman.plugin.loss/1");
        let native_report = loss_report::json(&cli_report);
        // The harness's evaluate refuses every check where the CLI had verification off: the
        // same unchecked status, and the CLI's reason within the plugin's, which names the host's
        // refusal where the CLI says verification is off (and may say which check it was).
        let guest = &details["outcome"]["verification"];
        let native = &native_report["outcome"]["verification"];
        assert_eq!(
            guest["status"], native["status"],
            "{fixture}: verification must retain the CLI's unchecked status"
        );
        let message = |value: &serde_json::Value| value["message"].as_str().map(str::to_owned);
        let guest_message =
            message(guest).map(|text| text.replace(NO_HOST_EVALUATION, "verification turned off"));
        match (guest_message, message(native)) {
            (Some(guest), Some(native)) => assert!(
                guest.contains(&native),
                "{fixture}: verification must retain the CLI's reason: {guest} / {native}"
            ),
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
