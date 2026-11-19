//! Public share links, resolved only within their provider and downloaded into private scratch space.
//! No browser session, referrer, credentials, or original link is forwarded to another service.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use url::Url;

mod network;

/// Matches the web app's default upload cap.
pub const MAX_DOWNLOAD_BYTES: u64 = 200 << 20;
const MAX_PAGE_BYTES: u64 = 2 << 20;
const MAX_REQUESTS: usize = 12;
const TIMEOUT: Duration = Duration::from_secs(120);
const MANUAL: &str = "The Fusion link has no public file download available without login. Download the design in Fusion, then drop the file into Dutchman.";
const PRIVATE: &str = "The share link is private, expired, or has no public file download. Enable public downloads, or download the file yourself and drop it into Dutchman.";
const NETWORK: &str = "The shared file could not be downloaded within the time limit. Check your connection, or download it yourself and drop the file into Dutchman.";
const UNSAFE: &str = "The share link leads to a host or network address that is not allowed. Download the file yourself and drop it into Dutchman.";

/// A downloaded file. Dropping this owner removes its private folder, including partial downloads.
#[derive(Debug)]
pub struct Downloaded {
    folder: tempfile::TempDir,
    path: PathBuf,
}

impl Downloaded {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a queued file or result still uses this download.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(self.folder.path())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provider {
    Drive,
    Fusion,
}

/// Whether an input is URL-shaped, including an unsupported or insecure URL that needs an explanation.
pub fn is_url(input: &str) -> bool {
    input.trim().split_once("://").is_some_and(|(scheme, _)| {
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn clean_url(text: &str) -> Result<Url, String> {
    if text.len() > 8192 {
        return Err("The share link is too long. Copy the public file share link again.".into());
    }
    let mut url = Url::parse(text.trim()).map_err(|_| UNSAFE.to_owned())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(UNSAFE.into());
    }
    url.set_fragment(None);
    Ok(url)
}

fn under(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|s| s.ends_with('.'))
}

impl Provider {
    fn login(self, url: &Url) -> bool {
        match self {
            Self::Drive => url.host_str() == Some("accounts.google.com"),
            Self::Fusion => matches!(
                url.host_str(),
                Some("accounts.autodesk.com" | "signin.autodesk.com")
            ),
        }
    }

    fn allows(self, url: &Url) -> bool {
        if clean_url(url.as_str()).is_err() {
            return false;
        }
        let host = url.host_str().unwrap_or_default();
        match self {
            Self::Drive => {
                matches!(host, "drive.google.com" | "drive.usercontent.google.com")
                    || under(host, "googleusercontent.com")
            }
            Self::Fusion => {
                host == "a360.co" || under(host, "autodesk360.com") || under(host, "autodesk.com")
            }
        }
    }
}

fn source(text: &str) -> Result<(Provider, Url), String> {
    let url = clean_url(text)?;
    let host = url.host_str().unwrap_or_default();
    if host == "drive.google.com" {
        let segments: Vec<_> = url.path_segments().into_iter().flatten().collect();
        let id = if segments.first() == Some(&"file") && segments.get(1) == Some(&"d") {
            segments.get(2).map(|s| (*s).to_owned())
        } else if matches!(url.path(), "/open" | "/uc") {
            url.query_pairs()
                .find(|(key, _)| key == "id")
                .map(|(_, v)| v.into_owned())
        } else {
            None
        };
        let id = id
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 256
                    && id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            })
            .ok_or_else(|| {
                "Copy a Google Drive file share link, rather than a folder link.".to_owned()
            })?;
        let mut download =
            Url::parse("https://drive.google.com/uc").map_err(|_| UNSAFE.to_owned())?;
        download
            .query_pairs_mut()
            .append_pair("export", "download")
            .append_pair("id", &id);
        if let Some((_, key)) = url.query_pairs().find(|(k, _)| k == "resourcekey") {
            download.query_pairs_mut().append_pair("resourcekey", &key);
        }
        return Ok((Provider::Drive, download));
    }
    if host == "a360.co" || under(host, "autodesk360.com") {
        return Ok((Provider::Fusion, url));
    }
    Err("Use a public Fusion (a360.co or autodesk360.com) or Google Drive file share link over HTTPS.".into())
}

/// Downloads a public Fusion or Google Drive file. All errors omit the original URL and tokens.
pub fn download(text: &str) -> Result<Downloaded, String> {
    download_under(text, None)
}

/// Keeps scratch files under a frontend-owned folder, so shutdown can clean active downloads too.
pub fn download_in(text: &str, parent: &Path) -> Result<Downloaded, String> {
    download_under(text, Some(parent))
}

fn download_under(text: &str, parent: Option<&Path>) -> Result<Downloaded, String> {
    let (provider, url) = source(text)?;
    download_using(provider, url, parent, TIMEOUT, network::fetch)
}

fn download_using(
    provider: Provider,
    url: Url,
    parent: Option<&Path>,
    timeout: Duration,
    mut fetch: impl FnMut(&Url, Instant) -> Result<ureq::Response, String>,
) -> Result<Downloaded, String> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| NETWORK.to_owned())?;
    resolve_until(
        provider,
        url,
        MAX_DOWNLOAD_BYTES,
        deadline,
        scratch(parent)?,
        |url, _| fetch(url, deadline),
    )
}

fn scratch(parent: Option<&Path>) -> Result<tempfile::TempDir, String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("dutchman-share-");
    match parent {
        Some(parent) => builder.tempdir_in(parent),
        None => builder.tempdir(),
    }
    .map_err(|_| "Cannot create a temporary download folder. Check free disk space.".to_owned())
}

#[cfg(test)]
fn resolve(
    provider: Provider,
    url: Url,
    cap: u64,
    timeout: Duration,
    fetch: impl FnMut(&Url, Duration) -> Result<ureq::Response, String>,
) -> Result<Downloaded, String> {
    resolve_to(provider, url, cap, timeout, scratch(None)?, fetch)
}

#[cfg(test)]
fn resolve_to(
    provider: Provider,
    url: Url,
    cap: u64,
    timeout: Duration,
    folder: tempfile::TempDir,
    fetch: impl FnMut(&Url, Duration) -> Result<ureq::Response, String>,
) -> Result<Downloaded, String> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| NETWORK.to_owned())?;
    resolve_until(provider, url, cap, deadline, folder, fetch)
}

fn resolve_until(
    provider: Provider,
    mut url: Url,
    cap: u64,
    deadline: Instant,
    folder: tempfile::TempDir,
    mut fetch: impl FnMut(&Url, Duration) -> Result<ureq::Response, String>,
) -> Result<Downloaded, String> {
    for _ in 0..MAX_REQUESTS {
        if clean_url(url.as_str()).is_ok() && provider.login(&url) {
            return Err(if provider == Provider::Fusion {
                MANUAL
            } else {
                PRIVATE
            }
            .to_owned());
        }
        if !provider.allows(&url) {
            return Err(UNSAFE.into());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| NETWORK.to_owned())?;
        let response = fetch(&url, remaining)?;
        if matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            let location = response
                .header("Location")
                .ok_or_else(|| PRIVATE.to_owned())?;
            url = url.join(location).map_err(|_| UNSAFE.to_owned())?;
            continue;
        }
        if response.status() != 200 {
            return Err(PRIVATE.into());
        }
        let html = response.header("Content-Type").is_some_and(|kind| {
            let kind = kind.to_ascii_lowercase();
            kind.contains("text/html")
                || kind.contains("application/xhtml")
                || kind.contains("application/json")
        });
        let limit = if html { MAX_PAGE_BYTES } else { cap };
        if response
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|n| n > limit)
        {
            return Err(too_large(html));
        }
        if html {
            let mut page = String::new();
            network::reader(response.into_reader(), deadline)?
                .take(limit + 1)
                .read_to_string(&mut page)
                .map_err(|_| NETWORK.to_owned())?;
            if page.len() as u64 > limit {
                return Err(too_large(true));
            }
            url = match provider {
                Provider::Drive => drive_confirmation(&page, &url),
                Provider::Fusion => fusion_download(&page, &url),
            }
            .ok_or_else(|| {
                if provider == Provider::Fusion {
                    MANUAL
                } else {
                    PRIVATE
                }
                .to_owned()
            })?;
            // Check again before the next request, including confirmation actions from HTML.
            continue;
        }
        let filename = response.header("Content-Disposition").and_then(disposition_name)
            .or_else(|| url.path_segments()?.next_back().map(decode_percent))
            .map(|name| dutchman_serve::upload::safe_name(&name))
            .filter(|name| supported_name(Path::new(name)))
            .ok_or_else(|| "The link did not provide a named CAD file. Download the file yourself and drop it into Dutchman.".to_owned())?;
        let path = folder.path().join(filename);
        let mut file = File::create(&path)
            .map_err(|_| "Cannot save the shared file. Check free disk space.".to_owned())?;
        let count = std::io::copy(
            &mut network::reader(response.into_reader(), deadline)?.take(cap + 1),
            &mut file,
        )
        .map_err(|_| NETWORK.to_owned())?;
        if count > cap {
            return Err(too_large(false));
        }
        if Instant::now() >= deadline {
            return Err(NETWORK.into());
        }
        return Ok(Downloaded { folder, path });
    }
    Err("The share link redirected too many times. Download the file yourself and drop it into Dutchman.".into())
}

fn supported_name(path: &Path) -> bool {
    let extension = crate::batch::extension_of(path);
    extension == "zip"
        || dutchman_core::Format::part_revision(path).is_some()
        || dutchman_core::Format::ALL.iter().any(|format| {
            format
                .extensions()
                .iter()
                .any(|suffix| extension.eq_ignore_ascii_case(suffix))
        })
}

fn too_large(page: bool) -> String {
    if page {
        "The share page is too large. Download the file yourself and drop it into Dutchman.".into()
    } else {
        "The shared file exceeds the 200 MB download limit. Download it yourself and drop the file into Dutchman.".into()
    }
}

fn decode_percent(value: &str) -> String {
    // Decode as a query value while preserving literal '+' in file names.
    url::form_urlencoded::parse(
        format!("v={}", value.replace('&', "%26").replace('+', "%2B")).as_bytes(),
    )
    .next()
    .map(|(_, value)| value.into_owned())
    .unwrap_or_default()
}

fn disposition_name(header: &str) -> Option<String> {
    let fields: Vec<_> = header
        .split(';')
        .filter_map(|field| field.trim().split_once('='))
        .collect();
    fields
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("filename*"))
        .and_then(|(_, value)| value.trim_matches('"').split_once("''"))
        .filter(|(encoding, _)| encoding.eq_ignore_ascii_case("UTF-8"))
        .map(|(_, value)| decode_percent(value))
        .or_else(|| {
            fields
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("filename"))
                .map(|(_, value)| value.trim_matches('"').to_owned())
        })
}

// Small, bounded HTML attribute reader. It does not execute scripts, load images, or follow other links.
#[derive(Debug)]
struct Tag<'a> {
    name: String,
    attrs: Vec<(String, String)>,
    rest: &'a str,
}

impl Tag<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let end = after
            .char_indices()
            .take(13)
            .find(|(_, c)| *c == ';')
            .map(|(i, _)| i);
        if let Some((entity, tail)) =
            end.and_then(|end| Some((after.get(..end)?, after.get(end + 1..)?)))
        {
            let decoded = match entity {
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                _ => entity
                    .strip_prefix("#x")
                    .and_then(|v| u32::from_str_radix(v, 16).ok())
                    .or_else(|| entity.strip_prefix('#')?.parse().ok())
                    .and_then(char::from_u32),
            };
            if let Some(c) = decoded {
                out.push(c);
                rest = tail;
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn tags(mut html: &str) -> impl Iterator<Item = Tag<'_>> {
    std::iter::from_fn(move || {
        let (_, after) = html.split_once('<')?;
        let mut quoted = None;
        let end = after.char_indices().find_map(|(i, c)| {
            if let Some(q) = quoted {
                if c == q {
                    quoted = None;
                }
            } else if c == '\'' || c == '"' {
                quoted = Some(c);
            } else if c == '>' {
                return Some(i);
            }
            None
        })?;
        let inside = after.get(..end)?.trim();
        html = after.get(end + 1..)?;
        let split = inside.find(char::is_whitespace).unwrap_or(inside.len());
        let name = inside.get(..split)?.to_ascii_lowercase();
        let mut fields = inside.get(split..)?.trim();
        let mut attrs = Vec::new();
        while !fields.is_empty() && attrs.len() < 64 {
            let end = fields
                .find(|c: char| c.is_whitespace() || c == '=')
                .unwrap_or(fields.len());
            let key = fields.get(..end)?.to_ascii_lowercase();
            if key.is_empty() {
                break;
            }
            fields = fields.get(end..)?.trim_start();
            let mut value = String::new();
            if let Some(after) = fields.strip_prefix('=') {
                fields = after.trim_start();
                if let Some(quote @ ('\'' | '"')) = fields.chars().next() {
                    let after = fields.get(1..)?;
                    let (text, tail) = after.split_once(quote)?;
                    value = entities(text);
                    fields = tail.trim_start();
                } else {
                    let end = fields.find(char::is_whitespace).unwrap_or(fields.len());
                    value = entities(fields.get(..end)?);
                    fields = fields.get(end..)?.trim_start();
                }
            }
            attrs.push((key, value));
        }
        Some(Tag {
            name,
            attrs,
            rest: html,
        })
    })
}

fn drive_confirmation(page: &str, base: &Url) -> Option<Url> {
    for form in tags(page)
        .take(10_000)
        .filter(|tag| tag.name == "form")
        .take(64)
    {
        if form.attr("id") != Some("download-form")
            || form
                .attr("method")
                .is_some_and(|v| !v.eq_ignore_ascii_case("get"))
        {
            continue;
        }
        let mut action = base.join(form.attr("action")?).ok()?;
        // A confirmation is for this file only, even if the form contains an unrelated id.
        let id = base
            .query_pairs()
            .find(|(key, _)| key == "id")?
            .1
            .into_owned();
        let mut values = Vec::new();
        for tag in tags(form.rest)
            .take(128)
            .take_while(|tag| tag.name != "/form")
        {
            if tag.name == "input"
                && tag
                    .attr("type")
                    .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
            {
                if let (Some(name), Some(value)) = (tag.attr("name"), tag.attr("value")) {
                    if matches!(name, "id" | "export" | "confirm" | "uuid" | "resourcekey") {
                        values.push((name.to_owned(), value.to_owned()));
                    }
                }
            }
        }
        if !values.iter().any(|(k, v)| k == "confirm" && !v.is_empty()) {
            continue;
        }
        values.retain(|(key, _)| key != "id");
        if !values.iter().any(|(key, _)| key == "resourcekey") {
            if let Some((_, key)) = base.query_pairs().find(|(key, _)| key == "resourcekey") {
                values.push(("resourcekey".to_owned(), key.into_owned()));
            }
        }
        action.set_query(None);
        action
            .query_pairs_mut()
            .extend_pairs(values)
            .append_pair("id", &id);
        return Some(action);
    }
    // Older Drive warning pages expose a confirmation link instead of a form.
    tags(page)
        .take(10_000)
        .filter(|tag| tag.name == "a")
        .filter_map(|tag| base.join(tag.attr("href")?).ok())
        .find(|url| {
            url.query_pairs()
                .any(|(k, v)| k == "confirm" && !v.is_empty())
                && url
                    .query_pairs()
                    .find(|(k, _)| k == "id")
                    .map(|(_, v)| v.into_owned())
                    == base
                        .query_pairs()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.into_owned())
        })
}

fn fusion_download(page: &str, base: &Url) -> Option<Url> {
    for key in [
        "downloadUrl",
        "downloadURL",
        "download_url",
        "nativeDownloadUrl",
    ] {
        let quoted = format!("\"{key}\"");
        for (offset, _) in page.match_indices(&quoted) {
            let Some(tail) = page
                .get(offset + quoted.len()..)
                .and_then(|s| s.trim_start().strip_prefix(':'))
            else {
                continue;
            };
            let tail = tail.trim_start();
            if let Some(Ok(value)) = serde_json::Deserializer::from_str(tail)
                .into_iter::<String>()
                .next()
            {
                if !value.trim().is_empty() {
                    if let Ok(url) = base.join(&entities(&value)) {
                        if url != *base {
                            return Some(url);
                        }
                    }
                }
            }
        }
    }
    let mut fallback = None;
    for tag in tags(page).take(10_000).filter(|tag| tag.name == "a") {
        // Download labels often wrap their text in a span. Bound the lookahead so
        // malformed pages with many unterminated anchors cannot multiply page work.
        let inner = tag
            .rest
            .chars()
            .take(1024)
            .collect::<String>()
            .to_ascii_lowercase();
        let label = inner.split("</a").next().unwrap_or_default();
        if tag.attr("download").is_none() && !label.contains("download") {
            continue;
        }
        let Some(url) = tag.attr("href").and_then(|href| base.join(href).ok()) else {
            continue;
        };
        if !Provider::Fusion.allows(&url) {
            continue;
        }
        // Prefer a named design over a site's general download or installer link.
        if supported_name(Path::new(&decode_percent(url.path()))) {
            return Some(url);
        }
        if fallback.is_none() {
            fallback = Some(url);
        }
    }
    fallback
}

#[cfg(test)]
mod tests;
