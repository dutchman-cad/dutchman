//! The running Dutchman Sync: what the tray menu and a second launch ask for, and the loop that
//! keeps the watched folders in step with the shared state file.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use dutchman::sync_state::{self, State};

use super::engine::Supervisor;

/// What the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Pause, or resume.
    TogglePause,
    /// Open the window (a second launch, or Add folder).
    ShowWindow,
    /// Open the Dutchman app.
    OpenApp,
    /// Stop: turn the folders off and exit.
    Quit,
}

/// What the controller does to the outside: windows, other programs, the tray's own text.
pub trait Shell {
    /// Opens the small window where a folder is chosen.
    fn show_window(&mut self);
    /// Opens the Dutchman app.
    fn open_app(&mut self);
    /// The tray menu says Resume (`true`) or Pause (`false`).
    fn show_paused(&mut self, paused: bool);
    /// Called once, when the loop ends.
    fn finished(&mut self) {}
}

/// How often the state file is read for changes made by the app or the window.
pub const POLL: Duration = Duration::from_millis(1000);

/// Runs until [`Msg::Quit`] (or the senders are gone). `state_file` is the shared state;
/// `background` is set when the system started the program at login: nothing opens then. When
/// it is not and no folder is on, the window opens so one can be chosen.
pub fn run(
    messages: &Receiver<Msg>,
    supervisor: &mut Supervisor,
    shell: &mut dyn Shell,
    state_file: &std::path::Path,
    background: bool,
) {
    let mut state = sync_state::read_from(state_file);
    shell.show_paused(state.paused);
    if !background && state.enabled().next().is_none() {
        shell.show_window();
    }
    supervisor.reconcile(&state);
    loop {
        match messages.recv_timeout(POLL) {
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => {
                supervisor.stop_all();
                // The app's switch shows off, as the user quit Sync.
                let _ = sync_state::update_at(state_file, State::disable_all);
                break;
            }
            Ok(Msg::TogglePause) => {
                let _ = sync_state::update_at(state_file, |state| state.paused = !state.paused);
            }
            Ok(Msg::ShowWindow) => shell.show_window(),
            Ok(Msg::OpenApp) => shell.open_app(),
            Err(RecvTimeoutError::Timeout) => {}
        }
        let latest: State = sync_state::read_from(state_file);
        if latest.paused != state.paused {
            shell.show_paused(latest.paused);
        }
        supervisor.reconcile(&latest);
        state = latest;
    }
    shell.finished();
}

#[cfg(test)]
mod tests {
    use super::super::engine::tests::{Call, Fake, Recorder};
    use super::super::engine::Supervisor;
    use super::*;
    use dutchman::sync_state::Folder;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Log {
        windows: usize,
        apps: usize,
        paused: Vec<bool>,
        finished: bool,
    }

    impl Shell for Log {
        fn show_window(&mut self) {
            self.windows += 1;
        }
        fn open_app(&mut self) {
            self.apps += 1;
        }
        fn show_paused(&mut self, paused: bool) {
            self.paused.push(paused);
        }
        fn finished(&mut self) {
            self.finished = true;
        }
    }

    fn supervisor(path: &std::path::Path) -> (Supervisor, std::sync::mpsc::Receiver<Call>) {
        let (calls, seen) = channel();
        let (auto_seen, _) = channel();
        let (notes, _) = channel();
        let engine = Arc::new(Fake {
            calls,
            script: Vec::new(),
            auto_seen,
        });
        let notifier = Arc::new(Recorder(Mutex::new(notes)));
        (
            Supervisor::new(engine, notifier, Some(path.to_path_buf())),
            seen,
        )
    }

    fn folder(output: &str, enabled: bool) -> Folder {
        Folder {
            output: output.into(),
            source: None,
            auto_update: true,
            enabled,
            last_checked: None,
        }
    }

    #[test]
    fn first_run_without_folders_opens_the_window_but_a_login_start_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(sync_state::FILE_NAME);
        for (background, windows) in [(false, 1), (true, 0)] {
            let (sender, receiver) = channel();
            sender.send(Msg::Quit).unwrap();
            let (mut supervisor, _) = supervisor(&file);
            let mut shell = Log::default();
            run(&receiver, &mut supervisor, &mut shell, &file, background);
            assert_eq!(shell.windows, windows);
            assert!(shell.finished);
        }
    }

    #[test]
    fn pause_resume_and_quit_go_through_the_shared_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(sync_state::FILE_NAME);
        let state = State {
            folders: vec![folder("/a", true), folder("/b", false)],
            ..State::default()
        };
        sync_state::write_to(&file, &state).unwrap();
        let (sender, receiver) = channel();
        let (mut supervisor, calls) = supervisor(&file);
        let thread_file = file.clone();
        let handle = std::thread::spawn(move || {
            let mut shell = Log::default();
            run(&receiver, &mut supervisor, &mut shell, &thread_file, false);
            (shell, supervisor.watching().len())
        });
        // Only the enabled folder is watched.
        assert_eq!(
            calls.recv_timeout(Duration::from_secs(5)).unwrap(),
            Call::Prepare("/a".into())
        );
        assert_eq!(
            calls.recv_timeout(Duration::from_secs(5)).unwrap(),
            Call::Watch("/a".into())
        );
        sender.send(Msg::TogglePause).unwrap();
        sender.send(Msg::ShowWindow).unwrap();
        sender.send(Msg::OpenApp).unwrap();
        // Resuming starts the folder again.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !sync_state::read_from(&file).paused {
            assert!(
                std::time::Instant::now() < deadline,
                "pause never reached the state file"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        sender.send(Msg::TogglePause).unwrap();
        assert_eq!(
            calls.recv_timeout(Duration::from_secs(5)).unwrap(),
            Call::Prepare("/a".into())
        );
        sender.send(Msg::Quit).unwrap();
        let (shell, watching) = handle.join().unwrap();
        assert_eq!((shell.windows, shell.apps, watching), (1, 1, 0));
        assert_eq!(shell.paused, [false, true, false]);
        let after = sync_state::read_from(&file);
        assert!(!after.paused);
        // Quitting turned every folder off, and kept them.
        assert_eq!(after.folders.len(), 2);
        assert!(after.folders.iter().all(|f| !f.enabled));
    }
}
