#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const HELPER: &str = "share_scratch::tests::share_scratch_subprocess";
const SCENARIO: &str = "DUTCHMAN_TEST_SHARE_EXIT";

// Only the test harness accepts this mock transport; the shipped CLI has no network bypass.
#[test]
fn share_scratch_subprocess() {
    let Ok(scenario) = std::env::var(SCENARIO) else {
        return;
    };
    crate::quiet_when_stdout_closes();
    let scratch = Scratch::new().unwrap();
    let folder = tempfile::Builder::new()
        .prefix("dutchman-share-")
        .tempdir_in(scratch.path())
        .unwrap();
    let path = folder.path().join("synthetic.f3d");
    let mut file = std::fs::File::create(&path).unwrap();
    let address = std::env::var("DUTCHMAN_TEST_SHARE_MOCK").unwrap();
    let response = ureq::AgentBuilder::new()
        .try_proxy_from_env(false)
        .build()
        .get(&address)
        .timeout(Duration::from_secs(5))
        .call()
        .unwrap();
    let mut body = response.into_reader();
    let mut first = [0; 1];
    body.read_exact(&mut first).unwrap();
    file.write_all(&first).unwrap();
    println!("READY {}", scratch.path().display());
    std::io::stdout().flush().unwrap();
    if scenario == "interrupt" {
        // Remain inside a local mock download when the parent sends SIGINT.
        let _ = std::io::copy(&mut body, &mut file);
        panic!("The streaming child should have been interrupted");
    }
    // Wait until the parent has closed stdout, then finish the synthetic download.
    std::io::stdin().read_exact(&mut first).unwrap();
    std::io::copy(&mut body, &mut file).unwrap();
    drop(file);
    let output = std::env::var_os("DUTCHMAN_TEST_SHARE_OUTPUT").unwrap();
    std::fs::write(output, b"synthetic converted output").unwrap();
    match scenario.as_str() {
        "pipe" => {
            println!("{}", "Converted synthetic file\n".repeat(8192));
            std::io::stdout().flush().unwrap();
        }
        "exit" => exit(2),
        "error" => {
            // Model an ordinary error return while the scratch guard is still live.
            fn fail(_scratch: Scratch) -> Result<(), String> {
                Err("Synthetic conversion failure".into())
            }
            assert!(fail(scratch).is_err());
        }
        _ => panic!("Unknown test scenario"),
    }
}

fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Scratch cleanup subprocess timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn check(scenario: &str) {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("converted.sc");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (release, blocked) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut socket = crate::test_support::accept_blocking(&listener, deadline).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nx")
            .unwrap();
        let _ = blocked.recv_timeout(Duration::from_secs(10));
        let _ = socket.write_all(b"xxxx");
    });
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", HELPER, "--nocapture"])
        .env(SCENARIO, scenario)
        .env("DUTCHMAN_TEST_SHARE_MOCK", address)
        .env("DUTCHMAN_TEST_SHARE_OUTPUT", &output)
        .env("TMPDIR", root.path())
        .env("TMP", root.path())
        .env("TEMP", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut stdout = std::io::BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            if stdout.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if let Some((_, path)) = line.split_once("READY ") {
                ready.send(PathBuf::from(path.trim())).unwrap();
                break;
            }
        }
        stdout
    });
    let scratch = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(scratch.starts_with(root.path()));
    assert!(scratch.exists());
    let mut stdout = reader.join().unwrap();
    let drain = if scenario == "pipe" {
        drop(stdout);
        None
    } else {
        Some(std::thread::spawn(move || {
            let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        }))
    };
    if scenario == "interrupt" {
        #[cfg(unix)]
        assert!(Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success());
    } else {
        release.send(()).unwrap();
        child.stdin.take().unwrap().write_all(b"x").unwrap();
    }
    let status = wait(&mut child);
    if scenario == "interrupt" {
        let _ = release.send(());
    }
    server.join().unwrap();
    if let Some(drain) = drain {
        drain.join().unwrap();
    }
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(
        status.code(),
        Some(match scenario {
            "pipe" => 141,
            "interrupt" => 130,
            "exit" => 2,
            _ => 0,
        }),
        "{stderr}"
    );
    assert!(!scratch.exists(), "Scratch survived {scenario}: {stderr}");
    if scenario != "interrupt" {
        assert_eq!(
            std::fs::read(&output).unwrap(),
            b"synthetic converted output"
        );
    }
}

#[test]
fn share_scratch_cleans_broken_stdout_and_preserves_output() {
    check("pipe");
}

#[test]
fn share_scratch_cleans_intentional_exit_and_preserves_output() {
    check("exit");
}

#[test]
fn share_scratch_cleans_error_return_and_preserves_output() {
    check("error");
}

#[test]
#[cfg(unix)]
fn share_scratch_cleans_interrupt_during_streaming() {
    check("interrupt");
}
