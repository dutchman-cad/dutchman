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
    assert_eq!(names, ["convert", "inspect", "formats", "convert_all"]);

    let formats = client.call("formats", json!({}));
    assert_eq!(formats["isError"], false);
    assert_eq!(formats["structuredContent"]["schema"], "dutchman.formats/1");

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
    assert!(out_dir.join("generated_unit_cube.solidcraft").is_file());
    assert!(out_dir.join("synthetic_cube.solidcraft").is_file());

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
