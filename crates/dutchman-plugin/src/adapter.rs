//! SolidCraft's `craft:plugin@0.1.0` importer contract. All guest ABI names live here.
//! `wit/craft-plugin.wit` is a copy of SolidCraft's `crates/craft-plugin/wit/plugin.wit`.
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    wasmtime::component::bindgen!({
        path: "wit/craft-plugin.wit",
        world: "importer-plugin",
    });
    pub use craft::plugin::{clock, control, design, log, source, types};

    pub fn add_to_linker<T>(linker: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
    where
        T: source::Host
            + design::Host
            + log::Host
            + control::Host
            + clock::Host
            + types::Host
            + Send
            + 'static,
    {
        ImporterPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(linker, |host| host)
    }

    pub fn import_file<T>(
        plugin: &ImporterPlugin,
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
        path: "wit/craft-plugin.wit",
        world: "importer-plugin",
    });

    use craft::plugin::types::{FieldFailure, PluginError};
    use craft::plugin::{control, design, source};
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    /// The manifest's only importer entry.
    const ENTRY: &str = "convert";

    // TODO(eval-seam, PR #580): the importer world has no command import yet. When the host
    // supplies isolated archive sessions, bind its reviewed run import here and enable this
    // bridge ONLY inside IsolatedHost::with_design. It must never target the active design.
    #[cfg(any())]
    struct ScopedHostRun;
    #[cfg(any())]
    impl crate::evaluator::Session for ScopedHostRun {
        fn run(
            &mut self,
            command: &str,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            let args = serde_json::to_string(args).map_err(|e| e.to_string())?;
            let raw = craft::plugin::commands::run(command, &args)
                .map_err(|e| format!("Host evaluation transport: {e:?}"))?;
            if raw.len() > 64 << 20 {
                return Err("Host evaluation result exceeds 64 MiB".into());
            }
            serde_json::from_str(&raw).map_err(|e| format!("Host evaluation JSON: {e}"))
        }
    }

    fn invalid(field: &str, reason: impl Into<String>) -> PluginError {
        PluginError::InvalidInput(FieldFailure {
            field: field.into(),
            reason: reason.into(),
        })
    }

    struct Importer;
    impl exports::craft::plugin::importer::Guest for Importer {
        fn import_file(
            entry: String,
            file: source::FileInfo,
            options: String,
        ) -> Result<String, PluginError> {
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
            let options = serde_json::from_str(&options)
                .map_err(|e| invalid("options", format!("Conversion options: {e}")))?;
            if control::cancelled() {
                return Err(PluginError::Cancelled);
            }
            control::progress(0.0, "Reading the design");
            let budget = Cell::new(0u64);
            let main_bytes = RefCell::new(Some(read("", file.size, &budget)?));
            let files = Cell::new(1usize);
            // The first error a host import returned, passed back to the host unchanged.
            let host_error: RefCell<Option<PluginError>> = RefCell::new(None);
            let keep = |e: PluginError| {
                let message = format!("Host input: {e:?}");
                host_error.borrow_mut().get_or_insert(e);
                dutchman_core::Error::Decode(message)
            };
            let reader = |path: &Path, limit: u64| {
                if path == Path::new(&file.name) {
                    return main_bytes.borrow_mut().take().ok_or_else(|| {
                        dutchman_core::Error::Decode("Main file requested more than once".into())
                    });
                }
                if control::cancelled() {
                    return Err(keep(PluginError::Cancelled));
                }
                let host_path = path.to_string_lossy();
                let size = source::size(&host_path).map_err(keep)?.ok_or_else(|| {
                    dutchman_core::Error::Assembly(format!(
                        "{host_path}: part not found in the input folder"
                    ))
                })?;
                if size > limit {
                    return Err(dutchman_core::Error::TooLarge {
                        path: path.into(),
                        bytes: size,
                        limit,
                    });
                }
                files.set(files.get() + 1);
                if files.get() > 4096 {
                    return Err(keep(PluginError::LimitExceeded(
                        "More than 4096 input files; convert a smaller assembly".into(),
                    )));
                }
                read(&host_path, size, &budget).map_err(keep)
            };
            let resolve = |stored: &str, folder: &Path| {
                let requested = crate::resolve(stored, folder, &|path| {
                    source::resolve(&path.to_string_lossy())
                        .ok()
                        .flatten()
                        .is_some()
                })?;
                let canonical = source::resolve(&requested.to_string_lossy())
                    .ok()
                    .flatten()?;
                // Validate host metadata as a bounded relative name too. Retain its
                // actual case: SolidWorks body names derive from the member's name.
                crate::resolve(&canonical, Path::new(""), &|path| {
                    path == Path::new(&canonical)
                })
            };
            let converted = crate::convert(&file.name, options, &reader, &resolve);
            let (design, report) = converted.map_err(|e| match host_error.take() {
                Some(host) => host,
                None => error(e),
            })?;
            if control::cancelled() {
                return Err(PluginError::Cancelled);
            }
            control::progress(0.95, "Writing the SolidCraft design");
            for chunk in design.chunks(crate::CHUNK_BYTES) {
                design::write(chunk)?;
            }
            control::progress(1.0, "Converted");
            Ok(report)
        }
    }

    /// An unsupported or unreadable file is invalid input; a size budget is a limit.
    fn error(e: dutchman_core::Error) -> PluginError {
        use dutchman_core::Error;
        match e {
            Error::TooLarge { .. } | Error::MemoryBudget(_) => {
                PluginError::LimitExceeded(e.to_string())
            }
            Error::Encode(ref reason) if reason.contains("exceeds") => {
                PluginError::LimitExceeded(e.to_string())
            }
            _ => invalid("file", e.to_string()),
        }
    }

    fn read(path: &str, size: u64, budget: &Cell<u64>) -> Result<Vec<u8>, PluginError> {
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
            let chunk = source::read(path, bytes.len() as u64, len)?;
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
