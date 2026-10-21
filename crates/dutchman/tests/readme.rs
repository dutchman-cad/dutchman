//! The README's download table names this release's archives.

use std::path::Path;

#[test]
fn the_readme_links_to_this_versions_downloads() {
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .unwrap();
    let start = readme.find("<!-- downloads -->").unwrap();
    let end = readme.find("<!-- /downloads -->").unwrap();
    let table = readme.get(start..end).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    for target in [
        "aarch64-apple-darwin.tar.gz",
        "x86_64-apple-darwin.tar.gz",
        "x86_64-pc-windows-msvc.zip",
        "x86_64-unknown-linux-gnu.tar.gz",
        "aarch64-unknown-linux-gnu.tar.gz",
    ] {
        let url = format!(
            "https://github.com/dutchman-cad/dutchman/releases/download/v{version}/dutchman-{version}-{target}"
        );
        assert!(
            table.contains(&format!("({url})")) && table.contains(&format!("({url}.sha256)")),
            "README's downloads do not link {url}; run scripts/downloads.py {version} README.md"
        );
    }
}
