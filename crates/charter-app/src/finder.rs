//! Finding a song's timing from its audio, off the window's thread: the model's weights
//! downloaded once the user agrees, then either Beat This!'s beats with a tempo map fitted to
//! them, or Mapperatorinator's timing points.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use charter_audio::{Onset, SampleRate, Track};
use charter_core::{BeatLength, ResolvedTime, Section, TicksPerBeat, Timing};
use charter_ui::{Tracker, Window};
use sha2::{Digest, Sha256};

use crate::report::chain;

/// Where each model's weights come from, the file they are kept in and their published SHA-256.
/// Beat This!'s are its authors' `final0`, the one the reference uses by default;
/// Mapperatorinator's are its V32 model pruned to what timing reads.
struct Source {
    /// Download URL.
    url: &'static str,
    /// Name in the data folder.
    file_name: &'static str,
    /// Published SHA-256 in lowercase hex.
    sha256: &'static str,
}

/// The published source of `tracker`'s weights.
const fn source(tracker: Tracker) -> Source {
    match tracker {
        Tracker::BeatThis => Source {
            url: "https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt",
            file_name: "beat_this-final0.ckpt",
            sha256: "8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331",
        },
        Tracker::Mapperatorinator => Source {
            url: "https://huggingface.co/cnf3rd/timingator-v32-f16-gguf/resolve/main/timingator-v32-f16.gguf",
            file_name: "timingator-v32-f16.gguf",
            sha256: "8554fca86c384d633442205a20f48b1072a1c842d82e2346bfc80e0811fd2792",
        },
    }
}

/// A song to time with a model.
struct Job {
    /// Song audio.
    file: PathBuf,
    /// Chart resolution.
    ticks_per_beat: TicksPerBeat,
    /// Already found audio onsets, if any.
    onsets: Option<Vec<Onset>>,
    /// Model to use.
    tracker: Tracker,
}

/// The one timing job pending or running in the finder.
#[derive(Default)]
enum State {
    /// No job is pending or running.
    #[default]
    Idle,
    /// Waiting for permission to download weights.
    NeedsWeights(Job),
    /// Downloading weights for the job.
    Downloading {
        /// Job to start after the weights are stored.
        job: Job,
        /// Progress and result from the download worker.
        work: Receiver<Progress>,
    },
    /// Waiting for permission to run on the CPU.
    NeedsCpu(Job),
    /// Finding the timing on a worker.
    Finding(Receiver<Progress>),
}

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
    /// The current timing job, if any.
    state: State,
}

impl std::fmt::Debug for Finder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Finder")
            .field("folder", &self.folder)
            .field(
                "busy",
                &matches!(&self.state, State::Downloading { .. } | State::Finding(_)),
            )
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
        folder.map(|folder| folder.join(source(tracker).file_name))
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
        if matches!(&self.state, State::Downloading { .. } | State::Finding(_)) {
            window.timing_refused("a timing search is already running");
            return;
        }
        self.start(
            Job {
                file,
                ticks_per_beat,
                onsets,
                tracker,
            },
            false,
            window,
        );
    }

    /// Finds the timing waiting on the user's agreement to run on the CPU.
    pub(crate) fn find_on_cpu(&mut self, window: &mut Window) {
        match std::mem::take(&mut self.state) {
            State::NeedsCpu(job) => self.start(job, true, window),
            state @ (State::Idle
            | State::NeedsWeights(_)
            | State::Downloading { .. }
            | State::Finding(_)) => {
                self.state = state;
                window.timing_refused("no CPU run is waiting");
            }
        }
    }

    /// Starts finding the timing `job` asks for, on the CPU when `on_cpu` and otherwise on the
    /// graphics card when its model wants one, or asks the window to let the weights download
    /// first.
    fn start(&mut self, job: Job, on_cpu: bool, window: &mut Window) {
        let Job {
            file,
            ticks_per_beat,
            onsets,
            tracker,
        } = job;
        let Some(weights) = self.weights(tracker, window) else {
            return;
        };
        if !weights.exists() {
            self.state = State::NeedsWeights(Job {
                file,
                ticks_per_beat,
                onsets,
                tracker,
            });
            window.weights_needed();
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use charter_mapperatorinator_burn::Device;

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
        self.state = State::Finding(receive);
    }

    /// Downloads the waiting song's model's weights, the user having agreed, then finds its
    /// timing.
    pub(crate) fn download(&mut self, window: &mut Window) {
        let job = match std::mem::take(&mut self.state) {
            State::NeedsWeights(job) => job,
            state @ (State::Idle
            | State::Downloading { .. }
            | State::NeedsCpu(_)
            | State::Finding(_)) => {
                let why = if matches!(&state, State::Downloading { .. } | State::Finding(_)) {
                    "a timing search is already running"
                } else {
                    "no download is waiting"
                };
                self.state = state;
                window.timing_refused(why);
                return;
            }
        };
        let tracker = job.tracker;
        let Some(weights) = self.weights(tracker, window) else {
            self.state = State::NeedsWeights(job);
            return;
        };
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let done = fetch(&source(tracker), &weights, &send);
            drop(send.send(Progress::Weights(done)));
        });
        self.state = State::Downloading { job, work: receive };
    }

    /// Passes on what the finder's thread has to say. Says whether it is still at work, so the
    /// window keeps drawing its progress.
    pub(crate) fn update(&mut self, window: &mut Window) -> bool {
        let work = match &self.state {
            State::Downloading { work, .. } | State::Finding(work) => work,
            State::Idle | State::NeedsWeights(_) | State::NeedsCpu(_) => return false,
        };
        loop {
            match work.try_recv() {
                Ok(Progress::Downloaded(share)) => window.weights_progress(share),
                Ok(Progress::Finding(share)) => window.finding_progress(share),
                Ok(Progress::NoGpu(file, ticks_per_beat)) => {
                    self.state = State::NeedsCpu(Job {
                        file,
                        ticks_per_beat,
                        onsets: None,
                        tracker: Tracker::Mapperatorinator,
                    });
                    window.gpu_missing();
                    return false;
                }
                Ok(Progress::Weights(Ok(()))) => {
                    if let State::Downloading { job, .. } = std::mem::take(&mut self.state) {
                        self.start(job, false, window);
                    }
                    return true;
                }
                Ok(Progress::Weights(Err(why))) => {
                    self.state = State::Idle;
                    window.timing_found(Err(format!("the beat tracker did not download: {why}")));
                    return false;
                }
                Ok(Progress::Found(found)) => {
                    self.state = State::Idle;
                    window.timing_found(found);
                    return false;
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    self.state = State::Idle;
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
        .track(mono.samples(), mono.rate().get())
        .map_err(|why| chain(&why).to_string())?;
    let marked: Vec<_> = beats
        .beats
        .iter()
        .map(|&seconds| charter_autotiming::Beat {
            seconds,
            downbeat: beats
                .downbeats
                .iter()
                .any(|&down| (down - seconds).abs() < 1e-3),
        })
        .collect();
    let onsets = onsets.unwrap_or_else(|| charter_audio::onsets(&track));
    charter_autotiming::estimate(&marked, &onsets, ticks_per_beat)
        .map_err(|why| chain(&why).to_string())?
        .map(|found| found.timing)
        .ok_or_else(|| "the audio has too few beats to fit a tempo to".to_owned())
}

/// Whether there is a graphics card to run Mapperatorinator's model on. Asked once, since asking
/// sets the device up.
fn gpu() -> bool {
    static GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *GPU.get_or_init(charter_mapperatorinator_burn::graphics_card)
}

/// The timing of the audio in `file` as Mapperatorinator's timing model, with the weights at
/// `weights`, finds it on `device`, telling `progress` the share of the way it has got.
fn mapperatorinator(
    file: &Path,
    weights: &Path,
    ticks_per_beat: TicksPerBeat,
    device: charter_mapperatorinator_burn::Device,
    mut progress: impl FnMut(f32),
) -> Result<Timing, String> {
    use charter_mapperatorinator_burn::SAMPLE_RATE;

    let bytes =
        std::fs::read(file).map_err(|why| format!("{}: {}", file.display(), chain(&why)))?;
    let extension = file.extension().and_then(|extension| extension.to_str());
    let track = Track::decode(bytes, extension).map_err(|why| chain(&why).to_string())?;
    let rate = u32::try_from(SAMPLE_RATE).map_err(|why| chain(&why).to_string())?;
    let rate = SampleRate::new(rate).ok_or_else(|| format!("invalid model sample rate: {rate}"))?;
    let mut samples = track
        .to_mono()
        .resampled(rate)
        .map_err(|why| chain(&why).to_string())?
        .into_samples();
    // Divided by the peak, as Mapperatorinator's model hears it.
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
    let timer = charter_mapperatorinator_burn::Timer::load(&model, device)
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
fn sections(points: &[charter_mapperatorinator_burn::TimingPoint]) -> Result<Vec<Section>, String> {
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
            let start = ResolvedTime::from_seconds(seconds)
                .ok_or_else(|| format!("the model found a time of {} us", point.offset_us))?;
            let beat = BeatLength::from_millis(point.ms_per_beat)
                .ok_or_else(|| format!("the model found a beat of {} ms", point.ms_per_beat))?;
            Ok(Section { start, beat, meter })
        })
        .collect()
}

/// Downloads `source`'s weights to `weights` through a partial file beside it, checking the
/// published SHA-256 before renaming and telling `progress` how far it has got.
fn fetch(source: &Source, weights: &Path, progress: &Sender<Progress>) -> Result<(), String> {
    let partial = crate::disk::partial_path(weights);
    if let Some(folder) = weights.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|why| format!("{}: {}", folder.display(), chain(&why)))?;
    }
    let downloaded = (|| {
        let mut response = ureq::get(source.url)
            .call()
            .map_err(|why| chain(&why).to_string())?;
        let body = response.body_mut();
        let length = body.content_length();
        let mut reader = body.as_reader();
        let mut file = std::fs::File::create(&partial)
            .map_err(|why| format!("{}: {}", partial.display(), chain(&why)))?;
        copy_weights(&mut reader, &mut file, length, source.sha256, progress)
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

/// Copies a response body, checking its SHA-256 and reporting the share received when known.
fn copy_weights(
    reader: &mut impl Read,
    file: &mut impl Write,
    length: Option<u64>,
    expected_sha256: &str,
    progress: &Sender<Progress>,
) -> Result<(), String> {
    let mut buffer = vec![0; 1 << 16];
    let (mut read, mut told) = (0_u64, 0_u64);
    let mut hash = Sha256::new();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|why| format!("reading the beat tracker: {}", chain(&why)))?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).unwrap_or_default();
        file.write_all(chunk)
            .map_err(|why| format!("writing the beat tracker: {}", chain(&why)))?;
        hash.update(chunk);
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
    if format!("{:x}", hash.finalize()) != expected_sha256 {
        return Err("the download does not match the published weights".into());
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
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    fn window() -> Window {
        let mut window = Window::new(crate::PLUGINS);
        window.open_demo();
        window
    }

    fn job(tracker: Tracker) -> Job {
        Job {
            file: PathBuf::from("/songs/a.ogg"),
            ticks_per_beat: TicksPerBeat::default(),
            onsets: None,
            tracker,
        }
    }

    #[test]
    fn a_stopped_worker_drops_the_request_waiting_on_its_weights() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Downloading {
                job: job(Tracker::BeatThis),
                work: receive,
            },
            ..Finder::default()
        };
        drop(send);

        assert!(!finder.update(&mut window));
        assert!(matches!(finder.state, State::Idle));
        assert!(window.message().unwrap().contains("timing finder stopped"));
    }

    #[test]
    fn a_download_failure_reports_its_cause_and_forgets_the_waiting_song() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Downloading {
                job: job(Tracker::BeatThis),
                work: receive,
            },
            ..Finder::default()
        };
        send.send(Progress::Weights(Err("server refused the request".into())))
            .unwrap();

        assert!(!finder.update(&mut window));
        assert!(matches!(finder.state, State::Idle));
        assert!(
            window
                .message()
                .unwrap()
                .contains("the beat tracker did not download: server refused the request")
        );
        assert!(!finder.update(&mut window));
    }

    #[test]
    fn a_second_download_is_refused_without_replacing_its_worker() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Downloading {
                job: job(Tracker::BeatThis),
                work: receive,
            },
            ..Finder::default()
        };

        finder.download(&mut window);

        assert!(window.message().unwrap().contains("already running"));
        send.send(Progress::Downloaded(0.5)).unwrap();
        assert!(finder.update(&mut window));
        assert!(matches!(finder.state, State::Downloading { .. }));
    }

    #[test]
    fn a_cpu_run_without_a_waiting_job_is_refused() {
        let mut window = window();
        let mut finder = Finder::default();

        finder.find_on_cpu(&mut window);

        assert!(window.message().unwrap().contains("CPU run"));
        assert!(!finder.update(&mut window));
    }

    #[test]
    fn another_search_is_refused_while_a_worker_is_running() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Finding(receive),
            ..Finder::default()
        };

        finder.find(
            PathBuf::from("/songs/b.ogg"),
            TicksPerBeat::default(),
            None,
            Tracker::BeatThis,
            &mut window,
        );

        assert!(window.message().unwrap().contains("already running"));
        send.send(Progress::Found(Err("too few beats".into())))
            .unwrap();
        assert!(!finder.update(&mut window));
        assert!(window.message().unwrap().contains("too few beats"));
    }

    #[test]
    fn no_graphics_card_asks_first_and_keeps_the_song_for_the_cpu() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Finding(receive),
            ..Finder::default()
        };
        send.send(Progress::Finding(0.5)).unwrap();
        send.send(Progress::NoGpu(
            PathBuf::from("/songs/a.ogg"),
            charter_core::TicksPerBeat::default(),
        ))
        .unwrap();

        assert!(!finder.update(&mut window));
        assert!(matches!(
            &finder.state,
            State::NeedsCpu(Job {
                file,
                tracker: Tracker::Mapperatorinator,
                ..
            }) if file == Path::new("/songs/a.ogg")
        ));
        assert!(window.message().is_none());
        finder.find_on_cpu(&mut window);
        assert!(matches!(finder.state, State::Idle));
    }

    #[test]
    fn a_download_without_a_waiting_song_is_refused() {
        let mut window = window();
        let mut finder = Finder::default();

        finder.download(&mut window);

        assert!(window.message().unwrap().contains("download"));
        assert!(matches!(finder.state, State::Idle));
        assert!(!finder.update(&mut window));
    }

    #[test]
    fn progress_keeps_the_finder_active_until_the_worker_reports_a_result() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Finding(receive),
            ..Finder::default()
        };

        assert!(finder.update(&mut window));
        send.send(Progress::Finding(0.42)).unwrap();
        assert!(finder.update(&mut window));
        assert!(matches!(finder.state, State::Finding(_)));
        send.send(Progress::Found(Err("too few beats".into())))
            .unwrap();
        assert!(!finder.update(&mut window));
        assert!(matches!(finder.state, State::Idle));
        assert!(window.message().unwrap().contains("too few beats"));
    }

    #[test]
    fn found_timing_is_applied_to_the_open_song() {
        let mut window = window();
        let (send, receive) = std::sync::mpsc::channel();
        let mut finder = Finder {
            state: State::Finding(receive),
            ..Finder::default()
        };
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
        assert!(matches!(finder.state, State::Idle));
        assert!(window.message().unwrap().contains("132 BPM"));
    }

    #[test]
    fn copying_weights_reports_each_crossed_percentage_and_preserves_the_bytes() {
        let bytes = vec![7; 2 * (1 << 16)];
        let expected = format!("{:x}", Sha256::digest(&bytes));
        let mut output = Vec::new();
        let (send, receive) = std::sync::mpsc::channel();

        copy_weights(
            &mut Cursor::new(&bytes),
            &mut output,
            Some(u64::try_from(bytes.len()).unwrap()),
            &expected,
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
        let expected = format!("{:x}", Sha256::digest(b"weights"));
        for length in [None, Some(0)] {
            let mut output = Vec::new();
            let (send, receive) = std::sync::mpsc::channel();
            copy_weights(
                &mut Cursor::new(b"weights"),
                &mut output,
                length,
                &expected,
                &send,
            )
            .unwrap();
            assert_eq!(output, b"weights");
            assert!(receive.try_recv().is_err());
        }
    }

    #[test]
    fn a_weight_stream_matching_its_expected_hash_is_accepted() {
        let expected = format!("{:x}", Sha256::digest(b"weights"));
        let mut output = Vec::new();
        let (send, _) = std::sync::mpsc::channel();

        copy_weights(
            &mut Cursor::new(b"weights"),
            &mut output,
            Some(7),
            &expected,
            &send,
        )
        .unwrap();

        assert_eq!(output, b"weights");
    }

    #[test]
    fn a_weight_stream_with_the_wrong_hash_is_refused_even_when_its_length_matches() {
        let expected = format!("{:x}", Sha256::digest(b"weights"));
        let mut output = Vec::new();
        let (send, _) = std::sync::mpsc::channel();

        let error = copy_weights(
            &mut Cursor::new(b"weightz"),
            &mut output,
            Some(7),
            &expected,
            &send,
        )
        .unwrap_err();

        assert!(error.contains("does not match the published weights"));
        assert_eq!(output, b"weightz");
    }

    #[test]
    fn a_short_weight_response_is_not_accepted_as_a_complete_checkpoint() {
        let mut output = Vec::new();
        let (send, _) = std::sync::mpsc::channel();

        let expected = format!("{:x}", Sha256::digest(b"partial"));
        let error = copy_weights(
            &mut Cursor::new(b"partial"),
            &mut output,
            Some(10),
            &expected,
            &send,
        )
        .unwrap_err();

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
        let expected = format!("{:x}", Sha256::digest(b"weights"));
        let error = copy_weights(
            &mut Cursor::new(b"weights"),
            &mut FailingWriter,
            Some(7),
            &expected,
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
        let expected = format!("{:x}", Sha256::digest(b"weights"));
        let error =
            copy_weights(&mut FailingReader, &mut output, Some(7), &expected, &send).unwrap_err();
        assert!(error.contains("reading the beat tracker"));
        assert!(error.contains("stream broke"));
        assert!(output.is_empty());
        assert!(receive.try_recv().is_err());
    }

    #[test]
    fn without_a_data_folder_the_window_hears_why_nothing_was_found() {
        let mut window = window();
        let mut finder = Finder::new(None);
        finder.find(
            PathBuf::from("/songs/a.ogg"),
            charter_core::TicksPerBeat::default(),
            None,
            Tracker::Mapperatorinator,
            &mut window,
        );
        assert!(
            window
                .message()
                .is_some_and(|message| message.contains("no data folder")),
            "{:?}",
            window.message()
        );
        assert!(matches!(finder.state, State::Idle));
        assert!(!finder.update(&mut window));
    }

    #[test]
    fn the_models_timing_points_become_sections_and_a_bar_of_no_beats_is_refused() {
        use charter_mapperatorinator_burn::TimingPoint;
        let point = |offset_us, ms_per_beat, meter| TimingPoint {
            offset_us,
            ms_per_beat,
            meter,
        };
        let found = sections(&[point(4_000, 600.0, 4), point(2_382_000, 300.0, 7)]).unwrap();
        assert_eq!(found.len(), 2);
        assert!((found[0].start.seconds() - 0.004).abs() < 1e-12);
        assert!((found[1].beat.seconds() - 0.3).abs() < 1e-12);
        assert_eq!(found[1].meter.get(), 7);
        for meter in [0, -1, 70_000] {
            sections(&[point(0, 500.0, meter)]).unwrap_err();
        }
    }

    #[test]
    fn the_models_invalid_beat_lengths_are_refused() {
        use charter_mapperatorinator_burn::TimingPoint;
        for ms_per_beat in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::from_bits(1)] {
            let error = sections(&[TimingPoint {
                offset_us: 0,
                ms_per_beat,
                meter: 4,
            }])
            .unwrap_err();
            assert!(error.contains("beat"), "{ms_per_beat}: {error}");
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
                .url
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
