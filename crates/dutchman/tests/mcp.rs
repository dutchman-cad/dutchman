//! The `dutchman-mcp` binary spoken to over stdio, as an MCP client would: the handshake, the
//! tool list, and each tool called on a synthetic fixture.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and returns the response to it.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id, "{response}");
        response
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let response = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        response["result"].clone()
    }

    /// Closes stdin, as a client does when it is done, and waits for the server to exit.
    fn finish(mut self) {
        drop(self.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn an_mcp_client_converts_through_dutchman() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start();

    let init = client.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "dutchman");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    client.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let tools = client.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["convert", "inspect", "formats", "convert_all", "repro"]
    );

    let formats = client.call("formats", json!({}));
    assert_eq!(formats["isError"], false);
    assert_eq!(formats["structuredContent"]["schema"], "dutchman.formats/1");
    let rows = formats["structuredContent"]["formats"].as_array().unwrap();
    for (id, level) in [
        ("fusion_archive", json!("L5")),
        ("unclassified_part", Value::Null),
    ] {
        assert_eq!(
            rows.iter()
                .find(|row| row["id"] == id)
                .unwrap()
                .get("level"),
            Some(&level)
        );
    }

    let cube = fixture("generated_unit_cube.f3d");
    let inspected = client.call("inspect", json!({"input": cube}));
    assert_eq!(inspected["isError"], false, "{inspected}");
    let report = &inspected["structuredContent"];
    assert_eq!(report["schema"], "dutchman.inspect/1");
    assert_eq!(report["geometry"]["bodies"], 1);
    assert_eq!(report["conversion"]["possible"], true);
    // The text content is the same report, for clients that read only text.
    let text: Value =
        serde_json::from_str(inspected["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(&text, report);

    let output = dir.path().join("cube.solidcraft");
    let converted = client.call(
        "convert",
        json!({"input": cube, "output": output, "verify": false}),
    );
    assert_eq!(converted["isError"], false, "{converted}");
    assert_eq!(
        converted["structuredContent"]["schema"],
        "dutchman.convert/1"
    );
    assert!(output.is_file());
    // Nothing but the design was written.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

    let failed = client.call(
        "convert",
        json!({"input": cube, "output": dir.path().join("cube.step")}),
    );
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["structuredContent"]["ok"], false);

    // overwrite: false leaves an existing design alone, and says so.
    std::fs::write(&output, b"SENTINEL").unwrap();
    let kept = client.call(
        "convert",
        json!({"input": cube, "output": output, "verify": false, "overwrite": false}),
    );
    assert_eq!(kept["isError"], true, "{kept}");
    assert!(kept["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("already exists"));
    assert_eq!(std::fs::read(&output).unwrap(), b"SENTINEL");
    let schema = tools["result"]["tools"][0]["inputSchema"]["properties"].clone();
    assert!(schema["output"]["description"]
        .as_str()
        .unwrap()
        .contains("replaced, unless overwrite is false"));
    assert_eq!(schema["overwrite"]["type"], "boolean");

    let out_dir = dir.path().join("all");
    let all = client.call(
        "convert_all",
        json!({"inputs": [cube, fixture("synthetic_cube.ipt")], "out_dir": out_dir,
               "verify": false, "jobs": 2}),
    );
    assert_eq!(all["isError"], false, "{all}");
    assert_eq!(all["structuredContent"]["schema"], "dutchman.convert-all/1");
    assert_eq!(all["structuredContent"]["converted"], 2);
    assert!(out_dir.join("generated_unit_cube.sc").is_file());
    assert!(out_dir.join("synthetic_cube.sc").is_file());

    // The same batch, each file in a converter of its own, with an anonymous report.
    let anon = dir.path().join("anon.json");
    let journal = dir.path().join("journal.jsonl");
    let all = client.call(
        "convert_all",
        json!({"inputs": [cube], "out_dir": out_dir, "verify": false, "timeout": 300,
               "max_memory_mb": 4096, "journal": journal, "report_anon": anon}),
    );
    assert_eq!(all["isError"], false, "{all}");
    let salt = all["structuredContent"]["report_anon"]["salt"]
        .as_str()
        .unwrap();
    assert_eq!(salt.len(), 32);
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&anon).unwrap()).unwrap();
    assert_eq!(report["schema"], "dutchman.report-anon/1");
    assert_eq!(report["totals"]["converted"], 1);
    assert_eq!(report["run"]["max_memory_mb"], 4096);
    assert!(journal.is_file());

    client.finish();
}

#[test]
fn iges_tools_return_the_shared_refusal_without_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.iges");
    let output = dir.path().join("out.solidcraft");
    std::fs::write(&input, b"not IGES data").unwrap();
    let mut client = Client::start();
    for (tool, arguments) in [
        ("convert", json!({"input": input, "output": output})),
        ("inspect", json!({"input": input})),
        (
            "convert_all",
            json!({"inputs": [input], "out_dir": dir.path().join("out")}),
        ),
    ] {
        let result = client.call(tool, arguments);
        assert_eq!(result["isError"], true);
        let content = &result["structuredContent"];
        let report = if tool == "convert_all" {
            &content["results"][0]
        } else {
            content
        };
        assert_eq!(report["input"]["words"], "open it in SolidCraft directly");
        assert_eq!(
            report["error"]["message"],
            "Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import)."
        );
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(&text, content);
    }
    client.finish();
    assert!(!output.exists());
    assert!(!dir.path().join("out").join("design.solidcraft").exists());
}

#[test]
fn dry_run_estimates_without_decoding_or_writing_and_narrows_presets() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("synthetic.ipt");
    std::fs::write(&input, b"not a CAD design; pre-read only").unwrap();
    let output = dir.path().join("never-written.solidcraft");
    let mut client = Client::start();
    for preset in ["fast", "standard", "full"] {
        let reply = client.call(
            "convert",
            json!({"input": input, "output": output, "dry_run": true,
            "preset": preset, "history": false, "sketches": false, "joints": false}),
        );
        assert_eq!(reply["isError"], false, "{reply}");
        let report = &reply["structuredContent"];
        assert!(report["estimate"]["peak_bytes"].as_u64().unwrap() > 0);
        assert!(report["estimate"]["seconds"].as_u64().unwrap() > 0);
        assert!(report["estimate"]["band"].as_f64().unwrap() > 0.0);
        assert_eq!(
            report["choices"],
            json!({"preset": preset, "history": false, "sketches": false, "joints": false})
        );
        assert!(report["fits"].is_boolean() || report["fits"].is_null());
        assert!(!output.exists());
    }
    let reply = client.call("convert", json!({"input": input, "dry_run": true}));
    assert_eq!(
        reply["isError"], false,
        "output is optional for a dry run: {reply}"
    );
    let reply = client.call(
        "convert",
        json!({"input": input, "dry_run": true, "preset": "quick"}),
    );
    assert_eq!(reply["isError"], true);
    let reply = client.call(
        "convert",
        json!({"input": input, "dry_run": true, "history": "no"}),
    );
    assert_eq!(reply["isError"], true);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    client.finish();
}

#[test]
fn memory_refusals_and_dry_run_share_the_estimate_without_writing() {
    let Some(available) = dutchman::choices::available_memory() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("synthetic.step");
    std::fs::File::create(&input)
        .unwrap()
        .set_len(available.saturating_add(2_000_000_000))
        .unwrap();
    let output = dir.path().join("never-written.solidcraft");
    let out_dir = dir.path().join("never-created");
    let mut client = Client::start();
    let preview = client.call(
        "convert",
        json!({"input": input, "preset": "full", "dry_run": true}),
    );
    assert_eq!(preview["isError"], false);
    assert_eq!(preview["structuredContent"]["fits"], false);
    let converted = client.call(
        "convert",
        json!({"input": input, "output": output, "preset": "full"}),
    );
    let batch = client.call(
        "convert_all",
        json!({"inputs": [input], "out_dir": out_dir, "preset": "full"}),
    );
    for reply in [&converted, &batch] {
        assert_eq!(reply["isError"], true, "{reply}");
        let report = &reply["structuredContent"];
        assert!(report["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("this conversion needs about "));
        assert_eq!(report["estimate"], preview["structuredContent"]["estimate"]);
        assert_eq!(report["choices"], preview["structuredContent"]["choices"]);
    }
    assert_eq!(
        batch["structuredContent"]["results"][0]["error"],
        batch["structuredContent"]["error"]
    );
    assert!(!output.exists() && !out_dir.exists());
    client.finish();
}

#[test]
fn diagnostics_paths_are_optional_on_both_converting_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::start();
    let tools = client.request("tools/list", json!({}));
    for name in ["convert", "convert_all"] {
        let tool = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["diagnostics"]["type"],
            "string"
        );
        assert_eq!(
            tool["inputSchema"]["properties"]["diagnostics_default"]["type"],
            "boolean"
        );
    }
    for batch in [false, true] {
        let log = dir.path().join(format!("{batch}.jsonl"));
        let (name, args) = if batch {
            (
                "convert_all",
                json!({"inputs": [fixture("generated_unit_cube.f3d")],
                "out_dir": dir.path().join("batch"), "diagnostics": log, "verify": false, "jobs": 1}),
            )
        } else {
            (
                "convert",
                json!({"input": fixture("generated_unit_cube.f3d"),
                "output": dir.path().join("cube.solidcraft"), "diagnostics": log, "verify": false}),
            )
        };
        let result = client.call(name, args);
        assert_eq!(result["isError"], false, "{result}");
        let records: Vec<Value> = std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[0]["type"], "start");
        assert_eq!(records.last().unwrap()["type"], "end");
    }
    let result = client.call("convert", json!({"input": fixture("generated_unit_cube.f3d"),
        "output": dir.path().join("refused.solidcraft"), "diagnostics": dir.path().join("refused.jsonl"), "diagnostics_default": true}));
    assert_eq!(result["isError"], true);
    assert!(!dir.path().join("refused.jsonl").exists());
    client.finish();
}

#[test]
fn repro_returns_anonymous_content_and_only_explicitly_returns_the_local_key() {
    let mut client = Client::start();
    for include in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let result = client.call(
            "repro",
            json!({"input": fixture("generated_unit_cube.f3d"),
            "out": dir.path(), "include_local_key": include}),
        );
        assert_eq!(result["isError"], false, "{result}");
        let content = &result["structuredContent"];
        assert_eq!(content["anonymous"]["status"], "no_failure_to_reproduce");
        assert_eq!(content.get("local_key").is_some(), include);
        assert!(content["paths"]["local_key"]
            .as_str()
            .unwrap()
            .ends_with("local-key.json"));
    }
}
