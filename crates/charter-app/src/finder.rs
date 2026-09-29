//! Finding a song's timing from its audio, off the window's thread: the beat tracker's weights
//! downloaded once the user agrees, then the song's beats tracked and a tempo map fitted to them.

use std::io::{self, Read, Write};
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
    /// Receiver for the current download or analysis job, if one is running.
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
            drop(send.send(Progress::Found(found)));
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
            drop(send.send(Progress::Weights(done)));
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
                    self.waiting = None;
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
    let tracker = charter_beatthis_burn::Tracker::from_checkpoint(weights)
        .map_err(|why| format!("{}: {why}", weights.display()))?;
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
        std::fs::create_dir_all(folder).map_err(|why| format!("{}: {why}", folder.display()))?;
    }
    let downloaded = (|| {
        let mut response = ureq::get(WEIGHTS_URL)
            .call()
            .map_err(|why| why.to_string())?;
        let body = response.body_mut();
        let length = body.content_length();
        let mut reader = body.as_reader();
        let mut file = std::fs::File::create(&partial)
            .map_err(|why| format!("{}: {why}", partial.display()))?;
        copy_weights(&mut reader, &mut file, length, progress)
            .map_err(|why| format!("{}: {why}", partial.display()))?;
        file.sync_all()
            .map_err(|why| format!("{}: {why}", partial.display()))?;
        std::fs::rename(&partial, weights)
            .map_err(|why| format!("{} to {}: {why}", partial.display(), weights.display()))
    })();
    cleanup_partial_on_error(downloaded, &partial, |path| std::fs::remove_file(path))
}

/// Removes `partial` after a failed write-then-rename download.
fn cleanup_partial_on_error<T>(
    result: Result<T, String>,
    partial: &Path,
    remove: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<T, String> {
    if result.is_err() {
        drop(remove(partial));
    }
    result
}

/// Adds the failed stream operation without discarding the underlying I/O error.
/// Copies a response body while reporting the share received, when its length is known.
fn copy_weights(
    reader: &mut impl Read,
    file: &mut impl Write,
    length: Option<u64>,
    progress: &Sender<Progress>,
) -> Result<(), String> {
    let mut buffer = vec![0; 1 << 16];
    let (mut read, mut told) = (0_u64, 0_u64);
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|why| format!("reading the beat tracker: {why}"))?;
        if count == 0 {
            break;
        }
        file.write_all(buffer.get(..count).unwrap_or_default())
            .map_err(|why| format!("writing the beat tracker: {why}"))?;
        read = read.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        if let Some(length) = length.filter(|length| *length > 0)
            && read.saturating_mul(100).div_euclid(length) > told
        {
            told = read.saturating_mul(100).div_euclid(length);
            let share = download_share(read, length);
            drop(progress.send(Progress::Downloaded(share)));
        }
    }
    if length.is_some_and(|length| length > 0 && read != length) {
        return Err("incomplete beat tracker download".into());
    }
    Ok(())
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "download progress is approximate"
)]
/// Approximate fraction of the weights downloaded, from 0 to 1.
fn download_share(read: u64, length: u64) -> f32 {
    read as f32 / length as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn window() -> Window {
        let mut window = Window::new(crate::PLUGINS);
        window.open_demo();
        window
    }

    #[test]
    fn a_stopped_worker_drops_the_request_waiting_on_its_weights() {
        let mut window = window();
        let mut finder = Finder {
            waiting: Some((
                PathBuf::from("/songs/a.ogg"),
                charter_core::TicksPerBeat::default(),
                None,
            )),
            ..Finder::default()
        };
        let (send, receive) = std::sync::mpsc::channel();
        finder.work = Some(receive);
        drop(send);

        assert!(!finder.update(&mut window));
        assert!(finder.waiting.is_none());
        assert!(window.message().unwrap().contains("timing finder stopped"));
    }

    #[test]
    fn a_download_failure_reports_its_cause_and_forgets_the_waiting_song() {
        let mut window = window();
        let mut finder = Finder {
            waiting: Some((
                PathBuf::from("/songs/a.ogg"),
                charter_core::TicksPerBeat::default(),
                None,
            )),
            ..Finder::default()
        };
        let (send, receive) = std::sync::mpsc::channel();
        finder.work = Some(receive);
        send.send(Progress::Weights(Err("server refused the request".into())))
            .unwrap();

        assert!(!finder.update(&mut window));
        assert!(finder.waiting.is_none());
        assert!(finder.work.is_none());
        assert!(
            window
                .message()
                .unwrap()
                .contains("the beat tracker did not download: server refused the request")
        );
        assert!(!finder.update(&mut window));
    }

    #[test]
    fn a_download_without_a_waiting_song_finishes_without_starting_a_tracker() {
        let mut window = window();
        let mut finder = Finder::default();
        let (send, receive) = std::sync::mpsc::channel();
        finder.work = Some(receive);
        send.send(Progress::Weights(Ok(()))).unwrap();

        assert!(finder.update(&mut window));
        assert!(finder.work.is_none());
        assert!(window.message().is_none());
        assert!(!finder.update(&mut window));
    }

    #[test]
    fn progress_keeps_the_finder_active_until_the_worker_reports_a_result() {
        let mut window = window();
        let mut finder = Finder::default();
        let (send, receive) = std::sync::mpsc::channel();
        finder.work = Some(receive);

        assert!(finder.update(&mut window));
        send.send(Progress::Downloaded(0.42)).unwrap();
        assert!(finder.update(&mut window));
        assert!(finder.work.is_some());
        send.send(Progress::Found(Err("too few beats".into())))
            .unwrap();
        assert!(!finder.update(&mut window));
        assert!(finder.work.is_none());
        assert!(window.message().unwrap().contains("too few beats"));
    }

    #[test]
    fn found_timing_is_applied_to_the_open_song() {
        let mut window = window();
        let mut finder = Finder::default();
        let (send, receive) = std::sync::mpsc::channel();
        finder.work = Some(receive);
        let timing = Timing {
            anchors: vec![charter_core::Anchor {
                tick: charter_core::Tick::new(0),
                seconds: charter_core::Origin::try_from(3.0).unwrap(),
            }],
            tempo: charter_core::Bpm::try_from(132.0).unwrap(),
            events: vec![],
        };
        send.send(Progress::Found(Ok(timing))).unwrap();

        assert!(!finder.update(&mut window));
        assert!(finder.work.is_none());
        assert!(window.message().unwrap().contains("132 BPM"));
    }

    #[test]
    fn copying_weights_reports_each_crossed_percentage_and_preserves_the_bytes() {
        let bytes = vec![7; 2 * (1 << 16)];
        let mut output = Vec::new();
        let (send, receive) = std::sync::mpsc::channel();

        copy_weights(
            &mut Cursor::new(&bytes),
            &mut output,
            Some(u64::try_from(bytes.len()).unwrap()),
            &send,
        )
        .unwrap();

        assert_eq!(output, bytes);
        let reports: Vec<_> = receive.try_iter().collect();
        assert!(matches!(reports.first(), Some(Progress::Downloaded(0.5))));
        assert!(matches!(reports.get(1), Some(Progress::Downloaded(1.0))));
        assert_eq!(reports.len(), 2);
    }

    #[test]
    fn a_weight_stream_without_a_known_positive_length_sends_no_percentages() {
        for length in [None, Some(0)] {
            let mut output = Vec::new();
            let (send, receive) = std::sync::mpsc::channel();
            copy_weights(&mut Cursor::new(b"weights"), &mut output, length, &send).unwrap();
            assert_eq!(output, b"weights");
            assert!(receive.try_recv().is_err());
        }
    }

    #[test]
    fn a_short_weight_response_is_not_accepted_as_a_complete_checkpoint() {
        let mut output = Vec::new();
        let (send, _) = std::sync::mpsc::channel();

        let error =
            copy_weights(&mut Cursor::new(b"partial"), &mut output, Some(10), &send).unwrap_err();

        assert!(error.contains("incomplete"));
        assert_eq!(output, b"partial");
    }

    #[test]
    fn a_failed_weight_write_reports_the_error_without_claiming_a_complete_download() {
        struct FailingWriter;
        #[expect(
            clippy::missing_trait_methods,
            reason = "a failing test writer; the standard provided methods are right"
        )]
        impl Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("out of space"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (send, receive) = std::sync::mpsc::channel();
        let error = copy_weights(
            &mut Cursor::new(b"weights"),
            &mut FailingWriter,
            Some(7),
            &send,
        )
        .unwrap_err();
        assert!(error.contains("writing the beat tracker"));
        assert!(error.contains("out of space"));
        assert!(receive.try_recv().is_err());
    }

    #[test]
    fn a_failed_weight_read_preserves_the_error_without_claiming_a_complete_download() {
        struct FailingReader;
        #[expect(
            clippy::missing_trait_methods,
            reason = "a failing test reader; the standard provided methods are right"
        )]
        impl Read for FailingReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("stream broke"))
            }
        }
        let (send, receive) = std::sync::mpsc::channel();
        let mut output = Vec::new();
        let error = copy_weights(&mut FailingReader, &mut output, Some(7), &send).unwrap_err();
        assert!(error.contains("reading the beat tracker"));
        assert!(error.contains("stream broke"));
        assert!(output.is_empty());
        assert!(receive.try_recv().is_err());
    }

    #[test]
    fn without_a_data_folder_the_window_hears_why_nothing_was_found() {
        let mut window = Window::new(&[]);
        let mut finder = Finder::new(None);
        finder.find(
            PathBuf::from("/songs/a.ogg"),
            charter_core::TicksPerBeat::default(),
            None,
            &mut window,
        );
        assert!(!finder.update(&mut window));
        finder.download(&mut window);
        assert!(finder.work.is_none());
    }

    #[test]
    fn a_failed_weight_download_removes_its_partial_file() {
        let mut removed = None;
        let result = cleanup_partial_on_error(
            Err::<(), _>("stream broke".to_owned()),
            Path::new("/data/beat_this-final0.ckpt.partial"),
            |path| {
                removed = Some(path.to_path_buf());
                Ok(())
            },
        );

        assert_eq!(result, Err("stream broke".to_owned()));
        assert_eq!(
            removed.as_deref(),
            Some(Path::new("/data/beat_this-final0.ckpt.partial"))
        );
    }
}
