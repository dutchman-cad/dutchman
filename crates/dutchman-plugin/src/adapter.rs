//! SolidCraft's `craft:plugin@0.2.0` importer contract. All guest ABI names live here.
//! `wit/craft-plugin.wit` is a copy of SolidCraft's `crates/craft-plugin/wit/plugin.wit`;
//! `wit/dutchman.wit` names the world that adds the dialog hook, the per-user settings and the
//! picked folder to its importer.
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "dutchman-importer",
    });
    pub use craft::plugin::{
        clock, control, design, evaluate, folders, log, source, types, user_storage,
    };

    pub fn add_to_linker<T>(linker: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
    where
        T: source::Host
            + design::Host
            + evaluate::Host
            + folders::Host
            + user_storage::Host
            + log::Host
            + control::Host
            + clock::Host
            + types::Host
            + Send
            + 'static,
    {
        DutchmanImporter::add_to_linker::<_, wasmtime::component::HasSelf<_>>(linker, |host| host)
    }

    /// The dialog hook: the form's field updates for a selected file.
    pub fn update_fields<T>(
        plugin: &DutchmanImporter,
        store: &mut wasmtime::Store<T>,
        entry: &str,
        context: &str,
    ) -> wasmtime::Result<Result<String, types::PluginError>> {
        plugin
            .craft_plugin_dialog_provider()
            .call_update_fields(store, entry, &context.to_owned())
    }

    pub fn import_file<T>(
        plugin: &DutchmanImporter,
        store: &mut wasmtime::Store<T>,
        entry: &str,
        file: &source::FileInfo,
        options: &str,
    ) -> wasmtime::Result<Result<String, types::PluginError>> {
        plugin
            .craft_plugin_importer()
            .call_import_file(store, entry, file, &options.to_owned())
    }
}
#[cfg(target_arch = "wasm32")]
mod guest {
    wit_bindgen::generate!({
        path: "wit",
        world: "dutchman-importer",
    });

    use craft::plugin::types::{FieldFailure, PluginError};
    use craft::plugin::{control, design, evaluate, folders, log, source, user_storage};
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    /// The manifest's only importer entry.
    const ENTRY: &str = "convert";

    /// SolidCraft's `evaluate` import: each design is staged and opened in a private host
    /// session, apart from every open design and every file, and discarded after its commands.
    #[derive(Debug)]
    struct HostSessions(std::sync::Arc<crate::progress::Progress>);

    /// Tell the operations panel where the conversion is; SolidCraft 0.48 draws it and
    /// older versions ignore it.
    fn report(step: Option<(f32, String)>) {
        if let Some((fraction, message)) = step {
            control::progress(fraction, &message);
        }
    }
    struct Staged;
    impl crate::evaluator::Session for Staged {
        fn run(
            &mut self,
            command: &str,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            let args = serde_json::to_string(args).map_err(|e| e.to_string())?;
            let raw =
                evaluate::run(command, &args).map_err(|e| format!("Host evaluation: {e:?}"))?;
            serde_json::from_str(&raw).map_err(|e| format!("Host evaluation JSON: {e}"))
        }
    }
    impl crate::evaluator::IsolatedHost for HostSessions {
        fn with_design(
            &self,
            design: &[u8],
            _timeout: std::time::Duration,
            _label: &str,
            execute: &mut crate::evaluator::Execute<'_>,
        ) -> Result<Vec<serde_json::Value>, String> {
            if control::cancelled() {
                return Err("Canceled".into());
            }
            report(self.0.check_started());
            // Staging after a run starts a new design; reset also drops the last one.
            evaluate::reset();
            for chunk in design.chunks(crate::CHUNK_BYTES) {
                evaluate::stage(chunk).map_err(|e| format!("Host evaluation: {e:?}"))?;
            }
            let result = execute(&mut Staged);
            evaluate::reset();
            result
        }
    }

    fn invalid(field: &str, reason: impl Into<String>) -> PluginError {
        PluginError::InvalidInput(FieldFailure {
            field: field.into(),
            reason: reason.into(),
        })
    }

    /// Where an assembly's part files come from: the folder of the opened file (the plugin's
    /// install grant, `input_folder`), or the folder the person picked in the dialog's parts
    /// folder field, which SolidCraft grants for one import and names after that field.
    #[derive(Clone, Copy, PartialEq)]
    enum Parts {
        Input,
        Picked,
    }
    impl Parts {
        /// The file being opened is always read through `source`, whichever folder holds the parts.
        fn read(self, path: &str, offset: u64, len: u32) -> Result<Vec<u8>, PluginError> {
            match self {
                Parts::Picked if !path.is_empty() => {
                    folders::read(crate::PARTS_FOLDER, path, offset, len)
                }
                _ => source::read(path, offset, len),
            }
        }
        fn size(self, path: &str) -> Result<Option<u64>, PluginError> {
            match self {
                Parts::Picked if !path.is_empty() => folders::size(crate::PARTS_FOLDER, path),
                _ => source::size(path),
            }
        }
        fn resolve(self, path: &str) -> Result<Option<String>, PluginError> {
            match self {
                Parts::Picked if !path.is_empty() => folders::resolve(crate::PARTS_FOLDER, path),
                _ => source::resolve(path),
            }
        }
        /// Ask for the grant itself, before anything is read.
        fn probe(self, file_name: &str) -> Result<(), PluginError> {
            match self {
                Parts::Input => source::resolve(file_name).map(drop),
                Parts::Picked => folders::resolve(crate::PARTS_FOLDER, "").map(drop),
            }
        }
    }

    struct Importer;
    impl exports::craft::plugin::dialog_provider::Guest for Importer {
        /// The form's defaults are the person's last accepted options. A file-aware update
        /// needs nothing from the file itself, and a settings read that fails leaves the
        /// manifest's defaults rather than keeping the dialog from opening.
        fn update_fields(entry: String, _context: String) -> Result<String, PluginError> {
            if entry != ENTRY {
                return Err(invalid(
                    "entry",
                    format!("Dutchman has one importer entry, \"{ENTRY}\""),
                ));
            }
            let stored = user_storage::get(crate::settings::KEY).ok().flatten();
            Ok(crate::settings::field_defaults(stored.as_deref()))
        }
    }
    impl exports::craft::plugin::importer::Guest for Importer {
        fn import_file(
            entry: String,
            file: source::FileInfo,
            options: String,
        ) -> Result<String, PluginError> {
            report_panics();
            if entry != ENTRY {
                return Err(invalid(
                    "entry",
                    format!("Dutchman has one importer entry, \"{ENTRY}\""),
                ));
            }
            crate::validate_name(&file.name).map_err(|e| invalid("file", e))?;
            if options.len() > 16 * 1024 {
                return Err(PluginError::LimitExceeded(
                    "Conversion options exceed 16 KiB; use the declared conversion options".into(),
                ));
            }
            let options: crate::Options = serde_json::from_str(&options)
                .map_err(|e| invalid("options", format!("Conversion options: {e}")))?;
            if control::cancelled() {
                return Err(PluginError::Cancelled);
            }
            control::progress(0.0, "Reading the design");
            // The folder field's value is only a switch: the path is never read, and the
            // grant it names exists only if SolidCraft saw the person pick the folder.
            let parts = match options.parts_folder.as_deref() {
                Some(path) if !path.is_empty() => Parts::Picked,
                _ => Parts::Input,
            };
            let saved = crate::settings::encode(&options);
            // An assembly cannot convert without its parts: ask for the folder now, before
            // reading the assembly, instead of failing on the first missing part.
            if crate::notice::needs_part_folder(&file.name) || parts == Parts::Picked {
                if let Err(e) = parts.probe(&file.name) {
                    return Err(folder_denied(&file.name, parts == Parts::Picked, e));
                }
            }
            // Parts are counted by size before they are read, so a design the guest cannot hold
            // is refused in one plain line instead of by a memory or fuel trap.
            let load = crate::preflight::PartsLoad::new(file.size);
            let progress = std::sync::Arc::new(crate::progress::Progress::default());
            let budget = Cell::new(0u64);
            let index_error = RefCell::new(None);
            let checked =
                if let Some(format) = dutchman_core::Format::from_path(Path::new(&file.name)) {
                    crate::preflight::check(
                        format,
                        file.size,
                        HostIndex {
                            size: file.size,
                            position: 0,
                            error: &index_error,
                        },
                        options.editable_history,
                    )
                } else {
                    Ok(())
                };
            if let Some(error) = index_error.into_inner() {
                return Err(error);
            }
            checked.map_err(|e| error(&file.name, e))?;
            let main_bytes = RefCell::new(Some(read(Parts::Input, "", file.size, &budget)?));
            report(progress.file_read());
            let files = Cell::new(1usize);
            // The first error a host import returned, passed back to the host unchanged.
            let host_error: RefCell<Option<PluginError>> = RefCell::new(None);
            let keep = |e: PluginError| {
                let e = folder_denied(&file.name, parts == Parts::Picked, e);
                let message = format!("Host input: {e:?}");
                host_error.borrow_mut().get_or_insert(e);
                dutchman_core::Error::Decode(message)
            };
            let reader = |path: &Path, limit: u64| {
                if host_error.borrow().is_some() {
                    return Err(dutchman_core::Error::Decode(
                        "Host input stopped after an earlier failure".into(),
                    ));
                }
                if path == Path::new(&file.name) {
                    return main_bytes.borrow_mut().take().ok_or_else(|| {
                        dutchman_core::Error::Decode("Main file requested more than once".into())
                    });
                }
                if control::cancelled() {
                    return Err(keep(PluginError::Cancelled));
                }
                let host_path = path.to_string_lossy();
                let size = parts.size(&host_path).map_err(keep)?.ok_or_else(|| {
                    dutchman_core::Error::Assembly(format!(
                        "{host_path}: part not found in {}",
                        if parts == Parts::Picked {
                            "the parts folder you picked"
                        } else {
                            "the input folder"
                        }
                    ))
                })?;
                if size > limit {
                    return Err(keep(PluginError::LimitExceeded(format!(
                        "{host_path}: input exceeds the {limit}-byte file limit; choose a smaller CAD file"
                    ))));
                }
                // The 1 GiB aggregate limit is `read`'s to report; the memory refusal is for
                // parts that are within it.
                if size <= (1u64 << 30).saturating_sub(budget.get()) {
                    if let Err(dutchman_core::Error::MemoryBudget(reason)) = load.add(size) {
                        return Err(keep(PluginError::LimitExceeded(reason)));
                    }
                }
                report(progress.part_read());
                files.set(files.get() + 1);
                if files.get() > 4096 {
                    return Err(keep(PluginError::LimitExceeded(
                        "More than 4096 input files; convert a smaller assembly".into(),
                    )));
                }
                read(parts, &host_path, size, &budget).map_err(keep)
            };
            let resolve = |stored: &str, folder: &Path| {
                let canonical = RefCell::new(None);
                crate::resolve(stored, folder, &|path| {
                    if host_error.borrow().is_some() {
                        return false;
                    }
                    match parts.resolve(&path.to_string_lossy()) {
                        Ok(Some(name)) => {
                            *canonical.borrow_mut() = Some(name);
                            true
                        }
                        Ok(None) => false,
                        Err(e) => {
                            keep(e);
                            false
                        }
                    }
                })?;
                let canonical = canonical.into_inner()?;
                // Validate host metadata as a bounded relative name too. Retain its
                // actual case: SolidWorks body names derive from the member's name.
                crate::resolve(&canonical, Path::new(""), &|path| {
                    path == Path::new(&canonical)
                })
            };
            let evaluator = std::sync::Arc::new(crate::evaluator::HostEvaluator(HostSessions(
                progress.clone(),
            )));
            let converted = crate::convert(&file.name, options, &reader, &resolve, Some(evaluator));
            // Assembly salvage may have succeeded. A fatal capability failure still
            // aborts the whole import, before any design bytes reach the host.
            if let Some(host) = host_error.take() {
                return Err(host);
            }
            let (design, report) = converted.map_err(|e| error(&file.name, e))?;
            if control::cancelled() {
                return Err(PluginError::Cancelled);
            }
            control::progress(0.95, "Writing the SolidCraft design");
            for chunk in design.chunks(crate::CHUNK_BYTES) {
                design::write(chunk)?;
            }
            // SolidCraft keeps this only when it accepts the design; a failure to stage it
            // never costs the person their conversion.
            if let Err(e) = user_storage::set(crate::settings::KEY, &saved) {
                note(&format!("Options not remembered: {e:?}"));
            }
            control::progress(1.0, "Converted");
            Ok(report)
        }
    }

    /// A refused folder read says which grant is missing, what it is for and how to give it.
    fn folder_denied(file_name: &str, picked: bool, e: PluginError) -> PluginError {
        match e {
            PluginError::PermissionDenied(host) => PluginError::PermissionDenied(if picked {
                crate::notice::parts_folder_access(file_name, &host)
            } else {
                crate::notice::folder_access(file_name, &host)
            }),
            other => other,
        }
    }

    /// A panic inside the plugin is a trap on this target (no unwinding), and the host sees only
    /// "unreachable". Say what panicked, and where, through the host's log before it traps.
    fn report_panics() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                log::log(log::Level::Error, &format!("Dutchman plugin panic: {info}"));
            }));
        });
    }

    fn note(message: &str) {
        log::log(log::Level::Info, message);
    }

    /// A format the plugin does not convert is `unsupported`, which a SolidCraft with a refusal
    /// dialog shows to the person; an unreadable file is invalid input; a size budget is a
    /// limit. Every message carries the full failure report (`notice::failure_in`).
    fn error(file_name: &str, e: dutchman_core::Error) -> PluginError {
        use dutchman_core::Error;
        let text = crate::notice::failure_in(file_name, &e);
        match e {
            Error::TooLarge { .. } | Error::MemoryBudget(_) => PluginError::LimitExceeded(text),
            Error::Encode(ref reason) if reason.contains("exceeds") => {
                PluginError::LimitExceeded(text)
            }
            Error::NotSupportedYet { .. } | Error::UnknownFormat { .. } => {
                PluginError::Unsupported(text)
            }
            _ => invalid("file", text),
        }
    }

    /// Random access for metadata: never materialize the input before checking its budget.
    struct HostIndex<'a> {
        size: u64,
        position: u64,
        error: &'a RefCell<Option<PluginError>>,
    }
    impl std::io::Read for HostIndex<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.error.borrow().is_some() {
                return Err(std::io::Error::other("Input stopped"));
            }
            if control::cancelled() {
                *self.error.borrow_mut() = Some(PluginError::Cancelled);
                return Err(std::io::Error::other("Canceled"));
            }
            let length = (buf.len() as u64)
                .min(self.size.saturating_sub(self.position))
                .min(crate::CHUNK_BYTES as u64) as u32;
            if length == 0 {
                return Ok(0);
            }
            let data = source::read("", self.position, length).map_err(|error| {
                *self.error.borrow_mut() = Some(error);
                std::io::Error::other("Could not read archive index")
            })?;
            if data.len() > length as usize {
                return Err(std::io::Error::other(
                    "Archive index exceeds requested bytes",
                ));
            }
            let target = buf
                .get_mut(..data.len())
                .ok_or_else(|| std::io::Error::other("Archive index exceeds buffer"))?;
            target.copy_from_slice(&data);
            self.position = self.position.saturating_add(data.len() as u64);
            Ok(data.len())
        }
    }
    impl std::io::Seek for HostIndex<'_> {
        fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
            let position = match from {
                std::io::SeekFrom::Start(n) => i128::from(n),
                std::io::SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
                std::io::SeekFrom::End(n) => i128::from(self.size) + i128::from(n),
            };
            if position < 0 || position > i128::from(self.size) {
                return Err(std::io::Error::other("Archive index exceeds file"));
            }
            self.position = position as u64;
            Ok(self.position)
        }
    }

    fn read(
        parts: Parts,
        path: &str,
        size: u64,
        budget: &Cell<u64>,
    ) -> Result<Vec<u8>, PluginError> {
        if size > dutchman_core::MAX_INPUT_BYTES || size > (1u64 << 30).saturating_sub(budget.get())
        {
            return Err(PluginError::LimitExceeded(
                "Input exceeds the 1 GiB conversion limit; choose a smaller CAD file".into(),
            ));
        }
        budget.set(budget.get() + size);
        let mut bytes = Vec::new();
        while (bytes.len() as u64) < size {
            let len = (size - bytes.len() as u64).min(crate::CHUNK_BYTES as u64) as u32;
            let chunk = parts.read(path, bytes.len() as u64, len)?;
            if chunk.is_empty() || chunk.len() > len as usize {
                return Err(invalid(
                    "file",
                    "Input stream ended early or exceeded the requested length; select the file again",
                ));
            }
            bytes.extend(chunk);
        }
        Ok(bytes)
    }

    // The canonical ABI thunk is generated by wit-bindgen, including raw pointers
    // and export_name attributes. No handwritten unsafe code belongs in this module.
    #[allow(unsafe_code)]
    mod component_abi {
        use super::*;
        export!(Importer with_types_in super);
    }
}
