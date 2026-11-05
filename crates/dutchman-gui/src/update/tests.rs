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
    assert!(
        notes::html_to_markdown(&"é".repeat(MAX_BYTES))
            .chars()
            .count()
            <= notes::MAX_NOTES
    );
    assert_eq!(
        notes::html_to_markdown("<a href='file:///secret'>hello</a>").trim(),
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
    let checked = check(&fetch, known, Some("feed-etag"), false);
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
fn new_feed_tag_is_offered_from_the_feed_alone() {
    let (checked, calls) = run_check(
        Ok(Response::Body {
            bytes: atom(&["v9.0.0"]),
            etag: Some("next".into()),
        }),
        "v1.0.0",
    );
    let checked = checked.unwrap();
    assert!(calls.iter().all(|(url, _)| url != API));
    assert_eq!(checked.etag.as_deref(), Some("next"));
    let release = checked.release.unwrap();
    assert_eq!(release.notes.trim(), "Safer & faster.");
    assert!(release.download.contains("/v9.0.0"), "{}", release.download);
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
        updater.fresh = manual;
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
    let release = check(&fetch, "v1.0.0", None, false)
        .unwrap()
        .release
        .unwrap();
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

#[test]
fn msi_handoff_blocks_new_conversions_until_failure_or_restart() {
    let mut updater = Updater::new(None);
    updater.install = Install::Msi("C:/Local/Programs/Dutchman".into());
    for (phase, blocked) in [
        (Phase::Idle, false),
        (Phase::Running, true),
        (Phase::Ready, true),
        (Phase::Restarting, true),
        (Phase::Restarted, true),
        (Phase::Failed("Download failed".into()), false),
    ] {
        updater.job.preview("v1.2.3".into(), phase, "");
        assert_eq!(updater.blocks_conversions(), blocked);
    }
    updater.install = Install::Direct;
    assert!(!updater.blocks_conversions());
}

#[test]
fn msi_confirmation_waits_for_active_files_and_cannot_be_bypassed_when_they_finish() {
    let mut updater = Updater::new(None);
    updater.install = Install::Msi("installed".into());
    updater.job.preview("v1.2.3".into(), Phase::Ready, "");
    assert!(!updater.msi_can_handoff(false));
    assert!(!updater.msi_can_handoff(true));
    assert!(!updater.stopping_for_install());
    updater.msi_finish_confirmed = true;
    assert!(updater.stopping_for_install());
    assert!(!updater.msi_can_handoff(false));
    assert!(updater.msi_can_handoff(true));
    updater
        .job
        .preview("v1.2.3".into(), Phase::Failed("Helper failed".into()), "");
    assert!(!updater.stopping_for_install());
    assert!(!updater.blocks_conversions());
    updater.install = Install::Zip("portable".into());
    for (phase, blocked) in [
        (Phase::Running, false),
        (Phase::Installed, false),
        (Phase::Restarting, true),
        (Phase::Restarted, true),
    ] {
        updater.job.preview("v1.2.3".into(), phase, "");
        assert_eq!(updater.blocks_conversions(), blocked);
    }
}

#[test]
fn a_verified_msi_reopens_a_hidden_progress_window_before_closing() {
    let mut updater = Updater::new(None);
    updater.allowed = false;
    updater.install = Install::Msi("installed".into());
    updater.open = false;
    updater.job.preview("v1.2.3".into(), Phase::Ready, "");
    updater.tick(&egui::Context::default());
    assert!(updater.open);
    assert!(updater.rx.is_none());
}

const FIXTURE: &[u8] = include_bytes!("fixtures/releases.atom");

#[test]
fn feed_fixture_picks_the_newest_stable_release_whatever_the_order() {
    let release = parse_feed(FIXTURE).unwrap().unwrap();
    assert_eq!(release.tag, "v0.2.7");
    assert!(release.notes.contains("## What's new"), "{}", release.notes);
    assert!(release.notes.contains("`dutchman report`"));
    assert!(!release.notes.contains("<"), "no markup is left");
    assert!(
        !release.notes.contains("raw.githubusercontent"),
        "images are dropped"
    );
    let tags: Vec<_> = feed_entries(FIXTURE)
        .unwrap()
        .into_iter()
        .map(|r| r.tag)
        .collect();
    assert_eq!(
        tags,
        ["v0.2.6", "v0.2.7"],
        "nightly and -rc tags are skipped"
    );
}

#[test]
fn download_names_follow_the_release_tag() {
    assert_eq!(
        asset_name("v0.2.7", "macos", "aarch64", false).unwrap(),
        "dutchman-0.2.7-macos-universal.dmg"
    );
    assert_eq!(
        asset_name("v0.2.7", "windows", "x86_64", false).unwrap(),
        "dutchman-0.2.7-x86_64-pc-windows-msvc.zip"
    );
    assert_eq!(
        asset_name("v0.2.7", "linux", "x86_64", false).unwrap(),
        "dutchman-0.2.7-x86_64-unknown-linux-gnu.tar.gz"
    );
    assert!(asset_name("v0.2.7", "linux", "x86_64", true).is_none());
    for (os, arch, appimage) in [
        ("macos", "aarch64", false),
        ("windows", "x86_64", false),
        ("linux", "aarch64", false),
    ] {
        let name = asset_name("v0.2.7", os, arch, appimage).unwrap();
        assert!(matches_asset(&name, os, arch, appimage), "{name}");
    }
}

fn updater_with(
    fetch: impl Fn(&str, Option<&str>) -> Result<Response, String> + Send + Sync + 'static,
    cached: &str,
) -> Updater {
    let mut updater = Updater::new(None);
    updater.install = Install::Direct;
    updater.allowed = true;
    updater.fetcher = std::sync::Arc::new(fetch);
    updater.release = Some(Release {
        fixes_reports: Vec::new(),
        tag: cached.into(),
        notes: "Old notes".into(),
        download: RELEASES.into(),
    });
    updater.etag = Some("old-etag".into());
    updater.next = now() + HOUR;
    updater
}

fn finish(updater: &mut Updater, ctx: &egui::Context) {
    for _ in 0..500 {
        updater.tick(ctx);
        if updater.rx.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the check did not finish");
}

#[test]
fn pressing_update_asks_again_and_a_newer_release_replaces_the_cached_one() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let mut updater = updater_with(
        move |url, etag| {
            log.lock()
                .unwrap()
                .push((url.to_owned(), etag.map(str::to_owned)));
            Ok(Response::Body {
                bytes: atom(&["v9.0.0", "v8.0.0"]),
                etag: Some("new".into()),
            })
        },
        "v8.0.0",
    );
    let ctx = egui::Context::default();
    updater.press(&ctx);
    // While it runs the window says so; the cached offer is not shown.
    assert_eq!(updater.click, Click::Checking);
    assert!(updater.open);
    finish(&mut updater, &ctx);
    assert_eq!(updater.release.as_ref().unwrap().tag, "v9.0.0");
    assert_eq!(updater.click, Click::None);
    assert!(updater.open);
    // One request to the feed, with no ETag, so GitHub cannot answer "unchanged".
    assert_eq!(*seen.lock().unwrap(), [(FEED.to_owned(), None)]);
}

#[test]
fn pressing_update_drops_a_cached_release_that_is_no_longer_newer() {
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));
    let mut updater = updater_with(
        move |_, _| {
            Ok(Response::Body {
                bytes: atom(&[&current]),
                etag: None,
            })
        },
        "v8.0.0",
    );
    let ctx = egui::Context::default();
    updater.press(&ctx);
    finish(&mut updater, &ctx);
    assert!(updater.release.is_none());
    assert_eq!(updater.click, Click::UpToDate);
    assert!(!updater.newer_known());
}

#[test]
fn a_failed_check_says_why_and_retry_checks_again() {
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = attempts.clone();
    let mut updater = updater_with(
        move |url, _| {
            use std::sync::atomic::Ordering::SeqCst;
            let n = counted.fetch_add(1, SeqCst);
            match (url == FEED, n) {
                (true, 0) => Err("dns error: failed to lookup address".into()),
                (false, _) => Err("status code 403".into()),
                _ => Ok(Response::Body {
                    bytes: atom(&["v9.0.0"]),
                    etag: None,
                }),
            }
        },
        "v8.0.0",
    );
    let ctx = egui::Context::default();
    updater.press(&ctx);
    finish(&mut updater, &ctx);
    // Feed down, API limited: the reason is the limit.
    assert_eq!(
        updater.click,
        Click::Failed("GitHub is limiting requests right now. Try again in a few minutes.".into())
    );
    updater.press(&ctx);
    finish(&mut updater, &ctx);
    assert_eq!(updater.release.as_ref().unwrap().tag, "v9.0.0");
    assert_eq!(updater.click, Click::None);
    assert!(failure_text("dns error: failed to lookup address").contains("internet connection"));
    assert!(failure_text("weird").starts_with("Could not check"));
}

#[test]
fn background_checks_still_reuse_the_etag_and_the_cached_release() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let mut updater = updater_with(
        move |url, etag| {
            log.lock()
                .unwrap()
                .push((url.to_owned(), etag.map(str::to_owned)));
            Ok(Response::Unchanged)
        },
        "v8.0.0",
    );
    let ctx = egui::Context::default();
    updater.start(&ctx, false);
    finish(&mut updater, &ctx);
    assert_eq!(
        *seen.lock().unwrap(),
        [(FEED.to_owned(), Some("old-etag".to_owned()))]
    );
    assert_eq!(updater.release.as_ref().unwrap().tag, "v8.0.0");
    assert_eq!(updater.click, Click::None);
}
