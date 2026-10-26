//! HTTP guards shared by the web and print services.

use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine as _;

/// Whether a request comes from this host or an allowed browser origin.
pub fn allowed_origin(headers: &HeaderMap, public_origin: &[String]) -> bool {
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"cross-site"));
    let wrong_origin = headers.get(header::ORIGIN).is_some_and(|origin| {
        let Ok(origin) = origin.to_str() else {
            return true;
        };
        let Some(authority) = origin_authority(origin) else {
            return true;
        };
        let same_host = headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .is_some_and(|host| authority.eq_ignore_ascii_case(host));
        !same_host
            && !public_origin.iter().any(|allowed| {
                origin_authority(allowed).is_some() && allowed.eq_ignore_ascii_case(origin)
            })
    });
    !cross_site && !wrong_origin
}

fn origin_authority(origin: &str) -> Option<String> {
    let uri: axum::http::Uri = origin.parse().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri
            .path_and_query()
            .is_some_and(|path| path.as_str() != "/")
    {
        return None;
    }
    let authority = uri.authority()?.as_str();
    (!authority.contains('@')).then(|| authority.to_owned())
}

/// The user name and password of a basic `Authorization` header.
pub fn credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, password) = text.split_once(':')?;
    Some((user.to_owned(), password.to_owned()))
}

/// Whether `a` and `b` are equal, taking the same time whatever their contents.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    let mut diff = u8::from(a.len() != b.len());
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::REFERRER_POLICY, "no-referrer"),
        (
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// Storage admission shared by upload services.
#[derive(Debug)]
pub enum StorageError {
    Full,
    Io(std::io::Error),
}

pub fn check_storage(
    data_dir: &std::path::Path,
    max_data: u64,
    min_free: u64,
) -> Result<(), StorageError> {
    #[cfg(not(unix))]
    let _ = min_free;
    let mut total = 0u64;
    let mut pending = vec![data_dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && dir != data_dir => continue,
            Err(err) => return Err(StorageError::Io(err)),
        };
        for entry in entries {
            let entry = entry.map_err(StorageError::Io)?;
            let metadata = match entry.path().symlink_metadata() {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(StorageError::Io(err)),
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
                if total > max_data {
                    return Err(StorageError::Full);
                }
            }
        }
    }
    #[cfg(unix)]
    {
        let stats = rustix::fs::statvfs(data_dir).map_err(|err| StorageError::Io(err.into()))?;
        if stats.f_bavail.saturating_mul(stats.f_frsize) < min_free {
            return Err(StorageError::Full);
        }
    }
    Ok(())
}

/// Creates a private data folder and checks an existing folder before using it.
pub fn prepare_data_dir(path: &std::path::Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::other(
            "Choose a data folder that is a directory, not a symbolic link.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
            return Err(std::io::Error::other("Choose a data folder belonging to the current user with no group or other write permission."));
        }
    }
    Ok(())
}
