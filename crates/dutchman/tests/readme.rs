//! The install page's download table links the latest published release's version-less files, so it
//! never points at a release before it is published.

use std::path::Path;

#[test]
fn the_install_page_links_the_latest_published_downloads() {
    let readme = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/install.md"),
    )
    .unwrap();
    let start = readme.find("<!-- downloads -->").unwrap();
    let end = readme.find("<!-- /downloads -->").unwrap();
    let table = readme.get(start..end).unwrap();
    assert!(
        !table.contains("/releases/download/v"),
        "Install page's downloads name a version; run scripts/downloads.py latest docs/install.md"
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
            "Install page's downloads do not link {url}; run scripts/downloads.py latest docs/install.md"
        );
    }
}

#[test]
fn the_readmes_format_listing_matches_the_command() {
    let formats = include_str!("../../../README.md");
    let listing = formats
        .split_once("<!-- formats: generated from dutchman formats -->")
        .unwrap()
        .1
        .split_once("<!-- /formats -->")
        .unwrap()
        .0;
    let lines = |text: &str| {
        text.trim()
            .lines()
            .filter(|line| cfg!(feature = "freecad") || !line.starts_with("| FreeCAD document |"))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(listing), lines(&dutchman::formats::markdown()));
}
