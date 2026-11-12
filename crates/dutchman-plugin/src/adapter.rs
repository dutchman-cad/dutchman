//! Provisional RFC 0002 / #4980 contract. All guest ABI names live here.
//! Replace the inline WIT with craft-plugin's reviewed WIT when it is published.
macro_rules! contract {
    ($bind:ident) => { $bind!({
    inline: r#"
        package solidcraft:plugin@1.0.0;
        interface types {
            variant plugin-error { failed(string), denied(string), limit(string), invalid(string) }
        }
        interface input {
            use types.{plugin-error};
            record file-info { name: string, size: u64 }
            read: func(path: string, offset: u64, len: u32) -> result<list<u8>, plugin-error>;
            size: func(path: string) -> result<option<u64>, plugin-error>;
            resolve: func(path: string) -> result<option<string>, plugin-error>;
        }
        interface output {
            use types.{plugin-error};
            write: func(bytes: list<u8>) -> result<_, plugin-error>;
        }
        interface importer {
            use types.{plugin-error};
            use input.{file-info};
            manifest: func() -> string;
            import-design: func(main: file-info, params: string) -> result<string, plugin-error>;
        }
        world importer-plugin {
            import input;
            import output;
            export importer;
        }
    "#,
    world: "importer-plugin",
}); };
}
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    use wasmtime::component::bindgen as bind;
    contract!(bind);
    pub use solidcraft::plugin::{input, output, types};

    pub fn add_to_linker<T>(linker: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
    where
        T: input::Host + output::Host + types::Host + Send + 'static,
    {
        ImporterPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(linker, |host| host)
    }

    pub fn import_design<T>(
        plugin: &ImporterPlugin,
        store: &mut wasmtime::Store<T>,
        main: &input::FileInfo,
        params: &str,
    ) -> wasmtime::Result<Result<String, types::PluginError>> {
        plugin
            .solidcraft_plugin_importer()
            .call_import_design(store, main, params)
    }
}
#[cfg(target_arch = "wasm32")]
mod guest {
    use wit_bindgen::generate as bind;
    contract!(bind);

    use solidcraft::plugin::{input, output, types::PluginError};
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    // TODO(eval-seam, PR #580): after the host supplies isolated archive sessions,
    // add its reviewed engine.run import to contract! above and enable this bridge
    // ONLY inside IsolatedHost::with_design. It must never target the active design.
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
            let raw = solidcraft::plugin::engine::run(command, &args)
                .map_err(|e| format!("Host evaluation transport: {e:?}"))?;
            if raw.len() > 64 << 20 {
                return Err("Host evaluation result exceeds 64 MiB".into());
            }
            serde_json::from_str(&raw).map_err(|e| format!("Host evaluation JSON: {e}"))
        }
    }
    struct Importer;
    impl exports::solidcraft::plugin::importer::Guest for Importer {
        fn manifest() -> String {
            include_str!("../plugin.json").into()
        }
        fn import_design(main: input::FileInfo, params: String) -> Result<String, PluginError> {
            crate::validate_name(&main.name).map_err(PluginError::Invalid)?;
            if params.len() > 16 * 1024 {
                return Err(PluginError::Limit(
                    "Conversion options exceed 16 KiB; use the declared conversion options".into(),
                ));
            }
            let options = serde_json::from_str(&params)
                .map_err(|e| PluginError::Invalid(format!("Conversion options: {e}")))?;
            let budget = Cell::new(0u64);
            let main_bytes = RefCell::new(Some(read("", main.size, &budget)?));
            let files = Cell::new(1usize);
            let reader = |path: &Path, limit: u64| {
                if path == Path::new(&main.name) {
                    return main_bytes
                        .borrow_mut()
                        .take()
                        .ok_or_else(|| core_error("Main file requested more than once"));
                }
                let host_path = path.to_string_lossy();
                let size = input::size(&host_path)
                    .map_err(core_error)?
                    .ok_or_else(|| core_error("Part not found in the input folder"))?;
                if size > limit {
                    return Err(dutchman_core::Error::TooLarge {
                        path: path.into(),
                        bytes: size,
                        limit,
                    });
                }
                files.set(files.get() + 1);
                if files.get() > 4096 {
                    return Err(core_error(
                        "More than 4096 input files; convert a smaller assembly",
                    ));
                }
                read(&host_path, size, &budget).map_err(core_error)
            };
            let resolve = |stored: &str, folder: &Path| {
                let requested = crate::resolve(stored, folder, &|path| {
                    input::resolve(&path.to_string_lossy())
                        .ok()
                        .flatten()
                        .is_some()
                })?;
                let canonical = input::resolve(&requested.to_string_lossy())
                    .ok()
                    .flatten()?;
                // Validate host metadata as a bounded relative name too. Retain its
                // actual case: SolidWorks body names derive from the member's name.
                crate::resolve(&canonical, Path::new(""), &|path| {
                    path == Path::new(&canonical)
                })
            };
            let (design, report) = crate::convert(&main.name, options, &reader, &resolve)
                .map_err(|e| PluginError::Failed(e.to_string()))?;
            for chunk in design.chunks(crate::CHUNK_BYTES) {
                output::write(chunk)?;
            }
            Ok(report)
        }
    }
    fn core_error(e: impl std::fmt::Debug) -> dutchman_core::Error {
        dutchman_core::Error::Decode(format!("Host input: {e:?}"))
    }
    fn read(path: &str, size: u64, budget: &Cell<u64>) -> Result<Vec<u8>, PluginError> {
        if size > dutchman_core::MAX_INPUT_BYTES || size > (1u64 << 30).saturating_sub(budget.get())
        {
            return Err(PluginError::Limit(
                "Input exceeds the 1 GiB conversion limit; choose a smaller CAD file".into(),
            ));
        }
        budget.set(budget.get() + size);
        let mut bytes = Vec::new();
        while (bytes.len() as u64) < size {
            let len = (size - bytes.len() as u64).min(crate::CHUNK_BYTES as u64) as u32;
            let chunk = input::read(path, bytes.len() as u64, len)?;
            if chunk.is_empty() || chunk.len() > len as usize {
                return Err(PluginError::Failed("Input stream ended early or exceeded the requested length; select the file again".into()));
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
