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
#[path = "../src/preflight.rs"]
#[allow(dead_code)]
mod preflight;
#[path = "../src/stand_up.rs"]
mod stand_up;
use adapter::host::{
    add_to_linker, clock, control, design, evaluate, folders, import_file, log, source, types,
    update_fields, user_storage, DutchmanImporter,
};
use dutchman_core::{Format, PartSearch, Report, Writer};
use dutchman_solidcraft::{
    CliEvaluator, SolidCraftEvaluator, SolidCraftVersion, SolidCraftWriter, Verification,
};
use limits::MAX_REPORT_BYTES;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
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
    /// The folder SolidCraft granted for the `parts_folder` field of this import, if the
    /// person picked one. `None` is what the real host answers when no grant exists.
    picked: Option<PathBuf>,
    folder_reads: usize,
    folder_bytes: usize,
    /// Per-user settings as SolidCraft holds them: what is saved, and what this import has
    /// staged. Staged settings are published only when the host accepts the import.
    settings: BTreeMap<String, String>,
    staged_settings: BTreeMap<String, Option<String>>,
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
    PartTooBig,
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
            if let Some(Fault::PartTooBig) = self.fault {
                self.input_failed = true;
                return Ok(Some(900_000_000));
            }
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
/// The text the real host gives a plugin that asks for a folder it was not granted.
const NO_PICKED_FOLDER: &str = "folders:parts_folder: no approved directory for this entry";
impl Host {
    fn picked(&mut self, path: &str) -> Result<PathBuf, PluginError> {
        let Some(root) = &self.picked else {
            return Err(PluginError::PermissionDenied(NO_PICKED_FOLDER.into()));
        };
        if !path.is_empty() && !safe(path) {
            return Err(PluginError::PermissionDenied(
                "folders:parts_folder: path must stay inside the directory".into(),
            ));
        }
        Ok(root.join(path))
    }
    /// SolidCraft publishes staged settings only when it accepts a successful import.
    fn commit_staged(&mut self) {
        for (key, value) in std::mem::take(&mut self.staged_settings) {
            match value {
                Some(value) => self.settings.insert(key, value),
                None => self.settings.remove(&key),
            };
        }
    }
}
impl folders::Host for Host {
    fn read(
        &mut self,
        grant: String,
        path: String,
        offset: u64,
        len: u32,
    ) -> Result<Vec<u8>, PluginError> {
        assert_eq!(grant, "parts_folder", "the grant is the property key");
        assert!(len as usize <= limits::CHUNK_BYTES);
        let target = self.picked(&path)?;
        let bytes = std::fs::read(target).map_err(failed)?;
        let start = usize::try_from(offset).unwrap();
        let end = start.saturating_add(len as usize).min(bytes.len());
        self.folder_reads += 1;
        self.folder_bytes += end.saturating_sub(start);
        Ok(bytes.get(start..end).unwrap_or_default().to_vec())
    }
    fn size(&mut self, grant: String, path: String) -> Result<Option<u64>, PluginError> {
        assert_eq!(grant, "parts_folder");
        let target = self.picked(&path)?;
        Ok(target
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len()))
    }
    fn resolve(&mut self, grant: String, path: String) -> Result<Option<String>, PluginError> {
        assert_eq!(grant, "parts_folder");
        let target = self.picked(&path)?;
        if path.is_empty() {
            return Ok(Some(String::new()));
        }
        let mut found = None;
        let (parent, wanted) = (
            target.parent().unwrap().to_path_buf(),
            target.file_name().unwrap().to_string_lossy().to_lowercase(),
        );
        let Ok(listing) = std::fs::read_dir(parent) else {
            return Ok(None);
        };
        for entry in listing.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_lowercase() == wanted && entry.path().is_file() {
                found = Some(name);
                if entry.file_name() == target.file_name().unwrap() {
                    break;
                }
            }
        }
        Ok(found.map(|name| match path.rsplit_once('/') {
            Some((folder, _)) => format!("{folder}/{name}"),
            None => name,
        }))
    }
}
impl user_storage::Host for Host {
    fn get(&mut self, key: String) -> Result<Option<String>, PluginError> {
        Ok(match self.staged_settings.get(&key) {
            Some(staged) => staged.clone(),
            None => self.settings.get(&key).cloned(),
        })
    }
    fn set(&mut self, key: String, value: String) -> Result<(), PluginError> {
        serde_json::from_str::<Value>(&value).map_err(|e| {
            PluginError::InvalidInput(FieldFailure {
                field: key.clone(),
                reason: e.to_string(),
            })
        })?;
        self.staged_settings.insert(key, Some(value));
        Ok(())
    }
    fn remove(&mut self, key: String) -> Result<bool, PluginError> {
        let existed = self.get(key.clone())?.is_some();
        self.staged_settings.insert(key, None);
        Ok(existed)
    }
    fn list(&mut self) -> Result<Vec<String>, PluginError> {
        let mut keys: Vec<_> = self.settings.keys().cloned().collect();
        keys.extend(self.staged_settings.keys().cloned());
        keys.sort();
        keys.dedup();
        keys.retain(|key| self.staged_settings.get(key).is_none_or(Option::is_some));
        Ok(keys)
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
        picked: None,
        folder_reads: 0,
        folder_bytes: 0,
        settings: BTreeMap::new(),
        staged_settings: BTreeMap::new(),
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
/// Every Fusion design among the fixtures must come back from the component as a result or a
/// refusal, never a trap. A panic inside the guest (a thread that cannot start, a clock) is a
/// trap on this target, and the plugin's panic hook names it through the host log above.
#[test]
fn fusion_designs_do_not_trap() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let path = std::env::var("DUTCHMAN_PLUGIN_COMPONENT").map_err(|_| {
                "Set DUTCHMAN_PLUGIN_COMPONENT to the wasm32-unknown-unknown component"
            })?;
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
            let component = Component::from_file(&engine, &path)?;
            let mut linker = Linker::new(&engine);
            add_to_linker(&mut linker)?;
            let root =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../dutchman-core/tests/fixtures");
            let mut fixtures = Vec::new();
            collect_fixtures(&root, &mut fixtures)?;
            fixtures.sort();
            let mut refused = Vec::new();
            for source in fixtures
                .iter()
                .filter(|path| path.extension().is_some_and(|e| e == "f3d"))
            {
                let name = source.file_name().unwrap().to_str().unwrap();
                let bytes = std::fs::read(source)?;
                let size = bytes.len() as u64;
                let host = host(source.parent().unwrap(), bytes);
                let (result, _) = convert_in(&engine, &component, &linker, host, name, size, "{}")
                    .map_err(|trap| format!("{name}: the component trapped: {trap}"))?;
                // What the native pipeline (the app) does with the same file.
                let mut native_report = Report::default();
                let mut native_out = Vec::new();
                let native = dutchman_core::read_in(
                    Format::from_path(source).unwrap(),
                    source,
                    &PartSearch {
                        threads: 1,
                        project: source.parent().map(Into::into),
                        ..PartSearch::default()
                    },
                    &mut native_report,
                )
                .and_then(|model| {
                    SolidCraftWriter::new()
                        .threads(1)
                        .target_version(Some(SolidCraftVersion::default()))
                        .verification(Verification::Off)
                        .hide_unverified_history(true)
                        .write(&model, &mut native_out, &mut native_report)
                });
                match (result, native) {
                    (Ok(_), _) => eprintln!("{name}: converted"),
                    (Err(reason), native) => {
                        eprintln!("{name}: plugin refused: {reason:?}; native: {native:?}");
                        if native.is_ok() {
                            refused.push(format!("{name}: {reason:?}"));
                        }
                    }
                }
            }
            let unexpected = &refused;
            assert!(
                unexpected.is_empty(),
                "Fusion fixtures the app converts but the plugin refused: {unexpected:#?}"
            );
            Ok(())
        })?
        .join()
        .map_err(|_| "Component harness worker failed")?
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
    let instance = DutchmanImporter::instantiate(&mut store, &component, &linker)?;
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
        let instance = DutchmanImporter::instantiate(&mut store, &component, &linker)?;
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
    let instance = DutchmanImporter::instantiate(&mut store, &component, &linker)?;
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
    picked_parts_folder(&engine, &component, &linker, &root)?;
    part_too_big_for_the_guest(&engine, &component, &linker, &root)?;
    failures_carry_the_full_report(&engine, &component, &linker, &root)?;
    picked_folder_not_granted(&engine, &component, &linker, &root)?;
    settings_are_saved_and_offered(&engine, &component, &linker, &root)?;
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
        let instance = DutchmanImporter::instantiate(&mut store, &component, &linker)?;
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
        assert!(
            store.data().progress.windows(2).all(|w| w[0] <= w[1]),
            "{fixture}: progress must never go back: {:?}",
            store.data().progress
        );
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
        let instance = DutchmanImporter::instantiate(&mut store, component, linker)?;
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
            // Not in this loop; part_too_big_for_the_guest covers it.
            Fault::PartTooBig => PluginError::LimitExceeded(String::new()),
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
    let instance = DutchmanImporter::instantiate(&mut store, component, linker)?;
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
    let instance = DutchmanImporter::instantiate(&mut store, component, linker)?;
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
    // SolidCraft up to 0.49 lists the shared component's local definition once; 0.50 lists each
    // occurrence-qualified body. Either way, nothing hidden may be displayed.
    assert!(
        (1..=2).contains(&bodies.len()),
        "document.inspect lists the definition or both occurrences: {inspected:?}"
    );
    for body in bodies {
        assert_eq!(
            body["visible"], false,
            "hidden definition was displayed: {inspected:?}"
        );
    }
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
    let instance = DutchmanImporter::instantiate(&mut store, component, linker)?;
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

/// A conversion's result and the store it ran in.
type Converted = (Result<String, PluginError>, Store<Host>);

/// One conversion in a fresh instance of the component.
fn convert_in(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    host: Host,
    name: &str,
    size: u64,
    options: &str,
) -> Result<Converted, Box<dyn std::error::Error + Send + Sync>> {
    let mut store = Store::new(engine, host);
    store.limiter(|host| &mut host.limits);
    store.set_fuel(50_000_000_000)?;
    let instance = DutchmanImporter::instantiate(&mut store, component, linker)?;
    let result = import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: name.into(),
            size,
        },
        options,
    )?;
    Ok((result, store))
}

/// A folder the person picked in the dialog holds the parts; the assembly's own folder holds
/// nothing. The conversion reads every part through the `parts_folder` grant and matches the
/// one that reads the folder beside the file.
fn picked_parts_folder(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parts = root.join("freecad-1.1.4-A01");
    let main = std::fs::read(parts.join("A01a.FCStd"))?;
    let size = main.len() as u64;
    let (beside, besides_store) = convert_in(
        engine,
        component,
        linker,
        host(&parts, main.clone()),
        "A01a.FCStd",
        size,
        "{}",
    )?;
    beside.map_err(|e| format!("{e:?}"))?;
    assert_eq!(besides_store.data().folder_reads, 0);
    let elsewhere = Path::new(env!("CARGO_TARGET_TMPDIR")).join("dutchman-plugin-empty-input");
    std::fs::create_dir_all(&elsewhere)?;
    let mut picked = host(&elsewhere, main);
    picked.picked = Some(parts.clone());
    let (result, store) = convert_in(
        engine,
        component,
        linker,
        picked,
        "A01a.FCStd",
        size,
        r#"{"parts_folder":"/the/folder/the/person/picked"}"#,
    )?;
    result.map_err(|e| format!("{e:?}"))?;
    assert!(
        store.data().folder_reads > 0,
        "the parts come through the folder grant"
    );
    assert_eq!(
        contents(&store.data().output),
        contents(&besides_store.data().output),
        "a picked folder converts the same design as the folder beside the file"
    );
    // Dutchman never looks for a part anywhere else: nothing but the file itself came through
    // `source`, whose folder was empty.
    assert!(store.data().calls > 0);
    eprintln!("picked_parts_folder: parts read through the grant, same design as beside the file");
    Ok(())
}

/// The field is filled but SolidCraft granted no folder: the refusal names that grant, the
/// field to use and what it is for, and nothing is read first.
fn picked_folder_not_granted(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let main = std::fs::read(root.join("freecad-1.1.4-A01/A01a.FCStd"))?;
    let size = main.len() as u64;
    let (result, store) = convert_in(
        engine,
        component,
        linker,
        host(root, main),
        "A01a.FCStd",
        size,
        r#"{"parts_folder":"/saved/value/is/not/consent"}"#,
    )?;
    let Err(PluginError::PermissionDenied(message)) = result else {
        return Err(format!("Expected a folder refusal, got {result:?}").into());
    };
    for needle in [
        "parts folder for A01a.FCStd",
        "\"Folder with the parts\"",
        "SolidCraft has not granted it",
        "never writes",
        NO_PICKED_FOLDER,
    ] {
        assert!(message.contains(needle), "{needle}: {message}");
    }
    assert!(
        !message.contains("input folder"),
        "it must name the picked folder's grant, not the install grant: {message}"
    );
    assert_eq!(store.data().calls, 0, "nothing is read before the grant");
    assert!(store.data().output.is_empty());
    assert!(store.data().staged_settings.is_empty());
    Ok(())
}

type HookRun = (Result<String, PluginError>, Store<Host>);

/// The three dialog options are saved after an accepted import and offered back as the
/// dialog's defaults; a failed import saves nothing, and damaged settings change nothing.
fn settings_are_saved_and_offered(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cube = std::fs::read(root.join("generated_unit_cube.f3d"))?;
    let size = cube.len() as u64;
    let hook = |host: Host,
                entry: &str|
     -> Result<HookRun, Box<dyn std::error::Error + Send + Sync>> {
        let mut store = Store::new(engine, host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(50_000_000)?;
        let instance = DutchmanImporter::instantiate(&mut store, component, linker)?;
        let context = r#"{"values":{},"file":{"name":"cube.f3d","extension":"f3d","size":1,"format":"dutchman.convert","generation":1}}"#;
        let reply = update_fields(&instance, &mut store, entry, context)?;
        Ok((reply, store))
    };
    // Nothing saved yet: the form keeps the manifest's defaults.
    let (reply, _) = hook(host(root, Vec::new()), "convert")?;
    assert_eq!(
        serde_json::from_str::<Value>(&reply.unwrap())?,
        json!({"fields":{}})
    );
    // An accepted import saves the options it ran with, and only after the host accepts it.
    let chosen = r#"{"editable_history":false,"up_axis":"stand_up","include_hidden_bodies":true,"parts_folder":"/not/saved"}"#;
    let (result, mut store) = convert_in(
        engine,
        component,
        linker,
        {
            // The person picked a folder this time; it is used for this import and not saved.
            let mut host = host(root, cube.clone());
            host.picked = Some(root.to_path_buf());
            host
        },
        "generated_unit_cube.f3d",
        size,
        chosen,
    )?;
    result.map_err(|e| format!("{e:?}"))?;
    assert!(store.data().settings.is_empty(), "staged until accepted");
    store.data_mut().commit_staged();
    let saved = store.data().settings.get("options").cloned().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved)?,
        json!({"editable_history":false,"up_axis":"stand_up","include_hidden_bodies":true})
    );
    assert_eq!(store.data().settings.len(), 1);
    // The next dialog offers them.
    let mut next = host(root, Vec::new());
    next.settings = store.data().settings.clone();
    let (reply, _) = hook(next, "convert")?;
    assert_eq!(
        serde_json::from_str::<Value>(&reply.unwrap())?,
        json!({"fields":{
            "editable_history":{"default":false},
            "up_axis":{"default":"stand_up"},
            "include_hidden_bodies":{"default":true}
        }})
    );
    // A failed import stages nothing.
    let (failed, store) = convert_in(
        engine,
        component,
        linker,
        host(root, vec![0; 16]),
        "broken.f3d",
        16,
        chosen,
    )?;
    assert!(failed.is_err());
    assert!(store.data().staged_settings.is_empty());
    // Settings that do not parse, or hold a value the form does not offer, change nothing.
    let mut damaged = host(root, Vec::new());
    damaged.settings.insert(
        "options".into(),
        r#"{"editable_history":"yes","up_axis":"sideways","include_hidden_bodies":true}"#.into(),
    );
    let (reply, _) = hook(damaged, "convert")?;
    assert_eq!(
        serde_json::from_str::<Value>(&reply.unwrap())?,
        json!({"fields":{"include_hidden_bodies":{"default":true}}})
    );
    let mut garbage = host(root, Vec::new());
    garbage.settings.insert("options".into(), "not json".into());
    let (reply, _) = hook(garbage, "convert")?;
    assert_eq!(
        serde_json::from_str::<Value>(&reply.unwrap())?,
        json!({"fields":{}})
    );
    let (wrong, _) = hook(host(root, Vec::new()), "other")?;
    assert!(matches!(wrong, Err(PluginError::InvalidInput(_))));
    eprintln!(
        "settings: saved after an accepted import, offered as defaults, none after a failure"
    );
    Ok(())
}

/// Opt-in measurement of one large assembly through the real component: set
/// `DUTCHMAN_PLUGIN_ASSEMBLY` to the absolute path of an `.iam` or `.sldasm` whose parts sit
/// beside it. It prints the guest's peak linear memory, the fuel used, and the time, or the plugin's
/// refusal, and checks only that the result is not a trap.
#[test]
fn measure_one_large_assembly() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(assembly) = std::env::var_os("DUTCHMAN_PLUGIN_ASSEMBLY").map(PathBuf::from) else {
        eprintln!("measure_one_large_assembly: DUTCHMAN_PLUGIN_ASSEMBLY not set; not measuring");
        return Ok(());
    };
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || measure(&assembly))?
        .join()
        .map_err(|_| "measurement worker failed")?
}

fn measure(assembly: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::var("DUTCHMAN_PLUGIN_COMPONENT")
        .map_err(|_| "Set DUTCHMAN_PLUGIN_COMPONENT to the wasm32-unknown-unknown component")?;
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .consume_fuel(true)
        .max_wasm_stack(16 * 1024 * 1024)
        .async_stack_size(18 * 1024 * 1024);
    let engine = Engine::new(&config)?;
    let component = Component::from_file(&engine, &path)?;
    let mut linker = Linker::new(&engine);
    add_to_linker(&mut linker)?;
    let main = std::fs::read(assembly)?;
    let name = assembly.file_name().unwrap().to_str().unwrap();
    let mut host = host(assembly.parent().unwrap(), main.clone());
    host.evaluator = None;
    // With DUTCHMAN_PLUGIN_PARTS_ROOT the parts come from that folder, through the grant the
    // person would pick in the dialog; without it, from the folder beside the file.
    let parts_root = std::env::var_os("DUTCHMAN_PLUGIN_PARTS_ROOT").map(PathBuf::from);
    let options = match &parts_root {
        Some(_) => r#"{"editable_history":true,"parts_folder":"/picked"}"#,
        None => r#"{"editable_history":true}"#,
    };
    host.picked = parts_root;
    let mut store = Store::new(&engine, host);
    store.limiter(|host| &mut host.limits);
    // SolidCraft's own ceiling is 100,000,000,000; measure with room to finish.
    let fuel = std::env::var("DUTCHMAN_PLUGIN_FUEL")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(2_000_000_000_000u64);
    store.set_fuel(fuel)?;
    let instance = DutchmanImporter::instantiate(&mut store, &component, &linker)?;
    let started = Instant::now();
    let result = import_file(
        &instance,
        &mut store,
        "convert",
        &source::FileInfo {
            name: name.into(),
            size: main.len() as u64,
        },
        options,
    );
    let used = fuel - store.get_fuel().unwrap_or(0);
    eprintln!(
        "assembly_measurement file={name} main_bytes={} peak_guest_mib={:.1} fuel_used={used} seconds={:.1} reads={} read_bytes={} folder_reads={} folder_bytes={} output_bytes={}",
        main.len(),
        store.data().limits.bytes as f64 / 1048576.0,
        started.elapsed().as_secs_f64(),
        store.data().calls,
        store.data().read,
        store.data().folder_reads,
        store.data().folder_bytes,
        store.data().output.len(),
    );
    match result {
        Ok(Ok(report)) => {
            let report: Value = serde_json::from_str(&report)?;
            eprintln!(
                "assembly_measurement result=converted summary={} assembly={} items={}",
                report["summary"],
                report["details"]["assembly"],
                report["items"]
                    .as_array()
                    .map(|items| items
                        .iter()
                        .take(8)
                        .map(|i| format!("{}: {}", i["severity"], i["title"]))
                        .collect::<Vec<_>>()
                        .join(" | "))
                    .unwrap_or_default()
            )
        }
        Ok(Err(refusal)) => eprintln!("assembly_measurement result=refused {refusal:?}"),
        Err(trap) => {
            eprintln!("assembly_measurement result=trap {trap:#}");
            return Err(format!("trap: {trap}").into());
        }
    }
    Ok(())
}

/// A part whose size alone would not fit the guest's memory is refused before it is read, in
/// one plain line that says which limit, needed against available, and where to go.
fn part_too_big_for_the_guest(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let folder = root.join("freecad-1.1.4-A01");
    let main = std::fs::read(folder.join("A01a.FCStd"))?;
    let size = main.len() as u64;
    let mut host = host(&folder, main);
    host.fault = Some(Fault::PartTooBig);
    let (result, store) = convert_in(engine, component, linker, host, "A01a.FCStd", size, "{}")?;
    let Err(PluginError::LimitExceeded(message)) = result else {
        return Err(format!("Expected a size refusal, got {result:?}").into());
    };
    assert!(
        message.starts_with("This design is too large to open inside SolidCraft: it needs about "),
        "{message}"
    );
    assert!(
        message.contains(" MB of memory for the 90")
            && message.contains(" MB of files it reads, and the plugin has 1700 MB"),
        "{message}"
    );
    assert!(message.contains("Open it in the Dutchman app"), "{message}");
    assert_eq!(message.lines().count(), 1, "{message}");
    assert!(store.data().output.is_empty());
    Ok(())
}

/// A failed conversion returns its full `dutchman.convert/1` report in the error text, which is
/// the one path a plugin has (SolidCraft gives it no clipboard and no file to write); a format it
/// does not convert is `unsupported`, the kind a refusal dialog shows.
fn failures_carry_the_full_report(
    engine: &Engine,
    component: &Component,
    linker: &Linker<Host>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let report_of = |message: &str| -> Value {
        assert!(message.contains("Dutchman app"), "{message}");
        serde_json::from_str(message.lines().last().unwrap()).unwrap()
    };
    let (broken, store) = convert_in(
        engine,
        component,
        linker,
        host(root, vec![0; 64]),
        "broken.f3d",
        64,
        "{}",
    )?;
    let Err(PluginError::InvalidInput(FieldFailure { reason, .. })) = broken else {
        return Err(format!("Expected invalid input, got {broken:?}").into());
    };
    let report = report_of(&reason);
    assert_eq!(report["schema"], "dutchman.convert/1");
    assert_eq!(report["ok"], false);
    assert_eq!(report["input"]["path"], "broken.f3d");
    assert!(report["error"]["kind"].is_string() && report["error"]["stage"].is_string());
    assert!(reason.contains("broken.failed.json"), "{reason}");
    assert!(store.data().output.is_empty());
    let (unknown, _) = convert_in(
        engine,
        component,
        linker,
        host(root, vec![0; 4]),
        "weird.xyz",
        4,
        "{}",
    )?;
    let Err(PluginError::Unsupported(message)) = unknown else {
        return Err(format!("Expected unsupported, got {unknown:?}").into());
    };
    assert_eq!(report_of(&message)["error"]["kind"], "unknown_format");
    Ok(())
}
