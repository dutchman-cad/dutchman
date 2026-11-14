//! API-key HMAC signing. Credentials print as "hidden" and have no Serialize implementation.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

use super::{Error, ErrorKind, Result};

pub struct Credentials {
    access: String,
    secret: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials(hidden)")
    }
}

impl Credentials {
    /// Pasted keys often carry a stray space, newline or pair of quotes around them: those are
    /// removed first. A key that still has a character it can't contain is refused with which key
    /// and what kind of character, never with the key itself.
    pub fn new(access: String, secret: String) -> Result<Self> {
        let access = clean_key(&access);
        let secret = clean_key(&secret);
        let refuse = |what: &'static str| Err(Error::new(ErrorKind::Auth, what));
        if access.is_empty() || secret.is_empty() {
            return refuse("Set both Onshape API keys; one of them is empty. Keys are never included in diagnostics.");
        }
        if access.len() > 4096 || secret.len() > 4096 {
            return refuse("An Onshape API key is longer than any real key; check that only the key was pasted.");
        }
        // Current access keys carry a prefix such as `on_`: any printable character but a space is
        // allowed, as for the secret.
        if !access.bytes().all(|b| b.is_ascii_graphic()) {
            return refuse("The Onshape access key contains a space, tab or line break inside it; paste it again on one line.");
        }
        if !secret.bytes().all(|b| b.is_ascii_graphic()) {
            return refuse("The Onshape secret key contains a space, tab or line break inside it; paste it again on one line.");
        }
        Ok(Self { access, secret })
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let meta = std::fs::symlink_metadata(path)
            .map_err(|_| Error::new(ErrorKind::Auth, super::store::NOT_CONNECTED))?;
        if !meta.is_file() || meta.len() > 16 * 1024 {
            return Err(Error::new(
                ErrorKind::Auth,
                "Use a regular Onshape config file smaller than 16 KiB with mode 600.",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o7777 != 0o600 {
                return Err(Error::new(
                    ErrorKind::Auth,
                    "Run chmod 600 on the Onshape config file, then retry.",
                ));
            }
        }
        #[cfg(not(unix))]
        return Err(Error::new(ErrorKind::Auth, "Use ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY on this platform; config mode 600 cannot be checked."));
        #[cfg(unix)]
        {
            let bytes = super::read_bounded(path, 16 * 1024)?;
            Self::from_json(&bytes)
        }
    }

    /// Reads `{"access_key": ..., "secret_key": ...}`, the form kept in the credential store
    /// and in the private file.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| {
            Error::new(
                ErrorKind::Auth,
                "The saved Onshape keys could not be read; run `dutchman onshape login` again.",
            )
        })?;
        let a = value.get("access_key").and_then(serde_json::Value::as_str);
        let s = value.get("secret_key").and_then(serde_json::Value::as_str);
        Self::new(a.unwrap_or_default().into(), s.unwrap_or_default().into())
    }

    /// The form [`Self::from_json`] reads. It holds the keys: write it only to a private place.
    pub fn to_json(&self) -> String {
        serde_json::json!({"access_key": self.access, "secret_key": self.secret}).to_string()
    }

    pub(super) fn scope(&self) -> String {
        super::hash(self.access.as_bytes())
    }

    pub(super) fn check_bytes(&self, bytes: &[u8]) -> Result<()> {
        if bytes
            .windows(self.access.len())
            .any(|w| w == self.access.as_bytes())
            || bytes
                .windows(self.secret.len())
                .any(|w| w == self.secret.as_bytes())
        {
            return Err(Error::new(ErrorKind::Privacy, "Onshape returned credential material; nothing from this response was saved. Check your keys and retry."));
        }
        Ok(())
    }

    pub(super) fn check_json(&self, value: &serde_json::Value) -> Result<()> {
        match value {
            serde_json::Value::String(text) => self.check_bytes(text.as_bytes())?,
            serde_json::Value::Array(list) => {
                for value in list {
                    self.check_json(value)?;
                }
            }
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    self.check_bytes(key.as_bytes())?;
                    self.check_json(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Onshape's canonical string is lower-case, including the final newline.
    pub fn authorization(
        &self,
        method: &str,
        path: &str,
        query: &str,
        date: &str,
        nonce: &str,
    ) -> String {
        let canonical = format!("{method}\n{nonce}\n{date}\napplication/json\n{path}\n{query}\n")
            .to_lowercase();
        format!(
            "On {}:HmacSHA256:{}",
            self.access,
            STANDARD.encode(
                ring::hmac::sign(
                    &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, self.secret.as_bytes()),
                    canonical.as_bytes()
                )
                .as_ref()
            )
        )
    }
}

/// A pasted key without surrounding whitespace and one pair of matching quotes around it.
fn clean_key(key: &str) -> String {
    let key = key.trim();
    let unquoted = ['"', '\'']
        .iter()
        .find_map(|q| key.strip_prefix(*q).and_then(|k| k.strip_suffix(*q)))
        .unwrap_or(key);
    unquoted.trim().to_owned()
}
