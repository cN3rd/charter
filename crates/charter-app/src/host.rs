//! The native host for the editor window: file dialogs, disk I/O, playback and timing search.

use std::path::{Path, PathBuf};

use charter_ui::{Request, Saved, Window};

use crate::disk::Disk;
use crate::finder::Finder;
use crate::player::Player;
use crate::{SETTINGS, disk, is_copy_of, picture, project_file, song_dialog, stamp};

/// The editor window, and the files and dialogs it asks for.
pub(crate) struct Host {
    /// Editor UI that requests file and audio operations from this host.
    pub(crate) window: Window,
    /// Where the autosave is kept, when the user's data folder is known.
    pub(crate) recovery: Option<PathBuf>,
    /// Audio playback and loading for the open song.
    pub(crate) player: Player,
    /// Finds timing from the audio, and downloads the beat tracker it needs.
    pub(crate) finder: Finder,
    /// File operations, replaceable with in-memory storage in tests.
    pub(crate) disk: Box<dyn Disk>,
}

#[expect(
    clippy::missing_trait_methods,
    reason = "eframe's defaults for saving, clearing, exiting and input are what charter wants"
)]
impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        if self.player.update(&mut self.window) | self.finder.update(&mut self.window) {
            ui.ctx().request_repaint();
        }
        self.window.ui(ui);
        let requests = self.window.take_requests();
        self.handle_requests(requests);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(SETTINGS, self.window.settings().to_kept());
    }
}

impl Host {
    /// Handles UI requests and any requests they enqueue in turn.
    pub(crate) fn handle_requests(&mut self, mut requests: Vec<Request>) {
        while !requests.is_empty() {
            for request in requests {
                match request {
                    Request::SaveAs { name, bytes } => {
                        // The name the dialog suggests is the project's, which becomes a folder.
                        let name = name.strip_suffix(".json").unwrap_or(&name);
                        match song_dialog().set_file_name(name).save_file() {
                            Some(path) => self.save_as(&path, &bytes),
                            None => self.window.saved(Saved::Cancelled),
                        }
                    }
                    Request::Open => {
                        if let Some(path) = song_dialog().pick_file() {
                            self.read(&path);
                        }
                    }
                    Request::ChooseSnapshotFolder => {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            self.window.use_snapshot_folder(&folder);
                        }
                    }
                    Request::ChooseTimingFile => {
                        let file = rfd::FileDialog::new()
                            .add_filter("osu! beatmap", &["osu"])
                            .add_filter("text", &["txt"])
                            .pick_file();
                        if let Some(path) = file {
                            self.read_timing(&path);
                        }
                    }
                    Request::ChooseImport {
                        formats,
                        companions,
                    } => {
                        let file = filtered(rfd::FileDialog::new(), &formats).pick_file();
                        if let Some(path) = file {
                            self.read_import(&path, &companions);
                        }
                    }
                    Request::ChooseExport { name, formats } => {
                        let dialog = filtered(rfd::FileDialog::new(), &formats);
                        if let Some(path) = dialog.set_file_name(name).save_file() {
                            self.window.export_song(&path);
                        }
                    }
                    Request::ChooseAudio => {
                        let audio = rfd::FileDialog::new()
                            .add_filter("audio", &["ogg", "mp3", "wav", "flac"])
                            .pick_file();
                        if let Some(path) = audio {
                            self.window.use_audio(&path);
                        }
                    }
                    Request::ChooseArtwork => {
                        let file = rfd::FileDialog::new()
                            .add_filter("picture", &["png", "jpg", "jpeg"])
                            .pick_file();
                        if let Some(path) = file {
                            let picture = self.disk.read(&path).ok().and_then(picture);
                            self.window.use_artwork(&path, picture);
                        }
                    }
                    Request::FindTiming {
                        file,
                        ticks_per_beat,
                        tracker,
                    } => {
                        let onsets = self.window.onsets().map(<[_]>::to_vec);
                        self.finder
                            .find(file, ticks_per_beat, onsets, tracker, &mut self.window);
                    }
                    Request::DownloadWeights => self.finder.download(&mut self.window),
                    Request::FindTimingOnCpu => self.finder.find_on_cpu(&mut self.window),
                    Request::Play { .. }
                    | Request::Preview { .. }
                    | Request::Volume(_)
                    | Request::Pause
                    | Request::Seek(_)
                    | Request::Rate(_) => {
                        self.player.answer(&request, &mut self.window);
                    }
                    file_request @ (Request::Write { .. }
                    | Request::Autosave { .. }
                    | Request::CopyAudio { .. }
                    | Request::ReadTags { .. }
                    | Request::DecodeArtwork { .. }
                    | Request::ReadArtwork { .. }
                    | Request::OpenPath { .. }
                    | Request::Snapshot { .. }
                    | Request::WriteExport { .. }
                    | Request::DropAutosave) => self.answer(file_request),
                }
            }
            requests = self.window.take_requests();
        }
    }

    /// Does what the window asked of the files; dialogs and playback are the caller's.
    pub(crate) fn answer(&mut self, request: Request) {
        match request {
            Request::Write { path, bytes } => self.write(path, &bytes),
            Request::WriteExport {
                path,
                bytes,
                beside,
                copies,
            } => {
                let disk = self.disk.as_ref();
                if let Some(folder) = path.parent()
                    && let Err(error) = disk.make_folder(folder)
                {
                    return self.window.export_failed(folder, &error.to_string());
                }
                let written = std::iter::once((&path, &bytes))
                    .chain(beside.iter().map(|(place, made)| (place, made)))
                    .map(|(place, made)| (place, disk::write_whole(disk, place, made)))
                    .chain(
                        copies
                            .iter()
                            .map(|(from, to)| (to, disk::copy_over(disk, from, to))),
                    );
                for (place, result) in written {
                    if let Err(error) = result {
                        self.window.export_failed(place, &error.to_string());
                    }
                }
            }
            Request::Autosave { bytes } => self.autosave(&bytes),
            Request::CopyAudio { from, to } => {
                if let Err(error) = disk::copy_whole(self.disk.as_ref(), &from, &to) {
                    let why = format!("{} to {}: {error}", from.display(), to.display());
                    self.window.audio_not_copied(&why);
                }
            }
            Request::ReadTags { file } => {
                // A file that cannot be read has no tags, which leaves the song as it is.
                if let Ok(bytes) = self.disk.read(&file) {
                    let extension = file.extension().and_then(|extension| extension.to_str());
                    let tags = charter_audio::tags(bytes, extension);
                    self.window.tags_read(&file, tags.title, tags.artist);
                    if let Some(artwork) = tags.artwork
                        && let Some(found) = picture(artwork.bytes)
                    {
                        self.window
                            .artwork_embedded(&file, artwork.extension, found);
                    }
                }
            }
            Request::DecodeArtwork { bytes } => {
                let found = picture(bytes);
                self.window.artwork_restored(found);
            }
            Request::ReadArtwork { file } => {
                let found = self.disk.read(&file).ok().and_then(picture);
                self.window.artwork_read(&file, found);
            }
            Request::OpenPath { path } => self.read(&path),
            Request::Snapshot {
                name,
                bytes,
                folders,
                kept,
            } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs());
                self.snapshot(&name, &bytes, (&folders, kept), now);
            }
            Request::DropAutosave => {
                if let Some(recovery) = &self.recovery {
                    // Nothing to remove is fine, and a file left over is offered, then
                    // removed, on the next run.
                    drop(self.disk.remove(recovery));
                }
            }
            Request::SaveAs { .. }
            | Request::Open
            | Request::ChooseSnapshotFolder
            | Request::ChooseTimingFile
            | Request::ChooseImport { .. }
            | Request::ChooseExport { .. }
            | Request::ChooseAudio
            | Request::ChooseArtwork
            | Request::FindTiming { .. }
            | Request::DownloadWeights
            | Request::FindTimingOnCpu
            | Request::Play { .. }
            | Request::Preview { .. }
            | Request::Volume(_)
            | Request::Pause
            | Request::Seek(_)
            | Request::Rate(_) => {}
        }
    }

    /// Opens the song at `path`, or reports the read failure to the window.
    pub(crate) fn read(&mut self, path: &Path) {
        match self.disk.read(path) {
            Ok(bytes) => self.window.open(path, &bytes),
            Err(error) => self.window.refuse(path, &error),
        }
    }

    /// Writes the song and its accompanying assets, reporting the outcome to the window.
    pub(crate) fn write(&mut self, path: PathBuf, bytes: &[u8]) {
        let copied = match self.window.audio_copy_for(&path) {
            Some((from, to)) => disk::copy_whole(self.disk.as_ref(), &from, &to)
                .map_err(|error| format!("{} to {}: {error}", from.display(), to.display())),
            None => Ok(()),
        };
        let artwork = self.window.artwork_write_for(&path);
        // What the artwork's file held before, to put back when the song's own write fails.
        let before = if let Some((to, _)) = &artwork {
            let old = if self.disk.exists(to) {
                match self.disk.read(to) {
                    Ok(original_bytes) => Some(original_bytes),
                    Err(error) => {
                        self.window
                            .saved(Saved::Failed(format!("{}: {error}", to.display())));
                        return;
                    }
                }
            } else {
                None
            };
            Some((to, old))
        } else {
            None
        };
        let copied = copied.and_then(|()| match &artwork {
            Some((to, artwork_bytes)) => self.write_artwork(to, artwork_bytes),
            None => Ok(()),
        });
        let wrote_artwork = copied.is_ok();
        let saved = match copied.and_then(|()| {
            disk::write_whole(self.disk.as_ref(), &path, bytes)
                .map_err(|error| format!("{}: {error}", path.display()))
        }) {
            Ok(()) => Saved::To(path),
            Err(error) => {
                if wrote_artwork && let Some((to, old)) = before {
                    // Best effort: the save says it failed either way.
                    drop(match old {
                        Some(old) => disk::write_whole(self.disk.as_ref(), to, &old),
                        None => self.disk.remove(to),
                    });
                }
                Saved::Failed(error)
            }
        };
        self.window.saved(saved);
    }

    /// Writes `bytes` as a copy of the song `name` into each of `folders`, named for `now`, seconds
    /// since 1970, and removes the oldest copies of it there past `kept`. Says what fails.
    pub(crate) fn snapshot(
        &mut self,
        name: &str,
        bytes: &[u8],
        (folders, kept): (&[PathBuf], u32),
        now: u64,
    ) {
        let file = format!("{name} {}.json", stamp(now));
        for folder in folders {
            let written = self
                .disk
                .make_folder(folder)
                .and_then(|()| disk::write_whole(self.disk.as_ref(), &folder.join(&file), bytes));
            if let Err(error) = written {
                self.window
                    .snapshot_failed(&format!("{}: {error}", folder.display()));
                continue;
            }
            // Copies of this song alone, which a date sorts oldest first.
            let mut copies: Vec<PathBuf> = self
                .disk
                .list(folder)
                .unwrap_or_default()
                .into_iter()
                .filter(|path| {
                    let candidate = path.file_name().and_then(|file_name| file_name.to_str());
                    candidate.is_some_and(|file_name| is_copy_of(file_name, name))
                })
                .collect();
            copies.sort();
            let extra = copies
                .len()
                .saturating_sub(usize::try_from(kept).unwrap_or(usize::MAX));
            for old in copies.iter().take(extra) {
                // A copy left over is only a little disk.
                drop(self.disk.remove(old));
            }
        }
    }

    /// Writes the song's artwork to `to`, unless a different picture the song did not name is
    /// there already, which is left alone and says why.
    fn write_artwork(&self, to: &Path, bytes: &[u8]) -> Result<(), String> {
        let theirs = self.disk.exists(to)
            && self.disk.read(to).map_or(true, |kept| kept != bytes)
            && !self.window.may_replace(to);
        if theirs {
            let name = to
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            return Err(format!(
                "{} already holds another picture named {name}; give the artwork another name",
                to.parent()
                    .map_or_else(String::new, |folder| folder.display().to_string())
            ));
        }
        disk::write_whole(self.disk.as_ref(), to, bytes)
            .map_err(|error| format!("{}: {error}", to.display()))
    }

    /// Saves a song the user chose to save as `chosen` into a project folder named for it,
    /// where its audio is copied too and, later, anything worked out from it.
    pub(crate) fn save_as(&mut self, chosen: &Path, bytes: &[u8]) {
        let Some(file) = project_file(chosen) else {
            let why = format!("{} cannot name a song", chosen.display());
            self.window.saved(Saved::Failed(why));
            return;
        };
        let folder = file.parent().unwrap_or_else(|| Path::new(""));
        if file != chosen && self.disk.exists(&file) {
            let why = format!(
                "{} already holds a song. Choose that file to replace it, or another name",
                folder.display()
            );
            self.window.saved(Saved::Failed(why));
            return;
        }
        if let Err(error) = self.disk.make_folder(folder) {
            let why = format!("{}: {error}", folder.display());
            self.window.saved(Saved::Failed(why));
            return;
        }
        self.write(file, bytes);
    }

    /// Hands the window the song at `path` to import with those of the `companions` beside it
    /// that can be read, or says why the song could not be.
    pub(crate) fn read_import(&mut self, path: &Path, companions: &[String]) {
        let bytes = match self.disk.read(path) {
            Ok(bytes) => bytes,
            Err(error) => return self.window.refuse(path, &error),
        };
        let folder = path.parent().unwrap_or_else(|| Path::new(""));
        // A companion missing or unreadable is one the format goes without.
        let found = companions
            .iter()
            .filter_map(|name| Some((name.clone(), self.disk.read(&folder.join(name)).ok()?)))
            .collect();
        self.window.import_song(path, &bytes, &found);
    }

    /// Hands the text of the file at `path` to the import timing window.
    pub(crate) fn read_timing(&mut self, path: &Path) {
        match self.disk.read(path) {
            Ok(bytes) => self
                .window
                .timing_file_read(&String::from_utf8_lossy(&bytes)),
            Err(error) => {
                let why = format!("{}: {error}", path.display());
                self.window.timing_file_failed(&why);
            }
        }
    }

    /// Writes the current unsaved song into the recovery file.
    pub(crate) fn autosave(&mut self, bytes: &[u8]) {
        let Some(recovery) = &self.recovery else {
            self.window
                .autosave_failed("there is no folder to keep it in");
            return;
        };
        let kept = recovery
            .parent()
            .map_or(Ok(()), |folder| self.disk.make_folder(folder))
            .and_then(|()| disk::write_whole(self.disk.as_ref(), recovery, bytes));
        if let Err(error) = kept {
            let why = format!("{}: {error}", recovery.display());
            self.window.autosave_failed(&why);
        }
    }

    /// Offers to restore the autosave an earlier run left, if there is one.
    pub(crate) fn recover(&mut self) {
        let Some(recovery) = &self.recovery else {
            return;
        };
        match self.disk.read(recovery) {
            Ok(bytes) => self.window.recover(&bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => self.window.refuse(recovery, &error),
        }
    }
}

/// `dialog` offering a filter for each of `formats`, a name and its extensions.
fn filtered(dialog: rfd::FileDialog, formats: &[(String, Vec<String>)]) -> rfd::FileDialog {
    formats.iter().fold(dialog, |dialog, (name, extensions)| {
        dialog.add_filter(name, extensions)
    })
}
