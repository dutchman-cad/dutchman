//! Bounded, injectable HTTP transport. Redirects never forward credentials.
use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, SystemTime};

use super::{auth::Auth, Error, ErrorKind, Result, MAX_RESPONSE};

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Implementations must bound response allocation and must not log request headers.
pub trait HttpClient {
    fn send(&mut self, request: &Request) -> Result<Response>;
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
    fn nonce(&self) -> Result<String> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| {
            Error::new(
                ErrorKind::Transport,
                "Could not generate an Onshape signing nonce; retry.",
            )
        })?;
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub struct NetworkClient {
    agent: ureq::Agent,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .redirects(0)
                .timeout(Duration::from_secs(120))
                .build(),
        }
    }
}

impl HttpClient for NetworkClient {
    fn send(&mut self, request: &Request) -> Result<Response> {
        self.send_to(request, "https://cad.onshape.com")
    }
}

impl NetworkClient {
    fn send_to(&self, request: &Request, origin: &str) -> Result<Response> {
        let query = if request.query.is_empty() {
            String::new()
        } else {
            format!("?{}", request.query)
        };
        let url = format!("{origin}{}{}", request.path, query);
        let mut call = self.agent.request(&request.method, &url);
        for (key, value) in &request.headers {
            call = call.set(key, value);
        }
        let response = match call.send_bytes(&request.body) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(_) => return Err(Error::new(ErrorKind::Transport, "Could not reach Onshape; check the connection and retry. No response details or credentials were logged.")),
        };
        let status = response.status();
        let headers = response
            .headers_names()
            .iter()
            .filter_map(|name| {
                response
                    .header(name)
                    .map(|v| (name.to_lowercase(), v.to_owned()))
            })
            .collect();
        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut body)
            .map_err(|_| {
                Error::new(
                    ErrorKind::Transport,
                    "Could not finish reading the Onshape response; retry.",
                )
            })?;
        if body.len() as u64 > MAX_RESPONSE {
            return Err(Error::new(
                ErrorKind::Limit,
                "The Onshape response exceeds 64 MiB; fetch a smaller element.",
            ));
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

pub(super) struct Session<'a, C> {
    pub client: &'a mut C,
    pub credentials: Auth<'a>,
    pub calls: u32,
    pub max_calls: u32,
    /// With no `--max-calls`, the limit starts at [`AUTO_BASE_CALLS`] and grows as the
    /// document's elements and body-detail requests are discovered, up to [`AUTO_CEILING_CALLS`].
    pub auto: bool,
    /// Told after each call is sent.
    pub progress: Option<&'a mut dyn FetchProgress>,
    /// The fetch's estimate of the total calls the document needs, once it is known.
    pub needed: Option<u32>,
}

/// Follows a fetch as it runs.
pub trait FetchProgress {
    /// After each call is sent: the calls so far, and the estimated total once it is known.
    fn calls(&mut self, sent: u32, needed: Option<u32>);
    /// Asked before each call is sent: true ends the fetch with [`ErrorKind::Stopped`].
    fn stop_requested(&mut self) -> bool {
        false
    }
    /// Once, as soon as the document name and the linked tab's name are known: the title
    /// the design goes by ([`super::bundle_title`]). Costs no call.
    fn named(&mut self, _title: &str) {}
}

/// Calls an automatic budget starts with: document name, tab list, microversion checks.
pub(super) const AUTO_BASE_CALLS: u32 = 50;
/// Calls added for each Part Studio or assembly found: features, configuration, variables,
/// sketches, and the STEP export with its polling.
pub(super) const AUTO_ELEMENT_CALLS: u32 = 30;
/// An automatic budget never grows past this, whatever the document claims to contain.
pub(super) const AUTO_CEILING_CALLS: u32 = 2000;

impl<C: HttpClient> Session<'_, C> {
    /// Grows an automatic budget by `calls` for work just discovered. A fixed `--max-calls`
    /// never changes.
    pub fn allow(&mut self, calls: u32) {
        if self.auto {
            self.max_calls = self.max_calls.saturating_add(calls).min(AUTO_CEILING_CALLS);
        }
    }

    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        query: &str,
        body: &[u8],
    ) -> Result<Response> {
        // A cached identifier must not put credential material into a transport URL or log.
        for bytes in [path.as_bytes(), query.as_bytes(), body] {
            self.credentials.check_bytes(bytes).map_err(|mut error| {
                error.calls = self.calls;
                error
            })?;
        }
        for attempt in 0..=3 {
            if self.calls >= self.max_calls {
                return Err(self.error(ErrorKind::Budget, "Onshape call budget reached; raise it with --max-calls (MCP: max_calls) to continue using cached replies."));
            }
            if let Some(progress) = self.progress.as_mut() {
                if progress.stop_requested() {
                    return Err(self.error(ErrorKind::Stopped, "Fetch stopped. What it fetched is kept; fetch the same link again to continue with only the missing calls."));
                }
            }
            let date = httpdate::fmt_http_date(self.client.now());
            let nonce = self.client.nonce()?;
            let authorization = self
                .credentials
                .authorization(method, path, query, &date, &nonce);
            let request = Request {
                method: method.into(),
                path: path.into(),
                query: query.into(),
                body: body.to_vec(),
                headers: BTreeMap::from([
                    ("Authorization".into(), authorization),
                    ("Date".into(), date),
                    ("On-Nonce".into(), nonce),
                    ("Content-Type".into(), "application/json".into()),
                    ("Accept".into(), "application/json".into()),
                ]),
            };
            self.calls += 1;
            if let Some(progress) = self.progress.as_mut() {
                progress.calls(self.calls, self.needed);
            }
            let response = self.client.send(&request).map_err(|mut e| {
                e.calls = self.calls;
                e
            })?;
            if response.body.len() as u64 > MAX_RESPONSE {
                return Err(self.error(
                    ErrorKind::Limit,
                    "The Onshape response exceeds 64 MiB; fetch a smaller element.",
                ));
            }
            if response.status == 402 {
                return Err(self.error(ErrorKind::YearlyLimit, "Onshape's yearly API allowance is exhausted (HTTP 402); wait for renewal or contact Onshape about more calls."));
            }
            if response.status == 429 {
                if attempt == 3 {
                    return Err(self.error(ErrorKind::Throttle, "Onshape is still throttling requests (HTTP 429) after three retries; retry later using the cache."));
                }
                let delay = response
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
                    .and_then(|(_, value)| {
                        value
                            .parse::<u64>()
                            .ok()
                            .map(Duration::from_secs)
                            .or_else(|| {
                                httpdate::parse_http_date(value).ok().map(|date| {
                                    date.duration_since(self.client.now()).unwrap_or_default()
                                })
                            })
                    })
                    .unwrap_or(Duration::from_secs(1));
                if delay > Duration::from_secs(600) {
                    return Err(self.error(ErrorKind::Throttle, "Onshape requests a wait longer than ten minutes (HTTP 429); retry later. Retry-After was not shortened."));
                }
                // Do not sleep if the next attempt cannot fit the user's budget.
                if self.calls >= self.max_calls {
                    return Err(self.error(ErrorKind::Budget, "Onshape call budget reached; raise it with --max-calls (MCP: max_calls) to retry the throttled request."));
                }
                self.client.sleep(delay);
                continue;
            }
            if response.status == 401 || response.status == 403 {
                return Err(self.error(ErrorKind::Auth, self.credentials.denied_message()));
            }
            if (300..400).contains(&response.status) {
                return Err(self.error(ErrorKind::Transport, "Onshape redirected the request; redirects are not followed with credentials. Use a cad.onshape.com document URL."));
            }
            self.credentials
                .check_bytes(&response.body)
                .map_err(|mut e| {
                    e.calls = self.calls;
                    e
                })?;
            return Ok(response);
        }
        Err(self.error(
            ErrorKind::Throttle,
            "Onshape throttled the request; retry later.",
        ))
    }

    pub fn error(&self, kind: ErrorKind, message: &'static str) -> Error {
        Error {
            kind,
            message,
            detail: None,
            calls: self.calls,
            // A stop at the call limit carries what the whole fetch is expected to need.
            estimate: (kind == ErrorKind::Budget).then_some(self.needed).flatten(),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod bearer_tests {
    use super::*;
    use crate::onshape::{fetch_authenticated, BearerToken, DocumentUrl, FetchAuth};
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Mutex, Once};
    use std::thread;
    use std::time::Instant;

    const TOKEN: &str = "synthetic-oauth-token_123+/=";
    const DID: &str = "111111111111111111111111";
    const VID: &str = "222222222222222222222222";
    const MID: &str = "333333333333333333333333";
    const EID: &str = "444444444444444444444444";

    struct Logs(Mutex<Vec<String>>);
    static LOGS: Logs = Logs(Mutex::new(Vec::new()));
    static INIT_LOGS: Once = Once::new();
    impl log::Log for Logs {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target().starts_with("ureq")
        }
        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                self.0.lock().unwrap().push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }

    struct LocalNetwork {
        client: NetworkClient,
        origin: String,
    }
    impl HttpClient for LocalNetwork {
        fn send(&mut self, request: &Request) -> Result<Response> {
            self.client.send_to(request, &self.origin)
        }
    }

    fn server(replies: Vec<(u16, String)>) -> (LocalNetwork, thread::JoinHandle<()>) {
        INIT_LOGS.call_once(|| {
            log::set_logger(&LOGS).unwrap();
            log::set_max_level(log::LevelFilter::Trace);
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            for (status, body) in replies {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut stream = crate::test_support::accept_blocking(&listener, deadline).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut authorization = None;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("authorization") {
                            authorization = Some(value.trim().to_owned());
                        }
                    }
                }
                assert_eq!(
                    authorization.as_deref(),
                    Some(format!("Bearer {TOKEN}").as_str())
                );
                write!(stream, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nX-Echo: {TOKEN}\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (
            LocalNetwork {
                client: NetworkClient::default(),
                origin,
            },
            handle,
        )
    }

    fn source() -> DocumentUrl {
        DocumentUrl::parse(
            &format!("https://cad.onshape.com/documents/{DID}/v/{VID}/e/{EID}"),
            None,
        )
        .unwrap()
    }

    fn auth() -> FetchAuth {
        FetchAuth::Bearer(BearerToken::new(TOKEN.into()).unwrap())
    }

    #[test]
    fn bearer_header_and_denied_errors_are_safe_in_logs_and_reports() {
        for status in [401, 403] {
            let dir = tempfile::tempdir().unwrap();
            let (mut client, server) = server(vec![(status, json!({"error": TOKEN}).to_string())]);
            let result = fetch_authenticated(
                &mut client,
                &auth(),
                &source(),
                &dir.path().join("output.onshape-bundle"),
                &dir.path().join("cache"),
                5,
            );
            server.join().unwrap();
            let error = result.as_ref().err().unwrap();
            assert_eq!(error.kind, ErrorKind::Auth);
            assert_eq!(error.calls, 1);
            assert!(error.message.contains("expired or lacks access"));
            assert!(error.message.contains("fresh token with read access"));
            for text in [
                error.to_string(),
                format!("{error:?}"),
                crate::json::onshape_fetch(&result).to_string(),
                format!("{:?}", auth()),
            ] {
                assert!(!text.contains(TOKEN));
            }
        }
        assert!(LOGS
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|line| !line.contains(TOKEN)));
    }

    #[test]
    fn echoed_bearer_is_refused_before_writing_cache_or_bundle() {
        // Include an escaped JSON spelling, which must be caught after JSON decoding too.
        for body in [
            json!({"microversion": TOKEN}).to_string(),
            format!("{{\"microversion\":\"{}\"}}", TOKEN.replace('s', "\\u0073")),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path().join("output.onshape-bundle");
            let (mut client, server) = server(vec![(200, body)]);
            let result = fetch_authenticated(
                &mut client,
                &auth(),
                &source(),
                &output,
                &dir.path().join("cache"),
                5,
            );
            server.join().unwrap();
            assert_eq!(result.as_ref().err().unwrap().kind, ErrorKind::Privacy);
            assert!(!output.exists());
            assert!(!crate::json::onshape_fetch(&result)
                .to_string()
                .contains(TOKEN));
        }
    }

    #[test]
    fn bearer_in_output_path_or_cached_request_identifier_is_never_reported_or_sent() {
        let dir = tempfile::tempdir().unwrap();
        let token = "synthetic-cached-token";
        let auth = FetchAuth::Bearer(BearerToken::new(token.into()).unwrap());
        let mut client = NetworkClient::default();
        let result = fetch_authenticated(
            &mut client,
            &auth,
            &source(),
            &dir.path().join(token),
            &dir.path().join("cache"),
            0,
        );
        assert_eq!(result.as_ref().err().unwrap().kind, ErrorKind::Privacy);
        assert!(!crate::json::onshape_fetch(&result)
            .to_string()
            .contains(token));
        let mut session = Session {
            client: &mut client,
            credentials: auth.as_auth(),
            calls: 0,
            max_calls: 0,
            auto: false,
            progress: None,
            needed: None,
        };
        for (path, query, body) in [
            (format!("/api/{token}"), String::new(), Vec::new()),
            ("/api".into(), format!("id={token}"), Vec::new()),
            ("/api".into(), String::new(), token.as_bytes().to_vec()),
        ] {
            let result = session.request("GET", &path, &query, &body);
            let error = result.err().unwrap();
            assert_eq!(error.kind, ErrorKind::Privacy);
            assert_eq!(error.calls, 0);
            assert!(!error.to_string().contains(token));
        }
    }

    #[test]
    fn bearer_fetch_success_and_cache_never_store_token() {
        let dir = tempfile::tempdir().unwrap();
        let replies = [
            json!({"microversion": MID}),
            json!({"name": "Synthetic assembly"}),
            json!([{"id": EID, "elementType": "ASSEMBLY", "name": "Synthetic tab"}]),
            json!({"rootAssembly": {"instances": [], "occurrences": []}}),
            json!({"features": []}),
        ];
        let (mut client, server) = server(
            replies
                .into_iter()
                .map(|body| (200, body.to_string()))
                .collect(),
        );
        let output = dir.path().join("output.onshape-bundle");
        let cache = dir.path().join("cache");
        let result = fetch_authenticated(&mut client, &auth(), &source(), &output, &cache, 5);
        server.join().unwrap();
        assert_eq!(result.as_ref().unwrap().calls, 5);
        assert!(!crate::json::onshape_fetch(&result)
            .to_string()
            .contains(TOKEN));
        let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        for index in 0..archive.len() {
            let mut text = String::new();
            archive
                .by_index(index)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            assert!(!text.contains(TOKEN));
            assert!(!text.contains("Authorization"));
        }
        fn check_cache(path: &std::path::Path) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                assert!(!path.to_string_lossy().contains(TOKEN));
                if path.is_dir() {
                    check_cache(&path);
                } else {
                    assert!(!String::from_utf8_lossy(&std::fs::read(path).unwrap()).contains(TOKEN));
                }
            }
        }
        check_cache(&cache);
        let cached = fetch_authenticated(
            &mut client,
            &auth(),
            &source(),
            &dir.path().join("cached.onshape-bundle"),
            &cache,
            0,
        )
        .unwrap();
        assert_eq!(cached.calls, 0);
        assert!(LOGS
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|line| !line.contains(TOKEN)));
    }
}
