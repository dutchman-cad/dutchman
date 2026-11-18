//! Synthetic credentials and subprocess checks; no external requests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use dutchman::onshape::{BearerToken, Credentials, ErrorKind, FetchAuth, FileBackend, Store};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const TOKEN: &str = "synthetic-oauth-token_123+/=";
const URL: &str = "https://cad.onshape.com/documents/111111111111111111111111/v/222222222222222222222222/e/333333333333333333333333";

fn store(pairs: &[(&str, &str)]) -> Store {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Store::new(
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
        None,
        None,
    )
}

fn private_file(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn authentication_defaults_to_keys_and_refuses_all_mixed_sources() {
    let keys = [
        ("ONSHAPE_ACCESS_KEY", "synthetic-access"),
        ("ONSHAPE_SECRET_KEY", "synthetic-secret"),
    ];
    assert!(!FetchAuth::find(&store(&keys), None)
        .unwrap()
        .unwrap()
        .is_bearer());
    assert!(FetchAuth::find(&store(&[]), None).unwrap().is_none());
    assert!(
        FetchAuth::find(&store(&[("DUTCHMAN_ONSHAPE_BEARER_TOKEN", TOKEN)]), None)
            .unwrap()
            .unwrap()
            .is_bearer()
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("token");
    private_file(&path, &format!("{TOKEN}\n"));
    for api in [keys.to_vec(), vec![keys[0]], vec![keys[1]]] {
        for file in [false, true] {
            let mut pairs = api.clone();
            if !file {
                pairs.push(("DUTCHMAN_ONSHAPE_BEARER_TOKEN", TOKEN));
            }
            let error =
                FetchAuth::find(&store(&pairs), file.then_some(path.as_path())).unwrap_err();
            assert_eq!(error.kind, ErrorKind::Auth);
            assert!(error.message.contains("Choose either"));
            assert!(!format!("{error:?}").contains(TOKEN));
        }
    }
    // Saved keys conflict too, with both token sources.
    let saved = dir.path().join("keys");
    private_file(
        &saved,
        &Credentials::new(keys[0].1.into(), keys[1].1.into())
            .unwrap()
            .to_json(),
    );
    for file in [false, true] {
        let saved_store = Store::new(
            move |key| (!file && key == "DUTCHMAN_ONSHAPE_BEARER_TOKEN").then(|| TOKEN.into()),
            None,
            Some(Box::new(FileBackend {
                path: saved.clone(),
            })),
        );
        let error = FetchAuth::find(&saved_store, file.then_some(path.as_path())).unwrap_err();
        assert!(error.message.contains("Choose either"));
    }
    // The file takes precedence over the token environment variable.
    let selected = FetchAuth::find(
        &store(&[("DUTCHMAN_ONSHAPE_BEARER_TOKEN", "bad\ntoken")]),
        Some(&path),
    )
    .unwrap()
    .unwrap();
    assert!(selected.is_bearer());
    assert!(!format!("{selected:?}").contains(TOKEN));
}

#[test]
fn token_validation_and_file_errors_never_echo_credentials_or_paths() {
    for token in [
        String::new(),
        "bad\ntoken".into(),
        "bad\rheader".into(),
        "é".into(),
        "x".repeat(16 * 1024 + 1),
    ] {
        assert_eq!(BearerToken::new(token).unwrap_err().kind, ErrorKind::Auth);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private-token-file");
    let missing = BearerToken::from_file(&path).unwrap_err();
    assert!(!missing.to_string().contains("private-token-file"));
    private_file(&path, TOKEN);
    assert!(BearerToken::from_file(&path).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            BearerToken::from_file(&path).unwrap_err().kind,
            ErrorKind::Auth
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(BearerToken::from_file(&link).is_err());
    }
    private_file(&path, &"x".repeat(16 * 1024 + 1));
    assert!(BearerToken::from_file(&path).is_err());
}

fn command(program: &str, home: &std::path::Path) -> Command {
    let mut command = Command::new(program);
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ONSHAPE_ACCESS_KEY", "synthetic-access")
        .env("ONSHAPE_SECRET_KEY", "synthetic-secret")
        .env_remove("DUTCHMAN_ONSHAPE_BEARER_TOKEN");
    command
}

#[test]
fn cli_file_and_environment_conflicts_are_redacted_in_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("token");
    private_file(&token_file, TOKEN);
    for file in [false, true] {
        for structured in [false, true] {
            let mut child = command(env!("CARGO_BIN_EXE_dutchman"), dir.path());
            child.args(["onshape", "fetch", URL]);
            if file {
                child.arg("--bearer-token-file").arg(&token_file);
            } else {
                child.env("DUTCHMAN_ONSHAPE_BEARER_TOKEN", TOKEN);
            }
            if structured {
                child.arg("--json");
            }
            let output = child.output().unwrap();
            assert!(!output.status.success());
            let stdout = String::from_utf8(output.stdout).unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(!stdout.contains(TOKEN));
            assert!(!stderr.contains(TOKEN));
            if structured {
                let report: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(report["error"]["code"], "onshape/auth");
                assert_eq!(report["calls"], 0);
            } else {
                assert!(stderr.contains("Choose either"));
            }
        }
    }
    let output = command(env!("CARGO_BIN_EXE_dutchman"), dir.path())
        .args(["onshape", "fetch", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--bearer-token-file"));
    assert!(!help.contains("--bearer-token <"));
}

#[test]
fn mcp_exposes_token_file_and_redacts_conflicts_in_both_report_forms() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("token");
    private_file(&path, TOKEN);
    for file in [false, true] {
        let mut command = command(env!("CARGO_BIN_EXE_dutchman-mcp"), dir.path());
        if !file {
            command.env("DUTCHMAN_ONSHAPE_BEARER_TOKEN", TOKEN);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut args = json!({"document_url": URL});
        if file {
            args["bearer_token_file"] = json!(path);
        }
        let messages = [
            json!({"jsonrpc":"2.0", "id":1, "method":"tools/list", "params":{}}),
            json!({"jsonrpc":"2.0", "id":2, "method":"tools/call", "params":{"name":"onshape_fetch", "arguments":args}}),
        ];
        let mut stdin = child.stdin.take().unwrap();
        for message in messages {
            writeln!(stdin, "{message}").unwrap();
        }
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.contains(TOKEN));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
        let replies: Vec<Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let tool = replies[0]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "onshape_fetch")
            .unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["bearer_token_file"]["type"],
            "string"
        );
        let report = &replies[1]["result"]["structuredContent"];
        assert_eq!(report["error"]["code"], "onshape/auth");
        assert_eq!(report["calls"], 0);
        assert_eq!(replies[1]["result"]["isError"], true);
    }
}
