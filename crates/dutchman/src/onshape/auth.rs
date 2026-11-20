//! API-key HMAC signing and OAuth bearer authentication.
//! Credentials print as "hidden" and have no Serialize implementation.
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

/// An OAuth token kept out of diagnostics and serialization.
pub struct BearerToken(String);

impl std::fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BearerToken(hidden)")
    }
}

impl BearerToken {
    pub fn new(token: String) -> Result<Self> {
        let token = token.trim();
        if token.is_empty()
            || token.len() > 16 * 1024
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
        {
            return Err(Error::new(ErrorKind::Auth,
                "Use a nonempty Onshape bearer token of at most 16 KiB on one line; replace the token file or DUTCHMAN_ONSHAPE_BEARER_TOKEN and retry."));
        }
        Ok(Self(token.to_owned()))
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            Self::from_open_file(Self::open_file(path)?)
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(Error::new(ErrorKind::Auth,
                "Onshape bearer token files are not supported yet on this platform because privacy cannot be checked. Clear the token file selection and use DUTCHMAN_ONSHAPE_BEARER_TOKEN in the process environment, then retry."))
        }
    }

    #[cfg(unix)]
    fn open_file(path: &Path) -> Result<std::fs::File> {
        use rustix::fs::{Mode, OFlags};
        // Never follow a substituted final symlink or wait for a FIFO writer. Parent
        // directories may move or resolve through symlinks: admission depends only on
        // the opened object's owner, type and permissions, never pathname metadata.
        let fd = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| token_file_error())?;
        Ok(std::fs::File::from(fd))
    }

    #[cfg(unix)]
    fn from_open_file(file: std::fs::File) -> Result<Self> {
        use std::io::Read;
        use std::os::unix::fs::MetadataExt;

        let meta = file.metadata().map_err(|_| token_file_error())?;
        if !meta.is_file()
            || meta.len() > 16 * 1024
            || meta.mode() & 0o7777 != 0o600
            || meta.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(token_file_error());
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| token_file_error())?;
        if bytes.len() > 16 * 1024 {
            return Err(token_file_error());
        }
        let text = String::from_utf8(bytes).map_err(|_| token_file_error())?;
        Self::new(text)
    }
}

#[cfg(unix)]
fn token_file_error() -> Error {
    Error::new(ErrorKind::Auth,
        "Could not read the Onshape bearer token file; use a regular UTF-8 file of at most 16 KiB, owned by you and readable only by you (mode 600 on Unix), without a symbolic link, and retry.")
}

/// Fetch authentication. API keys remain the default; tokens are never saved by login.
#[derive(Debug)]
pub enum FetchAuth {
    ApiKeys(Credentials),
    Bearer(BearerToken),
}

impl FetchAuth {
    /// A token file overrides the token environment variable. Mixing a token with API keys,
    /// including saved keys, is refused before any request is sent.
    pub fn find(store: &super::Store, token_file: Option<&Path>) -> Result<Option<Self>> {
        let token = match token_file {
            Some(path) => Some(BearerToken::from_file(path)?),
            None => (store.env)("DUTCHMAN_ONSHAPE_BEARER_TOKEN")
                .map(BearerToken::new)
                .transpose()?,
        };
        if token.is_some()
            && ((store.env)("ONSHAPE_ACCESS_KEY").is_some()
                || (store.env)("ONSHAPE_SECRET_KEY").is_some())
        {
            return Err(auth_conflict());
        }
        let keys = store.find()?;
        match (token, keys) {
            (Some(_), Some(_)) => Err(auth_conflict()),
            (Some(token), None) => Ok(Some(Self::Bearer(token))),
            (None, Some((keys, _))) => Ok(Some(Self::ApiKeys(keys))),
            (None, None) => Ok(None),
        }
    }

    pub fn is_bearer(&self) -> bool {
        matches!(self, Self::Bearer(_))
    }

    pub(super) fn as_auth(&self) -> Auth<'_> {
        match self {
            Self::ApiKeys(keys) => Auth::ApiKeys(keys),
            Self::Bearer(token) => Auth::Bearer(token),
        }
    }
}

fn auth_conflict() -> Error {
    Error::new(ErrorKind::Auth,
        "Choose either an Onshape bearer token or API keys. Remove ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY and run `dutchman onshape logout` to use a token, or remove the bearer token option and environment variable to use API keys.")
}

#[derive(Clone, Copy)]
pub(super) enum Auth<'a> {
    ApiKeys(&'a Credentials),
    Bearer(&'a BearerToken),
}

impl Auth<'_> {
    pub fn authorization(
        self,
        method: &str,
        path: &str,
        query: &str,
        date: &str,
        nonce: &str,
    ) -> String {
        match self {
            Self::ApiKeys(keys) => keys.authorization(method, path, query, date, nonce),
            Self::Bearer(token) => format!("Bearer {}", token.0),
        }
    }

    pub fn denied_message(self) -> &'static str {
        match self {
            Self::ApiKeys(_) => "Onshape denied access; check that your API keys can read your own document.",
            Self::Bearer(_) => "The Onshape bearer token expired or lacks access to this document. Obtain a fresh token with read access, replace the token file or DUTCHMAN_ONSHAPE_BEARER_TOKEN, and retry.",
        }
    }

    pub fn scope(self) -> String {
        match self {
            Self::ApiKeys(keys) => keys.scope(),
            Self::Bearer(token) => super::hash(format!("bearer:{}", token.0).as_bytes()),
        }
    }

    pub fn check_bytes(self, bytes: &[u8]) -> Result<()> {
        match self {
            Self::ApiKeys(keys) => keys.check_bytes(bytes),
            Self::Bearer(token) => {
                if bytes
                    .windows(token.0.len())
                    .any(|w| w == token.0.as_bytes())
                {
                    return Err(Error::new(ErrorKind::Privacy,
                        "Onshape returned credential material; nothing from this response was saved. Replace your bearer token and retry."));
                }
                Ok(())
            }
        }
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
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod token_file_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn write_private(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn replacements_cannot_change_the_opened_token_or_its_metadata() {
        #[derive(PartialEq)]
        enum Replacement {
            Private,
            Shared,
            Symlink,
            Fifo,
        }
        for replacement in [
            Replacement::Private,
            Replacement::Shared,
            Replacement::Symlink,
            Replacement::Fifo,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("token");
            let original = dir.path().join("original");
            write_private(&path, "synthetic-original-token");
            let file = BearerToken::open_file(&path).unwrap();
            // Deterministically replace the name between open and metadata/read.
            std::fs::rename(&path, &original).unwrap();
            match replacement {
                Replacement::Private => write_private(&path, "synthetic-replacement-token"),
                Replacement::Shared => {
                    write_private(&path, "synthetic-replacement-token");
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                        .unwrap();
                }
                Replacement::Symlink => symlink(&original, &path).unwrap(),
                Replacement::Fifo => rustix::fs::mkfifoat(
                    rustix::fs::CWD,
                    &path,
                    rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
                )
                .unwrap(),
            }
            assert_eq!(
                BearerToken::from_open_file(file).unwrap().0,
                "synthetic-original-token"
            );
            if replacement == Replacement::Private {
                assert_eq!(
                    BearerToken::from_file(&path).unwrap().0,
                    "synthetic-replacement-token"
                );
            } else {
                // Also cover a replacement that wins the race before open. A FIFO
                // without a writer must be refused without blocking the test process.
                let (send, receive) = std::sync::mpsc::channel();
                std::thread::spawn(move || send.send(BearerToken::from_file(&path)).unwrap());
                let error = receive
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("token file open/read must not block")
                    .unwrap_err();
                assert_eq!(error.kind, ErrorKind::Auth);
            }
        }
    }

    #[test]
    fn private_path_cannot_substitute_for_a_shared_or_oversized_open_handle() {
        for oversized in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("token");
            write_private(
                &path,
                &if oversized {
                    "x".repeat(16 * 1024 + 1)
                } else {
                    "synthetic-token".into()
                },
            );
            if !oversized {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            }
            let file = BearerToken::open_file(&path).unwrap();
            std::fs::rename(&path, dir.path().join("opened")).unwrap();
            write_private(&path, "synthetic-replacement-token");
            assert_eq!(
                BearerToken::from_open_file(file).unwrap_err().kind,
                ErrorKind::Auth
            );
        }
    }
}
