//! Finding a song's timing from its audio, off the window's thread: the beat tracker's weights
//! downloaded once the user agrees, then the song's beats tracked and a tempo map fitted to them.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use charter_audio::{Onset, Track};
use charter_core::{TicksPerBeat, Timing};
use charter_ui::Window;

/// Where the authors publish the weights of Beat This!'s `final0` model, the one the reference
/// uses by default.
const WEIGHTS_URL: &str =
    "https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt";

/// What a thread of the finder has to say.
enum Progress {
    /// This share of the weights has downloaded.
    Downloaded(f32),
    /// The weights are in place, or why they are not.
    Weights(Result<(), String>),
    /// The timing found, or why none was.
    Found(Result<Timing, String>),
}

/// Finds timing when the window asks, a job at a time.
#[derive(Default)]
pub struct Finder {
    /// Where the weights are kept, when the user's data folder is known.
    weights: Option<PathBuf>,
    /// The audio and resolution waiting on the weights.
    waiting: Option<(PathBuf, TicksPerBeat, Option<Vec<Onset>>)>,
    work: Option<Receiver<Progress>>,
}

impl std::fmt::Debug for Finder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Finder")
            .field("weights", &self.weights)
            .field("busy", &self.work.is_some())
            .finish_non_exhaustive()
    }
}

impl Finder {
    /// A finder keeping the weights at `weights`.
    pub fn new(weights: Option<PathBuf>) -> Self {
        Self {
            weights,
            ..Self::default()
        }
    }

    /// Starts finding the timing of the audio in `file` at `ticks_per_beat`, with the `onsets`
    /// already found in it, or asks the window to let the weights download first.
    pub fn find(
        &mut self,
        file: PathBuf,
        ticks_per_beat: TicksPerBeat,
        onsets: Option<Vec<Onset>>,
        window: &mut Window,
    ) {
        let Some(weights) = self.weights.clone() else {
            window.timing_found(Err(
                "charter has no data folder to keep the beat tracker in".into(),
            ));
            return;
        };
        if !weights.exists() {
            self.waiting = Some((file, ticks_per_beat, onsets));
            window.weights_needed();
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let found = timing_of(&file, &weights, ticks_per_beat, onsets);
            let _ = send.send(Progress::Found(found));
        });
        self.work = Some(receive);
    }

    /// Downloads the weights, the user having agreed, then finds the timing waiting on them.
    pub fn download(&mut self, window: &mut Window) {
        let Some(weights) = self.weights.clone() else {
            window.timing_found(Err(
                "charter has no data folder to keep the beat tracker in".into(),
            ));
            return;
        };
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let done = fetch(&weights, &send);
            let _ = send.send(Progress::Weights(done));
        });
        self.work = Some(receive);
    }

    /// Passes on what the finder's thread has to say. Says whether it is still at work, so the
    /// window keeps drawing its progress.
    pub fn update(&mut self, window: &mut Window) -> bool {
        let Some(work) = &self.work else {
            return false;
        };
        loop {
            match work.try_recv() {
                Ok(Progress::Downloaded(share)) => window.weights_progress(share),
                Ok(Progress::Weights(Ok(()))) => {
                    self.work = None;
                    if let Some((file, ticks_per_beat, onsets)) = self.waiting.take() {
                        self.find(file, ticks_per_beat, onsets, window);
                    }
                    return true;
                }
                Ok(Progress::Weights(Err(why))) => {
                    self.work = None;
                    self.waiting = None;
                    window.timing_found(Err(format!("the beat tracker did not download: {why}")));
                    return false;
                }
                Ok(Progress::Found(found)) => {
                    self.work = None;
                    window.timing_found(found);
                    return false;
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    self.work = None;
                    window.timing_found(Err("the timing finder stopped".into()));
                    return false;
                }
            }
        }
    }
}

/// The timing of the audio in `file`: its beats from the tracker with the weights at `weights`,
/// and a tempo map fitted to them and to `onsets`, or the audio's own when they are not given.
fn timing_of(
    file: &Path,
    weights: &Path,
    ticks_per_beat: TicksPerBeat,
    onsets: Option<Vec<Onset>>,
) -> Result<Timing, String> {
    // ponytail: the tracker reads its weights by path and a song's audio is read once here, so
    // the finder goes to the file system directly rather than through the host's Disk.
    let bytes = std::fs::read(file).map_err(|why| format!("{}: {why}", file.display()))?;
    let extension = file.extension().and_then(|extension| extension.to_str());
    let track = Track::decode(bytes, extension).map_err(|why| why.to_string())?;
    let mono: Vec<f32> = track
        .frames()
        .iter()
        .map(|[left, right]| (left + right) / 2.0)
        .collect();
    let tracker =
        charter_beatthis_burn::Tracker::from_checkpoint(weights).map_err(|why| why.to_string())?;
    let beats = tracker
        .track(&mono, track.rate().get())
        .map_err(|why| why.to_string())?;
    let onsets = onsets.unwrap_or_else(|| charter_audio::onsets(&track));
    charter_autotiming::estimate(&beats.beats, &beats.downbeats, &onsets, ticks_per_beat)
        .map_err(|why| why.to_string())?
        .map(|found| found.timing)
        .ok_or_else(|| "the audio has too few beats to fit a tempo to".to_owned())
}

/// Downloads the weights to `weights` through a partial file beside it, telling `progress` how
/// far it has got about every hundredth.
fn fetch(weights: &Path, progress: &Sender<Progress>) -> Result<(), String> {
    let partial = weights.with_extension("ckpt.partial");
    if let Some(folder) = weights.parent() {
        std::fs::create_dir_all(folder).map_err(|why| why.to_string())?;
    }
    let mut response = ureq::get(WEIGHTS_URL)
        .call()
        .map_err(|why| why.to_string())?;
    let body = response.body_mut();
    let length = body.content_length();
    let mut reader = body.as_reader();
    let mut file = std::fs::File::create(&partial).map_err(|why| why.to_string())?;
    let mut buffer = vec![0; 1 << 16];
    let (mut read, mut told) = (0_u64, 0_u64);
    loop {
        let count = reader.read(&mut buffer).map_err(|why| why.to_string())?;
        if count == 0 {
            break;
        }
        file.write_all(buffer.get(..count).unwrap_or_default())
            .map_err(|why| why.to_string())?;
        read += count as u64;
        if let Some(length) = length.filter(|length| *length > 0)
            && read * 100 / length > told
        {
            told = read * 100 / length;
            #[allow(clippy::cast_precision_loss)] // a share of a download
            let share = read as f32 / length as f32;
            let _ = progress.send(Progress::Downloaded(share));
        }
    }
    file.sync_all().map_err(|why| why.to_string())?;
    std::fs::rename(&partial, weights).map_err(|why| why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_data_folder_the_window_hears_why_nothing_was_found() {
        let mut window = Window::new(&[]);
        let mut finder = Finder::new(None);
        finder.find(
            PathBuf::from("/songs/a.ogg"),
            charter_core::DEFAULT_TICKS_PER_BEAT,
            None,
            &mut window,
        );
        assert!(!finder.update(&mut window));
        finder.download(&mut window);
        assert!(finder.work.is_none());
    }
}
