//! Where the Onshape API keys live: the system's credential store (Windows Credential
//! Manager, macOS Keychain, the Linux Secret Service), or where there is none, a private file
//! that Dutchman creates itself. Environment variables override both, for scripts and CI.
//! Keys are never logged, printed or put in an error.
use std::path::PathBuf;

use super::{Credentials, Error, ErrorKind, Result};

/// Said when no keys are configured anywhere, by every command that needs them.
pub const NOT_CONNECTED: &str = "Onshape is not connected. Run `dutchman onshape login`, or use Connect Onshape in the Dutchman app. Scripts can set ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY instead.";

const SERVICE: &str = "dutchman-onshape";
const ACCOUNT: &str = "api-keys";

/// Where a set of keys was found or saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Environment,
    SystemStore,
    PrivateFile,
}
impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Environment => "environment variables",
            Self::SystemStore => "the system credential store",
            Self::PrivateFile => "a private file in your Dutchman settings folder",
        }
    }
}

/// One place keys can be kept. Implementations must never log or print the keys.
pub trait Backend {
    /// The saved keys, or none when nothing is saved here.
    fn load(&self) -> Result<Option<Credentials>>;
    fn save(&self, credentials: &Credentials) -> Result<()>;
    /// Removes the saved keys; nothing saved is not an error.
    fn forget(&self) -> Result<()>;
}

fn unavailable() -> Error {
    Error::new(
        ErrorKind::Io,
        "Dutchman could not use the credential store on this computer.",
    )
}
fn write_failed() -> Error {
    Error::new(
        ErrorKind::Io,
        "Dutchman could not save the Onshape keys; check that your home folder is writable, then retry.",
    )
}

/// The system credential store.
pub struct SystemBackend;

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl SystemBackend {
    fn entry() -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| unavailable())
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl Backend for SystemBackend {
    fn load(&self) -> Result<Option<Credentials>> {
        match Self::entry()?.get_password() {
            Ok(text) => Credentials::from_json(text.as_bytes()).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(unavailable()),
        }
    }
    fn save(&self, credentials: &Credentials) -> Result<()> {
        Self::entry()?
            .set_password(&credentials.to_json())
            .map_err(|_| unavailable())
    }
    fn forget(&self) -> Result<()> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(unavailable()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl Backend for SystemBackend {
    fn load(&self) -> Result<Option<Credentials>> {
        Err(unavailable())
    }
    fn save(&self, _: &Credentials) -> Result<()> {
        Err(unavailable())
    }
    fn forget(&self) -> Result<()> {
        Err(unavailable())
    }
}

/// The private config file, `~/.config/dutchman/onshape.json`, readable only by its owner.
pub struct FileBackend {
    pub path: PathBuf,
}

impl Backend for FileBackend {
    fn load(&self) -> Result<Option<Credentials>> {
        match std::fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Credentials::from_file(&self.path).map(Some),
        }
    }

    #[cfg(unix)]
    fn save(&self, credentials: &Credentials) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        let dir = self.path.parent().ok_or_else(write_failed)?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|_| write_failed())?;
        // Write beside the file, owner-only from the first byte, then replace it in one step.
        let temp = self.path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&temp);
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file.write_all(credentials.to_json().as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
            return Err(write_failed());
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn save(&self, _: &Credentials) -> Result<()> {
        Err(unavailable())
    }

    fn forget(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(write_failed()),
        }
    }
}

type Env = Box<dyn Fn(&str) -> Option<String>>;

/// Finds, saves and forgets the Onshape keys. Lookup order: environment variables, then the
/// system credential store, then the private file.
pub struct Store {
    env: Env,
    system: Option<Box<dyn Backend>>,
    file: Option<Box<dyn Backend>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("system", &self.system.is_some())
            .field("file", &self.file.is_some())
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn new(
        env: impl Fn(&str) -> Option<String> + 'static,
        system: Option<Box<dyn Backend>>,
        file: Option<Box<dyn Backend>>,
    ) -> Self {
        Self {
            env: Box::new(env),
            system,
            file,
        }
    }

    /// The real environment, credential store and config file of this computer.
    pub fn system() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let file: Option<Box<dyn Backend>> = if cfg!(unix) {
            home.map(|home| {
                Box::new(FileBackend {
                    path: home.join(".config/dutchman/onshape.json"),
                }) as Box<dyn Backend>
            })
        } else {
            None
        };
        Self::new(
            |name| match std::env::var(name) {
                Ok(value) => Some(value),
                Err(std::env::VarError::NotPresent) => None,
                // Set, but unusable: report it as incomplete keys rather than skipping it.
                Err(std::env::VarError::NotUnicode(_)) => Some(String::new()),
            },
            Some(Box::new(SystemBackend)),
            file,
        )
    }

    /// The keys in use and where they came from, or none when nothing is configured.
    pub fn find(&self) -> Result<Option<(Credentials, Source)>> {
        match ((self.env)("ONSHAPE_ACCESS_KEY"), (self.env)("ONSHAPE_SECRET_KEY")) {
            (Some(access), Some(secret)) => {
                return Credentials::new(access, secret).map(|c| Some((c, Source::Environment)))
            }
            (None, None) => {}
            _ => {
                return Err(Error::new(
                    ErrorKind::Auth,
                    "Set ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY together, or remove both to use the keys saved by `dutchman onshape login`.",
                ))
            }
        }
        // A credential store that cannot be reached is the same as one with nothing saved.
        if let Some(Ok(Some(found))) = self.system.as_ref().map(|backend| backend.load()) {
            return Ok(Some((found, Source::SystemStore)));
        }
        match &self.file {
            Some(backend) => Ok(backend.load()?.map(|found| (found, Source::PrivateFile))),
            None => Ok(None),
        }
    }

    /// The keys in use, or the error that says how to connect.
    pub fn load(&self) -> Result<Credentials> {
        match self.find()? {
            Some((credentials, _)) => Ok(credentials),
            None => Err(Error::new(ErrorKind::Auth, NOT_CONNECTED)),
        }
    }

    /// Saves in the credential store, or in the private file where there is none.
    pub fn save(&self, credentials: &Credentials) -> Result<Source> {
        if let Some(system) = &self.system {
            if system.save(credentials).is_ok() {
                // One home for the keys: drop an older copy in the file.
                if let Some(file) = &self.file {
                    let _ = file.forget();
                }
                return Ok(Source::SystemStore);
            }
        }
        match &self.file {
            Some(file) => file.save(credentials).map(|()| Source::PrivateFile),
            None => Err(unavailable()),
        }
    }

    /// Removes the saved keys from every place Dutchman keeps them. Environment variables
    /// are the user's own and stay.
    pub fn forget(&self) -> Result<()> {
        let file = self.file.as_ref().map(|backend| backend.forget());
        if let Some(system) = &self.system {
            // A store that cannot be reached holds nothing of ours; one that still holds
            // the keys after a failed removal must say so.
            if let Err(error) = system.forget() {
                if matches!(system.load(), Ok(Some(_))) {
                    return Err(error);
                }
            }
        }
        match file {
            Some(Err(error)) => Err(error),
            _ => Ok(()),
        }
    }

    /// Whether both environment variables are set, so saved keys are not the ones in use.
    pub fn environment_overrides(&self) -> bool {
        (self.env)("ONSHAPE_ACCESS_KEY").is_some() && (self.env)("ONSHAPE_SECRET_KEY").is_some()
    }
}
