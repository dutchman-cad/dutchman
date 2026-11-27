//! One Dutchman Sync at a time. The running copy holds a lock file in the config folder that
//! names a loopback port it listens on; a second launch connects to that port, says "show",
//! and exits, and the running copy opens its window.
//!
//! The lock is the file's existence. A copy that died without removing it leaves a stale file,
//! which a launch recognises by the port not answering, and replaces.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// What the two ends say to each other.
const HELLO: &str = "dutchman-sync/1 show";
const REPLY: &str = "ok";
/// How long to wait for the other copy to answer, or for a lock file to be filled in.
const PATIENCE: Duration = Duration::from_millis(1500);

/// The lock file's name inside the config folder.
pub const LOCK_NAME: &str = "sync.lock";

/// What a launch found.
pub enum Launch {
    /// This is the only copy; hold the guard while running.
    First(Guard),
    /// Another copy is running and was asked to open its window.
    Running,
}

/// Holds the lock; dropping it removes the lock file.
pub struct Guard {
    lock: PathBuf,
    listener: TcpListener,
}

impl Guard {
    /// Listens for later launches in a thread of its own; each sends `()` on `show`.
    pub fn serve(&self, show: Sender<()>) -> std::io::Result<()> {
        let listener = self.listener.try_clone()?;
        std::thread::Builder::new()
            .name("sync-instance".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    if greeted(stream) && show.send(()).is_err() {
                        break;
                    }
                }
            })
            .map(|_| ())
    }

    /// The port later launches connect to.
    pub fn port(&self) -> u16 {
        self.listener.local_addr().map_or(0, |addr| addr.port())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock);
    }
}

/// Reads one greeting and answers it. Whether it was ours.
fn greeted(mut stream: TcpStream) -> bool {
    let _ = stream.set_read_timeout(Some(PATIENCE));
    let _ = stream.set_write_timeout(Some(PATIENCE));
    let mut line = String::new();
    // A greeting is one short line; never read more than that from a stranger.
    let mut reader = BufReader::new((&stream).take(128));
    let read = reader.read_line(&mut line);
    drop(reader);
    if read.is_err() || line.trim_end() != HELLO {
        return false;
    }
    writeln!(stream, "{REPLY}").is_ok()
}

/// Asks the copy listening on `port` to show its window. Whether it answered.
fn ask(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, PATIENCE) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(PATIENCE));
    let _ = stream.set_write_timeout(Some(PATIENCE));
    if writeln!(stream, "{HELLO}").is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply).is_ok() && reply.trim_end() == REPLY
}

/// The port a lock file names, once it is filled in.
fn locked_port(lock: &Path) -> Option<u16> {
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        let text = std::fs::read_to_string(lock).ok()?;
        if let Some(port) = text
            .lines()
            .next()
            .and_then(|line| line.trim().parse().ok())
        {
            return Some(port);
        }
        // Created a moment ago and not yet written: the other copy is still starting.
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Takes the lock in `dir`, or tells the copy that holds it to show its window.
pub fn launch(dir: &Path) -> std::io::Result<Launch> {
    std::fs::create_dir_all(dir)?;
    let lock = dir.join(LOCK_NAME);
    for _ in 0..4 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut file) => {
                let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
                    Ok(listener) => listener,
                    Err(e) => {
                        let _ = std::fs::remove_file(&lock);
                        return Err(e);
                    }
                };
                let port = listener.local_addr()?.port();
                if let Err(e) = writeln!(file, "{port}\n{}", std::process::id()) {
                    let _ = std::fs::remove_file(&lock);
                    return Err(e);
                }
                return Ok(Launch::First(Guard { lock, listener }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let seen = std::fs::read_to_string(&lock).ok();
                if locked_port(&lock).is_some_and(ask) {
                    return Ok(Launch::Running);
                }
                // Nobody answers: the copy that took the lock is gone. Remove its file, but
                // only if it is still the one that was seen not to answer (a copy that
                // started in the meantime has written another).
                if std::fs::read_to_string(&lock).ok() == seen {
                    let _ = std::fs::remove_file(&lock);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(
        "another Dutchman Sync holds the lock but does not answer",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn a_second_launch_signals_the_first_and_exits() {
        let dir = tempfile::tempdir().unwrap();
        let Launch::First(guard) = launch(dir.path()).unwrap() else {
            panic!("the first launch must take the lock");
        };
        let (show, shown) = channel();
        guard.serve(show).unwrap();
        assert!(matches!(launch(dir.path()).unwrap(), Launch::Running));
        shown.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(launch(dir.path()).unwrap(), Launch::Running));
        shown.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn a_stale_lock_is_replaced_and_dropping_the_guard_frees_it() {
        let dir = tempfile::tempdir().unwrap();
        // A port nothing listens on: bind one and let it go.
        let dead = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(dir.path().join(LOCK_NAME), format!("{dead}\n1\n")).unwrap();
        let Launch::First(guard) = launch(dir.path()).unwrap() else {
            panic!("a stale lock must not stop a launch");
        };
        assert!(dir.path().join(LOCK_NAME).is_file());
        drop(guard);
        assert!(!dir.path().join(LOCK_NAME).exists());
        assert!(matches!(launch(dir.path()).unwrap(), Launch::First(_)));
    }

    #[test]
    fn a_stranger_on_the_port_is_not_taken_for_a_running_copy() {
        let dir = tempfile::tempdir().unwrap();
        let stranger = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = stranger.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in stranger.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.write_all(b"hello\n");
            }
        });
        std::fs::write(dir.path().join(LOCK_NAME), format!("{port}\n")).unwrap();
        assert!(matches!(launch(dir.path()).unwrap(), Launch::First(_)));
    }
}
