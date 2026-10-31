//! The README's download table links the latest published release's version-less files, so it
//! never points at a release before it is published.

use std::path::Path;

#[test]
fn the_readme_links_the_latest_published_downloads() {
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .unwrap();
    let start = readme.find("<!-- downloads -->").unwrap();
    let end = readme.find("<!-- /downloads -->").unwrap();
    let table = readme.get(start..end).unwrap();
    assert!(
        !table.contains("/releases/download/v"),
        "README's downloads name a version; run scripts/downloads.py latest README.md"
    );
    for target in [
        "macos-universal.dmg",
        "macos-universal.tar.gz",
        "x86_64-pc-windows-msvc.zip",
        "x86_64-unknown-linux-gnu.tar.gz",
        "aarch64-unknown-linux-gnu.tar.gz",
    ] {
        let url = format!(
            "https://github.com/dutchman-cad/dutchman/releases/latest/download/dutchman-{target}"
        );
        assert!(
            table.contains(&format!("({url})")) && table.contains(&format!("({url}.sha256)")),
            "README's downloads do not link {url}; run scripts/downloads.py latest README.md"
        );
    }
}

#[test]
fn the_readmes_format_table_matches_the_command() {
    let readme = include_str!("../../../README.md");
    let table = readme
        .split_once("<!-- formats: generated from dutchman formats --markdown -->")
        .unwrap()
        .1
        .split_once("<!-- /formats -->")
        .unwrap()
        .0;
    // Ignore padding, and the FreeCAD row in a build without FreeCAD.
    let rows = |text: &str| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| cfg!(feature = "freecad") || !line.starts_with("| FreeCAD document |"))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(table),
        rows(&dutchman::formats::markdown()),
        "Update README's formats table: dutchman formats --markdown"
    );
}

#[test]
fn macos_installation_has_complete_archive_steps_and_current_dmg_links() {
    let install = include_str!("../../../docs/install.md");
    for step in [
        "releases/latest/download",
        "curl -fLO \"$base/dutchman-macos-universal.tar.gz\"",
        "curl -fLO \"$base/dutchman-macos-universal.tar.gz.sha256\"",
        "shasum -a 256 -c dutchman-macos-universal.tar.gz.sha256",
        "tar xzf dutchman-macos-universal.tar.gz",
        "install -m 755 dutchman dutchman-mcp dutchman-gui",
        "xattr -dr com.apple.quarantine . 2>/dev/null || true",
        "dutchman --version",
    ] {
        assert!(install.contains(step), "missing macOS step: {step}");
    }
    let dmg = include_str!("../../../scripts/macos-dmg.sh");
    assert!(dmg.contains("docs/install.md#opening-the-app-the-first-time"));
    assert!(dmg.contains("docs/command-line.md"));
    assert!(!dmg.contains("docs/guide.md"));
    assert!(dmg.contains("2>/dev/null || true"));
}
