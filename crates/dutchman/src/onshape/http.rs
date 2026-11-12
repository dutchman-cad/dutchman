//! Bounded, injectable HTTP transport. Redirects never forward signed credentials.
use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, SystemTime};

use super::{Credentials, Error, ErrorKind, Result, MAX_RESPONSE};

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

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
        let query = if request.query.is_empty() {
            String::new()
        } else {
            format!("?{}", request.query)
        };
        let url = format!("https://cad.onshape.com{}{}", request.path, query);
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
    pub credentials: &'a Credentials,
    pub calls: u32,
    pub max_calls: u32,
}

impl<C: HttpClient> Session<'_, C> {
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        query: &str,
        body: &[u8],
    ) -> Result<Response> {
        for attempt in 0..=3 {
            if self.calls >= self.max_calls {
                return Err(self.error(ErrorKind::Budget, "Onshape call budget reached; increase --max-calls to continue using cached replies."));
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
                    return Err(self.error(ErrorKind::Budget, "Onshape call budget reached; increase --max-calls to retry the throttled request."));
                }
                self.client.sleep(delay);
                continue;
            }
            if response.status == 401 || response.status == 403 {
                return Err(self.error(
                    ErrorKind::Auth,
                    "Onshape denied access; check that your API keys can read your own document.",
                ));
            }
            if (300..400).contains(&response.status) {
                return Err(self.error(ErrorKind::Transport, "Onshape redirected the request; redirects are not followed with API keys. Use a cad.onshape.com document URL."));
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
            calls: self.calls,
        }
    }
}
