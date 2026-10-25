//! Conversions, off the window's thread. A run plans its files and converts them with the
//! shared batch code `dutchman convert-all` uses, sending each step back as a message and
//! asking the window to repaint.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use dutchman::batch::{self, Event, Job};
use dutchman::ConvertOptions;
use eframe::egui;
use serde_json::Value;

/// What the background work tells the window.
#[derive(Debug)]
pub enum Message {
    /// Files and folders chosen in a dialog, for what it was opened for.
    Picked { pick: Pick, paths: Vec<PathBuf> },
    /// A run's plan: inputs that could not be planned (as failed reports), then its jobs.
    Planned {
        problems: Vec<Value>,
        jobs: Vec<Job>,
    },
    /// A job of the run started or finished.
    Progress(Event),
    /// The run is over: every job finished, or it was stopped.
    Done,
}

/// A run in progress.
pub struct Run {
    stop: Arc<AtomicBool>,
}

impl Run {
    /// Converts `inputs` into `out_dir` in the background.
    pub fn start(
        inputs: Vec<PathBuf>,
        out_dir: PathBuf,
        options: &ConvertOptions,
        messages: Sender<Message>,
        ctx: egui::Context,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let writer = options.writer();
        let options = options.clone();
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || {
            let send = |message: Message| {
                let _ = messages.send(message);
                ctx.request_repaint();
            };
            // A panic here must not take the window down; it ends the run.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let batch::Plan {
                    mut jobs, problems, ..
                } = batch::plan(&inputs, &out_dir);
                batch::prepare(&mut jobs, &options);
                send(Message::Planned {
                    problems,
                    jobs: jobs.clone(),
                });
                batch::run_each(&jobs, &writer, batch::workers(None), &flag, |event| {
                    send(Message::Progress(event));
                });
            }));
            send(Message::Done);
        });
        Run { stop }
    }

    /// Converts `job`, the run's job at `index`, again in the background, as it now says.
    pub fn retry(
        job: Job,
        index: usize,
        options: &ConvertOptions,
        messages: Sender<Message>,
        ctx: egui::Context,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let writer = options.writer();
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || {
            let send = |message: Message| {
                let _ = messages.send(message);
                ctx.request_repaint();
            };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                batch::run_each(&[job], &writer, 1, &flag, |event| {
                    send(Message::Progress(match event {
                        Event::Started(_) => Event::Started(index),
                        Event::Finished(_, report) => Event::Finished(index, report),
                    }));
                });
            }));
            send(Message::Done);
        });
        Run { stop }
    }

    /// Starts no further file; those converting finish.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Opens a file dialog in the background and sends what was picked.
pub fn pick(kind: Pick, messages: Sender<Message>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let dialog = rfd::AsyncFileDialog::new();
        let paths: Vec<PathBuf> = pollster::block_on(async {
            match kind {
                Pick::Files => dialog
                    .set_title("Add designs")
                    .add_filter(
                        "CAD designs",
                        &[
                            "f3d", "f3z", "ipt", "iam", "sldprt", "sldasm", "FCStd", "fcstd",
                            "CATPart", "catpart", "prt", "pvz", "3dm", "sat", "sab", "smt", "smb",
                            "step", "stp", "igs", "iges",
                        ],
                    )
                    .pick_files()
                    .await
                    .unwrap_or_default(),
                Pick::Folder => dialog
                    .set_title("Add a folder of designs")
                    .pick_folders()
                    .await
                    .unwrap_or_default(),
                Pick::Output => dialog
                    .set_title("Choose the output folder")
                    .pick_folder()
                    .await
                    .into_iter()
                    .collect(),
                Pick::Project => dialog
                    .set_title("Choose the project folder for assemblies' part files")
                    .pick_folder()
                    .await
                    .into_iter()
                    .collect(),
                Pick::SearchFolder(_) => dialog
                    .set_title("Choose a folder holding the missing part files")
                    .pick_folder()
                    .await
                    .into_iter()
                    .collect(),
                Pick::SaveLog(_) => dialog
                    .set_title("Save the full log")
                    .set_file_name("dutchman.log")
                    .add_filter("Log", &["log", "txt"])
                    .save_file()
                    .await
                    .into_iter()
                    .collect(),
                Pick::SearchFile(_) => dialog
                    .set_title("Find a missing part file")
                    .add_filter("Inventor parts and assemblies", &["ipt", "iam"])
                    .pick_file()
                    .await
                    .into_iter()
                    .collect(),
            }
            .into_iter()
            .map(|handle| handle.path().to_path_buf())
            .collect()
        });
        if !paths.is_empty() {
            let _ = messages.send(Message::Picked { pick: kind, paths });
            ctx.request_repaint();
        }
    });
}

/// What a file dialog picks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// Designs to convert.
    Files,
    Folder,
    /// The folder designs are written to.
    Output,
    /// The project folder an assembly's part files are searched in.
    Project,
    /// A folder to search for the part files the result row at this index is missing.
    SearchFolder(usize),
    /// One of those files, whose folder is then searched.
    SearchFile(usize),
    /// Where to save the full log of the result row at this index.
    SaveLog(usize),
}
