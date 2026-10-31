//! Dutchman Web: a small web server that converts CAD designs uploaded from a browser, for a
//! Dutchman hosted on a server. Each upload becomes a job folder; `dutchman::batch::convert_all`
//! converts it exactly as `dutchman convert-all` and the MCP server's `convert_all` do, each
//! file in a child process of its own; the page shows each file's report and links its design.
//! Jobs are deleted after a while. `docs/web.md` describes it.

pub mod hardening;
pub mod report;
pub mod save;
pub mod upload;

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use askama::Template;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as UrlPath, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dutchman::worker::Limits;
use dutchman::{ConvertOptions, OutputFormat};
use dutchman_core::Format;
use hardening::{credentials, same, security_headers};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::upload::ZipLimits;

/// The name of a job's saved report, beside its `in` and `out` folders.
const REPORT_FILE: &str = "report.json";

/// The most a saved report may hold, in bytes.
const MAX_REPORT_BYTES: u64 = 64 << 20;

/// How a server runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// The folder jobs are kept in, one folder each.
    pub data_dir: PathBuf,
    /// The most one upload may hold, in bytes, all its files together.
    pub max_upload: u64,
    /// How many uploads may be received at once; more are refused.
    pub uploads: usize,
    /// Refuse new jobs when the saved data exceeds this many bytes.
    pub max_data: u64,
    /// Refuse new jobs below this many available filesystem bytes (Unix).
    pub min_free: u64,
    /// Additional browser origins allowed to submit uploads through a reverse proxy.
    pub public_origin: Vec<String>,
    /// What all zips of one upload may unpack to.
    pub zip: ZipLimits,
    /// How long a job's designs are kept.
    pub keep: Duration,
    /// How many uploads are converted at once; more wait their turn.
    pub runs: usize,
    /// How many files of one upload are converted at once (default: the machine's cores, at
    /// most 8).
    pub jobs: Option<NonZeroUsize>,
    /// What one file's conversion may use.
    pub limits: Limits,
    /// The program each file is converted in, as a child process of its own (this program,
    /// normally); `None` converts in this process, without `limits`.
    pub worker: Option<PathBuf>,
    /// The user name and password a browser must give (HTTP basic authentication), if any.
    pub login: Option<(String, String)>,
    /// The path the server is reached under behind a reverse proxy (`/dutchman`), or empty at
    /// the root; see [`base_path`].
    pub base_path: String,
    /// A folder each job's designs are also saved in, as uploaded and never replacing a file
    /// already there; kept after the job expires.
    pub output_dir: Option<PathBuf>,
    /// Where the SolidCraft web app is; see [`solidcraft`].
    pub solidcraft: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            data_dir: std::env::temp_dir().join("dutchman-web"),
            max_upload: 200 << 20,
            uploads: 4,
            max_data: 10 << 30,
            min_free: 1 << 30,
            public_origin: Vec::new(),
            zip: ZipLimits {
                entries: 5_000,
                bytes: 2 << 30,
            },
            keep: Duration::from_secs(60 * 60),
            runs: 2,
            jobs: None,
            limits: Limits::default(),
            worker: None,
            login: None,
            base_path: String::new(),
            output_dir: None,
            solidcraft: None,
        }
    }
}

struct App {
    config: Config,
    runs: Semaphore,
    uploads: Semaphore,
}

type Shared = Arc<App>;

/// The server's routes, with `config`, under its base path.
pub fn router(config: Config) -> Router {
    let base = config.base_path.clone();
    let at = |path: &str| format!("{base}{path}");
    let body_limit = usize::try_from(config.max_upload)
        .unwrap_or(usize::MAX)
        .saturating_add(1 << 20);
    let app = Arc::new(App {
        runs: Semaphore::new(config.runs.max(1)),
        uploads: Semaphore::new(config.uploads.clamp(1, Semaphore::MAX_PERMITS)),
        config,
    });
    let protected = Router::new()
        .route(&at("/"), get(index))
        .route(&at("/convert"), post(convert_page))
        .route(&at("/api/convert"), post(convert_api))
        .route(&at("/jobs/{job}"), get(job_page))
        .route(&at("/jobs/{job}/report.json"), get(job_report))
        .route(&at("/jobs/{job}/files/{*path}"), get(download))
        .route(&at("/jobs/{job}/designs.zip"), get(download_all))
        .layer(DefaultBodyLimit::max(body_limit))
        .route_layer(middleware::from_fn_with_state(app.clone(), guard_origin))
        .route_layer(middleware::from_fn_with_state(app.clone(), require_login));
    let mut routes = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(protected);
    if !base.is_empty() {
        // The page's links are relative to `<base>/`, so the bare path moves there.
        let home = at("/");
        routes = routes.route(&at("/healthz"), get(|| async { "ok" })).route(
            &base,
            get(move || async move { Redirect::permanent(&home) }),
        );
    }
    routes
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

/// `path` as a server's base path: empty for the root, otherwise `/` and segments of letters,
/// digits, `-`, `_`, `.` and `~`, without a trailing `/` (`dutchman/` gives `/dutchman`).
pub fn base_path(path: &str) -> Result<String, String> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for segment in &segments {
        let plain = segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'));
        if !plain || *segment == "." || *segment == ".." {
            return Err(format!(
                "Choose a base path of plain names such as /dutchman, not {path}."
            ));
        }
    }
    Ok(segments.iter().map(|s| format!("/{s}")).collect())
}

/// Checks the SolidCraft app's same-origin path, without a query or fragment.
pub fn solidcraft(address: &str) -> Result<String, String> {
    let plain = !address
        .chars()
        .any(|c| c.is_control() || matches!(c, '\\' | '?' | '#'));
    let local = address.starts_with('/') && !address.starts_with("//");
    if !plain || !local {
        return Err("--solidcraft must be the path SolidCraft is served at on this server's origin, such as /: one leading /, without backslashes, control characters, a query or a fragment (see \"Open in SolidCraft\" in docs/web.md)".to_owned());
    }
    Ok(address.to_owned())
}

fn open_links(report: &mut Value, config: &Config) {
    if let Some(address) = &config.solidcraft {
        report::open_links(report, address, &format!("{}/", config.base_path));
    }
}

/// Creates the output folder if needed and checks that it is a folder.
pub fn prepare_output_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    if std::fs::metadata(path)?.is_dir() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "Choose an output folder that is a directory.",
        ))
    }
}

pub use dutchman_serve::hardening::prepare_data_dir;

fn check_storage(config: &Config) -> Result<(), Problem> {
    hardening::check_storage(&config.data_dir, config.max_data, config.min_free).map_err(|err| {
        match err {
            hardening::StorageError::Full => Problem::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "The server has too little storage for a new job. Try again after saved jobs expire.",
            ),
            hardening::StorageError::Io(err) => server_error(&err),
        }
    })
}

/// Deletes jobs older than `keep` in `data_dir`, now and then every few minutes; run it
/// beside the server.
pub async fn remove_expired(data_dir: PathBuf, keep: Duration) {
    loop {
        let dir = data_dir.clone();
        let _ = tokio::task::spawn_blocking(move || remove_older_than(&dir, keep)).await;
        tokio::time::sleep(Duration::from_secs(300).min(keep)).await;
    }
}

fn remove_older_than(data_dir: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > keep);
        if old && is_job_id(&entry.file_name().to_string_lossy()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

// ---- Pages -------------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    max_upload_mb: u64,
    keep_minutes: u64,
    extensions: String,
    formats: Vec<(&'static str, &'static str)>,
    /// Whether designs are also saved in an output folder.
    saves: bool,
}

#[derive(Template)]
#[template(path = "results.html")]
struct ResultsPage<'a> {
    job: &'a str,
    converted: u64,
    failed: u64,
    skipped: u64,
    keep_minutes: u64,
    cards: Vec<report::Card>,
}

#[derive(Template)]
#[template(path = "problem.html")]
struct ProblemPage<'a> {
    message: &'a str,
    /// The page's address, `<base>/`.
    home: &'a str,
}

/// This build's version, for the page header and footer.
const APP_VERSION: &str = dutchman::BUILD_VERSION;

fn render(page: impl Template, status: StatusCode) -> Response {
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{err}")).into_response(),
    }
}

async fn index(State(app): State<Shared>) -> Response {
    let formats: Vec<Format> = Format::ALL
        .into_iter()
        .filter(|f| f.can_convert())
        .collect();
    let extensions = formats
        .iter()
        .flat_map(|f| f.extensions().iter().map(|e| format!(".{e}")))
        .collect::<Vec<_>>()
        .join(" · ");
    render(
        IndexPage {
            max_upload_mb: app.config.max_upload >> 20,
            keep_minutes: app.config.keep.as_secs() / 60,
            extensions,
            formats: formats
                .into_iter()
                .map(|f| (f.name(), dutchman::formats::converts(f)))
                .collect(),
            saves: app.config.output_dir.is_some(),
        },
        StatusCode::OK,
    )
}

/// A failure for people: a page for a browser, `{"error": …}` for the API.
struct Problem {
    status: StatusCode,
    message: String,
}

impl Problem {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Problem {
            status,
            message: message.into(),
        }
    }

    /// The page for a browser, under the server's `base` path.
    fn page(self, base: &str) -> Response {
        render(
            ProblemPage {
                message: &self.message,
                home: &format!("{base}/"),
            },
            self.status,
        )
    }

    fn json(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

async fn convert_page(State(app): State<Shared>, form: Multipart) -> Response {
    match convert(&app, form).await {
        Ok((job, _)) => Redirect::to(&format!("jobs/{job}")).into_response(),
        Err(problem) => problem.page(&app.config.base_path),
    }
}

async fn convert_api(State(app): State<Shared>, form: Multipart) -> Response {
    match convert(&app, form).await {
        Ok((job, mut report)) => {
            if let Some(fields) = report.as_object_mut() {
                fields.insert("job".to_owned(), json!(job));
                fields.insert(
                    "designs_zip".to_owned(),
                    json!(format!("jobs/{job}/designs.zip")),
                );
            }
            Json(report).into_response()
        }
        Err(problem) => problem.json(),
    }
}

async fn job_page(State(app): State<Shared>, UrlPath(job): UrlPath<String>) -> Response {
    match saved_report(&app, &job).await {
        Ok(report) => {
            let count = |key: &str| report.get(key).and_then(Value::as_u64).unwrap_or(0);
            render(
                ResultsPage {
                    job: &job,
                    converted: count("converted"),
                    failed: count("failed"),
                    skipped: count("skipped"),
                    keep_minutes: app.config.keep.as_secs() / 60,
                    cards: report::cards(&report),
                },
                StatusCode::OK,
            )
        }
        Err(problem) => problem.page(&app.config.base_path),
    }
}

async fn job_report(State(app): State<Shared>, UrlPath(job): UrlPath<String>) -> Response {
    match saved_report(&app, &job).await {
        Ok(report) => Json(report).into_response(),
        Err(problem) => problem.json(),
    }
}

async fn saved_report(app: &App, job: &str) -> Result<Value, Problem> {
    let dir = job_dir(app, job)?;
    let bytes = tokio::task::spawn_blocking(move || {
        upload::read_small(&dir.join(REPORT_FILE), MAX_REPORT_BYTES)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .ok_or_else(expired)?;
    let mut report = serde_json::from_slice(&bytes).map_err(|_| expired())?;
    open_links(&mut report, &app.config);
    Ok(report)
}

// ---- Converting ----------------------------------------------------------------------------

/// Saves the upload in a new job folder, converts it, saves the report and returns the job
/// and its browser-ready report. A job that fails before converting leaves nothing behind.
async fn convert(app: &App, form: Multipart) -> Result<(String, Value), Problem> {
    let receiving = app.uploads.try_acquire().map_err(|_| {
        Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "The server is receiving too many uploads. Try again later.",
        )
    })?;
    let config = app.config.clone();
    tokio::task::spawn_blocking(move || check_storage(&config))
        .await
        .map_err(|err| server_error(&err))??;
    let job = new_job_id()?;
    let dir = app.config.data_dir.join(&job);
    let result = convert_in(app, &job, &dir, form, receiving).await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    result.map(|report| (job, report))
}

async fn convert_in(
    app: &App,
    job: &str,
    dir: &Path,
    mut form: Multipart,
    receiving: SemaphorePermit<'_>,
) -> Result<Value, Problem> {
    let (in_dir, out_dir) = (dir.join("in"), dir.join("out"));
    for folder in [&in_dir, &out_dir] {
        tokio::fs::create_dir_all(folder)
            .await
            .map_err(|err| server_error(&err))?;
    }
    let mut options = ConvertOptions::default();
    let mut files = 0usize;
    let mut total = 0u64;
    let mut zip_budget = app.config.zip;
    let too_big = || {
        Problem::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "Uploads are limited to {} MB. Convert fewer files at once.",
                app.config.max_upload >> 20
            ),
        )
    };
    loop {
        let field = match form.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) if err.status() == StatusCode::PAYLOAD_TOO_LARGE => return Err(too_big()),
            Err(err) => return Err(Problem::new(err.status(), err.body_text())),
        };
        match field.name() {
            Some("files") => {
                let Some(name) = field.file_name().filter(|n| !n.is_empty()) else {
                    continue;
                };
                let path = upload::unique(&in_dir.join(upload::safe_name(name)))
                    .map_err(|err| server_error(&err))?;
                let mut out = tokio::fs::File::create(&path)
                    .await
                    .map_err(|err| server_error(&err))?;
                let mut field = field;
                loop {
                    let chunk = match field.chunk().await {
                        Ok(Some(chunk)) => chunk,
                        Ok(None) => break,
                        Err(err) if err.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                            return Err(too_big())
                        }
                        Err(err) => return Err(Problem::new(err.status(), err.body_text())),
                    };
                    total = total.saturating_add(chunk.len() as u64);
                    if total > app.config.max_upload {
                        return Err(too_big());
                    }
                    out.write_all(&chunk)
                        .await
                        .map_err(|err| server_error(&err))?;
                }
                out.flush().await.map_err(|err| server_error(&err))?;
                drop(out);
                files += 1;
                if upload::is_zip(&path) {
                    let into = in_dir.clone();
                    zip_budget = tokio::task::spawn_blocking(move || {
                        upload::unzip(&path, &into, &mut zip_budget)?;
                        Ok::<_, String>(zip_budget)
                    })
                    .await
                    .map_err(|err| server_error(&err))?
                    .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, message))?;
                }
            }
            Some("solidcraft_version") => {
                let version = field.text().await.map_err(|_| {
                    Problem::new(
                        StatusCode::BAD_REQUEST,
                        "Enter a SolidCraft target version such as 0.25.1.",
                    )
                })?;
                options.solidcraft_version = if version.trim().is_empty() {
                    None
                } else {
                    Some(
                        version
                            .trim()
                            .parse()
                            .map_err(|message| Problem::new(StatusCode::BAD_REQUEST, message))?,
                    )
                };
            }
            Some("import_only") => {
                options.import_only = field.text().await.is_ok_and(|v| v == "true" || v == "on");
            }
            Some("to") => {
                options.to = match field.text().await.as_deref() {
                    Ok("freecad") => OutputFormat::Freecad,
                    Ok("solidcraft") | Ok("") => OutputFormat::Solidcraft,
                    _ => {
                        return Err(Problem::new(
                            StatusCode::BAD_REQUEST,
                            "Choose SolidCraft or a FreeCAD macro as the output.",
                        ))
                    }
                };
            }
            _ => {}
        }
    }
    if files == 0 {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "Choose at least one design file, or a zip of a project folder.",
        ));
    }

    drop(receiving);
    let settings = dutchman::batch::Settings {
        jobs: app.config.jobs,
        worker: app.config.worker.clone(),
        limits: app.config.limits.clone(),
        journal: None,
    };
    let _turn =
        app.runs.acquire().await.map_err(|_| {
            Problem::new(StatusCode::SERVICE_UNAVAILABLE, "The server is stopping.")
        })?;
    let salt = dutchman::anon::Salt::random()
        .map_err(|message| Problem::new(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    options.diagnostics_salt = Some(salt.clone());
    let (inputs, out) = (vec![in_dir.clone()], out_dir.clone());
    let mut report = tokio::task::spawn_blocking(move || {
        dutchman::batch::convert_all(&inputs, &out, &options, &settings)
    })
    .await
    .map_err(|err| server_error(&err))?
    .map_err(|message| Problem::new(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    if let Some(folder) = app.config.output_dir.clone() {
        let out = out_dir.clone();
        report = tokio::task::spawn_blocking(move || {
            save::save_all(&mut report, &out, &folder);
            report
        })
        .await
        .map_err(|err| server_error(&err))?;
    }
    report::mark_failures(&mut report, &salt);
    report::for_browser(&mut report, job, &in_dir, &out_dir);
    let saved = serde_json::to_vec(&report).map_err(|err| server_error(&err))?;
    tokio::fs::write(dir.join(REPORT_FILE), saved)
        .await
        .map_err(|err| server_error(&err))?;
    open_links(&mut report, &app.config);
    Ok(report)
}

fn server_error(err: &dyn std::fmt::Display) -> Problem {
    Problem::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("The server could not keep the upload: {err}"),
    )
}

fn expired() -> Problem {
    Problem::new(
        StatusCode::NOT_FOUND,
        "These results are gone: designs are kept for a limited time. Convert the files again.",
    )
}

/// A new job's id: 32 random hex digits.
fn new_job_id() -> Result<String, Problem> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|err| server_error(&err))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn is_job_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The folder of `job`, if it is a job's id and the job is still kept.
fn job_dir(app: &App, job: &str) -> Result<PathBuf, Problem> {
    let dir = app.config.data_dir.join(job);
    if is_job_id(job) && dir.is_dir() {
        Ok(dir)
    } else {
        Err(expired())
    }
}

// ---- Downloads -----------------------------------------------------------------------------

async fn download(
    State(app): State<Shared>,
    UrlPath((job, path)): UrlPath<(String, String)>,
) -> Response {
    let base = app.config.base_path.as_str();
    let not_found = || Problem::new(StatusCode::NOT_FOUND, "There is no such design.").page(base);
    let Ok(dir) = job_dir(&app, &job) else {
        return expired().page(base);
    };
    let mut file = dir.join("out");
    for segment in path.split('/') {
        let normal = !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.contains(['\\', ':']);
        if !normal {
            return not_found();
        }
        file.push(segment);
    }
    if !file.is_file() {
        return not_found();
    }
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    send_file(&file, &name, "application/octet-stream", base).await
}

async fn download_all(State(app): State<Shared>, UrlPath(job): UrlPath<String>) -> Response {
    let base = app.config.base_path.as_str();
    let dir = match job_dir(&app, &job) {
        Ok(dir) => dir,
        Err(problem) => return problem.page(base),
    };
    let archive = dir.join("designs.zip");
    let (out, to) = (dir.join("out"), archive.clone());
    let made = tokio::task::spawn_blocking(move || zip_folder(&out, &to)).await;
    if !matches!(made, Ok(Ok(()))) {
        return Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The designs could not be put in one zip. Download them one by one.",
        )
        .page(base);
    }
    let short = job.get(..8).unwrap_or(&job);
    send_file(
        &archive,
        &format!("dutchman-{short}.zip"),
        "application/zip",
        base,
    )
    .await
}

/// Writes every file under `folder` into a new zip at `archive`, keeping the layout.
fn zip_folder(folder: &Path, archive: &Path) -> Result<(), String> {
    use std::io::Write as _;
    let file = std::fs::File::create(archive).map_err(|err| err.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)
            .map_err(|err| err.to_string())?
            .flatten()
        {
            let path = entry.path();
            let kind = entry.file_type().map_err(|err| err.to_string())?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let name = path
                    .strip_prefix(folder)
                    .map_err(|err| err.to_string())?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                zip.start_file(name, options)
                    .map_err(|err| err.to_string())?;
                let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
                zip.write_all(&bytes).map_err(|err| err.to_string())?;
            }
        }
    }
    zip.finish().map_err(|err| err.to_string())?;
    Ok(())
}

async fn send_file(path: &Path, name: &str, kind: &'static str, base: &str) -> Response {
    let Ok(file) = tokio::fs::File::open(path).await else {
        return Problem::new(StatusCode::NOT_FOUND, "There is no such design.").page(base);
    };
    let length = file.metadata().await.map(|m| m.len()).ok();
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(file));
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    if let Some(length) = length.and_then(|n| HeaderValue::from_str(&n.to_string()).ok()) {
        headers.insert(header::CONTENT_LENGTH, length);
    }
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let disposition = format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        report::encode_path(name).replace('/', "%2F")
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

// ---- Middleware ------------------------------------------------------------------------------

async fn guard_origin(State(app): State<Shared>, request: Request, next: Next) -> Response {
    let base = app.config.base_path.as_str();
    let api = request.uri().path().strip_prefix(base) == Some("/api/convert");
    if request.method() == axum::http::Method::POST
        && (api || request.uri().path().strip_prefix(base) == Some("/convert"))
        && !hardening::allowed_origin(request.headers(), &app.config.public_origin)
    {
        let problem = Problem::new(
            StatusCode::FORBIDDEN,
            "Submit uploads from this server's page or a configured public origin.",
        );
        return if api {
            problem.json()
        } else {
            problem.page(base)
        };
    }
    next.run(request).await
}

async fn require_login(State(app): State<Shared>, request: Request, next: Next) -> Response {
    let Some((user, password)) = &app.config.login else {
        return next.run(request).await;
    };
    if credentials(request.headers()).is_some_and(|(u, p)| {
        same(u.as_bytes(), user.as_bytes()) & same(p.as_bytes(), password.as_bytes())
    }) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"Dutchman\", charset=\"UTF-8\"",
        )],
        "Sign in to use Dutchman.",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_are_32_hex_digits() {
        let id = new_job_id().ok().unwrap();
        assert!(is_job_id(&id));
        assert!(!is_job_id("../etc"));
        assert!(!is_job_id(&"A".repeat(32)));
    }

    #[test]
    fn base_paths_are_plain_names() {
        assert_eq!(base_path("").unwrap(), "");
        assert_eq!(base_path("/").unwrap(), "");
        assert_eq!(base_path("dutchman/").unwrap(), "/dutchman");
        assert_eq!(base_path("/cad/dutchman").unwrap(), "/cad/dutchman");
        for bad in ["/../x", "/a b", "/a?b", "/{job}", "/./x"] {
            assert!(base_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn solidcraft_addresses_are_same_origin_paths() {
        for address in ["/", "/cad/", "/cad/app"] {
            assert_eq!(solidcraft(address).unwrap(), address);
        }
        for bad in [
            "",
            "cad/",
            "http://cad.example",
            "https://cad.example",
            "https://cad.example/app/",
            "//cad.example",
            "///cad.example",
            "/a\\b",
            "/a\nb",
            "/a\rb",
            "/a\tb",
            "/a\u{7f}b",
            "/?x=1",
            "/#x",
            "https://cad.example/?x=1",
            "https://cad.example/#x",
            "https://cad.example/\\x",
            "https://",
            "https://user@cad.example",
        ] {
            let error = solidcraft(bad).unwrap_err();
            assert!(error.contains("Open in SolidCraft"), "{error}");
        }
    }

    #[test]
    fn basic_credentials_are_read() {
        use axum::http::HeaderMap;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwYTpzcw=="),
        );
        assert_eq!(
            credentials(&headers),
            Some(("user".to_owned(), "pa:ss".to_owned()))
        );
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
    }
}
