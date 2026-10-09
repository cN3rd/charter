//! Finding a song's timing from its audio, off the window's thread: the model's weights
//! downloaded once the user agrees, then either Beat This!'s beats with a tempo map fitted to
//! them, or Mapperatorinator's timing points.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use charter_audio::{Onset, Track};
use charter_core::{Section, TicksPerBeat, Timing};
use charter_ui::{Tracker, Window};

use crate::report::chain;

/// Where each model's weights come from, and the file they are kept in within the data folder.
/// Beat This!'s are its authors' `final0`, the one the reference uses by default;
/// Mapperatorinator's are its V32 model pruned to what timing reads.
const fn source(tracker: Tracker) -> (&'static str, &'static str) {
    match tracker {
        Tracker::BeatThis => (
            "https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt",
            "beat_this-final0.ckpt",
        ),
        Tracker::Mapperatorinator => (
            "https://huggingface.co/cnf3rd/timingator-v32-f16-gguf/resolve/main/timingator-v32-f16.gguf",
            "timingator-v32-f16.gguf",
        ),
    }
}

/// A song waiting on its model's weights: its audio, resolution, onsets and the model.
type Waiting = (PathBuf, TicksPerBeat, Option<Vec<Onset>>, Tracker);

/// What a thread of the finder has to say.
enum Progress {
    /// This share of the weights has downloaded.
    Downloaded(f32),
    /// The weights are in place, or why they are not.
    Weights(Result<(), String>),
    /// Finding the timing is this share of the way done.
    Finding(f32),
    /// There is no graphics card to find the timing of this audio at this resolution on.
    NoGpu(PathBuf, TicksPerBeat),
    /// The timing found, or why none was.
    Found(Result<Timing, String>),
}

/// Finds timing when the window asks, a job at a time.
#[derive(Default)]
pub(crate) struct Finder {
    /// The data folder the weights are kept in, when the user's is known.
    folder: Option<PathBuf>,
    /// The song waiting on its model's weights.
    waiting: Option<Waiting>,
    /// Receiver for the current download or analysis job, if one is running.
    work: Option<Receiver<Progress>>,
}

impl std::fmt::Debug for Finder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Finder")
            .field("folder", &self.folder)
            .field("busy", &self.work.is_some())
            .finish_non_exhaustive()
    }
}

impl Finder {
    /// A finder keeping the weights in `folder`.
    pub(crate) fn new(folder: Option<PathBuf>) -> Self {
        Self {
            folder,
            ..Self::default()
        }
    }

    /// Where `tracker`'s weights are kept, or tells the window there is nowhere to keep them.
    fn weights(&self, tracker: Tracker, window: &mut Window) -> Option<PathBuf> {
        let folder = self.folder.as_ref();
        if folder.is_none() {
            window.timing_found(Err(
                "charter has no data folder to keep the beat tracker in".into(),
            ));
        }
        folder.map(|folder| folder.join(source(tracker).1))
    }

    /// Starts finding the timing of the audio in `file` at `ticks_per_beat` with `tracker`, with
    /// the `onsets` already found in it, or asks the window to let the weights download first.
    pub(crate) fn find(
        &mut self,
        file: PathBuf,
        ticks_per_beat: TicksPerBeat,
        onsets: Option<Vec<Onset>>,
        tracker: Tracker,
        window: &mut Window,
    ) {
        self.start((file, ticks_per_beat, onsets, tracker), false, window);
    }

    /// Finds the timing waiting on the user's agreement to run on the CPU.
    pub(crate) fn find_on_cpu(&mut self, window: &mut Window) {
        if let Some(waiting) = self.waiting.take() {
            self.start(waiting, true, window);
        }
    }

    /// Starts finding the timing `job` asks for, on the CPU when `on_cpu` and otherwise on the
    /// graphics card when its model wants one, or asks the window to let the weights download
    /// first.
    fn start(&mut self, job: Waiting, on_cpu: bool, window: &mut Window) {
        let (file, ticks_per_beat, onsets, tracker) = job;
        let Some(weights) = self.weights(tracker, window) else {
            return;
        };
        if !weights.exists() {
            self.waiting = Some((file, ticks_per_beat, onsets, tracker));
            window.weights_needed();
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use timingator_wrap::Device;

            let tell = |share| drop(send.send(Progress::Finding(share)));
            let found = match tracker {
                Tracker::BeatThis => timing_of(&file, &weights, ticks_per_beat, onsets),
                Tracker::Mapperatorinator if on_cpu => {
                    mapperatorinator(&file, &weights, ticks_per_beat, Device::Cpu, tell)
                }
                Tracker::Mapperatorinator if gpu() => {
                    mapperatorinator(&file, &weights, ticks_per_beat, Device::Gpu, tell)
                }
                Tracker::Mapperatorinator => {
                    drop(send.send(Progress::NoGpu(file, ticks_per_beat)));
                    return;
                }
            };
            drop(send.send(Progress::Found(found)));
        });
        self.work = Some(receive);
    }

    /// Downloads the waiting song's model's weights, the user having agreed, then finds its
    /// timing.
    pub(crate) fn download(&mut self, window: &mut Window) {
        let tracker = self
            .waiting
            .as_ref()
            .map_or(Tracker::BeatThis, |waiting| waiting.3);
        let Some(weights) = self.weights(tracker, window) else {
            return;
        };
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let done = fetch(source(tracker).0, &weights, &send);
            drop(send.send(Progress::Weights(done)));
        });
        self.work = Some(receive);
    }

    /// Passes on what the finder's thread has to say. Says whether it is still at work, so the
    /// window keeps drawing its progress.
    pub(crate) fn update(&mut self, window: &mut Window) -> bool {
        let Some(work) = &self.work else {
            return false;
        };
        loop {
            match work.try_recv() {
                Ok(Progress::Downloaded(share)) => window.weights_progress(share),
                Ok(Progress::Finding(share)) => window.finding_progress(share),
                Ok(Progress::NoGpu(file, ticks_per_beat)) => {
                    self.work = None;
                    self.waiting = Some((file, ticks_per_beat, None, Tracker::Mapperatorinator));
                    window.gpu_missing();
                    return false;
                }
                Ok(Progress::Weights(Ok(()))) => {
                    self.work = None;
                    if let Some((file, ticks_per_beat, onsets, tracker)) = self.waiting.take() {
                        self.find(file, ticks_per_beat, onsets, tracker, window);
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
    let bytes =
        std::fs::read(file).map_err(|why| format!("{}: {}", file.display(), chain(&why)))?;
    let extension = file.extension().and_then(|extension| extension.to_str());
    let track = Track::decode(bytes, extension).map_err(|why| chain(&why).to_string())?;
    let mono = track.to_mono();
    let tracker = charter_beatthis_burn::Tracker::from_checkpoint(weights)
        .map_err(|why| format!("{}: {}", weights.display(), chain(&why)))?;
    let beats = tracker
        .track(&mono, track.rate().get())
        .map_err(|why| chain(&why).to_string())?;
    let onsets = onsets.unwrap_or_else(|| charter_audio::onsets(&track));
    charter_autotiming::estimate(&beats.beats, &beats.downbeats, &onsets, ticks_per_beat)
        .map_err(|why| chain(&why).to_string())?
        .map(|found| found.timing)
        .ok_or_else(|| "the audio has too few beats to fit a tempo to".to_owned())
}

/// Whether there is a graphics card to run Mapperatorinator's model on. Asked once, since asking
/// sets the device up.
fn gpu() -> bool {
    static GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *GPU.get_or_init(timingator_wrap::graphics_card)
}

/// The timing of the audio in `file` as Mapperatorinator's timing model, with the weights at
/// `weights`, finds it on `device`, telling `progress` the share of the way it has got.
fn mapperatorinator(
    file: &Path,
    weights: &Path,
    ticks_per_beat: TicksPerBeat,
    device: timingator_wrap::Device,
    mut progress: impl FnMut(f32),
) -> Result<Timing, String> {
    use timingator_wrap::SAMPLE_RATE;

    let bytes =
        std::fs::read(file).map_err(|why| format!("{}: {}", file.display(), chain(&why)))?;
    let extension = file.extension().and_then(|extension| extension.to_str());
    let track = Track::decode(bytes, extension).map_err(|why| chain(&why).to_string())?;
    let rate = u32::try_from(SAMPLE_RATE).map_err(|why| chain(&why).to_string())?;
    let mut samples = charter_audio::resample(&track.to_mono(), track.rate().get(), rate)
        .map_err(|why| chain(&why).to_string())?;
    // As timingator's `audio::normalize`: divided by the peak.
    let peak = samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    if peak > 0.0 {
        for sample in &mut samples {
            *sample /= peak;
        }
    }
    let model =
        std::fs::read(weights).map_err(|why| format!("{}: {}", weights.display(), chain(&why)))?;
    let timer = timingator_wrap::Timer::load(&model, device)
        .map_err(|why| format!("{}: {why:#}", weights.display()))?;
    let count = |windows: usize| u64::try_from(windows).unwrap_or(u64::MAX);
    let points = timer
        .run(&samples, |done, all| {
            progress(download_share(count(done), count(all)));
        })
        .map_err(|why| format!("{why:#}"))?;
    let sections = sections(&points)?;
    charter_core::sectioned(&sections, ticks_per_beat)
        .map(|made| made.timing)
        .map_err(|why| match why {
            charter_core::SectionError::Empty => {
                "the model found no timing in the audio".to_owned()
            }
            charter_core::SectionError::OutOfRange(_) => why.to_string(),
        })
}

/// Mapperatorinator's osu! timing points as the sections of a tempo map.
fn sections(points: &[timingator_wrap::TimingPoint]) -> Result<Vec<Section>, String> {
    points
        .iter()
        .map(|point| {
            let meter = u16::try_from(point.meter)
                .ok()
                .and_then(std::num::NonZeroU16::new)
                .ok_or_else(|| format!("the model found a bar of {} beats", point.meter))?;
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "microsecond offsets of a song are far inside f64's exact range"
            )]
            let seconds = point.offset_us as f64 / 1e6;
            Ok(Section {
                seconds,
                beat_seconds: point.ms_per_beat / 1000.0,
                meter,
            })
        })
        .collect()
}

/// Downloads the weights at `url` to `weights` through a partial file beside it, telling
/// `progress` how far it has got about every hundredth.
fn fetch(url: &str, weights: &Path, progress: &Sender<Progress>) -> Result<(), String> {
    let partial = crate::disk::partial_path(weights);
    if let Some(folder) = weights.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|why| format!("{}: {}", folder.display(), chain(&why)))?;
    }
    let downloaded = (|| {
        let mut response = ureq::get(url)
            .call()
            .map_err(|why| chain(&why).to_string())?;
        let body = response.body_mut();
        let length = body.content_length();
        let mut reader = body.as_reader();
        let mut file = std::fs::File::create(&partial)
            .map_err(|why| format!("{}: {}", partial.display(), chain(&why)))?;
        copy_weights(&mut reader, &mut file, length, progress)
            .map_err(|why| format!("{}: {why}", partial.display()))?;
        file.sync_all()
            .map_err(|why| format!("{}: {}", partial.display(), chain(&why)))?;
        std::fs::rename(&partial, weights).map_err(|why| {
            format!(
                "{} to {}: {}",
                partial.display(),
                weights.display(),
                chain(&why)
            )
        })
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
            .map_err(|why| format!("reading the beat tracker: {}", chain(&why)))?;
        if count == 0 {
            break;
        }
        file.write_all(buffer.get(..count).unwrap_or_default())
            .map_err(|why| format!("writing the beat tracker: {}", chain(&why)))?;
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
/// Approximate fraction of `length` that `read` is, from 0 to 1: of the weights downloaded, or
/// of the windows decoded.
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
                Tracker::BeatThis,
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
                Tracker::BeatThis,
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
    fn no_graphics_card_asks_first_and_keeps_the_song_for_the_cpu() {
        let mut window = window();
        let mut finder = Finder::default();
        let (send, receive) = std::sync::mpsc::channel();
        finder.work = Some(receive);
        send.send(Progress::Finding(0.5)).unwrap();
        send.send(Progress::NoGpu(
            PathBuf::from("/songs/a.ogg"),
            charter_core::TicksPerBeat::default(),
        ))
        .unwrap();

        assert!(!finder.update(&mut window));
        assert!(finder.work.is_none());
        assert_eq!(
            finder
                .waiting
                .as_ref()
                .map(|waiting| (&waiting.0, waiting.3)),
            Some((&PathBuf::from("/songs/a.ogg"), Tracker::Mapperatorinator))
        );
        assert!(window.message().is_none());
        finder.find_on_cpu(&mut window);
        assert!(finder.waiting.is_none());
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
            Tracker::Mapperatorinator,
            &mut window,
        );
        assert!(!finder.update(&mut window));
        finder.download(&mut window);
        assert!(finder.work.is_none());
    }

    #[test]
    fn the_models_timing_points_become_sections_and_a_bar_of_no_beats_is_refused() {
        use timingator_wrap::TimingPoint;
        let point = |offset_us, ms_per_beat, meter| TimingPoint {
            offset_us,
            ms_per_beat,
            meter,
        };
        let found = sections(&[point(4_000, 600.0, 4), point(2_382_000, 300.0, 7)]).unwrap();
        assert_eq!(found.len(), 2);
        assert!((found[0].seconds - 0.004).abs() < 1e-12);
        assert!((found[1].beat_seconds - 0.3).abs() < 1e-12);
        assert_eq!(found[1].meter.get(), 7);
        for meter in [0, -1, 70_000] {
            sections(&[point(0, 500.0, meter)]).unwrap_err();
        }
    }

    #[test]
    fn each_model_keeps_its_own_weights_in_the_data_folder() {
        let finder = Finder::new(Some(PathBuf::from("/data")));
        let mut window = window();
        assert_eq!(
            finder.weights(Tracker::BeatThis, &mut window),
            Some(PathBuf::from("/data/beat_this-final0.ckpt"))
        );
        assert_eq!(
            finder.weights(Tracker::Mapperatorinator, &mut window),
            Some(PathBuf::from("/data/timingator-v32-f16.gguf"))
        );
        assert!(
            source(Tracker::Mapperatorinator)
                .0
                .starts_with("https://huggingface.co/")
        );
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
