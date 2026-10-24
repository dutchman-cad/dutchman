//! Discovery and process dispatch through the standalone front command.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::process::Command;

static EXECUTABLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn executable_guard() -> std::sync::MutexGuard<'static, ()> {
    EXECUTABLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn version_identifies_the_launcher() {
    let _guard = executable_guard();
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_dutchman-launcher"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("dutchman launcher {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn invalid_names_are_refused() {
    let _guard = executable_guard();
    for name in [
        "",
        "../x",
        "-x",
        "a/b",
        "a\\b",
        ".foo",
        "Upper",
        "a_b",
        &"a".repeat(65),
    ] {
        assert!(dutchman_launcher::find_tool(name).is_none(), "{name}");
    }
}

#[test]
fn unknown_tool_has_install_message() {
    let _guard = executable_guard();
    let out = Command::new(env!("CARGO_BIN_EXE_dutchman-launcher"))
        .arg("unknown-tool-for-test")
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8(out.stderr).unwrap(), "dutchman: `dutchman unknown-tool-for-test` is not installed; get it at https://github.com/dutchman-cad/unknown-tool-for-test\n");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "dutchman-launcher-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("dutchman-{name}"));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    fn copy_launcher(dir: &Path) -> PathBuf {
        let path = dir.join("dutchman");
        std::fs::copy(env!("CARGO_BIN_EXE_dutchman-launcher"), &path).unwrap();
        path
    }

    #[test]
    fn dispatch_preserves_arguments_stdin_and_exit_code() {
        let _guard = executable_guard();
        let dir = Temp::new();
        script(
            &dir.0,
            "foo",
            "printf '<%s>\\n' \"$@\"; IFS= read -r line; printf 'stdin:%s\\n' \"$line\"; exit 7",
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman-launcher"))
            .args(["foo", "a", "b c"])
            .env("PATH", &dir.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"hello world\n")
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(out.stdout, b"<a>\n<b c>\nstdin:hello world\n");
    }

    #[test]
    fn adjacent_tool_wins_and_help_lists_versions_sorted_once() {
        let _guard = executable_guard();
        let adjacent = Temp::new();
        let path = Temp::new();
        let launcher = copy_launcher(&adjacent.0);
        script(&adjacent.0, "foo", "echo adjacent-1.0");
        script(&path.0, "foo", "echo path-2.0");
        script(&path.0, "aaa", "echo aaa-3.0");
        script(&path.0, "launcher", "echo excluded");
        let disabled = script(&path.0, "disabled", "echo disabled");
        std::fs::set_permissions(disabled, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::create_dir(path.0.join("dutchman-directory")).unwrap();
        script(&path.0, "Upper", "echo excluded");
        let out = Command::new(&launcher)
            .arg("foo")
            .env("PATH", &path.0)
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"adjacent-1.0\n");
        for args in [vec![], vec!["--help"], vec!["-h"]] {
            let out = Command::new(&launcher)
                .args(args)
                .env("PATH", &path.0)
                .output()
                .unwrap();
            assert!(out.status.success());
            let text = String::from_utf8(out.stdout).unwrap();
            assert!(text.contains("dutchman <tool> [args...]"));
            assert!(text.contains("aaa  aaa-3.0\n  foo  adjacent-1.0"), "{text}");
            for absent in ["path-2.0", "disabled", "excluded", "directory"] {
                assert!(!text.contains(absent), "{text}");
            }
            assert!(text.contains("https://github.com/dutchman-cad/dutchman"));
        }
    }

    #[test]
    fn relative_path_entries_work() {
        let _guard = executable_guard();
        let dir = Temp::new();
        std::fs::create_dir(dir.0.join("bin")).unwrap();
        script(&dir.0.join("bin"), "foo", "echo relative");
        let out = Command::new(env!("CARGO_BIN_EXE_dutchman-launcher"))
            .current_dir(&dir.0)
            .env("PATH", "bin")
            .arg("foo")
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"relative\n");
    }

    #[test]
    fn version_reads_only_the_trimmed_first_line() {
        let _guard = executable_guard();
        let dir = Temp::new();
        let path = script(&dir.0, "foo", "printf '  foo 1.0  \\nsecond line\\n'");
        for _ in 0..20 {
            assert_eq!(
                dutchman_launcher::tool_version(&path).as_deref(),
                Some("foo 1.0")
            );
        }
        let path = script(&dir.0, "foo", "echo failed; exit 1");
        assert!(dutchman_launcher::tool_version(&path).is_none());
        assert!(dutchman_launcher::tool_version(&dir.0.join("missing")).is_none());
    }

    #[test]
    fn version_timeout_kills_and_reaps_the_child() {
        let _guard = executable_guard();
        let dir = Temp::new();
        let path = script(&dir.0, "foo", "while :; do :; done");
        let start = std::time::Instant::now();
        assert!(dutchman_launcher::tool_version(&path).is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn non_utf8_arguments_pass_through() {
        let _guard = executable_guard();
        use std::os::unix::ffi::OsStringExt;
        let dir = Temp::new();
        script(&dir.0, "foo", "printf '%s' \"$1\"");
        let out = Command::new(env!("CARGO_BIN_EXE_dutchman-launcher"))
            .arg("foo")
            .arg(std::ffi::OsString::from_vec(vec![0xff, b'a']))
            .env("PATH", &dir.0)
            .output()
            .unwrap();
        assert_eq!(out.stdout, [0xff, b'a']);
    }

    #[test]
    fn exec_errors_are_reported() {
        let _guard = executable_guard();
        let dir = Temp::new();
        script(&dir.0, "foo", "exit 0");
        std::fs::write(dir.0.join("dutchman-foo"), b"#!/nonexistent/interpreter\n").unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_dutchman-launcher"))
            .arg("foo")
            .env("PATH", &dir.0)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("cannot run"));
    }
}
