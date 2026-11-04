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

#[test]
fn msi_identity_requires_both_the_user_location_and_installer_registration() {
    let local = Path::new("C:/Users/Owner Name/AppData/Local");
    let registered = Path::new("C:\\Users\\Owner Name\\AppData\\Local\\Programs\\Dutchman\\");
    let exe =
        Path::new("c:\\users\\owner name\\appdata\\local\\programs\\dutchman\\DUTCHMAN-GUI.EXE");
    assert_eq!(
        Install::detect_msi(exe, local, registered),
        Install::Msi(local.join("Programs/Dutchman"))
    );
    assert!(matches!(
        Install::detect_msi(
            Path::new(r"\\?\C:\Users\Owner Name\AppData\Local\Programs\Dutchman\dutchman-gui.exe"),
            local,
            registered
        ),
        Install::Msi(_)
    ));
    for (exe, registered) in [
        (exe, Path::new("")),
        (exe, Path::new("C:/Other/Programs/Dutchman")),
        (Path::new("C:/Downloads/dutchman-gui.exe"), registered),
        (
            Path::new("C:/Users/Other/AppData/Local/Programs/Dutchman/dutchman-gui.exe"),
            registered,
        ),
        (
            Path::new("C:/Users/Owner Name/AppData/Local/Programs/Dutchman-old/dutchman-gui.exe"),
            registered,
        ),
    ] {
        assert_eq!(Install::detect_msi(exe, local, registered), Install::Direct);
    }
}

#[test]
fn update_actions_follow_install_kind() {
    let msi = Install::Msi("C:/Local/Programs/Dutchman".into());
    assert_eq!(msi.label(), "Install (closes Dutchman)");
    for (install, action) in [
        (Install::Direct, Action::Download),
        (msi.clone(), Action::Msi),
        (Install::Zip("portable".into()), Action::Zip),
        (Install::Scoop("shims".into()), Action::Scoop),
        (Install::Cask("brew".into()), Action::Homebrew),
        (Install::Formula("brew".into()), Action::Homebrew),
    ] {
        assert_eq!(install.action(), action);
    }
    assert_eq!(Install::Direct.label(), "Download");
    assert_eq!(Install::Zip("portable".into()).label(), "Update now");
    assert!(msi.source().is_none());
    assert!(msi.command().is_none());
    assert!(plan(&msi).is_err()); // Windows updates must use the detached helper.
    assert_eq!(
        msi.restart().unwrap().program,
        PathBuf::from("C:/Local/Programs/Dutchman/dutchman-gui.exe")
    );
    assert_eq!(Install::Scoop("shims".into()).label(), "Update with Scoop");
    assert_eq!(Install::Cask("brew".into()).label(), "Update with Homebrew");
    assert!(Install::Direct.command().is_none());
    assert!(plan(&Install::Direct).is_err());
}

#[test]
fn portable_detection_requires_all_three_release_programs() {
    let folder = tempfile::tempdir().unwrap();
    let exe = folder.path().join("dutchman-gui.exe");
    for name in ["dutchman-gui.exe", "dutchman.exe"] {
        std::fs::write(folder.path().join(name), b"program").unwrap();
    }
    assert_eq!(Install::detect_zip(&exe), Install::Direct);
    std::fs::write(folder.path().join("dutchman-mcp.exe"), b"program").unwrap();
    assert_eq!(
        Install::detect_zip(&exe),
        Install::Zip(folder.path().to_owned())
    );
}
