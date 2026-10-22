//! The app carries its own copies of the logo files (a published crate holds only its own
//! folder); they must be the repository's artwork.

use std::path::Path;

#[test]
fn the_apps_logo_files_are_the_repositorys() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let logo = crate_dir.join("../../assets/logo");
    for name in [
        "dutchman-mark.svg",
        "dutchman-mark-night.svg",
        "dutchman-app-icon.svg",
    ] {
        let ours = std::fs::read(crate_dir.join("assets").join(name)).unwrap_or_default();
        let theirs = std::fs::read(logo.join(name)).unwrap_or_default();
        assert!(!ours.is_empty(), "{name} is missing from the app's assets");
        assert_eq!(ours, theirs, "{name} differs from assets/logo/{name}");
    }
}
