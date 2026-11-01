use super::*;
use std::cell::RefCell;

fn atom(tags: &[&str]) -> Vec<u8> {
    format!("<feed xmlns=\"http://www.w3.org/2005/Atom\">{}</feed>", tags.iter().map(|tag| format!("<entry><id>tag:github.com,2008:Repository/1/{tag}</id><content type=\"html\">&lt;p&gt;Safer &amp;amp; faster.&lt;/p&gt;</content></entry>")).collect::<String>()).into_bytes()
}

#[test]
fn feed_selects_only_stable_tags_by_numeric_version() {
    let release = parse_feed(&atom(&[
        "nightly",
        "v2.0.0-rc.1",
        "v1.2.9",
        "v1.2.10",
        "v1.2.11+build",
        "v01.3.0",
    ]))
    .unwrap()
    .unwrap();
    assert_eq!(release.tag, "v1.2.10");
    assert_eq!(release.notes.trim(), "Safer & faster.");
    assert!(parse_feed(&atom(&["nightly", "v1.2", "v1.2.3-beta"]))
        .unwrap()
        .is_none());
}

#[test]
fn garbage_and_oversize_inputs_are_bounded() {
    for body in [
        b"".as_slice(),
        b"\xff",
        b"<feed",
        b"<!DOCTYPE feed [<!ENTITY x 'x'>]><feed>&x;</feed>",
    ] {
        assert!(parse_feed(body).is_err());
    }
    assert!(parse_feed(&vec![b'x'; MAX_BYTES + 1]).is_err());
    let long = format!(
        "<feed><entry><id>v{}.0.0</id></entry></feed>",
        "9".repeat(200)
    );
    assert!(parse_feed(long.as_bytes()).unwrap().is_none());
    assert!(plain_text(&"é".repeat(MAX_BYTES)).chars().count() <= MAX_NOTES);
    assert_eq!(
        plain_text("<a href='file:///secret'>hello</a>").trim(),
        "hello"
    );
}

type Calls = Vec<(String, Option<String>)>;

fn run_check(feed: Result<Response, String>, known: &str) -> (Result<Checked, String>, Calls) {
    let calls = RefCell::new(Vec::new());
    let feed = RefCell::new(Some(feed));
    let fetch = |url: &str, etag: Option<&str>| {
        calls
            .borrow_mut()
            .push((url.to_owned(), etag.map(str::to_owned)));
        if url == FEED {
            feed.borrow_mut()
                .take()
                .unwrap_or_else(|| Err("Feed requested twice".into()))
        } else {
            Ok(Response::Body {
                bytes: br#"{"tag_name":"v9.0.0","assets":[]}"#.to_vec(),
                etag: None,
            })
        }
    };
    let checked = check(&fetch, known, Some("feed-etag"));
    (checked, calls.into_inner())
}

#[test]
fn unchanged_and_old_feeds_never_call_api() {
    for feed in [
        Response::Unchanged,
        Response::Body {
            bytes: atom(&["v1.0.0"]),
            etag: Some("next".into()),
        },
    ] {
        let (checked, calls) = run_check(Ok(feed), "v1.0.0");
        assert!(checked.unwrap().release.is_none());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], (FEED.into(), Some("feed-etag".into())));
    }
}

#[test]
fn new_feed_tag_calls_api_once_and_keeps_feed_notes() {
    let (checked, calls) = run_check(
        Ok(Response::Body {
            bytes: atom(&["v9.0.0"]),
            etag: Some("next".into()),
        }),
        "v1.0.0",
    );
    let checked = checked.unwrap();
    assert_eq!(calls.iter().filter(|(url, _)| url == API).count(), 1);
    assert_eq!(checked.etag.as_deref(), Some("next"));
    assert_eq!(checked.release.unwrap().notes.trim(), "Safer & faster.");
}

#[test]
fn broken_feed_falls_back_once() {
    for feed in [
        Err("offline".into()),
        Ok(Response::Body {
            bytes: b"garbage".to_vec(),
            etag: None,
        }),
    ] {
        let (checked, calls) = run_check(feed, "v1.0.0");
        assert!(checked.unwrap().release.is_some());
        assert_eq!(calls.iter().filter(|(url, _)| url == API).count(), 1);
    }
}

#[test]
fn environment_and_source_builds_disable_even_manual_checks() {
    assert!(!permitted(true, Some("0")));
    assert!(!permitted(false, None));
    assert!(!permitted(false, Some("1")));
    assert!(permitted(true, None));
    let mut updater = Updater::new(None);
    updater.allowed = false;
    updater.start(&egui::Context::default(), true);
    assert!(updater.rx.is_none());
}

#[test]
fn skip_and_three_day_reminder_survive_settings_restore() {
    let mut updater = Updater::new(None);
    updater.preview(Release {
        fixes_reports: Vec::new(),
        tag: "v9.0.0".into(),
        notes: String::new(),
        download: RELEASES.into(),
    });
    assert!(updater.offered(100));
    updater.skipped = "v9.0.0".into();
    assert!(!updater.offered(100));
    let saved = updater.saved();
    assert_eq!(Updater::new(Some(&saved)).skipped, "v9.0.0");
    updater.skipped = "v8.0.0".into();
    updater.remind_at = 100 + THREE_DAYS;
    assert!(!updater.offered(100 + THREE_DAYS - 1));
    assert!(updater.offered(100 + THREE_DAYS));
    let restored = Updater::new(Some(&updater.saved()));
    assert_eq!(restored.remind_at, 100 + THREE_DAYS);
    assert!(Updater::new(None).automatic);
}

#[test]
fn installation_paths_and_assets_match_platform() {
    assert_eq!(
        Install::detect(Path::new(
            "/opt/homebrew/Cellar/dutchman/0.2.4/bin/dutchman-gui"
        ))
        .command(),
        Some("brew update && brew upgrade dutchman-cad/tap/dutchman")
    );
    assert_eq!(Install::detect(Path::new("/opt/homebrew/Caskroom/dutchman-convert/0.2.4/Dutchman Convert.app/Contents/MacOS/dutchman-gui")).command(), Some("brew update && brew upgrade --cask dutchman-cad/tap/dutchman-convert"));
    assert_eq!(
        Install::detect(Path::new(
            "C:\\Users\\test\\scoop\\apps\\dutchman\\current\\dutchman-gui.exe"
        ))
        .command(),
        Some("scoop update; if ($?) { scoop update dutchman }")
    );
    assert_eq!(
        Install::detect(Path::new("/tmp/dutchman-gui")).command(),
        None
    );
    assert!(matches_asset(
        "dutchman-0.2.6-x86_64-unknown-linux-gnu.tar.gz",
        "linux",
        "x86_64",
        false
    ));
    assert!(!matches_asset(
        "dutchman-0.2.6-aarch64-unknown-linux-gnu.tar.gz",
        "linux",
        "x86_64",
        false
    ));
    assert!(matches_asset(
        "dutchman-macos-universal.dmg",
        "macos",
        "aarch64",
        false
    ));
    assert!(matches_asset(
        "dutchman-x86_64-pc-windows-msvc.zip",
        "windows",
        "x86_64",
        false
    ));
    assert!(is_appimage(Path::new(
        "/tmp/.mount_dutchman/usr/bin/dutchman-gui"
    )));
    assert!(matches_asset(
        "dutchman-x86_64.AppImage",
        "linux",
        "x86_64",
        true
    ));
    assert!(!matches_asset(
        "dutchman-x86_64-unknown-linux-gnu.tar.gz",
        "linux",
        "x86_64",
        true
    ));
}

#[test]
fn completed_checks_schedule_an_hour_and_only_manual_checks_open_the_window() {
    for manual in [false, true] {
        let mut updater = Updater::new(None);
        updater.allowed = true;
        updater.manual = manual;
        let (tx, rx) = channel();
        updater.rx = Some(rx);
        tx.send(Ok(Checked {
            clear_release: false,
            release: Some(Release {
                fixes_reports: Vec::new(),
                tag: "v9.0.0".into(),
                notes: "Notes".into(),
                download: RELEASES.into(),
            }),
            etag: Some("feed-etag".into()),
        }))
        .unwrap();
        let before = now();
        updater.tick(&egui::Context::default());
        assert_eq!(updater.open, manual);
        assert!(updater.next >= before + HOUR);
        assert!(updater.next <= now() + HOUR);
        assert_eq!(updater.etag.as_deref(), Some("feed-etag"));
        assert!(updater.rx.is_none());
    }
    let mut updater = Updater::new(None);
    updater.allowed = true;
    let (tx, rx) = channel();
    updater.rx = Some(rx);
    tx.send(Err("offline".into())).unwrap();
    let before = now();
    updater.tick(&egui::Context::default());
    assert!(updater.next >= before + HOUR);
    assert!(!updater.open);
    assert!(updater.rx.is_none());
}

#[test]
fn notes_drop_a_first_line_that_only_repeats_the_version() {
    use super::notes_body;
    assert_eq!(
        notes_body("Dutchman 0.2.6\n\nFaster assemblies.", "v0.2.6"),
        "Faster assemblies."
    );
    assert_eq!(
        notes_body("v0.2.6\nFaster assemblies.", "v0.2.6"),
        "Faster assemblies."
    );
    assert_eq!(
        notes_body("Faster assemblies.", "v0.2.6"),
        "Faster assemblies."
    );
    assert_eq!(notes_body("Dutchman 0.2.6", "v0.2.6"), "");
}

#[test]
fn tap_304_makes_no_further_request() {
    let calls = RefCell::new(Vec::new());
    let install = Install::Cask("/opt/homebrew".into());
    let checked = check_package(
        &|url, etag| {
            calls
                .borrow_mut()
                .push((url.to_owned(), etag.map(str::to_owned)));
            Ok(Response::Unchanged)
        },
        &install,
        "v0.2.4",
        "v0.2.4",
        Some("tap-etag"),
    )
    .unwrap();
    assert!(checked.release.is_none());
    assert_eq!(
        *calls.borrow(),
        vec![(install.source().unwrap().into(), Some("tap-etag".into()))]
    );
}

#[test]
fn homebrew_pill_waits_for_tap_version_and_uses_matching_feed_notes() {
    for tag in ["0.2.4", "0.2.5"] {
        let calls = RefCell::new(Vec::new());
        let install = Install::Cask("/opt/homebrew".into());
        let checked = check_package(
            &|url, _| {
                calls.borrow_mut().push(url.to_owned());
                let bytes = if url == FEED {
                    atom(&["v0.2.6", "v0.2.5"])
                } else {
                    format!("version \"{tag}\"").into_bytes()
                };
                Ok(Response::Body {
                    bytes,
                    etag: Some("tap-etag".into()),
                })
            },
            &install,
            "v0.2.4",
            "v0.2.4",
            None,
        )
        .unwrap();
        let mut updater = Updater::new(None);
        updater.release = checked.release;
        assert_eq!(updater.offered(100), tag == "0.2.5");
        assert_eq!(calls.borrow().len(), if tag == "0.2.5" { 2 } else { 1 });
        if tag == "0.2.5" {
            let release = updater.release.unwrap();
            assert_eq!(release.tag, "v0.2.5");
            assert_eq!(release.notes.trim(), "Safer & faster.");
        }
        assert!(!calls.borrow().iter().any(|url| url == API));
    }
}

#[test]
fn missing_feed_entry_or_failed_feed_still_offers_the_available_package() {
    for feed in [
        Ok(Response::Body {
            bytes: atom(&["v0.2.6"]),
            etag: None,
        }),
        Err("offline".into()),
    ] {
        let feed = RefCell::new(Some(feed));
        let checked = check_package(
            &|url, _| {
                if url == FEED {
                    feed.borrow_mut().take().unwrap()
                } else {
                    Ok(Response::Body {
                        bytes: br#"{"version":"0.2.5"}"#.to_vec(),
                        etag: None,
                    })
                }
            },
            &Install::Scoop("shims".into()),
            "v0.2.4",
            "v0.2.4",
            None,
        )
        .unwrap();
        let release = checked.release.unwrap();
        assert_eq!(release.tag, "v0.2.5");
        assert!(release.notes.is_empty());
        assert!(release.download.ends_with("/tag/v0.2.5"));
    }
}

#[test]
fn release_metadata_reads_only_lowercase_report_hashes_without_an_extra_request() {
    let id = "0123456789aba3f2";
    let hash = dutchman::report_send::hash(id.as_bytes());
    let fetch = |url: &str, _: Option<&str>| {
        if url == FEED {
            return Err("No feed".into());
        }
        assert_eq!(url, API);
        Ok(Response::Body { bytes: serde_json::json!({"tag_name": "v9.0.0", "assets": [], "fixes_reports": [hash, "A".repeat(64), "bad"]}).to_string().into_bytes(), etag: None })
    };
    let release = check(&fetch, "v1.0.0", None).unwrap().release.unwrap();
    assert_eq!(release.fixes_reports, vec![hash]);
    assert_eq!(
        dutchman::report_send::fixed_notice(&[id.into()], &release.fixes_reports).unwrap(),
        "This update fixes a problem you reported (A3F2)."
    );
}

#[test]
fn github_atom_release_metadata_keeps_fixes_without_showing_machine_text() {
    let hash = "a".repeat(64);
    let feed = format!("<feed><entry><id>v9.0.0</id><content>&lt;p&gt;A conversion fix.&lt;/p&gt;&lt;!-- dutchman-release {{&quot;fixes_reports&quot;:[&quot;{hash}&quot;]}} --&gt;</content></entry></feed>");
    let release = parse_feed(feed.as_bytes()).unwrap().unwrap();
    assert_eq!(release.fixes_reports, vec![hash]);
    assert!(!release.notes.contains("fixes_reports"));
    assert!(release.notes.contains("A conversion fix."));
}
