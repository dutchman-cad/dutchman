//! Fetch raw Onshape responses into a private, versioned bundle. No CAD decoding happens here.
mod auth;
mod export;
mod http;

pub use auth::Credentials;
use http::Session;
pub use http::{HttpClient, NetworkClient, Request, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
}
impl ErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Url => "onshape/url",
            Self::Auth => "onshape/auth",
            Self::Budget => "onshape/budget",
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
        }
    }
}
#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: &'static str,
    pub calls: u32,
}
impl Error {
    pub(crate) fn new(kind: ErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            calls: 0,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({} API calls)", self.message, self.calls)
    }
}
impl std::error::Error for Error {}
fn io_error(_: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, "Could not read or write the private Onshape cache or bundle; check permissions and free disk space, then retry.")
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
    /// Maximum HTTP attempts, including metadata, polling, retries and downloads.
    #[arg(long, default_value_t = 50)]
    pub max_calls: u32,
    /// Print the same structured report as the MCP tool.
    #[arg(long)]
    pub json: bool,
}

pub fn run(options: &FetchOptions) -> Value {
    let result = (|| {
        let parsed = DocumentUrl::parse(&options.document_url, options.version.as_deref())?;
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).ok_or_else(|| Error::new(ErrorKind::Io, "Set HOME or USERPROFILE so Dutchman can find its private Onshape config and cache."))?;
        let config = home.join(".config/dutchman/onshape.json");
        let credentials = Credentials::load(&config)?;
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
            .join("dutchman/onshape");
        let output = options
            .out
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}.onshape-bundle", parsed.element)));
        fetch(
            &mut NetworkClient::default(),
            &credentials,
            &parsed,
            &output,
            &cache,
            options.max_calls,
        )
    })();
    crate::json::onshape_fetch(&result)
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
    credentials.check_bytes(output.as_os_str().to_string_lossy().as_bytes())?;
    let mut session = Session {
        client,
        credentials,
        calls: 0,
        max_calls,
    };
    let result = fetch_inner(&mut session, source, output, cache);
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
    pending_cache: BTreeMap<PathBuf, Vec<u8>>,
    pending_bytes: usize,
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

    fn cache_response(&mut self, file: &Path, bytes: &[u8], path: &str) -> Result<()> {
        if path.contains("/w/") {
            self.pending_bytes = self
                .pending_bytes
                .checked_add(bytes.len())
                .ok_or_else(response_error)?;
            if self.pending_bytes > MAX_TOTAL {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "The provisional Onshape cache exceeds 512 MiB; fetch a smaller element.",
                ));
            }
            self.pending_cache.insert(file.into(), bytes.to_vec());
            Ok(())
        } else {
            private_write(file, bytes)
        }
    }

    fn get(&mut self, path: &str, query: &str, optional: bool) -> Result<Option<Vec<u8>>> {
        let cache_file = self.cache.join(hash(format!("{path}?{query}").as_bytes()));
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
            self.cache_response(&absent, b"", path)?;
            return Ok(None);
        }
        if !(200..300).contains(&response.status) {
            return Err(response_error());
        }
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| response_error())?;
        self.session.credentials.check_json(&value)?;
        privacy(&value)?;
        if path.contains("/w/") {
            self.skew |= check_microversion(&value, &self.microversion).is_err();
        } else {
            check_microversion(&value, &self.microversion)?;
        }
        self.cache_response(&cache_file, &response.body, path)?;
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

fn fetch_inner<C: HttpClient>(
    session: &mut Session<'_, C>,
    source: &DocumentUrl,
    output: &Path,
    cache: &Path,
) -> Result<FetchReport> {
    // A workspace must be resolved again: caching its mutable head would return stale geometry.
    // Versions are immutable, so their resolution may safely be reused without any API call.
    let scope = cache.join(session.credentials.scope());
    private_dir(&scope)?;
    let resolution = scope.join(format!(
        "{}-{}-{}-resolution.json",
        source.document, source.kind, source.version
    ));
    let mid = if source.kind == "v" && resolution.exists() {
        String::from_utf8(read_bounded(&resolution, 128)?).map_err(|_| response_error())?
    } else {
        let mid = current_microversion(session, source)?;
        if source.kind == "v" {
            private_write(&resolution, mid.as_bytes())?;
        }
        mid
    };
    let mut mid = id(&mid).map_err(|_| response_error())?;
    let mut hits = 0;
    for attempt in 0..2 {
        match capture(session, source, output, &scope, &mid, &mut hits)? {
            Capture::Complete(report) => return Ok(report),
            Capture::Changed(current) => {
                if attempt == 1 {
                    return Err(changed_error());
                }
                mid = current;
            }
        }
    }
    Err(changed_error())
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
    id(value
        .get("microversion")
        .and_then(Value::as_str)
        .ok_or_else(response_error)?)
    .map_err(|_| response_error())
}

fn capture<C: HttpClient>(
    session: &mut Session<'_, C>,
    source: &DocumentUrl,
    output: &Path,
    scope: &Path,
    mid: &str,
    hits: &mut u32,
) -> Result<Capture> {
    let content_kind = if source.kind == "v" { "v" } else { "m" };
    let content_id = if source.kind == "v" {
        source.version.as_str()
    } else {
        mid
    };
    let mut fetcher = Fetcher {
        session,
        cache: scope.join(&source.document).join(mid),
        microversion: mid.into(),
        content_kind,
        file_sources: BTreeMap::new(),
        pending_cache: BTreeMap::new(),
        pending_bytes: 0,
        skew: false,
        hits: 0,
        files: BTreeMap::new(),
        total: 0,
    };
    private_dir(&fetcher.cache)?;
    let document_path = fetcher.cache.join("document-name.json");
    let name: String = if document_path.exists() {
        serde_json::from_slice(&read_bounded(&document_path, MAX_RESPONSE)?)
            .map_err(|_| response_error())?
    } else {
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
        name
    };
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
                let mut indices = Vec::new();
                for (i, feature) in list.iter().enumerate() {
                    if feature.get("suppressed").and_then(Value::as_bool) == Some(true)
                        || !needs_topology(feature, 0)?
                    {
                        continue;
                    }
                    let file = format!("{prefix}/bodydetails/{i}.json");
                    fetcher.raw(
                        &format!("{base}/bodydetails"),
                        &format!("rollbackBarIndex={i}&includeGeometricData=true"),
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
                    .raw(
                        &format!(
                            "{API}/elements/d/{}/{content_kind}/{content_id}/e/{eid}/configuration",
                            source.document
                        ),
                        "",
                        config.clone(),
                        true,
                    )?
                    .is_some()
                {
                    paths.insert("configuration", config);
                }
                let variables = format!("{prefix}/variables.json");
                if fetcher
                    .raw(
                        &format!(
                            "{API}/variables/d/{}/{}/{}/e/{eid}/variables",
                            source.document, source.kind, source.version
                        ),
                        "includeValuesAndReferencedVariables=true",
                        variables.clone(),
                        true,
                    )?
                    .is_some()
                {
                    paths.insert("variables", variables);
                }
                let step = format!("{prefix}/export.step");
                fetcher.export_step(source, &eid, &step)?;
                paths.insert("step", step);
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
        if final_mid != mid || fetcher.skew {
            return Ok(Capture::Changed(final_mid));
        }
    }
    for (path, bytes) in &fetcher.pending_cache {
        private_write(path, bytes)?;
    }
    fetcher.record_source("manifest.json", content_kind);
    let manifest = json!({"schema": "dutchman.onshape-bundle/1", "fetched_at": timestamp(fetcher.session.client.now()), "fetcher": format!("dutchman {}", crate::BUILD_VERSION), "api_version": "v17", "document": {"id": source.document, "name": name, "workspace_or_version": source.kind, "wvid": source.version, "microversion": mid}, "elements": elements, "external": external.into_iter().map(|(d,e,m)| json!({"documentId":d,"elementId":e,"microversionId":m})).collect::<Vec<_>>(), "calls": fetcher.session.calls, "consistent": true, "file_sources": fetcher.file_sources});
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
