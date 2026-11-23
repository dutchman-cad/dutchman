//! Fetch raw Onshape responses into a private, versioned bundle. No CAD decoding happens here.
mod auth;
mod derived;
mod export;
mod http;
mod store;

use auth::Auth;
pub use auth::{BearerToken, Credentials, FetchAuth};
use http::Session;
pub use http::{FetchProgress, HttpClient, NetworkClient, Request, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
pub use store::{Backend, FileBackend, Source, Store, SystemBackend, NOT_CONNECTED};

pub const SCHEMA: &str = "dutchman.onshape-fetch/1";
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
const API: &str = "/api/v17";
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Url,
    Auth,
    Budget,
    Approval,
    YearlyLimit,
    Throttle,
    Transport,
    Response,
    Io,
    Limit,
    Privacy,
    Unsupported,
    Changed,
    Export,
    Stopped,
}
impl ErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Url => "onshape/url",
            Self::Auth => "onshape/auth",
            Self::Budget => "onshape/budget",
            Self::Approval => "onshape/approval",
            Self::YearlyLimit => "onshape/yearly-limit",
            Self::Throttle => "onshape/throttle",
            Self::Transport => "onshape/transport",
            Self::Response => "onshape/response",
            Self::Io => "onshape/io",
            Self::Limit => "onshape/limit",
            Self::Privacy => "onshape/privacy",
            Self::Unsupported => "onshape/not-supported",
            Self::Changed => "onshape/changed",
            Self::Export => "onshape/export",
            Self::Stopped => "onshape/stopped",
        }
    }
}
#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: &'static str,
    /// The system's own reason, when there is one (an I/O error's text; std's io errors carry no paths).
    pub detail: Option<String>,
    pub calls: u32,
    /// For [`ErrorKind::Approval`]: the calls the whole fetch is estimated to need.
    pub estimate: Option<u32>,
}
impl Error {
    pub(crate) fn new(kind: ErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            detail: None,
            calls: 0,
            estimate: None,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(f, "{} ({detail}; {} API calls)", self.message, self.calls),
            None => write!(f, "{} ({} API calls)", self.message, self.calls),
        }
    }
}
impl std::error::Error for Error {}
fn io_error(cause: impl std::fmt::Display) -> Error {
    let mut error = Error::new(
        ErrorKind::Io,
        "Could not read or write the private Onshape cache or bundle; check permissions and free disk space, then retry.",
    );
    error.detail = Some(cause.to_string());
    error
}
fn response_error() -> Error {
    Error::new(
        ErrorKind::Response,
        "Onshape returned an unexpected response; check document access, update Dutchman, and retry.",
    )
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn id(value: &str) -> Result<String> {
    if value.len() != 24 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new(ErrorKind::Url, "Use an Onshape document URL with 24-character document, workspace/version, and element IDs."));
    }
    Ok(value.into())
}

#[derive(Debug, Clone)]
pub struct DocumentUrl {
    pub document: String,
    pub kind: String,
    pub version: String,
    pub element: String,
}
impl DocumentUrl {
    pub fn parse(input: &str, version: Option<&str>) -> Result<Self> {
        let fail = || {
            Error::new(ErrorKind::Url, "Use https://cad.onshape.com/documents/<id>/w/<workspace>/e/<element> or a /v/<version> URL; --version takes a version ID.")
        };
        let url = url::Url::parse(input).map_err(|_| fail())?;
        if url.scheme() != "https"
            || url.host_str() != Some("cad.onshape.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
        {
            return Err(fail());
        }
        let segments: Vec<_> = url
            .path()
            .trim_end_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let ["documents", did, kind @ ("w" | "v"), vid, "e", eid] = segments.as_slice() else {
            return Err(fail());
        };
        Ok(Self {
            document: id(did)?,
            kind: if version.is_some() {
                "v".into()
            } else {
                (*kind).into()
            },
            version: id(version.unwrap_or(vid))?,
            element: id(eid)?,
        })
    }
}

#[derive(clap::Args, Clone)]
pub struct FetchOptions {
    /// URL of one of your own Onshape Part Studios or assemblies.
    pub document_url: String,
    /// Bundle file (default: <element-id>.onshape-bundle).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Immutable Onshape version ID, overriding the URL's workspace/version.
    #[arg(long)]
    pub version: Option<String>,
    /// Read an OAuth bearer token from a private file (or set DUTCHMAN_ONSHAPE_BEARER_TOKEN).
    #[arg(long)]
    pub bearer_token_file: Option<PathBuf>,
    /// Maximum HTTP attempts, including metadata, polling, retries and downloads. Without it
    /// the limit grows with the document's elements and parts, up to a fixed ceiling.
    #[arg(long)]
    pub max_calls: Option<u32>,
    /// Fetch without asking when the estimate is above 50 calls. Onshape's Free plan
    /// allows 2,500 API calls a year.
    #[arg(long)]
    pub yes: bool,
    /// Print the same structured report as the MCP tool.
    #[arg(long)]
    pub json: bool,
}

/// Everything a fetch needs that does not come from Onshape.
struct Setup {
    source: DocumentUrl,
    credentials: FetchAuth,
    cache: PathBuf,
    output: PathBuf,
}

fn setup(options: &FetchOptions) -> Result<Setup> {
    let source = DocumentUrl::parse(&options.document_url, options.version.as_deref())?;
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).ok_or_else(|| Error::new(ErrorKind::Io, "Set HOME or USERPROFILE so Dutchman can find its private Onshape config and cache."))?;
    let credentials = FetchAuth::find(&Store::system(), options.bearer_token_file.as_deref())?
        .ok_or_else(|| Error::new(ErrorKind::Auth, NOT_CONNECTED))?;
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("dutchman/onshape");
    let output = options
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("{}.onshape-bundle", source.element)));
    Ok(Setup {
        source,
        credentials,
        cache,
        output,
    })
}

/// Fetches, and asks nothing: the MCP tool and scripts get the same stop with the estimate
/// when the fetch is above [`ASK_ABOVE_CALLS`] and not allowed (see [`fetch_gated`]).
pub fn run(options: &FetchOptions) -> Value {
    run_with_progress(options, None)
}

/// [`run`], telling `progress` after each call.
pub fn run_with_progress(
    options: &FetchOptions,
    mut progress: Option<&mut dyn FetchProgress>,
) -> Value {
    let result = setup(options).and_then(|setup| {
        // Reborrow for this fetch alone; the caller's progress outlives it.
        let progress = progress
            .as_mut()
            .map(|progress| &mut **progress as &mut dyn FetchProgress);
        fetch_gated_watched(
            &mut NetworkClient::default(),
            &setup.credentials,
            &setup.source,
            &setup.output,
            &setup.cache,
            options.max_calls,
            options.yes,
            progress,
        )
    });
    crate::json::onshape_fetch(&result)
}

/// Estimates the calls a fetch will need, for the command line and the app to show before
/// anything is spent. Spends only the few calls that list the document.
pub fn estimate(options: &FetchOptions) -> Result<Estimate> {
    let setup = setup(options)?;
    estimate_calls(
        &mut NetworkClient::default(),
        &setup.credentials,
        &setup.source,
        &setup.cache,
    )
}

/// Fetches after the estimate was shown and, if it was above [`ASK_ABOVE_CALLS`], allowed.
pub fn run_after(options: &FetchOptions, estimate: &Estimate) -> Value {
    let result = setup(options).and_then(|setup| {
        fetch_approved(
            &mut NetworkClient::default(),
            &setup.credentials,
            &setup.source,
            &setup.output,
            &setup.cache,
            estimate,
        )
    });
    crate::json::onshape_fetch(&result)
}

/// A fetch above this many estimated calls needs the user's go. Onshape's Free and Standard
/// plans allow 2,500 API calls a year, so a large fetch is a real share of it.
pub const ASK_ABOVE_CALLS: u32 = 50;
/// Calls a year on Onshape's Free plan, named wherever Dutchman asks.
pub const FREE_PLAN_YEARLY_CALLS: u32 = 2500;

/// A call count with a thousands separator from 1,000 up: 2500 reads "2,500".
pub fn calls_text(calls: u32) -> String {
    let digits = calls.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// What a fetch is expected to cost, worked out from the document's own listing.
#[derive(Debug, Clone)]
pub struct Estimate {
    /// Calls the whole fetch is expected to use, including the listing already made. An
    /// estimate: bodies and referenced studios are only known once the fetch reads them.
    pub calls: u32,
    /// Real requests the estimate itself made. Cached replies are not counted.
    pub listing_calls: u32,
    /// Part Studios and assemblies the fetch is expected to read.
    pub elements: u32,
    mid: String,
}
impl Estimate {
    pub fn needs_approval(&self) -> bool {
        self.calls > ASK_ABOVE_CALLS
    }

    /// The text that asks for the user's go, without naming a command-line option.
    pub fn question(&self) -> String {
        format!(
            "This document needs about {} of your yearly Onshape API calls ({} on Free plans). Continue?",
            calls_text(self.calls),
            calls_text(FREE_PLAN_YEARLY_CALLS)
        )
    }

    /// The stop for a fetch that needs a go it did not get.
    pub fn approval_error(&self) -> Error {
        let mut error = Error::new(ErrorKind::Approval, APPROVAL_NEEDED);
        error.calls = self.listing_calls;
        error.estimate = Some(self.calls);
        error
    }
}

pub const APPROVAL_NEEDED: &str = "This Onshape document needs more of your yearly API calls than Dutchman spends without asking (Onshape's Free plan allows 2,500 a year), so it stopped before fetching. Ask the user, then allow it with --yes on the command line, or with an explicit --max-calls (MCP: max_calls) that caps the calls it may use.";
pub const APPROVAL_DECLINED: &str = "Fetch cancelled. No more Onshape API calls were used.";

/// Lists the document, which costs at most three calls and is cached, and counts what the
/// fetch will read.
pub fn estimate_calls<C: HttpClient>(
    client: &mut C,
    credentials: &FetchAuth,
    source: &DocumentUrl,
    cache: &Path,
) -> Result<Estimate> {
    let credentials = credentials.as_auth();
    check_source(&credentials, source)?;
    let mut session = Session {
        client,
        credentials,
        calls: 0,
        max_calls: http::AUTO_CEILING_CALLS,
        auto: false,
        progress: None,
        needed: None,
    };
    let (scope, mid) = resolve(&mut session, source, cache)?;
    let content_kind = if source.kind == "v" { "v" } else { "m" };
    let content_id = if source.kind == "v" {
        source.version.clone()
    } else {
        mid.clone()
    };
    let mut fetcher = Fetcher::new(&mut session, &scope, source, &mid, content_kind);
    document_name(&mut fetcher, source)?;
    let tabs = fetcher.json(
        &format!(
            "{API}/documents/d/{}/{content_kind}/{content_id}/elements",
            source.document
        ),
        "",
    )?;
    let tabs = tabs.as_array().ok_or_else(response_error)?;
    let kind_of = |tab: &Value| {
        tab.get("elementType")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let linked = tabs
        .iter()
        .find(|tab| tab.get("id").and_then(Value::as_str) == Some(&source.element))
        .ok_or_else(response_error)?;
    let studios = |tabs: &[Value]| {
        tabs.iter()
            .filter(|tab| matches!(kind_of(tab).as_deref(), Some("PARTSTUDIO" | "ASSEMBLY")))
            .count()
    };
    // An assembly may use any studio of the document; a studio is read alone.
    let elements = if kind_of(linked).as_deref() == Some("ASSEMBLY") {
        studios(tabs)
    } else {
        1
    };
    let mut remaining = u32::try_from(elements)
        .unwrap_or(u32::MAX)
        .saturating_mul(http::AUTO_ELEMENT_CALLS);
    if kind_of(linked).as_deref() == Some("PARTSTUDIO") {
        // Replies already cached cost nothing.
        let base = format!(
            "{API}/partstudios/d/{}/{content_kind}/{content_id}/e/{}",
            source.document, source.element
        );
        let features = format!("{base}/features");
        let cached = [
            fetcher.is_cached(
                &features,
                "rollbackBarIndex=-1&includeGeometryIds=true&noSketchGeometry=false",
            ),
            fetcher.is_cached(&format!("{base}/sketches"), ""),
        ];
        remaining = remaining.saturating_sub(u32::from(cached[0]) + u32::from(cached[1]));
    }
    let listing_calls = session.calls;
    Ok(Estimate {
        calls: listing_calls.saturating_add(remaining),
        listing_calls,
        elements: u32::try_from(elements).unwrap_or(u32::MAX),
        mid,
    })
}

/// Fetches once the estimate was shown and any needed go was given. The call limit is the
/// automatic one, and the calls the estimate made count against it.
pub fn fetch_approved<C: HttpClient>(
    client: &mut C,
    credentials: &FetchAuth,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    estimate: &Estimate,
) -> Result<FetchReport> {
    fetch_auth(
        client,
        credentials.as_auth(),
        source,
        output,
        cache,
        http::AUTO_BASE_CALLS.max(estimate.listing_calls),
        true,
        Some(estimate),
        None,
    )
}

/// What a command line decides about an estimate. `answer` is asked only when `interactive`
/// and the estimate is above [`ASK_ABOVE_CALLS`] without `yes`.
pub fn approval(
    estimate: &Estimate,
    yes: bool,
    interactive: bool,
    answer: impl FnOnce(&Estimate) -> bool,
) -> Result<()> {
    if !estimate.needs_approval() || yes {
        return Ok(());
    }
    if !interactive {
        return Err(estimate.approval_error());
    }
    if answer(estimate) {
        Ok(())
    } else {
        let mut error = estimate.approval_error();
        error.message = APPROVAL_DECLINED;
        Err(error)
    }
}

/// The whole gated fetch. An explicit `max_calls` is the user's own ceiling and the consent
/// to spend up to it, so nothing is estimated or asked. Otherwise the document is estimated
/// first; above [`ASK_ABOVE_CALLS`] and without `yes` it stops with the estimate and spends
/// no more than the listing.
pub fn fetch_gated<C: HttpClient>(
    client: &mut C,
    credentials: &FetchAuth,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    max_calls: Option<u32>,
    yes: bool,
) -> Result<FetchReport> {
    fetch_gated_watched(
        client,
        credentials,
        source,
        output,
        cache,
        max_calls,
        yes,
        None,
    )
}

/// [`fetch_gated`], telling `progress` after each call.
#[allow(clippy::too_many_arguments)]
fn fetch_gated_watched<'a, C: HttpClient>(
    client: &'a mut C,
    credentials: &'a FetchAuth,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    max_calls: Option<u32>,
    yes: bool,
    progress: Option<&'a mut dyn FetchProgress>,
) -> Result<FetchReport> {
    if max_calls.is_some() || yes {
        return fetch_watched(
            client,
            credentials,
            source,
            output,
            cache,
            max_calls,
            progress,
        );
    }
    let estimate = estimate_calls(client, credentials, source, cache)?;
    approval(&estimate, false, false, |_| false)?;
    fetch_auth(
        client,
        credentials.as_auth(),
        source,
        output,
        cache,
        http::AUTO_BASE_CALLS.max(estimate.listing_calls),
        true,
        Some(&estimate),
        progress,
    )
}

/// The name of the account the keys belong to, from one cheap authenticated call. Nothing
/// else about the account is read or kept.
pub fn session_user<C: HttpClient>(client: &mut C, credentials: &Credentials) -> Result<String> {
    let mut session = Session {
        client,
        credentials: Auth::ApiKeys(credentials),
        calls: 0,
        max_calls: 1,
        auto: false,
        progress: None,
        needed: None,
    };
    let response = session
        .request("GET", &format!("{API}/users/sessioninfo"), "", b"")
        .map_err(|mut error| {
            if error.kind == ErrorKind::Auth {
                error.message = "Onshape did not accept these keys; copy both in full from Onshape's developer portal, with the read profile and read documents permissions, and retry.";
            }
            error
        })?;
    if !(200..300).contains(&response.status) {
        return Err(Error::new(
            ErrorKind::Response,
            "Onshape could not check the keys right now; retry in a moment.",
        ));
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| response_error())?;
    Ok(value
        .get("name")
        .and_then(Value::as_str)
        .map(|name| {
            name.chars()
                .filter(|c| !c.is_control())
                .take(100)
                .collect::<String>()
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "your Onshape account".into()))
}

/// What `status` found.
#[derive(Debug)]
pub enum Connection {
    NotConnected,
    Connected {
        user: String,
        source: Source,
    },
    /// Keys are saved, but Onshape could not confirm them now.
    Unchecked {
        source: Source,
        error: Error,
    },
}

/// Looks for saved keys and, when there are some, asks Onshape who they belong to.
pub fn status<C: HttpClient>(store: &Store, client: &mut C) -> Result<Connection> {
    let Some((credentials, source)) = store.find()? else {
        return Ok(Connection::NotConnected);
    };
    Ok(match session_user(client, &credentials) {
        Ok(user) => Connection::Connected { user, source },
        Err(error) => Connection::Unchecked { source, error },
    })
}

/// Checks the keys with Onshape, and only then saves them. Returns the account name and where
/// they were saved. Wrong keys are never stored.
pub fn login<C: HttpClient>(
    store: &Store,
    client: &mut C,
    access: String,
    secret: String,
) -> Result<(String, Source)> {
    let credentials = Credentials::new(access, secret)?;
    let user = session_user(client, &credentials)?;
    let source = store.save(&credentials)?;
    Ok((user, source))
}

pub struct FetchReport {
    pub output: PathBuf,
    pub microversion: String,
    pub calls: u32,
    pub cache_hits: u32,
}

pub fn fetch<C: HttpClient>(
    client: &mut C,
    credentials: &Credentials,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    max_calls: u32,
) -> Result<FetchReport> {
    fetch_auth(
        client,
        Auth::ApiKeys(credentials),
        source,
        output,
        cache,
        max_calls,
        false,
        None,
        None,
    )
}

/// Fetch using either API keys or an OAuth bearer token.
pub fn fetch_authenticated<C: HttpClient>(
    client: &mut C,
    credentials: &FetchAuth,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    max_calls: u32,
) -> Result<FetchReport> {
    fetch_auth(
        client,
        credentials.as_auth(),
        source,
        output,
        cache,
        max_calls,
        false,
        None,
        None,
    )
}

/// Fetch with no fixed call limit. The limit starts at 50 and grows as the document's
/// elements and body-detail requests are found, so an ordinary document finishes, and it
/// stops at 2000 calls however large the document claims to be. Retries, polling and
/// refetches draw on the same limit and never raise it.
pub fn fetch_with_document_budget<C: HttpClient>(
    client: &mut C,
    credentials: &FetchAuth,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
) -> Result<FetchReport> {
    fetch_auth(
        client,
        credentials.as_auth(),
        source,
        output,
        cache,
        http::AUTO_BASE_CALLS,
        true,
        None,
        None,
    )
}

/// [`fetch_authenticated`] with `Some(max_calls)`, else [`fetch_with_document_budget`],
/// telling `progress` after each call.
pub fn fetch_watched<'a, C: HttpClient>(
    client: &'a mut C,
    credentials: &'a FetchAuth,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    max_calls: Option<u32>,
    progress: Option<&'a mut dyn FetchProgress>,
) -> Result<FetchReport> {
    fetch_auth(
        client,
        credentials.as_auth(),
        source,
        output,
        cache,
        max_calls.unwrap_or(http::AUTO_BASE_CALLS),
        max_calls.is_none(),
        None,
        progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn fetch_auth<'a, C: HttpClient>(
    client: &'a mut C,
    credentials: Auth<'a>,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    max_calls: u32,
    auto: bool,
    after: Option<&Estimate>,
    progress: Option<&'a mut dyn FetchProgress>,
) -> Result<FetchReport> {
    check_source(&credentials, source)?;
    credentials.check_bytes(output.as_os_str().to_string_lossy().as_bytes())?;
    let mut session = Session {
        client,
        credentials,
        // Calls the estimate already made count against the same limit.
        calls: after.map_or(0, |estimate| estimate.listing_calls),
        max_calls,
        auto,
        progress,
        needed: None,
    };
    let result = fetch_inner(
        &mut session,
        source,
        output,
        cache,
        after.map(|estimate| estimate.mid.as_str()),
    );
    result.map_err(|mut error| {
        error.calls = session.calls;
        error
    })
}

enum Capture {
    Complete(FetchReport),
    Changed(String),
}

struct Fetcher<'a, 'b, C> {
    session: &'a mut Session<'b, C>,
    cache: PathBuf,
    microversion: String,
    content_kind: &'static str,
    file_sources: BTreeMap<String, Value>,
    skew: bool,
    hits: u32,
    files: BTreeMap<String, Vec<u8>>,
    total: usize,
}
impl<C: HttpClient> Fetcher<'_, '_, C> {
    fn record_source(&mut self, name: &str, kind: &str) {
        self.file_sources.insert(
            name.into(),
            json!({"path_kind": kind, "microversion": self.microversion}),
        );
    }

    /// Where the reply to `path` is cached. Workspace replies (variables and the STEP export,
    /// which Onshape serves only from a workspace or version) go to a `w` folder of their
    /// own: they hold for this microversion only once a later check finds the workspace
    /// still at it, and a check that finds it changed deletes the folder.
    pub(super) fn cache_dir(&self, path: &str) -> PathBuf {
        if path.contains("/w/") {
            self.cache.join("w")
        } else {
            self.cache.clone()
        }
    }

    /// Saves each reply as it arrives, so a fetch stopped by the call limit, a network
    /// failure or a restart sends only the calls it is missing when run again.
    fn cache_response(&mut self, file: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = file.parent() {
            private_dir(parent)?;
        }
        private_write(file, bytes)
    }

    fn cache_file(&self, path: &str, query: &str) -> PathBuf {
        self.cache_dir(path)
            .join(hash(format!("{path}?{query}").as_bytes()))
    }

    /// 1 when [`Self::get`] would send this request, 0 when its reply is cached.
    fn missing(&self, path: &str, query: &str) -> u32 {
        let file = self.cache_file(path, query);
        u32::from(!file.exists() && !file.with_extension("absent").exists())
    }

    fn get(&mut self, path: &str, query: &str, optional: bool) -> Result<Option<Vec<u8>>> {
        let cache_file = self.cache_file(path, query);
        let absent = cache_file.with_extension("absent");
        if optional && absent.exists() {
            self.hits += 1;
            return Ok(None);
        }
        if cache_file.exists() {
            let bytes = read_bounded(&cache_file, MAX_RESPONSE)?;
            self.session.credentials.check_bytes(&bytes)?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| response_error())?;
            self.session.credentials.check_json(&value)?;
            privacy(&value)?;
            check_microversion(&value, &self.microversion)?;
            self.hits += 1;
            return Ok(Some(bytes));
        }
        let response = self.session.request("GET", path, query, &[])?;
        if optional && response.status == 404 {
            self.cache_response(&absent, b"")?;
            return Ok(None);
        }
        if !(200..300).contains(&response.status) {
            return Err(response_error());
        }
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| response_error())?;
        self.session.credentials.check_json(&value)?;
        privacy(&value)?;
        if path.contains("/w/") && check_microversion(&value, &self.microversion).is_err() {
            // The workspace has moved on; this attempt ends as changed, so keep nothing.
            self.skew = true;
        } else {
            check_microversion(&value, &self.microversion)?;
            self.cache_response(&cache_file, &response.body)?;
        }
        Ok(Some(response.body))
    }
    fn json(&mut self, path: &str, query: &str) -> Result<Value> {
        let bytes = self.get(path, query, false)?.ok_or_else(response_error)?;
        serde_json::from_slice(&bytes).map_err(|_| response_error())
    }
    fn add(&mut self, name: String, bytes: Vec<u8>) -> Result<()> {
        self.session.credentials.check_bytes(&bytes)?;
        self.total = self
            .total
            .checked_add(bytes.len())
            .ok_or_else(response_error)?;
        if self.total > MAX_TOTAL || self.files.len() >= 10000 {
            return Err(Error::new(
                ErrorKind::Limit,
                "The Onshape bundle exceeds 512 MiB or 10,000 files; fetch a smaller element.",
            ));
        }
        self.record_source(&name, self.content_kind);
        self.files.insert(name, bytes);
        Ok(())
    }
    fn raw(
        &mut self,
        path: &str,
        query: &str,
        name: String,
        optional: bool,
    ) -> Result<Option<Value>> {
        let Some(bytes) = self.get(path, query, optional)? else {
            return Ok(None);
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| response_error())?;
        privacy(&value)?;
        let kind = if path.contains("/w/") {
            "w"
        } else {
            self.content_kind
        };
        self.add(name.clone(), bytes)?;
        self.record_source(&name, kind);
        Ok(Some(value))
    }
}

fn check_source(credentials: &Auth<'_>, source: &DocumentUrl) -> Result<()> {
    id(&source.document)?;
    id(&source.version)?;
    id(&source.element)?;
    for identifier in [&source.document, &source.version, &source.element] {
        credentials.check_bytes(identifier.as_bytes())?;
    }
    if !matches!(source.kind.as_str(), "w" | "v") {
        return Err(Error::new(
            ErrorKind::Url,
            "Choose an Onshape workspace or version URL.",
        ));
    }
    Ok(())
}

/// The microversion to fetch, and the private folder for this account's cache. A workspace
/// must be resolved again: caching its mutable head would return stale geometry. Versions are
/// immutable, so their resolution may safely be reused without any API call.
fn resolve<C: HttpClient>(
    session: &mut Session<'_, C>,
    source: &DocumentUrl,
    cache: &Path,
) -> Result<(PathBuf, String)> {
    let scope = cache.join(session.credentials.scope());
    private_dir(&scope)?;
    let resolution = scope.join(format!(
        "{}-{}-{}-resolution.json",
        source.document, source.kind, source.version
    ));
    // A workspace fetch that stopped before it finished (call limit, network, a restart)
    // carries on at the microversion it started from, so its saved replies all still apply.
    // The closing microversion check then proves the workspace never moved, or starts over.
    let unfinished = unfinished_path(&scope, source);
    let mid = if source.kind == "v" && resolution.exists() {
        String::from_utf8(read_bounded(&resolution, 128)?).map_err(|_| response_error())?
    } else if source.kind == "w" && unfinished.exists() {
        String::from_utf8(read_bounded(&unfinished, 128)?).map_err(|_| response_error())?
    } else {
        let mid = current_microversion(session, source)?;
        if source.kind == "v" {
            private_write(&resolution, mid.as_bytes())?;
        }
        mid
    };
    Ok((scope, id(&mid).map_err(|_| response_error())?))
}

/// Where a workspace fetch records the microversion it started from until it finishes.
fn unfinished_path(scope: &Path, source: &DocumentUrl) -> PathBuf {
    scope.join(format!(
        "{}-w-{}-{}-unfinished.json",
        source.document, source.version, source.element
    ))
}

fn fetch_inner<C: HttpClient>(
    session: &mut Session<'_, C>,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
    known: Option<&str>,
) -> Result<FetchReport> {
    let (scope, mid) = match known {
        Some(mid) => {
            let scope = cache.join(session.credentials.scope());
            private_dir(&scope)?;
            (scope, mid.to_owned())
        }
        None => resolve(session, source, cache)?,
    };
    let mut mid = mid;
    let unfinished = unfinished_path(&scope, source);
    let mut hits = 0;
    for attempt in 0..2 {
        if source.kind == "w" {
            private_write(&unfinished, mid.as_bytes())?;
        }
        let captured = capture(
            session,
            source,
            output,
            &scope,
            &mid,
            &mut hits,
            &BTreeSet::from([(
                source.document.clone(),
                source.version.clone(),
                source.element.clone(),
            )]),
        )?;
        match captured {
            Capture::Complete(report) => {
                remove_if_present(&unfinished)?;
                return Ok(report);
            }
            Capture::Changed(current) => {
                if attempt == 1 {
                    remove_if_present(&unfinished)?;
                    return Err(changed_error());
                }
                mid = current;
            }
        }
    }
    remove_if_present(&unfinished)?;
    Err(changed_error())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(io_error(error)),
        _ => Ok(()),
    }
}

fn changed_error() -> Error {
    Error::new(
        ErrorKind::Changed,
        "the document changed while fetching; try again, or use a version link",
    )
}

fn current_microversion<C: HttpClient>(
    session: &mut Session<'_, C>,
    source: &DocumentUrl,
) -> Result<String> {
    let path = format!(
        "{API}/documents/d/{}/{}/{}/currentmicroversion",
        source.document, source.kind, source.version
    );
    let response = session.request("GET", &path, "", &[])?;
    if response.status != 200 {
        return Err(response_error());
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| response_error())?;
    session.credentials.check_json(&value)?;
    id(value
        .get("microversion")
        .and_then(Value::as_str)
        .ok_or_else(response_error)?)
    .map_err(|_| response_error())
}

#[allow(clippy::too_many_arguments)]
fn capture<C: HttpClient>(
    session: &mut Session<'_, C>,
    source: &DocumentUrl,
    output: &Path,
    scope: &Path,
    mid: &str,
    hits: &mut u32,
    ancestry: &BTreeSet<(String, String, String)>,
) -> Result<Capture> {
    let content_kind = if source.kind == "v" { "v" } else { "m" };
    let content_id = if source.kind == "v" {
        source.version.as_str()
    } else {
        mid
    };
    let mut fetcher = Fetcher::new(session, scope, source, mid, content_kind);
    let name = document_name(&mut fetcher, source)?;
    let tabs = fetcher.json(
        &format!(
            "{API}/documents/d/{}/{content_kind}/{content_id}/elements",
            source.document
        ),
        "",
    )?;
    let tabs = tabs.as_array().ok_or_else(response_error)?;
    if tabs.len() > 10000 {
        return Err(Error::new(
            ErrorKind::Limit,
            "The Onshape document has too many tabs; fetch a smaller document.",
        ));
    }
    let mut queue = VecDeque::from([source.element.clone()]);
    let mut visited = BTreeSet::new();
    let mut elements = Vec::new();
    let mut external = BTreeSet::new();
    while let Some(eid) = queue.pop_front() {
        if !visited.insert(eid.clone()) {
            continue;
        }
        if visited.len() > 1000 {
            return Err(Error::new(
                ErrorKind::Limit,
                "The Onshape assembly references too many elements; fetch a smaller assembly.",
            ));
        }
        let tab = tabs
            .iter()
            .find(|t| t.get("id").and_then(Value::as_str) == Some(&eid))
            .ok_or_else(response_error)?;
        let kind = tab
            .get("elementType")
            .and_then(Value::as_str)
            .ok_or_else(response_error)?;
        fetcher.session.allow(http::AUTO_ELEMENT_CALLS);
        let tab_name = tab
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(response_error)?;
        let mut paths = BTreeMap::new();
        match kind {
            "PARTSTUDIO" => {
                let base = format!(
                    "{API}/partstudios/d/{}/{content_kind}/{content_id}/e/{eid}",
                    source.document
                );
                let prefix = format!("partstudios/{eid}");
                let file = format!("{prefix}/features.json");
                let features = fetcher
                    .raw(
                        &format!("{base}/features"),
                        "rollbackBarIndex=-1&includeGeometryIds=true&noSketchGeometry=false",
                        file.clone(),
                        false,
                    )?
                    .ok_or_else(response_error)?;
                paths.insert("features", file);
                let list = features
                    .get("features")
                    .and_then(Value::as_array)
                    .ok_or_else(response_error)?;
                if list.len() > 10000 {
                    return Err(Error::new(
                        ErrorKind::Limit,
                        "The Onshape Part Studio has too many features; fetch a smaller element.",
                    ));
                }
                derived::capture_references(&mut fetcher, list, scope, ancestry)?;
                let mut wanted = BTreeSet::new();
                for (i, feature) in list.iter().enumerate() {
                    if feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                        && needs_topology(feature, 0)?
                    {
                        wanted.insert(i);
                    }
                }
                // The parts present before and after each part-making feature tell the
                // reader which part, by its name, that feature made.
                for i in part_creators(list) {
                    wanted.insert(i);
                    wanted.insert(i.saturating_add(1));
                }
                // Referenced geometry can omit STEP names. Its final parts snapshot
                // proves the name when the referenced studio has exactly one solid.
                if ancestry.len() > 1 {
                    wanted.insert(list.len());
                }
                fetcher
                    .session
                    .allow(u32::try_from(wanted.len()).unwrap_or(u32::MAX));
                let configuration_path = format!(
                    "{API}/elements/d/{}/{content_kind}/{content_id}/e/{eid}/configuration",
                    source.document
                );
                let variables_path = format!(
                    "{API}/variables/d/{}/{}/{}/e/{eid}/variables",
                    source.document, source.kind, source.version
                );
                const VARIABLES_QUERY: &str = "includeValuesAndReferencedVariables=true";
                if ancestry.len() == 1 && elements.is_empty() {
                    // Count only the calls not already cached, so a continued fetch
                    // estimates what it has left, not the whole document again.
                    let mut rest = wanted
                        .iter()
                        .map(|i| {
                            fetcher.missing(
                                &format!("{base}/bodydetails"),
                                &bodydetails_query(*i, list.len()),
                            )
                        })
                        .sum::<u32>();
                    rest += fetcher.missing(&configuration_path, "");
                    rest += fetcher.missing(&variables_path, VARIABLES_QUERY);
                    rest += fetcher.missing(&format!("{base}/sketches"), "");
                    if !sketch_only(list) {
                        rest += fetcher.step_calls_missing(source, &eid)?;
                    }
                    if source.kind == "w" {
                        rest += 1;
                    }
                    fetcher.session.needed = Some(fetcher.session.calls.saturating_add(rest));
                }
                let mut indices = Vec::new();
                for i in wanted {
                    let file = format!("{prefix}/bodydetails/{i}.json");
                    fetcher.raw(
                        &format!("{base}/bodydetails"),
                        &bodydetails_query(i, list.len()),
                        file,
                        false,
                    )?;
                    indices.push(i);
                }
                let index = format!("{prefix}/bodydetails/index.json");
                fetcher.add(
                    index.clone(),
                    serde_json::to_vec(&indices).map_err(|_| response_error())?,
                )?;
                paths.insert("bodydetails", index);
                let config = format!("{prefix}/configuration.json");
                if fetcher
                    .raw(&configuration_path, "", config.clone(), true)?
                    .is_some()
                {
                    paths.insert("configuration", config);
                }
                let variables = format!("{prefix}/variables.json");
                if fetcher
                    .raw(&variables_path, VARIABLES_QUERY, variables.clone(), true)?
                    .is_some()
                {
                    paths.insert("variables", variables);
                }
                if !sketch_only(list) {
                    let step = format!("{prefix}/export.step");
                    fetcher.export_step(source, &eid, &step)?;
                    paths.insert("step", step);
                }
                let sketches = format!("{prefix}/sketches.json");
                fetcher.raw(&format!("{base}/sketches"), "", sketches.clone(), false)?;
                paths.insert("sketches", sketches);
            }
            "ASSEMBLY" => {
                let base = format!(
                    "{API}/assemblies/d/{}/{content_kind}/{content_id}/e/{eid}",
                    source.document
                );
                let definition = format!("assemblies/{eid}/definition.json");
                let data = fetcher
                    .raw(
                        &base,
                        "includeMateFeatures=true&includeMateConnectors=true",
                        definition.clone(),
                        false,
                    )?
                    .ok_or_else(response_error)?;
                paths.insert("definition", definition);
                let file = format!("assemblies/{eid}/features.json");
                fetcher.raw(&format!("{base}/features"), "", file.clone(), false)?;
                paths.insert("features", file);
                references(&data, &source.document, mid, &mut queue, &mut external, 0)?;
            }
            _ => return Err(Error::new(
                ErrorKind::Unsupported,
                "This Onshape tab is not supported yet; choose a Part Studio or assembly element.",
            )),
        }
        elements.push(json!({"id": eid, "name": tab_name, "type": kind, "files": paths}));
    }
    *hits += fetcher.hits;
    if source.kind == "w" {
        let final_mid = current_microversion(fetcher.session, source)?;
        let workspace = fetcher.cache.join("w");
        if final_mid != mid || fetcher.skew {
            // Some workspace replies may postdate the change; none of them can be trusted.
            if workspace.exists() {
                std::fs::remove_dir_all(&workspace).map_err(io_error)?;
            }
            return Ok(Capture::Changed(final_mid));
        }
    }
    fetcher.record_source("manifest.json", content_kind);
    let manifest = json!({"schema": "dutchman.onshape-bundle/1", "fetched_at": timestamp(fetcher.session.client.now()), "fetcher": format!("dutchman {}", crate::BUILD_VERSION), "api_version": "v17", "document": {"id": source.document, "name": name, "workspace_or_version": source.kind, "wvid": source.version, "microversion": mid}, "root_element": source.element, "elements": elements, "external": external.into_iter().map(|(d,e,m)| json!({"documentId":d,"elementId":e,"microversionId":m})).collect::<Vec<_>>(), "calls": fetcher.session.calls, "consistent": true, "file_sources": fetcher.file_sources});
    privacy(&manifest)?;
    fetcher.add(
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest).map_err(|_| response_error())?,
    )?;
    write_bundle(output, &fetcher.files)?;
    Ok(Capture::Complete(FetchReport {
        output: output.into(),
        microversion: mid.into(),
        calls: fetcher.session.calls,
        cache_hits: *hits,
    }))
}

impl<'a, 'b, C: HttpClient> Fetcher<'a, 'b, C> {
    fn new(
        session: &'a mut Session<'b, C>,
        scope: &Path,
        source: &DocumentUrl,
        mid: &str,
        content_kind: &'static str,
    ) -> Self {
        Self {
            session,
            cache: scope.join(&source.document).join(mid),
            microversion: mid.into(),
            content_kind,
            file_sources: BTreeMap::new(),
            skew: false,
            hits: 0,
            files: BTreeMap::new(),
            total: 0,
        }
    }

    /// Whether a reply is already cached, so reading it costs no call.
    fn is_cached(&self, path: &str, query: &str) -> bool {
        self.missing(path, query) == 0
    }
}

/// The document's name, from the cache when it is there and otherwise from one call.
fn document_name<C: HttpClient>(
    fetcher: &mut Fetcher<'_, '_, C>,
    source: &DocumentUrl,
) -> Result<String> {
    private_dir(&fetcher.cache)?;
    let document_path = fetcher.cache.join("document-name.json");
    if document_path.exists() {
        return serde_json::from_slice(&read_bounded(&document_path, MAX_RESPONSE)?)
            .map_err(|_| response_error());
    }
    let response = fetcher.session.request(
        "GET",
        &format!("{API}/documents/{}", source.document),
        "",
        &[],
    )?;
    if response.status != 200 {
        return Err(response_error());
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| response_error())?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(response_error)?
        .to_owned();
    fetcher.session.credentials.check_bytes(name.as_bytes())?;
    let bytes = serde_json::to_vec(&name).map_err(|_| response_error())?;
    fetcher.session.credentials.check_bytes(&bytes)?;
    private_write(&document_path, &bytes)?;
    Ok(name)
}

/// Calls a STEP export usually takes: the request, one poll, and the download.
const STEP_CALLS: u32 = 3;

/// The body-detail query for the state before feature `index`; past the last feature is
/// the final state, which -1 asks for.
fn bodydetails_query(index: usize, features: usize) -> String {
    let bar = if index >= features {
        "-1".to_owned()
    } else {
        index.to_string()
    };
    format!("rollbackBarIndex={bar}&includeGeometricData=true")
}

fn check_microversion(value: &Value, expected: &str) -> Result<()> {
    if value.get("microversionSkew").and_then(Value::as_bool) == Some(true)
        || value.get("isComplete").and_then(Value::as_bool) == Some(false)
    {
        return Err(response_error());
    }
    for field in ["sourceMicroversion", "microversionId"] {
        if let Some(version) = value.get(field) {
            let actual = version
                .as_str()
                .or_else(|| version.get("theId").and_then(Value::as_str));
            if actual != Some(expected) {
                return Err(response_error());
            }
        }
    }
    Ok(())
}

// Known non-body features can be captured without asking the export service
// to translate a Part Studio with no bodies. Unknown features still need STEP.
fn sketch_only(features: &[Value]) -> bool {
    !features.is_empty()
        && features.iter().all(|feature| {
            feature.get("suppressed").and_then(Value::as_bool) == Some(true)
                || feature.get("parentSuppressed").and_then(Value::as_bool) == Some(true)
                || matches!(
                    feature.get("featureType").and_then(Value::as_str),
                    Some("newSketch" | "assignVariable")
                )
        })
}

/// The ordinals of the unsuppressed features that make new parts.
fn part_creators(features: &[Value]) -> Vec<usize> {
    features
        .iter()
        .enumerate()
        .filter(|(_, feature)| {
            feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                && matches!(
                    feature.get("featureType").and_then(Value::as_str),
                    Some("extrude" | "revolve" | "derived" | "importDerived")
                )
                && (matches!(
                    feature.get("featureType").and_then(Value::as_str),
                    Some("derived" | "importDerived")
                ) || feature
                    .get("parameters")
                    .and_then(Value::as_array)
                    .is_some_and(|parameters| {
                        parameters.iter().any(|p| {
                            p.get("parameterId").and_then(Value::as_str) == Some("operationType")
                                && p.get("value").and_then(Value::as_str) == Some("NEW")
                        })
                    }))
        })
        .map(|(i, _)| i)
        .collect()
}

fn needs_topology(value: &Value, depth: usize) -> Result<bool> {
    if depth > 100 {
        return Err(response_error());
    }
    match value {
        Value::Object(map) => {
            let query = map
                .get("btType")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if query.contains("Query") && !query.contains("Parameter") && !query.contains("Sketch")
            {
                return Ok(true);
            }
            for child in map.values() {
                if needs_topology(child, depth + 1)? {
                    return Ok(true);
                }
            }
        }
        Value::Array(list) => {
            for child in list {
                if needs_topology(child, depth + 1)? {
                    return Ok(true);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn references(
    value: &Value,
    did: &str,
    mid: &str,
    queue: &mut VecDeque<String>,
    external: &mut BTreeSet<(String, String, String)>,
    depth: usize,
) -> Result<()> {
    if depth > 100 {
        return Err(response_error());
    }
    match value {
        Value::Object(map) => {
            if let (Some(d), Some(e)) = (
                map.get("documentId").and_then(Value::as_str),
                map.get("elementId").and_then(Value::as_str),
            ) {
                let d = id(d).map_err(|_| response_error())?;
                let e = id(e).map_err(|_| response_error())?;
                let m = map
                    .get("documentMicroversion")
                    .or_else(|| map.get("microversionId"))
                    .and_then(Value::as_str)
                    .unwrap_or(mid);
                if d == did {
                    if m != mid {
                        return Err(Error::new(ErrorKind::Unsupported, "An assembly reference uses a different microversion; a single-microversion bundle is not supported yet for this assembly."));
                    }
                    if queue.len() >= 10000 {
                        return Err(response_error());
                    }
                    queue.push_back(e);
                } else {
                    let m = map
                        .get("documentMicroversion")
                        .or_else(|| map.get("microversionId"))
                        .and_then(Value::as_str)
                        .ok_or_else(response_error)?;
                    external.insert((d, e, id(m).map_err(|_| response_error())?));
                }
            }
            for child in map.values() {
                references(child, did, mid, queue, external, depth + 1)?;
            }
        }
        Value::Array(list) => {
            for child in list {
                references(child, did, mid, queue, external, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn privacy(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if [
                    "accesskey",
                    "secretkey",
                    "access_key",
                    "secret_key",
                    "authorization",
                    "token",
                    "accesstoken",
                    "accountid",
                    "account_id",
                    "account",
                    "user_id",
                    "userid",
                    "user",
                    "username",
                    "user_name",
                    "email",
                    "emailaddress",
                    "useremail",
                    "owner",
                    "creator",
                    "createdby",
                    "modifiedby",
                    "lastmodifyinguser",
                    "refresh_token",
                    "refreshtoken",
                    "access_token",
                ]
                .contains(&key.to_ascii_lowercase().as_str())
                {
                    return Err(Error::new(ErrorKind::Privacy, "Onshape returned account or credential fields; this response cannot be stored verbatim in a bundle. Nothing from this response was bundled."));
                }
                privacy(value)?;
            }
        }
        Value::Array(list) => {
            for value in list {
                privacy(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn read_bounded(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > cap {
        return Err(Error::new(
            ErrorKind::Limit,
            "The Onshape cache file exceeds its size limit; remove it and retry.",
        ));
    }
    Ok(bytes)
}
fn private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.persist(path).map_err(io_error)?;
    Ok(())
}
fn write_bundle(path: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    {
        let mut zip = zip::ZipWriter::new(file.as_file_mut());
        for (name, bytes) in files {
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(io_error)?;
            zip.write_all(bytes).map_err(io_error)?;
        }
        zip.finish().map_err(io_error)?;
    }
    file.persist(path).map_err(io_error)?;
    Ok(())
}
fn timestamp(now: std::time::SystemTime) -> String {
    // HTTP-date already supplies a checked calendar conversion; rearrange its fields to RFC 3339.
    let date = httpdate::fmt_http_date(now);
    let fields: Vec<_> = date.split_whitespace().collect();
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| Some(m) == fields.get(2))
    .map_or(1, |m| m + 1);
    format!(
        "{}-{month:02}-{}T{}Z",
        fields.get(3).unwrap_or(&"1970"),
        fields.get(1).unwrap_or(&"01"),
        fields.get(4).unwrap_or(&"00:00:00")
    )
}
