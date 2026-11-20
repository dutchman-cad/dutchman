//! User-facing notices; source archive paths never reach the host's notice.
#[cfg(any(test, target_arch = "wasm32"))]
use dutchman_core::Error;

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
        Error::MemoryBudget(reason) => reason.clone(),
        Error::Decode(reason)
        | Error::Encode(reason)
        | Error::Assembly(reason)
        | Error::Restate(reason) => plain(reason),
        _ => plain(&error.to_string()),
    };
    let intro = if matches!(error, Error::MemoryBudget(_)) {
        ""
    } else {
        "If this file opens in the app that made it, it's a bug in Dutchman. Help us fix it:\n"
    };
    format!("{reason}\n\n{intro}{}", help())
}
