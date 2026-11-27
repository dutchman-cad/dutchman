//! User-facing notices; source archive paths never reach the host's notice.
#[cfg(any(test, target_arch = "wasm32"))]
use dutchman_core::Error;

/// Formats whose parts are separate files beside the assembly.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn needs_part_folder(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".iam") || lower.ends_with(".sldasm")
}

/// Said when SolidCraft refuses the plugin's read of the folder beside the opened file. It
/// names the grant, what it is for, and how to give it; the host's own wording follows.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn folder_access(file_name: &str, host: &str) -> String {
    let what = if needs_part_folder(file_name) {
        "an assembly's part files are separate files in that folder"
    } else {
        "the design refers to other files in that folder"
    };
    format!(
        "Dutchman needs read access to the folder that holds {file_name}: {what}. It reads only that folder, never writes, and never looks elsewhere. Give it that access by approving \"read access to the input folder\" when you install Dutchman (Preferences > Plugins > Install plugin...), keep the parts beside the assembly, then open the file again. SolidCraft said: {host}"
    )
}

/// The `dutchman.convert/1` report of a failed conversion, the shape the Dutchman app writes to
/// `<name>.failed.json`: the input, and the error with its kind, stage and reason codes. Archive
/// paths are left out of the message as everywhere else in the plugin.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn failure_report(file_name: &str, error: &Error) -> serde_json::Value {
    let path = std::path::Path::new(file_name);
    let format = dutchman_core::Format::from_path(path);
    let mut fields = serde_json::json!({
        "message": plain(&error.to_string()).chars().take(1500).collect::<String>(),
        "stage": error.stage().id(),
        "upstream": error.upstream(),
        "kind": error.kind(),
    });
    if let (Some(cause), Some(object)) = (error.cause(), fields.as_object_mut()) {
        object.insert("cause".into(), serde_json::json!(cause));
    }
    serde_json::json!({
        "schema": "dutchman.convert/1",
        "ok": false,
        "input": {
            "path": file_name,
            "format": format.map(dutchman_core::Format::id),
            "format_name": format.map(dutchman_core::Format::name),
            "program": format.map(dutchman_core::Format::family),
            "extension": dutchman_core::Format::source_extension(path),
            "words": format.map(dutchman_core::Format::words),
        },
        "error": fields,
    })
}

/// What a person sees when `file_name` fails: the plain reason and the help links, then the full
/// failure report in the text itself. The host gives a plugin no clipboard and no file to write,
/// so the message is the only place the report can travel; it says how to get the same report
/// as a file from the Dutchman app.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn failure_in(file_name: &str, error: &Error) -> String {
    let base = failure(error);
    // A size refusal is one plain line that points to the app; the report adds nothing to it.
    if matches!(error, Error::MemoryBudget(_) | Error::EmptyExport(_)) {
        return base;
    }
    let stem = std::path::Path::new(file_name).file_stem().map_or_else(
        || file_name.to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let report = failure_report(file_name, error);
    format!(
        "{base}\n\nFull report ({stem}.failed.json, the dutchman.convert/1 report with its reason codes; the Dutchman app writes the same file when you open {file_name} in it: {DOWNLOAD}):\n{report}",
        DOWNLOAD = crate::preflight::DOWNLOAD
    )
}

/// Said when SolidCraft refuses a read of the folder the person picked for the parts. It names
/// the grant (the Parts folder field), what it is for, and how to give it.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn parts_folder_access(file_name: &str, host: &str) -> String {
    format!(
        "Dutchman needs read access to the parts folder for {file_name}, and SolidCraft has not granted it. Pick the folder in the \"Folder with the parts\" field when you open the file (Browse...), or leave that field empty to read the folder that holds {file_name} instead. A saved or scripted value does not grant access: the folder has to be picked or edited in the dialog. Dutchman only reads that folder, never writes, and never looks elsewhere. SolidCraft said: {host}"
    )
}

pub(crate) const QUESTION: &str = "Is there a problem? Is something missing?";
pub(crate) fn help() -> String {
    let policy: serde_json::Value =
        serde_json::from_str(include_str!("../../dutchman/assets/report-policy.json"))
            .unwrap_or_default();
    let invite = policy
        .get("inviteUrl")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let user = policy
        .get("dutchyUserId")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    format!("{QUESTION}\nJoin our Discord server: {invite}\nDM Dutchy: https://discord.com/users/{user}\nGitHub issue: {}", dutchman_core::FILE_DID_NOT_CONVERT)
}

/// Keep readable feature names while replacing archive paths and opaque byte records.
pub(crate) fn plain(text: &str) -> String {
    dutchman_core::clean_refusal(text)
        .split_whitespace()
        .map(|word| {
            if word.contains('/')
                || word.contains('\\')
                || word.contains("BlobParts")
                || word.contains("BulkStream.dat")
            {
                word.split(['/', '\\'])
                    .find(|part| {
                        let lower = part
                            .trim_matches(['\'', '"', ':', ',', ';'])
                            .to_ascii_lowercase();
                        [
                            ".f3d", ".f3z", ".ipt", ".iam", ".sldprt", ".sldasm", ".fcstd",
                        ]
                        .iter()
                        .any(|extension| lower.ends_with(extension))
                    })
                    .map(|part| part.trim_matches(['\'', '"', ':', ',', ';']).to_owned())
                    .unwrap_or_else(|| "a part of the design".to_owned())
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn failure(error: &Error) -> String {
    let reason = match error {
        Error::MemoryBudget(reason) | Error::EmptyExport(reason) => reason.clone(),
        Error::Decode(reason)
        | Error::Encode(reason)
        | Error::Assembly(reason)
        | Error::Restate(reason) => plain(reason),
        _ => plain(&error.to_string()),
    };
    // A reason can quote a whole record; the person needs the start of it.
    let reason: String = reason.chars().take(2000).collect();
    let intro = if matches!(error, Error::MemoryBudget(_) | Error::EmptyExport(_)) {
        ""
    } else {
        "If this file opens in the app that made it, it's a bug in Dutchman. Help us fix it:\n"
    };
    format!("{reason}\n\n{intro}{}", help())
}
