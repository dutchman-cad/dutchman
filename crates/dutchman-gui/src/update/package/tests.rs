use super::*;

#[test]
fn installed_locations_identify_the_actual_cask_formula_and_scoop() {
    assert_eq!(Install::detect(Path::new("/opt/homebrew/Caskroom/dutchman-convert/0.2.6/Dutchman Convert.app/Contents/MacOS/dutchman-gui")), Install::Cask("/opt/homebrew".into()));
    assert_eq!(
        Install::detect(Path::new(
            "/usr/local/Cellar/dutchman/0.2.6/bin/dutchman-gui"
        )),
        Install::Formula("/usr/local".into())
    );
    assert_eq!(
        Install::detect(Path::new(
            "C:\\Users\\Owner\\scoop\\apps\\dutchman\\0.2.6\\dutchman-gui.exe"
        )),
        Install::Scoop("C:/Users/Owner/scoop/shims".into())
    );
    assert_eq!(
        Install::detect(Path::new(
            "/tmp/Caskroom/something-else/Dutchman Convert.app"
        )),
        Install::Direct
    );
    assert_eq!(
        Install::detect(Path::new(
            "/Applications/Dutchman Convert.app/Contents/MacOS/dutchman-gui"
        )),
        Install::Direct
    );
    assert!(Install::Cask("/opt/homebrew".into())
        .command()
        .unwrap()
        .contains("--cask dutchman-cad/tap/dutchman-convert"));
}

#[test]
fn package_versions_are_bounded_stable_and_do_not_evaluate_ruby() {
    let cask = Install::Cask("/opt/homebrew".into());
    let formula = Install::Formula("/opt/homebrew".into());
    let scoop = Install::Scoop("shims".into());
    for (kind, input) in [
        (&cask, "cask \"dutchman-convert\" do\n  version \"0.2.6\"\nend"),
        (&formula, "class Dutchman < Formula\n  version '0.2.6'\nend"),
        (&formula, "url \"https://github.com/dutchman-cad/dutchman/releases/download/v0.2.6/dutchman.tar.gz\""),
        (&formula, "url \"https://github.com/dutchman-cad/dutchman/archive/refs/tags/v0.2.6.tar.gz\""),
        (&scoop, "{\"version\":\"0.2.6\",\"url\":\"unused\"}"),
    ] { assert_eq!(parse_version(kind, input.as_bytes()).unwrap(), "v0.2.6"); }
    for kind in [&cask, &formula, &scoop] {
        for garbage in [
            b"".as_slice(),
            b"garbage",
            b"\xff",
            b"version :latest",
            b"version \"0.2.6",
            b"version \"0.2.6-beta\"",
            b"{\"version\":\"nightly\"}",
            b"version \"#{system('do not run')}\"",
        ] {
            assert!(parse_version(kind, garbage).is_err());
        }
        assert!(parse_version(kind, &vec![b'x'; MAX_BYTES + 1]).is_err());
    }
}

#[test]
fn injected_command_runner_transitions_and_preserves_only_the_tail() {
    for failure in [
        None,
        Some("Command was not found"),
        Some("Exited with code 1"),
        Some("Update timed out after 15 minutes"),
    ] {
        let progress = Arc::new(Mutex::new(Progress {
            phase: Phase::Running,
            tail: VecDeque::new(),
        }));
        let state = progress.clone();
        let output: Output = Arc::new(move |line| {
            if let Ok(mut p) = state.lock() {
                p.line(line);
            }
        });
        let specs = vec![
            Spec::new("test-brew", &["update"]),
            Spec::new(
                "test-brew",
                &["upgrade", "--cask", "dutchman-cad/tap/dutchman-convert"],
            ),
        ];
        let mut calls = Vec::new();
        execute(
            &progress,
            Ok(specs),
            |spec, _, output| {
                calls.push(spec.clone());
                for n in 0..30 {
                    output(format!("output {n}"));
                }
                failure.map_or(Ok(()), |message| Err(message.to_owned()))
            },
            output,
        );
        let progress = progress.lock().unwrap();
        assert_eq!(progress.tail.len(), 12);
        assert_eq!(progress.tail.back().unwrap(), "output 29");
        match failure {
            None => {
                assert_eq!(progress.phase, Phase::Installed);
                assert_eq!(calls.len(), 2);
            }
            Some(error) => {
                assert_eq!(progress.phase, Phase::Failed(error.into()));
                assert_eq!(calls.len(), 1);
            }
        }
        assert!(Install::Cask("/opt/homebrew".into()).command().is_some());
    }
}

#[test]
fn restart_uses_stable_paths_and_argument_lists() {
    let cask = Install::Cask("/opt/homebrew".into()).restart().unwrap();
    assert_eq!(cask.program, PathBuf::from("/usr/bin/open"));
    assert_eq!(
        cask.args,
        ["-n", "/Applications/Dutchman Convert.app"].map(OsString::from)
    );
    let formula = Install::Formula("/home/linuxbrew/.linuxbrew".into())
        .restart()
        .unwrap();
    assert_eq!(
        formula.program,
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/dutchman-gui")
    );
    let scoop = Install::Scoop("C:/Users/Owner/scoop/shims".into())
        .restart()
        .unwrap();
    assert_eq!(
        scoop.program,
        PathBuf::from("C:/Users/Owner/scoop/shims/dutchman-gui.exe")
    );
    let mut upgrade = Spec::new("brew", &["upgrade", "dutchman-cad/tap/dutchman"]);
    upgrade.no_auto_update = true;
    assert!(upgrade
        .command()
        .get_envs()
        .any(|(key, value)| key == "HOMEBREW_NO_AUTO_UPDATE"
            && value == Some(std::ffi::OsStr::new("1"))));
}
