//! Clipboard classification and bounded, anonymous HTTPS downloads.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use url::Url;

pub(crate) const CANNOT_OPEN: &str =
    "Dutchman can't open this link. Download the file and drop it here.";
pub(crate) const MAX_DOWNLOAD: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Files(Vec<PathBuf>),
    Onshape(String),
    Download(Link),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) url: String,
    pub(crate) name: String,
    pub(crate) host: String,
}

fn safe_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

/// Native file lists take precedence over clipboard text on every platform.
pub(crate) fn classify(files: Vec<PathBuf>, text: &str) -> Input {
    if !files.is_empty() {
        return Input::Files(files);
    }
    let text = text.trim();
    if text.is_empty() {
        return Input::Other;
    }
    let paths: Option<Vec<PathBuf>> = text
        .lines()
        .map(|line| {
            let line = line.trim().trim_matches('"');
            let path = if let Ok(url) = Url::parse(line) {
                if url.scheme() == "file" {
                    url.to_file_path().ok()?
                } else {
                    PathBuf::from(line)
                }
            } else {
                PathBuf::from(line)
            };
            // A paste is a path only when it names an existing local file or folder.
            path.exists().then_some(path)
        })
        .collect();
    if let Some(paths) = paths {
        return Input::Files(paths);
    }
    if crate::onshape::looks_like_link(text) {
        return Input::Onshape(text.to_owned());
    }
    let Ok(mut url) = Url::parse(text) else {
        return Input::Other;
    };
    if !safe_https(&url) {
        return Input::Other;
    }
    if url.host_str() == Some("github.com") {
        let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
        let [owner, repo, "blob", rest @ ..] = parts.as_slice() else {
            return Input::Other;
        };
        if rest.len() < 2 {
            return Input::Other;
        }
        let path = format!("/{owner}/{repo}/{}", rest.join("/"));
        if url.set_host(Some("raw.githubusercontent.com")).is_err() {
            return Input::Other;
        }
        url.set_path(&path);
        url.set_query(None);
    }
    url.set_fragment(None);
    let Some(name) = url.path_segments().and_then(|mut parts| parts.next_back()) else {
        return Input::Other;
    };
    let Ok(name) = percent_encoding::percent_decode_str(name).decode_utf8() else {
        return Input::Other;
    };
    let name = name.into_owned();
    if name.contains(['/', '\\', ':']) || name.chars().any(char::is_control) {
        return Input::Other;
    }
    // Consult the same format metadata as file detection, including numbered .prt saves.
    let path = Path::new(&name);
    let suffix = dutchman_core::Format::part_revision(path)
        .map(|_| "prt")
        .or_else(|| path.extension().and_then(|s| s.to_str()));
    let supported = dutchman::batch::extension_of(path) == "zip"
        || suffix.is_some_and(|suffix| {
            dutchman_core::Format::ALL.into_iter().any(|format| {
                format.can_convert()
                    && format
                        .extensions()
                        .iter()
                        .any(|ext| suffix.eq_ignore_ascii_case(ext))
            })
        });
    if !supported {
        return Input::Other;
    }
    Input::Download(Link {
        url: url.to_string(),
        name,
        host: url.host_str().unwrap_or_default().to_owned(),
    })
}

pub(crate) fn clipboard() -> (Vec<PathBuf>, String) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return (Vec::new(), String::new());
    };
    let files = clipboard.get().file_list().unwrap_or_default();
    let text = if files.is_empty() {
        clipboard.get_text().unwrap_or_default()
    } else {
        String::new()
    };
    (files, text)
}

pub(crate) fn prefill_onshape(link: &mut String, clipboard: &str) {
    if link.is_empty() && crate::onshape::looks_like_link(clipboard) {
        *link = clipboard.trim().to_owned();
    }
}

/// Injectable transport; tests return response readers without making network requests.
pub struct Response {
    pub status: u16,
    pub length: Option<u64>,
    pub location: Option<String>,
    pub body: Box<dyn Read + Send>,
}

pub trait Transport: Send + Sync {
    fn request(&self, method: &str, url: &str) -> Result<Response, String>;
}

pub(crate) struct Network;
impl Transport for Network {
    fn request(&self, method: &str, url: &str) -> Result<Response, String> {
        // No cookie jar, authentication, or ambient proxy credentials. Redirects are checked below.
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(60))
            .build();
        let response = match agent.request(method, url).call() {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(_) => {
                return Err(
                    "Could not reach the download host. Check your connection and retry.".into(),
                )
            }
        };
        Ok(Response {
            status: response.status(),
            length: response
                .header("Content-Length")
                .and_then(|s| s.parse().ok()),
            location: response.header("Location").map(str::to_owned),
            body: response.into_reader(),
        })
    }
}

fn request(transport: &dyn Transport, method: &str, url: &str) -> Result<Response, String> {
    let mut url = Url::parse(url).map_err(|_| CANNOT_OPEN.to_owned())?;
    for redirect in 0..=5 {
        if !safe_https(&url) {
            return Err("Downloads need an HTTPS link without credentials.".into());
        }
        let response = transport.request(method, url.as_str())?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            if redirect == 5 {
                return Err("The download redirected more than five times. Download the file and drop it here.".into());
            }
            url = response.location.as_ref().and_then(|location| url.join(location).ok())
                .ok_or_else(|| "The download host did not supply a usable redirect. Download the file and drop it here.".to_owned())?;
        } else if (200..300).contains(&response.status) {
            return Ok(response);
        } else {
            return Err(format!(
                "The download host returned HTTP {}. Download the file and drop it here.",
                response.status
            ));
        }
    }
    Err("The download could not be followed. Download the file and drop it here.".into())
}

pub(crate) fn probe(transport: &dyn Transport, link: &Link) -> Result<Option<u64>, String> {
    // HEAD is advisory: hosts that refuse it can still supply a downloadable file.
    let length = request(transport, "HEAD", &link.url)
        .ok()
        .and_then(|response| response.length);
    if length.is_some_and(|length| length > MAX_DOWNLOAD) {
        return Err("The download exceeds the 2 GB limit. Use a smaller file.".into());
    }
    Ok(length)
}

pub(crate) fn download(
    transport: &dyn Transport,
    link: &Link,
    folder: &Path,
    progress: impl Fn(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let response = request(transport, "GET", &link.url)?;
    copy_download(response, &folder.join(&link.name), MAX_DOWNLOAD, progress)
}

fn copy_download(
    mut response: Response,
    path: &Path,
    cap: u64,
    progress: impl Fn(u64, Option<u64>),
) -> Result<PathBuf, String> {
    if response.length.is_some_and(|size| size > cap) {
        return Err("The download exceeds the 2 GB limit. Use a smaller file.".into());
    }
    let mut file = std::fs::File::create(path)
        .map_err(|_| "Could not save the download. Check free disk space and retry.".to_owned())?;
    let result = (|| {
        let mut total = 0_u64;
        let mut last_report = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = response.body.read(&mut buffer).map_err(|_| {
                "The download stopped before it finished. Check your connection and retry."
                    .to_owned()
            })?;
            if count == 0 {
                break;
            }
            total = total.saturating_add(count as u64);
            if total > cap {
                return Err("The download exceeds the 2 GB limit. Use a smaller file.".into());
            }
            file.write_all(buffer.get(..count).unwrap_or_default())
                .map_err(|_| {
                    "Could not save the download. Check free disk space and retry.".to_owned()
                })?;
            if total.saturating_sub(last_report) >= 1024 * 1024 {
                progress(total, response.length);
                last_report = total;
            }
        }
        if response.length.is_some_and(|size| total != size) {
            return Err(
                "The download stopped before it finished. Check your connection and retry.".into(),
            );
        }
        progress(total, response.length);
        Ok(path.to_path_buf())
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Onshape(String),
    Download(Link),
}

pub(crate) enum Message {
    Probed(Link, Result<Option<u64>, String>),
    Progress(u64, Option<u64>),
    /// Onshape calls sent so far, and the estimated total once it is known.
    Calls(u32, Option<u32>),
    Finished(Result<(PathBuf, Option<tempfile::TempDir>), String>),
    /// The Onshape link needs about this many calls, more than the app spends unasked.
    NeedsApproval(String, u32),
}

/// A question about Onshape API calls waiting for Continue or Cancel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Approval {
    pub(crate) link: String,
    /// The calls needed; `None` for a continued fetch whose remaining calls are unknown.
    pub(crate) estimate: Option<u32>,
    /// Asking to continue a fetch that stopped at its request limit.
    pub(crate) resume: bool,
}

/// Fetches an Onshape link into a bundle in the folder, telling the progress after each call.
pub type Fetch =
    fn(String, &Path, &mut dyn dutchman::onshape::FetchProgress) -> Result<PathBuf, String>;

/// Sends a fetch's call count to the window.
struct Calls {
    sender: std::sync::mpsc::Sender<Message>,
    ctx: eframe::egui::Context,
}

impl dutchman::onshape::FetchProgress for Calls {
    fn calls(&mut self, sent: u32, needed: Option<u32>) {
        let _ = self.sender.send(Message::Calls(sent, needed));
        self.ctx.request_repaint();
    }
}

pub(crate) struct Inbox {
    pub(crate) sender: std::sync::mpsc::Sender<Message>,
    pub(crate) receiver: std::sync::mpsc::Receiver<Message>,
    pub(crate) transport: std::sync::Arc<dyn Transport>,
    pub(crate) fetch: Fetch,
    /// Used instead of `fetch` when carrying on after a stop at the request limit.
    pub(crate) continue_fetch: Fetch,
    /// Set for the next `start`; it clears itself.
    pub(crate) continuing: bool,
    /// Set for the next `start` once the user allowed the estimated calls; it clears itself.
    pub(crate) approved: bool,
    /// Estimates the calls a link needs before any are spent.
    pub(crate) estimate: fn(String, Option<PathBuf>) -> Result<u32, String>,
    pub(crate) approval: Option<Approval>,
    /// The Onshape link whose fetch stopped at the request limit, while it can be continued.
    pub(crate) resumable: Option<String>,
    /// About how many more calls that stopped fetch needs, when known.
    pub(crate) stop_remaining: Option<u32>,
    pub(crate) last_onshape: Option<String>,
    pub(crate) bearer_token_file: Option<PathBuf>,
    pub(crate) active: bool,
    pub(crate) row: Option<usize>,
    pub(crate) caption: String,
    pub(crate) progress: Option<(u64, Option<u64>)>,
    pub(crate) queue: std::collections::VecDeque<Source>,
    pub(crate) choosing_output: Option<Source>,
    pub(crate) prompt: Option<(Link, Option<u64>)>,
    // Keep downloads alive for conversion, inspection and retry, then remove them on exit.
    pub(crate) folders: Vec<tempfile::TempDir>,
}

impl Inbox {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            sender,
            receiver,
            transport: std::sync::Arc::new(Network),
            fetch: crate::onshape::fetch,
            continue_fetch: crate::onshape::fetch_continuing,
            continuing: false,
            approved: false,
            estimate: crate::onshape::estimate_calls,
            approval: None,
            resumable: None,
            stop_remaining: None,
            last_onshape: None,
            bearer_token_file: None,
            active: false,
            row: None,
            caption: String::new(),
            progress: None,
            queue: Default::default(),
            choosing_output: None,
            prompt: None,
            folders: Vec::new(),
        }
    }

    pub(crate) fn probe(&mut self, link: Link, ctx: &eframe::egui::Context) {
        self.active = true;
        let sender = self.sender.clone();
        let transport = self.transport.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = probe(transport.as_ref(), &link);
            let _ = sender.send(Message::Probed(link, result));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start(
        &mut self,
        source: Source,
        output: PathBuf,
        row: usize,
        ctx: &eframe::egui::Context,
    ) {
        self.active = true;
        self.row = Some(row);
        let sender = self.sender.clone();
        let transport = self.transport.clone();
        let continuing = std::mem::take(&mut self.continuing);
        let approved = std::mem::take(&mut self.approved) || continuing;
        let estimate = self.estimate;
        let fetch = if continuing {
            self.continue_fetch
        } else {
            self.fetch
        };
        self.resumable = None;
        if let Source::Onshape(link) = &source {
            self.last_onshape = Some(link.clone());
        }
        let bearer_token_file = self.bearer_token_file.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // A panic must still end the job: without a message the card would show its
            // fetching caption forever.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let link = match source {
                    Source::Onshape(link) => {
                        if !approved {
                            let calls = estimate(link.clone(), bearer_token_file.clone())?;
                            if calls > dutchman::onshape::ASK_ABOVE_CALLS {
                                let _ = sender.send(Message::NeedsApproval(link, calls));
                                ctx.request_repaint();
                                return Ok(None);
                            }
                        }
                        let (again, token) = (link.clone(), bearer_token_file.clone());
                        let mut calls = Calls {
                            sender: sender.clone(),
                            ctx: ctx.clone(),
                        };
                        let result = if bearer_token_file.is_some() {
                            crate::onshape::fetch_with_budget(
                                link,
                                &output,
                                bearer_token_file,
                                continuing.then_some(crate::onshape::CONTINUE_CALLS),
                                &mut calls,
                            )
                        } else {
                            fetch(link, &output, &mut calls)
                        };
                        return match result {
                            // The replies fetched so far are cached, so estimating again
                            // costs no call and gives what is left.
                            Err(why) => match crate::onshape::parse_budget_stop(&why) {
                                Some((made, None)) => Err(match estimate(again, token) {
                                    Ok(more) if more > 0 => {
                                        crate::onshape::budget_stop_text(made, Some(more))
                                    }
                                    _ => why,
                                }),
                                _ => Err(why),
                            },
                            Ok(path) => Ok(Some((path, None))),
                        };
                    }
                    Source::Download(link) => link,
                };
                let folder = tempfile::Builder::new()
                    .prefix("dutchman-download-")
                    .tempdir()
                    .map_err(|_| {
                        "Could not make a download folder. Check free disk space and retry."
                            .to_owned()
                    })?;
                let path = download(transport.as_ref(), &link, folder.path(), |bytes, size| {
                    let _ = sender.send(Message::Progress(bytes, size));
                    ctx.request_repaint();
                })?;
                Ok(Some((path, Some(folder))))
            }))
            .unwrap_or_else(|_| {
                Err("Dutchman stopped unexpectedly while fetching. Retry; if it happens again, report it with the link's type (Part Studio or assembly).".to_owned())
            });
            match result {
                Ok(None) => {}
                Ok(Some(done)) => {
                    let _ = sender.send(Message::Finished(Ok(done)));
                }
                Err(why) => {
                    let _ = sender.send(Message::Finished(Err(why)));
                }
            }
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Mutex;

    const ONSHAPE: &str = "https://cad.onshape.com/documents/aaaaaaaaaaaaaaaaaaaaaaaa/w/bbbbbbbbbbbbbbbbbbbbbbbb/e/cccccccccccccccccccccccc";

    #[test]
    fn classifies_native_files_paths_onshape_and_supported_https_links_in_order() {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("Part with spaces.f3d");
        let second = folder.path().join("second.ipt");
        std::fs::write(&first, []).unwrap();
        std::fs::write(&second, []).unwrap();
        assert_eq!(
            classify(vec![first.clone()], ONSHAPE),
            Input::Files(vec![first.clone()])
        );
        assert_eq!(
            classify(
                vec![],
                &format!("\"{}\"\n{}", first.display(), second.display())
            ),
            Input::Files(vec![first.clone(), second])
        );
        assert_eq!(
            classify(vec![], Url::from_file_path(&first).unwrap().as_str()),
            Input::Files(vec![first])
        );
        assert_eq!(classify(vec![], ONSHAPE), Input::Onshape(ONSHAPE.into()));
        assert_eq!(
            classify(vec![], folder.path().to_str().unwrap()),
            Input::Files(vec![folder.path().into()])
        );
        for format in dutchman_core::Format::ALL
            .into_iter()
            .filter(|f| f.can_convert())
        {
            for extension in format.extensions() {
                assert!(
                    matches!(
                        classify(vec![], &format!("https://example.test/part.{extension}")),
                        Input::Download(_)
                    ),
                    "{extension}"
                );
            }
        }
        for link in [
            "https://example.test/page",
            "http://example.test/a.f3d",
            "https://user:secret@example.test/a.f3d",
            "some words",
            "https://drive.google.com/file/d/123/view",
            "https://example.test/a.exe",
        ] {
            assert_eq!(classify(vec![], link), Input::Other, "{link}");
        }
    }

    #[test]
    fn rewrites_github_blob_links_and_decodes_the_filename() {
        let Input::Download(link) = classify(
            vec![],
            "https://github.com/owner/repo/blob/main/folder/Part%20one.f3d?raw=true",
        ) else {
            panic!("download link")
        };
        assert_eq!(
            link.url,
            "https://raw.githubusercontent.com/owner/repo/main/folder/Part%20one.f3d"
        );
        assert_eq!(link.name, "Part one.f3d");
        assert_eq!(link.host, "raw.githubusercontent.com");
    }

    struct Mock {
        calls: Mutex<Vec<String>>,
        location: Option<&'static str>,
        length: Option<u64>,
    }
    impl Transport for Mock {
        fn request(&self, method: &str, url: &str) -> Result<Response, String> {
            self.calls
                .lock()
                .map_err(|_| "Mock call recorder is unavailable.".to_owned())?
                .push(format!("{method} {url}"));
            Ok(Response {
                status: if self.location.is_some() { 302 } else { 200 },
                length: self.length,
                location: self.location.map(str::to_owned),
                body: Box::new(Cursor::new(b"synthetic")),
            })
        }
    }
    fn link() -> Link {
        Link {
            url: "https://example.test/part.f3d".into(),
            name: "part.f3d".into(),
            host: "example.test".into(),
        }
    }

    #[test]
    fn head_reports_known_and_unknown_sizes_and_refuses_oversized_files() {
        for length in [None, Some(9)] {
            let mock = Mock {
                calls: Mutex::default(),
                location: None,
                length,
            };
            assert_eq!(probe(&mock, &link()).unwrap(), length);
            assert_eq!(
                *mock.calls.lock().unwrap(),
                vec!["HEAD https://example.test/part.f3d"]
            );
        }
        let mock = Mock {
            calls: Mutex::default(),
            location: None,
            length: Some(MAX_DOWNLOAD + 1),
        };
        assert!(probe(&mock, &link()).unwrap_err().contains("2 GB"));
    }

    #[test]
    fn follows_at_most_five_https_redirects_and_never_sends_credentials() {
        for location in [
            "http://other.test/file.f3d",
            "https://user:secret@other.test/file.f3d",
        ] {
            let mock = Mock {
                calls: Mutex::default(),
                location: Some(location),
                length: None,
            };
            assert!(request(&mock, "GET", &link().url).is_err());
            assert_eq!(mock.calls.lock().unwrap().len(), 1);
        }
        let mock = Mock {
            calls: Mutex::default(),
            location: Some("/again.f3d"),
            length: None,
        };
        assert!(request(&mock, "GET", &link().url)
            .err()
            .unwrap()
            .contains("five"));
        assert_eq!(mock.calls.lock().unwrap().len(), 6);
    }

    #[test]
    fn streams_to_disk_reports_progress_and_removes_capped_or_short_downloads() {
        let folder = tempfile::tempdir().unwrap();
        let mock = Mock {
            calls: Mutex::default(),
            location: None,
            length: Some(9),
        };
        let events = Mutex::new(Vec::new());
        let path = download(&mock, &link(), folder.path(), |n, size| {
            events.lock().unwrap().push((n, size))
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic");
        assert_eq!(*events.lock().unwrap(), vec![(9, Some(9))]);
        for (length, cap) in [(None, 4), (Some(10), 20)] {
            let path = folder.path().join("failed.f3d");
            let response = Response {
                status: 200,
                length,
                location: None,
                body: Box::new(Cursor::new(b"synthetic")),
            };
            assert!(copy_download(response, &path, cap, |_, _| {}).is_err());
            assert!(!path.exists());
        }
    }
}
