//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one. Songs of the built-in rulesets can be edited. The window asks this program to
//! read and write files and to show file dialogs.

use std::io::Write;
use std::path::{Path, PathBuf};

use charter_core::RulesetPlugin;
use charter_ui::{Request, Saved, Window};

/// The ruleset plugins built into charter.
const PLUGINS: &[&dyn RulesetPlugin] = &[
    &charter_stepmania::Plugin,
    &charter_ttr::Plugin,
    &charter_osu_mania::Plugin,
];

/// The editor window, and the files and dialogs it asks for.
struct Host {
    window: Window,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.window.ui(ui);
        for request in self.window.take_requests() {
            match request {
                Request::Write { path, bytes } => self.write(path, &bytes),
                Request::SaveAs { name, bytes } => {
                    match song_dialog().set_file_name(name).save_file() {
                        Some(path) => self.write(path, &bytes),
                        None => self.window.saved(Saved::Cancelled),
                    }
                }
                Request::Open => {
                    if let Some(path) = song_dialog().pick_file() {
                        self.read(&path);
                    }
                }
            }
        }
    }
}

impl Host {
    fn read(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => self.window.open(path, &bytes),
            Err(error) => self.window.refuse(path, &error),
        }
    }

    fn write(&mut self, path: PathBuf, bytes: &[u8]) {
        let saved = match write_whole(&path, bytes) {
            Ok(()) => Saved::To(path),
            Err(error) => Saved::Failed(format!("{}: {error}", path.display())),
        };
        self.window.saved(saved);
    }
}

/// Writes `bytes` beside `path`, and moves them over the file at `path` once they are on disk, so
/// neither a failed write nor a crash leaves the file half written.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".partial");
    let beside = PathBuf::from(beside);
    let written = write_then_move(&beside, path, bytes);
    if written.is_err() {
        // The write already failed; a partial file left over is the lesser problem.
        let _ = std::fs::remove_file(&beside);
    }
    written
}

fn write_then_move(beside: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(beside)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(beside, path)?;
    // The move itself is only on disk once the folder holding it is.
    #[cfg(unix)]
    {
        let folder = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty());
        std::fs::File::open(folder.unwrap_or(Path::new(".")))?.sync_all()?;
    }
    Ok(())
}

/// A native file dialog for charter songs.
fn song_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("charter song", &["json"])
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "charter",
        options,
        Box::new(|_| {
            let mut host = Host {
                window: Window::new(PLUGINS),
            };
            if let Some(path) = std::env::args_os().nth(1) {
                host.read(Path::new(&path));
            }
            Ok(Box::new(host))
        }),
    )
}
