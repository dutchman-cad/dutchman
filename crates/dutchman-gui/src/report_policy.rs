//! Keeps the report policy current: at most once a day, off the window's thread, and only
//! where update checks may run. The bundled copy is used until a newer one is read, and
//! whenever one cannot be used. The request says nothing about any file.

use std::sync::mpsc::{channel, Receiver, TryRecvError};

use dutchman::report_policy::{ReportPolicy, REFRESH_URL};
use eframe::egui;
use serde_json::{json, Value};

use crate::update::{self, Response};

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

/// The policy in use, and when to look for a newer one.
pub struct Refresher {
    policy: ReportPolicy,
    /// The text of a fetched policy in use; none while the bundled copy is.
    text: Option<String>,
    etag: Option<String>,
    next: u64,
    rx: Option<Receiver<Result<Response, String>>>,
    allowed: bool,
}

impl Refresher {
    /// Picks up from `saved`, the app's settings, where network use is allowed.
    pub fn new(saved: Option<&Value>) -> Self {
        Self::with(saved, update::network_allowed(), update::now())
    }

    fn with(saved: Option<&Value>, allowed: bool, now: u64) -> Self {
        let get = |key| saved.and_then(|v| v.get(key)).filter(|_| allowed);
        let text = get("text").and_then(Value::as_str).map(str::to_owned);
        let parsed = text
            .as_deref()
            .and_then(|text| ReportPolicy::parse(text).ok());
        let (policy, text, etag) = match parsed {
            Some(policy) => (
                policy,
                text,
                get("etag").and_then(Value::as_str).map(str::to_owned),
            ),
            None => (ReportPolicy::bundled().clone(), None, None),
        };
        let next = get("next").and_then(Value::as_u64).unwrap_or(0);
        Self {
            policy,
            text,
            etag,
            // A clock set back must not stop the refreshes.
            next: if next > now.saturating_add(DAY) {
                0
            } else {
                next
            },
            rx: None,
            allowed,
        }
    }

    pub fn policy(&self) -> &ReportPolicy {
        &self.policy
    }

    /// What to keep in the app's settings.
    pub fn saved(&self) -> Value {
        json!({"text": self.text, "etag": self.etag, "next": self.next})
    }

    /// Takes in a finished refresh and, when one is due and `enabled`, starts the next.
    pub fn tick(&mut self, ctx: &egui::Context, enabled: bool) {
        if let Some(rx) = &self.rx {
            let result = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("stopped".to_owned())),
            };
            if let Some(result) = result {
                self.rx = None;
                self.apply(result, update::now());
            }
        }
        if self.allowed && enabled && self.rx.is_none() && update::now() >= self.next {
            let etag = self.etag.clone();
            let (tx, rx) = channel();
            self.rx = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(update::fetch(REFRESH_URL, etag.as_deref()));
                ctx.request_repaint();
            });
        }
    }

    /// A failed request tries again in an hour. A copy that was read but cannot be used, such
    /// as one of a newer schema, leaves the bundled copy in use until tomorrow's look.
    fn apply(&mut self, result: Result<Response, String>, now: u64) {
        match result {
            Err(_) => {
                self.next = now.saturating_add(HOUR);
                return;
            }
            Ok(Response::Unchanged) => {}
            Ok(Response::Body { bytes, etag }) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if let Ok(policy) = ReportPolicy::parse(&text) {
                    self.policy = policy;
                    self.text = Some(text);
                    self.etag = etag;
                } else {
                    self.policy = ReportPolicy::bundled().clone();
                    self.text = None;
                    self.etag = None;
                }
            }
        }
        self.next = now.saturating_add(DAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    fn published(limit: u64, version: u64) -> Vec<u8> {
        let mut value: Value =
            serde_json::from_str(include_str!("../../dutchman/assets/report-policy.json")).unwrap();
        value["discordUploadLimitMb"] = json!(limit);
        value["version"] = json!(version);
        value.to_string().into_bytes()
    }

    fn body(bytes: Vec<u8>) -> Result<Response, String> {
        Ok(Response::Body {
            bytes,
            etag: Some("\"tag\"".to_owned()),
        })
    }

    #[test]
    fn the_refresh_address_says_nothing_about_a_file() {
        assert!(REFRESH_URL.starts_with("https://dutchman-cad.github.io/"));
        assert!(!REFRESH_URL.contains('?'));
    }

    #[test]
    fn a_new_copy_is_used_kept_and_not_looked_for_again_for_a_day() {
        let mut refresher = Refresher::with(None, true, NOW);
        assert_eq!(&refresher.policy, ReportPolicy::bundled());
        refresher.apply(body(published(25, 1)), NOW);
        assert_eq!(refresher.policy.discord_upload_limit_mb, 25);
        assert_eq!(refresher.next, NOW + DAY);
        let again = Refresher::with(Some(&refresher.saved()), true, NOW + 1);
        assert_eq!(again.policy.discord_upload_limit_mb, 25);
        assert_eq!(again.etag.as_deref(), Some("\"tag\""));
        assert_eq!(again.next, NOW + DAY);
    }

    #[test]
    fn a_newer_schema_or_a_bad_copy_keeps_the_bundled_one() {
        for bytes in [published(25, 2), b"<html>not found</html>".to_vec()] {
            let mut refresher = Refresher::with(None, true, NOW);
            refresher.apply(body(published(25, 1)), NOW);
            refresher.apply(body(bytes), NOW);
            assert_eq!(&refresher.policy, ReportPolicy::bundled());
            assert_eq!(refresher.etag, None);
            assert_eq!(refresher.next, NOW + DAY);
        }
    }

    #[test]
    fn offline_keeps_what_is_in_use_and_tries_again_in_an_hour() {
        let mut refresher = Refresher::with(None, true, NOW);
        refresher.apply(body(published(25, 1)), NOW);
        refresher.apply(Err("offline".to_owned()), NOW);
        assert_eq!(refresher.policy.discord_upload_limit_mb, 25);
        assert_eq!(refresher.next, NOW + HOUR);
    }

    #[test]
    fn an_unchanged_copy_waits_another_day() {
        let mut refresher = Refresher::with(None, true, NOW);
        refresher.apply(body(published(25, 1)), NOW);
        refresher.apply(Ok(Response::Unchanged), NOW + DAY);
        assert_eq!(refresher.policy.discord_upload_limit_mb, 25);
        assert_eq!(refresher.next, NOW + 2 * DAY);
    }

    #[test]
    fn where_checks_are_off_nothing_saved_is_used_and_nothing_starts() {
        let mut online = Refresher::with(None, true, NOW);
        online.apply(body(published(25, 1)), NOW);
        let mut off = Refresher::with(Some(&online.saved()), false, NOW);
        assert_eq!(&off.policy, ReportPolicy::bundled());
        off.tick(&egui::Context::default(), true);
        assert!(off.rx.is_none());
    }

    #[test]
    fn a_saved_time_far_ahead_does_not_stop_the_refreshes() {
        let saved = json!({"next": NOW + 30 * DAY});
        assert_eq!(Refresher::with(Some(&saved), true, NOW).next, 0);
    }
}
