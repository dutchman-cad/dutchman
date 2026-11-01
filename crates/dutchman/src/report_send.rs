//! Per-report consent, private local receipts and a bounded report transport.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = crate::json::REPORT_SEND;
pub const MAX_BUNDLE: usize = 25 * 1024 * 1024;
const MAX_REPORT: usize = 16 * 1024;

/// Only a replay-verified, minimal repro provider should implement this seam.
/// Names must describe roles, never source names. Bytes can contain private geometry.
pub trait BundleSource {
    fn parts(&self) -> Result<Vec<BundlePart>, String>;
}

pub struct BundlePart {
    pub role: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartSize {
    pub role: String,
    pub bytes: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub report_id: String,
    pub source_format: String,
    pub signature: String,
    pub parts: Vec<PartSize>,
    pub bundle_size: usize,
    pub bundle_sha256: Option<String>,
    claim_secret: String,
}

pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn valid_id(id: &str) -> bool {
    let digits = if id.len() == 17 {
        id.strip_prefix('f').unwrap_or("")
    } else {
        id
    };
    digits.len() == 16
        && digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Prepared {
    /// Reuses the diagnostic file id. No native design file is opened here.
    pub fn new(
        id: &str,
        format: &str,
        signature: &str,
        source: Option<&dyn BundleSource>,
        folder: &Path,
    ) -> Result<Self, String> {
        if !valid_id(id)
            || format.is_empty()
            || format.len() > 20
            || !format
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            || signature.len() > 300
            || !signature.bytes().all(|b| (32..=126).contains(&b))
        {
            return Err("Report metadata is not supported; prepare the report again.".into());
        }
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
        let mut prepared = Self {
            report_id: id.into(),
            source_format: format.into(),
            signature: signature.into(),
            parts: Vec::new(),
            bundle_size: 0,
            bundle_sha256: None,
            claim_secret: base64url(&secret),
        };
        if let Some(source) = source {
            let parts = source.parts()?;
            if parts
                .iter()
                .any(|p| p.role.len() > 80 || !p.role.bytes().all(|b| (32..=126).contains(&b)))
                || parts.len() > 64
                || parts
                    .iter()
                    .try_fold(0usize, |n, p| n.checked_add(p.bytes.len()))
                    .is_none_or(|n| n > MAX_BUNDLE)
            {
                return Err("The bundle is too large; send it with the private-send card.".into());
            }
            // A gzip JSON container keeps the role and byte boundaries explicit for replay.
            let mut gzip =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            let content: Vec<Value> = parts
                .iter()
                .map(|p| json!({"role": p.role, "bytes_base64url": base64url(&p.bytes)}))
                .collect();
            serde_json::to_writer(&mut gzip, &json!({"schema": 1, "parts": content}))
                .map_err(|e| e.to_string())?;
            let bytes = gzip.finish().map_err(|e| e.to_string())?;
            if bytes.len() > MAX_BUNDLE {
                return Err(
                    "The compressed bundle exceeds 25 MiB; use the private-send card.".into(),
                );
            }
            prepared.parts = parts
                .iter()
                .map(|p| PartSize {
                    role: p.role.clone(),
                    bytes: p.bytes.len(),
                })
                .collect();
            prepared.bundle_size = bytes.len();
            prepared.bundle_sha256 = Some(hash(&bytes));
            private_write(&folder.join("bundle.json.gz"), &bytes)?;
        }
        private_write(
            &folder.join("prepared.json"),
            &serde_json::to_vec(&prepared).map_err(|e| e.to_string())?,
        )?;
        Ok(prepared)
    }

    pub fn load(folder: &Path) -> Result<Self, String> {
        let bytes = bounded_read(&folder.join("prepared.json"), MAX_REPORT)?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    /// Safe for MCP: sizes and roles only, never geometry or the claim secret.
    pub fn preview(&self) -> Value {
        json!({"schema": SCHEMA, "ok": true, "report_id": self.report_id, "source_format": self.source_format, "signature": self.signature, "parts": self.parts, "bundle_size": self.bundle_size, "bundle_available": self.bundle_sha256.is_some(), "summary_size": self.metadata("summary", false).to_string().len(), "default_choice": "bundle", "public_test": false, "summary": if self.bundle_sha256.is_some() { "Review the bundle parts and sizes. Bundles contain private geometry." } else { "No bundle is available until repro extraction is supported. Choose summary to send only version, OS, format and failure signature." }})
    }

    /// The exact bounded envelope, without the private secret or local paths.
    pub fn metadata(&self, choice: &str, public_test: bool) -> Value {
        let mut report = json!({"schema": 1, "report_id": self.report_id, "dutchman_version": crate::BUILD_VERSION, "os": std::env::consts::OS, "source_format": self.source_format, "source_app": "unknown", "signature": self.signature, "claim_hash": hash(self.claim_secret.as_bytes()), "consent": {"choice": choice, "text_version": 1, "public_test": public_test}});
        if choice == "bundle" {
            if let Some(map) = report.as_object_mut() {
                map.insert("bundle_size".into(), json!(self.bundle_size));
                map.insert("bundle_sha256".into(), json!(self.bundle_sha256));
            }
        }
        report
    }

    pub fn claim(&self) -> String {
        let digest =
            Sha256::digest(format!("claim-code:{}", hash(self.claim_secret.as_bytes())).as_bytes());
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut code = String::new();
        let mut buffer = 0u32;
        let mut bits = 0;
        for byte in digest.iter().take(7) {
            buffer = (buffer << 8) | u32::from(*byte);
            bits += 8;
            while bits >= 5 && code.len() < 10 {
                bits -= 5;
                if let Some(c) = alphabet.get(((buffer >> bits) & 31) as usize) {
                    code.push(char::from(*c));
                }
            }
        }
        format!("claim {}-{code}", self.report_id)
    }
}

fn base64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("Cannot keep the local report: {e}"))
}

fn bounded_read(path: &Path, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(cap as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > cap {
        return Err("The local report exceeds its size limit.".into());
    }
    Ok(bytes)
}

pub fn endpoint() -> Option<String> {
    option_env!("DUTCHMAN_REPORT_ENDPOINT").map(str::to_owned)
}

/// Called only after a click or an explicit yes. Nothing schedules a later resend.
pub fn submit(
    prepared: &Prepared,
    folder: &Path,
    choice: &str,
    public_test: bool,
    explicit_yes: bool,
    endpoint: Option<&str>,
) -> Result<Value, String> {
    if !explicit_yes {
        return Err("Ask the user for an explicit yes before sending this report.".into());
    }
    if !matches!(choice, "bundle" | "summary") {
        return Err("Choose bundle or summary; whole files use the private-send card.".into());
    }
    let bundle = if choice == "bundle" {
        let bytes = bounded_read(&folder.join("bundle.json.gz"), MAX_BUNDLE)?;
        if prepared.bundle_sha256.as_deref() != Some(hash(&bytes).as_str())
            || prepared.bundle_size != bytes.len()
        {
            return Err("The bundle changed; prepare and review it again.".into());
        }
        Some(bytes)
    } else {
        None
    };
    let report = prepared.metadata(choice, public_test);
    let metadata = serde_json::to_vec(&report).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_REPORT {
        return Err("Report metadata exceeds 16 KiB.".into());
    }
    // Keep consent per report, and never reuse an id for different bytes after an attempt.
    let saved = folder.join("submission.json");
    if saved.exists() {
        if bounded_read(&saved, MAX_REPORT)? != metadata {
            return Err(
                "This report already has a saved consent choice. Use the same choice to retry."
                    .into(),
            );
        }
    } else {
        private_write(&saved, &metadata)?;
    }
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let boundary = hash(&random);
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"report\"\r\nContent-Type: application/json\r\n\r\n").into_bytes();
    body.extend(metadata);
    if let Some(bytes) = bundle {
        body.extend(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"bundle\"; filename=\"repro.gz\"\r\nContent-Type: application/gzip\r\n\r\n").bytes());
        body.extend(bytes);
    }
    body.extend(format!("\r\n--{boundary}--\r\n").bytes());
    let mut rate_limited = false;
    if let Some(endpoint) = endpoint {
        let url = url::Url::parse(endpoint).map_err(|e| e.to_string())?;
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        {
            return Err("Report sending requires HTTPS.".into());
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(8))
            .redirects(0)
            .build();
        for attempt in 0..2 {
            match agent
                .post(endpoint)
                .set(
                    "Content-Type",
                    &format!("multipart/form-data; boundary={boundary}"),
                )
                .send_bytes(&body)
            {
                Ok(response) => {
                    let status = response.status();
                    let mut bytes = Vec::new();
                    let read = response
                        .into_reader()
                        .take(MAX_REPORT as u64 + 1)
                        .read_to_end(&mut bytes);
                    if read.is_err() {
                        if attempt == 0 {
                            std::thread::sleep(Duration::from_millis(250));
                        }
                        continue;
                    }
                    if bytes.len() <= MAX_REPORT {
                        if let Ok(receipt) = serde_json::from_slice::<Value>(&bytes) {
                            if matches!(status, 200 | 201)
                                && receipt.get("report_id") == Some(&json!(prepared.report_id))
                                && receipt.get("received").and_then(Value::as_u64).is_some()
                            {
                                let receipt_path = folder.join("receipt.json");
                                if !receipt_path.exists() {
                                    private_write(&receipt_path, &bytes)?;
                                }
                                return Ok(
                                    json!({"schema": SCHEMA, "ok": true, "sent": true, "report_id": prepared.report_id, "received": receipt.get("received")}),
                                );
                            }
                        }
                    }
                    break;
                }
                Err(ureq::Error::Status(status, _)) => {
                    rate_limited = status == 429;
                    if status != 429 && status < 500 {
                        break;
                    }
                }
                Err(ureq::Error::Transport(_)) => {}
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Ok(
        json!({"schema": SCHEMA, "ok": false, "sent": false, "report_id": prepared.report_id, "summary": if rate_limited { "It did not go through. Try again tomorrow. Your report is kept locally." } else { "It did not go through. Your report is kept locally. Retry with a click, use the private-send card, or share only the anonymous summary on GitHub." }, "local_folder": folder, "bundle": prepared.bundle_sha256.as_ref().map(|_| folder.join("bundle.json.gz")), "github": crate::issue::NEW_ISSUE, "private_send": crate::json::DISCORD_URL}),
    )
}

/// Compare locally against release metadata; no per-report network request.
pub fn fixed_notice(ids: &[String], fixes: &[String]) -> Option<String> {
    ids.iter()
        .find(|id| fixes.contains(&hash(id.as_bytes())))
        .map(|id| {
            format!(
                "This update fixes a problem you reported ({}).",
                id.chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
                    .to_uppercase()
            )
        })
}

pub fn local_folder(out: &Path, id: &str) -> PathBuf {
    out.join("reports").join(id)
}

#[cfg(test)]
mod tests;
