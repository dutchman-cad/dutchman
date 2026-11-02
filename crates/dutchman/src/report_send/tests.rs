#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use std::net::TcpListener;

struct Synthetic;
impl BundleSource for Synthetic {
    fn parts(&self) -> Result<Vec<BundlePart>, String> {
        Ok(vec![BundlePart {
            role: "synthetic replay records".into(),
            bytes: vec![42; 1024],
        }])
    }
}

#[test]
fn preview_redacts_and_preserves_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let report = Prepared::new(
        "0123456789abcdef",
        "f3d",
        "decode",
        Some(&Synthetic),
        dir.path(),
    )
    .unwrap();
    assert!(report.bundle_size > 0);
    assert!(!report.preview().to_string().contains("claim_secret"));
    assert!(!report.preview().to_string().contains("bytes_base64url"));
    assert_eq!(report.preview()["public_test"], false);
    assert!(submit(&report, dir.path(), "bundle", false, false, None).is_err());
    assert!(submit(&report, dir.path(), "whole", false, true, None).is_err());
    let failed = submit(&report, dir.path(), "bundle", false, true, None).unwrap();
    assert_eq!(failed["sent"], false);
    assert!(dir.path().join("bundle.json.gz").exists());
    assert_eq!(Prepared::load(dir.path()).unwrap().claim(), report.claim());
    assert!(report.claim().split('-').next_back().unwrap().len() == 10);
}

fn stub(statuses: Vec<u16>, receipt: Value) -> (String, std::thread::JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/reports", listener.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let mut calls = Vec::new();
        for status in statuses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut one = [0u8; 1];
            while !bytes.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut one).unwrap();
                bytes.push(one[0]);
            }
            let header = String::from_utf8_lossy(&bytes);
            let len: usize = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; len];
            stream.read_exact(&mut body).unwrap();
            calls.push(body);
            let body = receipt.to_string();
            write!(
                stream,
                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        calls
    });
    (url, task)
}

#[test]
fn retry_is_identical_and_only_receipt_means_sent() {
    let dir = tempfile::tempdir().unwrap();
    let report = Prepared::new("0123456789abcdef", "f3d", "decode", None, dir.path()).unwrap();
    let (url, task) = stub(
        vec![503, 201],
        json!({"report_id": report.report_id, "received": 123}),
    );
    assert_eq!(
        submit(&report, dir.path(), "summary", false, true, Some(&url)).unwrap()["sent"],
        true
    );
    let calls = task.join().unwrap();
    assert_eq!(calls[0], calls[1]);
    let body = String::from_utf8_lossy(&calls[0]);
    assert_eq!(
        calls[0].len(),
        report.request_size("summary", false).unwrap()
    );
    assert!(body.contains("\"source_app\":\"Fusion\""));
    assert!(body.contains("claim_hash"));
    assert!(!body.contains(&report.claim_secret));
    assert!(!body.contains("name=\"bundle\""));
    let (url, task) = stub(vec![200], json!({"report_id": "wrong", "received": 123}));
    assert_eq!(
        submit(&report, dir.path(), "summary", false, true, Some(&url)).unwrap()["sent"],
        false
    );
    task.join().unwrap();
    for status in [429, 503, 500] {
        let (url, task) = stub(vec![status, status], json!({}));
        let result = submit(&report, dir.path(), "summary", false, true, Some(&url)).unwrap();
        assert_eq!(result["sent"], false);
        if status == 429 {
            assert!(result["summary"].as_str().unwrap().contains("tomorrow"));
        }
        assert_eq!(task.join().unwrap().len(), 2);
    }
}

#[test]
fn fixes_are_local_hash_matches() {
    let id = "0123456789aba3f2".to_owned();
    assert_eq!(
        fixed_notice(std::slice::from_ref(&id), &[hash(id.as_bytes())]).unwrap(),
        "This update fixes a problem you reported (A3F2)."
    );
    assert!(fixed_notice(&[id], &["A".repeat(64)]).is_none());
}

#[test]
fn cap_changed_bytes_and_consent_are_checked_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let report = Prepared::new(
        "fedcba9876543210",
        "f3d",
        "decode",
        Some(&Synthetic),
        dir.path(),
    )
    .unwrap();
    assert!(report.preview()["summary_size"].as_u64().unwrap() > 0);
    let _ = submit(&report, dir.path(), "bundle", false, true, None).unwrap();
    assert!(submit(&report, dir.path(), "summary", false, true, None).is_err());
    std::fs::write(dir.path().join("bundle.json.gz"), b"changed").unwrap();
    assert!(submit(&report, dir.path(), "bundle", false, true, None).is_err());
    let file = std::fs::File::create(dir.path().join("bundle.json.gz")).unwrap();
    file.set_len(MAX_BUNDLE as u64 + 1).unwrap();
    assert!(submit(&report, dir.path(), "bundle", false, true, None).is_err());
}

#[test]
fn network_failure_retains_local_report_and_has_no_resend_job() {
    let dir = tempfile::tempdir().unwrap();
    let report = Prepared::new("0123456789abcdef", "f3d", "decode", None, dir.path()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/api/reports", listener.local_addr().unwrap());
    drop(listener);
    let failed = submit(&report, dir.path(), "summary", false, true, Some(&endpoint)).unwrap();
    assert_eq!(failed["sent"], false);
    assert!(dir.path().join("submission.json").exists());
    assert!(!dir.path().join("receipt.json").exists());
}

#[test]
fn bundle_upload_has_ordered_parts_checksums_and_recorded_permission() {
    let dir = tempfile::tempdir().unwrap();
    let report = Prepared::new(
        "f0123456789abcdef",
        "f3d",
        "decode",
        Some(&Synthetic),
        dir.path(),
    )
    .unwrap();
    let (url, task) = stub(
        vec![201],
        json!({"report_id": report.report_id, "received": 123}),
    );
    let receipt = submit(&report, dir.path(), "bundle", true, true, Some(&url)).unwrap();
    assert_eq!(receipt["sent"], true);
    let calls = task.join().unwrap();
    let body = String::from_utf8_lossy(&calls[0]);
    assert!(body.find("name=\"report\"").unwrap() < body.find("name=\"bundle\"").unwrap());
    assert!(body.contains("application/gzip"));
    assert_eq!(calls[0].len(), report.request_size("bundle", true).unwrap());
    assert!(calls[0].len() <= MAX_REQUEST);
    assert!(body.contains("\"source_app\":\"Fusion\""));
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("submission.json")).unwrap())
            .unwrap();
    assert_eq!(saved["consent"]["public_test"], true);
    assert_eq!(saved["bundle_size"], report.bundle_size);
    assert_eq!(saved["bundle_sha256"], report.bundle_sha256.unwrap());
    assert!(dir.path().join("receipt.json").exists());
}

#[test]
fn claim_code_matches_the_endpoint_contract_vector() {
    let dir = tempfile::tempdir().unwrap();
    let mut report = Prepared::new("0123456789abcdef", "f3d", "decode", None, dir.path()).unwrap();
    report.claim_secret = "synthetic-secret".into();
    assert_eq!(report.claim(), "claim 0123456789abcdef-PVMFRAMQDO");
}

#[test]
fn metadata_identifies_native_apps_without_guessing_exchange_file_authors() {
    for (format, app) in [
        ("f3d", "Fusion"),
        ("f3z", "Fusion"),
        ("fusion_archive", "Fusion"),
        ("fusion_multi_document", "Fusion"),
        ("ipt", "Inventor"),
        ("iam", "Inventor"),
        ("inventor", "Inventor"),
        ("sldprt", "SolidWorks"),
        ("sldasm", "SolidWorks"),
        ("fcstd", "FreeCAD"),
        ("freecad_document", "FreeCAD"),
        ("catpart", "CATIA"),
        ("catia", "CATIA"),
        ("creo_part", "Creo"),
        ("nx_part", "NX"),
        ("pvz", "Creo View"),
        ("3dm", "Rhino"),
        ("prt", "unknown"),
        ("step", "unknown"),
        ("stp", "unknown"),
        ("iges", "unknown"),
        ("sat", "unknown"),
        ("future", "unknown"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let report = Prepared::new(
            "0123456789abcdef",
            format,
            "e2e-test-metadata",
            None,
            dir.path(),
        )
        .unwrap();
        let reloaded = Prepared::load(dir.path()).unwrap();
        for choice in ["summary", "bundle"] {
            let metadata = reloaded.metadata(choice, false);
            assert_eq!(metadata["source_app"], app, "{format}");
            assert_eq!(metadata["source_format"], format);
            assert_eq!(metadata["dutchman_version"], crate::BUILD_VERSION);
            assert_eq!(metadata["signature"], report.signature);
        }
    }
}

#[test]
fn debug_builds_never_contact_remote_report_service() {
    if cfg!(debug_assertions) {
        assert!(endpoint().is_none());
        let dir = tempfile::tempdir().unwrap();
        let report = Prepared::new("0123456789abcdef", "f3d", "decode", None, dir.path()).unwrap();
        assert!(submit(
            &report,
            dir.path(),
            "summary",
            false,
            true,
            Some("https://dutchman-reports.dutchman-cad.workers.dev/api/reports")
        )
        .is_err());
    }
}

#[test]
fn raw_size_is_not_the_compressed_upload_limit() {
    struct Large;
    impl BundleSource for Large {
        fn parts(&self) -> Result<Vec<BundlePart>, String> {
            Ok(vec![BundlePart {
                role: "synthetic repeated records".into(),
                bytes: vec![0; MAX_BUNDLE + 1],
            }])
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let report =
        Prepared::new("0123456789abcdef", "f3d", "check", Some(&Large), dir.path()).unwrap();
    assert!(report.bundle_size < MAX_BUNDLE);
    assert_eq!(report.preview()["bundle_sendable"], true);
}

#[test]
fn oversized_gzip_is_saved_for_private_send() {
    struct Incompressible;
    impl BundleSource for Incompressible {
        fn parts(&self) -> Result<Vec<BundlePart>, String> {
            let mut state = 0xabcdef1234567890u64;
            let bytes = (0..MAX_BUNDLE + 1024)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state as u8
                })
                .collect();
            Ok(vec![BundlePart {
                role: "Synthetic numeric records".into(),
                bytes,
            }])
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let report = Prepared::new(
        "0123456789abcdef",
        "f3d",
        "check",
        Some(&Incompressible),
        dir.path(),
    )
    .unwrap();
    assert!(report.bundle_size > MAX_BUNDLE);
    assert_eq!(report.preview()["bundle_sendable"], false);
    assert!(report.preview()["summary"]
        .as_str()
        .unwrap()
        .contains("25 MiB"));
    assert!(dir.path().join("bundle.json.gz").exists());
    assert!(submit(&report, dir.path(), "bundle", false, true, None).is_err());
    assert_eq!(
        submit(&report, dir.path(), "summary", false, true, None).unwrap()["sent"],
        false
    );
    eprintln!("Oversize synthetic gzip: {} bytes", report.bundle_size);
}

#[test]
fn multipart_limit_includes_framing_and_reserves_bundle_headroom() {
    let dir = tempfile::tempdir().unwrap();
    let mut report = Prepared::new("0123456789abcdef", "f3d", "check", None, dir.path()).unwrap();
    report.bundle_sha256 = Some("0".repeat(64));
    report.bundle_size = MAX_BUNDLE;
    assert!(report.can_send_bundle());
    assert!(report.request_size("bundle", false).unwrap() < MAX_REQUEST);
    assert!(report.request_size("bundle", true).unwrap() < MAX_REQUEST);
    report.bundle_size += 1;
    // Even when it would just fit today, leave the documented headroom.
    assert!(!report.can_send_bundle());
    assert!(submit(&report, dir.path(), "bundle", false, true, None)
        .unwrap_err()
        .contains("private-send"));
    assert!(!dir.path().join("submission.json").exists());
    report.bundle_size = MAX_REQUEST;
    assert!(report.request_size("bundle", false).unwrap() > MAX_REQUEST);
    assert_eq!(report.preview()["bundle_sendable"], false);
    assert!(report.preview()["summary"]
        .as_str()
        .unwrap()
        .contains("overhead"));
    let overhead = multipart_size(100, Some(0)).unwrap();
    assert_eq!(
        multipart_size(100, Some(MAX_REQUEST - overhead)),
        Some(MAX_REQUEST)
    );
    assert_eq!(
        multipart_size(100, Some(MAX_REQUEST - overhead + 1)),
        Some(MAX_REQUEST + 1)
    );
    assert!(multipart_size(usize::MAX, Some(0)).is_none());
}
