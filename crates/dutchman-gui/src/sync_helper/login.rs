//! Start at login: the entry that makes the system start Dutchman Sync when the user logs in.
//!
//! Windows: a value in the per-user Run key. macOS: a LaunchAgent plist in
//! `~/Library/LaunchAgents`. Linux: a `.desktop` file in `~/.config/autostart`. What each entry
//! holds and where it goes is worked out by [`entry`], which touches nothing; [`set`] applies it.

use std::path::{Path, PathBuf};

/// The program's name as the system lists it (the Run key's value name, the entry's name).
pub const NAME: &str = "Dutchman Sync";
/// The LaunchAgent's label, and its file's name without `.plist`.
pub const MAC_LABEL: &str = "io.github.dutchman-cad.dutchman-sync";
/// The autostart file's name on Linux.
pub const LINUX_FILE: &str = "dutchman-sync.desktop";
/// The Run key, under HKEY_CURRENT_USER.
pub const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
/// Given to the program when the system starts it: no window, unless there is nothing to run.
pub const BACKGROUND_ARG: &str = "--background";

/// The systems that have a login entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    /// The system this was built for.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// What a login entry is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A file with these contents at this path.
    File { path: PathBuf, contents: String },
    /// A string value in the Run key.
    Registry { name: String, command: String },
}

/// The command that starts Dutchman Sync: its program, then its arguments. Inside an AppImage
/// that is the AppImage file with `--run dutchman-sync`, as the mounted copy goes away.
pub fn command(exe: &Path, appimage: Option<&Path>) -> Vec<String> {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    match appimage {
        Some(image) => vec![
            text(image),
            "--run".to_owned(),
            "dutchman-sync".to_owned(),
            BACKGROUND_ARG.to_owned(),
        ],
        None => vec![text(exe), BACKGROUND_ARG.to_owned()],
    }
}

/// The login entry for `command` on `os`. `home` is the user's home folder, and `config_home`
/// the folder `~/.config` stands for (`$XDG_CONFIG_HOME`).
pub fn entry(os: Os, command: &[String], home: &Path, config_home: &Path) -> Entry {
    match os {
        Os::Windows => Entry::Registry {
            name: NAME.to_owned(),
            command: command
                .iter()
                .map(|part| windows_quote(part))
                .collect::<Vec<_>>()
                .join(" "),
        },
        Os::MacOs => Entry::File {
            path: home
                .join("Library")
                .join("LaunchAgents")
                .join(format!("{MAC_LABEL}.plist")),
            contents: launch_agent(command),
        },
        Os::Linux => Entry::File {
            path: config_home.join("autostart").join(LINUX_FILE),
            contents: autostart_desktop(command),
        },
    }
}

/// A command-line word as Windows parses it: quoted when it has a space, quotes escaped.
fn windows_quote(word: &str) -> String {
    if !word.is_empty() && !word.contains([' ', '\t', '"']) {
        return word.to_owned();
    }
    format!("\"{}\"", word.replace('"', "\\\""))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn launch_agent(command: &[String]) -> String {
    let arguments: String = command
        .iter()
        .map(|part| format!("    <string>{}</string>\n", xml_escape(part)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>Label</key>\n  <string>{MAC_LABEL}</string>\n\
  <key>ProgramArguments</key>\n  <array>\n{arguments}  </array>\n\
  <key>RunAtLoad</key>\n  <true/>\n\
  <key>ProcessType</key>\n  <string>Interactive</string>\n\
</dict>\n\
</plist>\n"
    )
}

/// A word of an `Exec=` line, quoted as the Desktop Entry specification asks: in double quotes
/// when it has anything but plain characters, with `"`, `` ` ``, `$` and `\` escaped, and `%`
/// doubled (in the file, each backslash is doubled again).
fn desktop_quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:+=@,".contains(c));
    if plain {
        return word.to_owned();
    }
    let mut quoted = String::from("\"");
    for c in word.chars() {
        match c {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

fn autostart_desktop(command: &[String]) -> String {
    let exec = command
        .iter()
        .map(|part| desktop_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name={NAME}\n\
Comment=Keeps converted designs up to date with their source files\n\
Exec={exec}\n\
Icon=dutchman-sync\n\
Terminal=false\n\
X-GNOME-Autostart-enabled=true\n"
    )
}

/// The `reg` arguments that add (or remove) the Run value.
pub fn registry_args(add: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    match add {
        Some(command) => {
            args.extend([
                "add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", command, "/f",
            ]);
        }
        None => args.extend(["delete", RUN_KEY, "/v", NAME, "/f"]),
    }
    args.into_iter().map(str::to_owned).collect()
}

/// Writes (`on`) or removes (`!on`) the entry. A missing entry is already removed.
pub fn apply(entry: &Entry, on: bool) -> Result<(), String> {
    match entry {
        Entry::File { path, contents } => {
            if on {
                let dir = path.parent().ok_or("the login entry has no folder")?;
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
                std::fs::write(path, contents)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))
            } else {
                match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        Err(format!("cannot remove {}: {e}", path.display()))
                    }
                    _ => Ok(()),
                }
            }
        }
        Entry::Registry { command, .. } => {
            let args = registry_args(on.then_some(command.as_str()));
            let mut reg = std::process::Command::new("reg");
            reg.args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                // No console window flashing up behind the app.
                std::os::windows::process::CommandExt::creation_flags(&mut reg, 0x0800_0000);
            }
            match reg.status() {
                Ok(status) if status.success() || !on => Ok(()),
                Ok(status) => Err(format!("reg failed: {status}")),
                Err(e) => Err(format!("cannot run reg: {e}")),
            }
        }
    }
}

/// Turns Start at login on or off for this user and this program.
pub fn set(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the program: {e}"))?;
    let appimage = std::env::var_os("APPIMAGE")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or("cannot find the home folder")?;
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let command = command(&exe, appimage.as_deref());
    apply(&entry(Os::current(), &command, &home, &config_home), on)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe(path: &str) -> Vec<String> {
        command(Path::new(path), None)
    }

    #[test]
    fn linux_entry_is_an_autostart_desktop_file() {
        let Entry::File { path, contents } = entry(
            Os::Linux,
            &exe("/opt/dutchman sync/dutchman-sync"),
            Path::new("/home/a"),
            Path::new("/home/a/.config"),
        ) else {
            panic!("a file");
        };
        assert_eq!(
            path,
            Path::new("/home/a/.config/autostart/dutchman-sync.desktop")
        );
        assert!(contents.starts_with("[Desktop Entry]\nType=Application\nName=Dutchman Sync\n"));
        assert!(contents.contains("Exec=\"/opt/dutchman sync/dutchman-sync\" --background\n"));
        assert!(contents.contains("Terminal=false"));
    }

    #[test]
    fn linux_appimage_starts_through_its_own_file() {
        let words = command(
            Path::new("/tmp/.mount_x/usr/bin/dutchman-sync"),
            Some(Path::new("/h/D.AppImage")),
        );
        let Entry::File { contents, .. } =
            entry(Os::Linux, &words, Path::new("/h"), Path::new("/h/.config"))
        else {
            panic!("a file");
        };
        assert!(contents.contains("Exec=/h/D.AppImage --run dutchman-sync --background\n"));
    }

    #[test]
    fn desktop_exec_escapes_what_the_specification_says() {
        assert_eq!(desktop_quote("plain-1.0/x"), "plain-1.0/x");
        assert_eq!(desktop_quote("a b"), "\"a b\"");
        assert_eq!(desktop_quote("$x"), "\"\\\\$x\"");
        assert_eq!(desktop_quote("100%"), "\"100%%\"");
    }

    #[test]
    fn mac_entry_is_a_launch_agent_that_runs_at_load() {
        let Entry::File { path, contents } = entry(
            Os::MacOs,
            &exe("/Applications/Dutchman Sync.app/Contents/MacOS/DutchmanSync"),
            Path::new("/Users/a"),
            Path::new("/Users/a/.config"),
        ) else {
            panic!("a file");
        };
        assert_eq!(
            path,
            Path::new("/Users/a/Library/LaunchAgents/io.github.dutchman-cad.dutchman-sync.plist")
        );
        assert!(contents.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(contents.contains(
            "<string>/Applications/Dutchman Sync.app/Contents/MacOS/DutchmanSync</string>\n    <string>--background</string>"
        ));
        assert!(contents.contains(&format!("<string>{MAC_LABEL}</string>")));
    }

    #[test]
    fn windows_entry_is_a_run_value_with_the_path_quoted() {
        let Entry::Registry { name, command } = entry(
            Os::Windows,
            &exe(r"C:\Users\A B\AppData\Local\Programs\Dutchman\dutchman-sync.exe"),
            Path::new(r"C:\Users\A B"),
            Path::new(r"C:\Users\A B\.config"),
        ) else {
            panic!("a registry value");
        };
        assert_eq!(name, "Dutchman Sync");
        assert_eq!(
            command,
            r#""C:\Users\A B\AppData\Local\Programs\Dutchman\dutchman-sync.exe" --background"#
        );
        let add = registry_args(Some(&command));
        assert_eq!(add[..3], ["add", RUN_KEY, "/v"]);
        assert_eq!(
            registry_args(None),
            ["delete", RUN_KEY, "/v", "Dutchman Sync", "/f"]
        );
    }

    #[test]
    fn a_file_entry_is_written_and_removed_under_the_given_home() {
        let home = tempfile::tempdir().unwrap();
        let words = exe("/opt/dutchman-sync");
        let planned = entry(Os::Linux, &words, home.path(), &home.path().join(".config"));
        let Entry::File { path, contents } = &planned else {
            panic!("a file");
        };
        apply(&planned, true).unwrap();
        assert_eq!(&std::fs::read_to_string(path).unwrap(), contents);
        apply(&planned, false).unwrap();
        assert!(!path.exists());
        // Removing what is not there is not an error.
        apply(&planned, false).unwrap();
    }
}
