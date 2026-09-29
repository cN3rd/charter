//! Beats and downbeats found in a song's audio by Beat This! (Foscarin, Schlüter and Widmer,
//! ISMIR 2024), reimplemented in burn from the reference at `github.com/CPJKU/beat_this` and
//! loading its released weights. Audio becomes a log-mel spectrogram at 50 frames a second, the
//! network gives each frame a beat and a downbeat logit, and peaks of those are the beats.
//!
//! This is the front end of automatic timing: the beats it finds are a frame apart at best, so
//! whoever turns them into a tempo map refines them against the audio first.

mod mel;
mod model;

use mel::{FPS, log_mel, resample};

use std::path::Path;

use burn::backend::flex::{Flex, FlexDevice};
use burn::prelude::*;
use burn_store::{ModuleSnapshot, PytorchStore, SafetensorsStore};

use crate::model::{BeatThis, Dims};

/// The backend the tracker runs on: the CPU, through burn's flex backend.
type Cpu = Flex;

/// Frames the network sees at once, 30 seconds, as it was trained.
const CHUNK: usize = 1500;
/// Frames at each end of a chunk whose predictions are dropped, since the network was never
/// trained on its input's edges.
const BORDER: usize = 6;

/// Failures while loading the model or converting input audio.
#[expect(
    clippy::error_impl_error,
    reason = "this is the crate's recoverable error type for weights and resampling"
)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The checkpoint did not load into the network.
    #[error("the weights would not load: {0}")]
    Weights(String),
    /// The audio could not be converted to the network's sample rate.
    #[error("the audio would not resample: {0}")]
    Resample(String),
}

/// The Beat This! network with its weights, ready to track beats.
#[derive(Debug)]
pub struct Tracker {
    /// Loaded network parameters for inference on the CPU.
    model: BeatThis<Cpu>,
    /// Device used to construct each inference tensor.
    device: FlexDevice,
}

/// What the tracker finds: each frame's beat and downbeat logits, and the beats and downbeats
/// they peak at, in seconds from the start of the audio, earliest first. Every downbeat is also
/// a beat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Beats {
    /// Network beat score at each 50 Hz spectrogram frame.
    pub beat_logits: Vec<f32>,
    /// Network downbeat score at each spectrogram frame.
    pub downbeat_logits: Vec<f32>,
    /// Beat positions in seconds from the start of the audio.
    pub beats: Vec<f64>,
    /// Downbeat positions in seconds, each also a beat.
    pub downbeats: Vec<f64>,
}

impl Tracker {
    /// The tracker with the released weights in the Lightning checkpoint at `path`, such as
    /// `final0.ckpt` from the authors' cloud space.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Weights`] when the file is not a Beat This! checkpoint of the full-size
    /// model.
    pub fn from_checkpoint(path: &Path) -> Result<Self, Error> {
        // ponytail: burn-store reads a zipped PyTorch checkpoint only from a file, so this is the
        // one place the crate touches the disk; the host still chooses the file.
        let mut store = remapped(PytorchStore::from_file(path).with_top_level_key("state_dict"));
        Self::loaded(Dims::FULL, |model| {
            model
                .load_from(&mut store)
                .map(|_| ())
                .map_err(|why| Error::Weights(why.to_string()))
        })
    }

    /// The tracker of `dims` with the weights in `bytes`, a safetensors file keyed as the
    /// checkpoint's state dict is.
    #[cfg(test)]
    fn from_safetensors(dims: Dims, bytes: Vec<u8>) -> Result<Self, Error> {
        let mut store = remapped(
            SafetensorsStore::from_bytes(Some(bytes))
                .with_from_adapter(burn_store::PyTorchToBurnAdapter),
        );
        Self::loaded(dims, |model| {
            model
                .load_from(&mut store)
                .map(|_| ())
                .map_err(|why| Error::Weights(why.to_string()))
        })
    }

    /// A model of `dims` populated by `load` on the CPU device.
    fn loaded(
        dims: Dims,
        load: impl FnOnce(&mut BeatThis<Cpu>) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        let device = FlexDevice;
        let mut model = BeatThis::new(dims, &device);
        load(&mut model)?;
        Ok(Self { model, device })
    }

    /// The beats in `samples`, mono audio at `rate` samples a second.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Resample`] when the audio cannot be brought to the network's rate.
    pub fn track(&self, samples: &[f32], rate: u32) -> Result<Beats, Error> {
        let audio = resample(samples, rate)?;
        let spect = log_mel(&audio);
        let (beat_logits, downbeat_logits) = self.logits(&spect);
        Ok(peaks(beat_logits, downbeat_logits))
    }

    /// Each frame's beat and downbeat logits for `spect`, frames of mel bins, found a chunk at a
    /// time with earlier chunks winning where two overlap.
    fn logits(&self, spect: &[[f32; mel::BINS]]) -> (Vec<f32>, Vec<f32>) {
        let frames = spect.len();
        let mut beat = vec![-1000.0; frames];
        let mut downbeat = vec![-1000.0; frames];
        let starts = chunk_starts(frames);
        // Chunks are independent, so each runs on a thread of its own; the network's element-wise
        // work is otherwise one core's.
        let predictions: Vec<(Vec<f32>, Vec<f32>)> = std::thread::scope(|scope| {
            #[expect(
                clippy::needless_collect,
                reason = "collecting spawns every chunk before the first join"
            )]
            let running: Vec<_> = starts
                .iter()
                .map(|&start| {
                    scope.spawn(move || {
                        let (input, len) = chunk(spect, start);
                        let tensor = Tensor::<Cpu, 1>::from_floats(input.as_slice(), &self.device)
                            .reshape([1, len, mel::BINS]);
                        let (beats, downbeats) = self.model.forward(tensor);
                        (floats(beats), floats(downbeats))
                    })
                })
                .collect();
            running
                .into_iter()
                .map(|handle| handle.join().unwrap_or_default())
                .collect()
        });
        // Later chunks go first, so earlier ones overwrite them where they overlap.
        for (&start, (beats, downbeats)) in starts.iter().zip(&predictions).rev() {
            for index in BORDER..beats.len().saturating_sub(BORDER) {
                let Ok(frame) = usize::try_from(start.saturating_add(as_signed(index))) else {
                    continue;
                };
                if let (Some(slot), Some(value)) = (beat.get_mut(frame), beats.get(index)) {
                    *slot = *value;
                }
                if let (Some(slot), Some(value)) = (downbeat.get_mut(frame), downbeats.get(index)) {
                    *slot = *value;
                }
            }
        }
        (beat, downbeat)
    }
}

/// `store` with the checkpoint's module paths renamed to the model's fields.
fn remapped<S: StoreRemap>(store: S) -> S {
    store
        .remap_key(r"^model\.", "")
        .remap_key(r"\.attnF\.", ".attn_f.")
        .remap_key(r"\.attnT\.", ".attn_t.")
        .remap_key(r"\.ffF\.", ".ff_f.")
        .remap_key(r"\.ffT\.", ".ff_t.")
        .remap_key(r"\.to_out\.0\.", ".to_out.")
        .remap_key(r"\.net\.0\.", ".norm.")
        .remap_key(r"\.net\.1\.", ".inner.")
        .remap_key(r"\.net\.4\.", ".outer.")
        .remap_key(r"\.layers\.(\d+)\.0\.", ".layers.$1.attn.")
        .remap_key(r"\.layers\.(\d+)\.1\.", ".layers.$1.ff.")
        .skip_unused()
}

/// The key remapping both stores offer, under one name.
trait StoreRemap: Sized {
    /// Rewrites checkpoint key paths from `from` to `to`.
    fn remap_key(self, from: &str, to: &str) -> Self;
    /// Leaves the checkpoint's own bookkeeping, such as `num_batches_tracked` and the rotary
    /// frequencies the model computes, unloaded.
    fn skip_unused(self) -> Self;
}

impl StoreRemap for PytorchStore {
    fn remap_key(self, from: &str, to: &str) -> Self {
        self.with_key_remapping(from, to)
    }

    fn skip_unused(self) -> Self {
        self.allow_partial(false).validate(true)
    }
}

impl StoreRemap for SafetensorsStore {
    fn remap_key(self, from: &str, to: &str) -> Self {
        self.with_key_remapping(from, to)
    }

    fn skip_unused(self) -> Self {
        self.allow_partial(false).validate(true)
    }
}

/// The values of a 1D tensor.
fn floats(tensor: Tensor<Cpu, 1>) -> Vec<f32> {
    tensor.into_data().to_vec::<f32>().unwrap_or_default()
}

/// Where each chunk of `frames` starts, as the reference splits a piece: the first a border
/// before the start, each next overlapping the one before by the two borders, and the last
/// moved back to end at the end of the piece.
fn chunk_starts(frames: usize) -> Vec<isize> {
    let (frames, chunk, border) = (as_signed(frames), as_signed(CHUNK), as_signed(BORDER));
    let step = chunk.saturating_sub(2_isize.saturating_mul(border));
    let limit = frames
        .checked_sub(border)
        .unwrap_or_else(|| border.saturating_sub(frames).saturating_neg());
    let mut starts: Vec<isize> = std::iter::successors(Some(border.saturating_neg()), |start| {
        Some(start.saturating_add(step))
    })
    .take_while(|start| *start < limit)
    .collect();
    if frames > step
        && let Some(last) = starts.last_mut()
    {
        *last = frames.saturating_sub(chunk.saturating_sub(border));
    }
    starts
}

/// `count` as a signed chunk position, saturating if too large.
fn as_signed(count: usize) -> isize {
    isize::try_from(count).unwrap_or(isize::MAX)
}

/// The frames of the chunk starting at `start`, padded with silence where it reaches past the
/// piece's start or, by up to a border, its end, flattened; and how many frames it holds.
fn chunk(spect: &[[f32; mel::BINS]], start: isize) -> (Vec<f32>, usize) {
    let frames = as_signed(spect.len());
    let (chunk, border) = (as_signed(CHUNK), as_signed(BORDER));
    let from = start.max(0);
    let to = start.saturating_add(chunk).min(frames);
    let left = start.saturating_neg().max(0);
    let right = border
        .min(start.saturating_add(chunk).saturating_sub(frames))
        .max(0);
    let len = usize::try_from(
        left.saturating_add(to.saturating_sub(from))
            .saturating_add(right),
    )
    .unwrap_or(0);
    let mut input = Vec::with_capacity(len.saturating_mul(mel::BINS));
    let silence = [0.0; mel::BINS];
    let rows = std::iter::repeat_n(&silence, usize::try_from(left).unwrap_or(0))
        .chain(
            spect
                .get(usize::try_from(from).unwrap_or(0)..usize::try_from(to).unwrap_or(0))
                .unwrap_or_default(),
        )
        .chain(std::iter::repeat_n(
            &silence,
            usize::try_from(right).unwrap_or(0),
        ));
    for row in rows {
        input.extend_from_slice(row);
    }
    (input, len)
}

/// The beats and downbeats the logits peak at.
///
/// A peak is a frame whose logit is the largest within three frames either side and above 0, so
/// a probability above one half. Runs of neighbouring peaks merge to their mean, and each downbeat
/// moves onto its nearest beat.
#[must_use]
pub(crate) fn peaks(beat_logits: Vec<f32>, downbeat_logits: Vec<f32>) -> Beats {
    let beats = merged(&peak_frames(&beat_logits));
    let mut downbeats: Vec<PredictionFrame> = merged(&peak_frames(&downbeat_logits))
        .into_iter()
        .filter_map(|downbeat| {
            beats
                .iter()
                .copied()
                .min_by(|a, b| a.distance(downbeat).total_cmp(&b.distance(downbeat)))
        })
        .collect();
    downbeats.dedup();
    Beats {
        beat_logits,
        downbeat_logits,
        beats: beats.into_iter().map(PredictionFrame::seconds).collect(),
        downbeats: downbeats
            .into_iter()
            .map(PredictionFrame::seconds)
            .collect(),
    }
}

/// The frames of `logits` that are the largest within three frames either side, and above 0.
fn peak_frames(logits: &[f32]) -> Vec<usize> {
    logits
        .iter()
        .enumerate()
        .filter(|&(frame, &value)| {
            let near = frame.saturating_sub(3)..frame.saturating_add(4).min(logits.len());
            value > 0.0
                && logits
                    .get(near)
                    .unwrap_or_default()
                    .iter()
                    .all(|other| *other <= value)
        })
        .map(|(frame, _)| frame)
        .collect()
}

/// A fractional frame in the network's prediction stream.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct PredictionFrame(f64);

impl PredictionFrame {
    /// Distance from another prediction frame, in frames.
    fn distance(self, other: Self) -> f64 {
        (self.0 - other.0).abs()
    }

    /// Position in seconds from the start of the audio.
    fn seconds(self) -> f64 {
        self.0 / f64::from(FPS)
    }
}

/// `frames` with each run of peaks at most a frame apart replaced by their mean.
fn merged(frames: &[usize]) -> Vec<PredictionFrame> {
    let mut merged: Vec<PredictionFrame> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for &frame in frames {
        if run
            .last()
            .is_some_and(|last| frame.saturating_sub(*last) > 1)
        {
            merged.push(mean(&run));
            run.clear();
        }
        run.push(frame);
    }
    if !run.is_empty() {
        merged.push(mean(&run));
    }
    merged
}

/// Mean frame position of a run of neighbouring peaks.
fn mean(frames: &[usize]) -> PredictionFrame {
    let sum: f64 = frames.iter().map(|frame| frame_float(*frame)).sum();
    let count = frame_float(frames.len());
    PredictionFrame(sum / count)
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "song-length frame positions are converted for peak averaging"
)]
/// A frame index as a floating-point value for peak averaging.
const fn frame_float(frame: usize) -> f64 {
    frame as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Little-endian floats.
    #[expect(
        clippy::little_endian_bytes,
        reason = "the reference model fixture stores float32 values in little-endian order"
    )]
    fn floats_of(bytes: &[u8]) -> Vec<f32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| f32::from_le_bytes(*word))
            .collect()
    }

    fn tiny() -> Tracker {
        let bytes = include_bytes!("../fixtures/tiny.safetensors").to_vec();
        Tracker::from_safetensors(Dims::TINY, bytes).unwrap()
    }

    #[test]
    fn malformed_or_wrong_sized_weights_are_rejected() {
        assert!(matches!(
            Tracker::from_safetensors(Dims::TINY, b"not safetensors".to_vec()),
            Err(Error::Weights(_))
        ));
        let wrong = Dims {
            stem: 16,
            ..Dims::TINY
        };
        assert!(matches!(
            Tracker::from_safetensors(
                wrong,
                include_bytes!("../fixtures/tiny.safetensors").to_vec()
            ),
            Err(Error::Weights(_))
        ));
    }

    #[test]
    fn a_misspelled_checkpoint_module_does_not_silently_load() {
        let mut bytes = include_bytes!("../fixtures/tiny.safetensors").to_vec();
        let key = b"model.frontend.blocks.0.partial.attnF.";
        let start = bytes
            .windows(key.len())
            .position(|window| window == key)
            .unwrap();
        bytes[start.saturating_add(key.len()).saturating_sub(2)] = b'X';
        assert!(matches!(
            Tracker::from_safetensors(Dims::TINY, bytes),
            Err(Error::Weights(_))
        ));
    }

    #[test]
    fn the_network_gives_the_reference_logits_for_the_reference_input() {
        let spect: Vec<[f32; mel::BINS]> = floats_of(include_bytes!("../fixtures/tiny_spect.f32"))
            .as_chunks::<{ mel::BINS }>()
            .0
            .to_vec();
        let tracker = tiny();
        let input = spect.iter().flatten().copied().collect::<Vec<_>>();
        let tensor = Tensor::<Cpu, 1>::from_floats(input.as_slice(), &tracker.device).reshape([
            1,
            spect.len(),
            mel::BINS,
        ]);
        let (beat, downbeat) = tracker.model.forward(tensor);
        for (got, want) in [
            (
                floats(beat),
                floats_of(include_bytes!("../fixtures/tiny_beat.f32")),
            ),
            (
                floats(downbeat),
                floats_of(include_bytes!("../fixtures/tiny_downbeat.f32")),
            ),
        ] {
            assert_eq!(got.len(), want.len());
            for (frame, (actual, expected)) in got.iter().zip(&want).enumerate() {
                assert!(
                    (actual - expected).abs() < 1e-4,
                    "frame {frame}: {actual} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn chunks_start_a_border_early_overlap_by_two_borders_and_end_at_the_end() {
        assert_eq!(chunk_starts(40), [-6]);
        assert_eq!(chunk_starts(3000), [-6, 1482, 1506]);
        let (input, len) = chunk(&vec![[1.0; mel::BINS]; 40], -6);
        assert_eq!(len, 52);
        assert_eq!(input.len(), 52 * mel::BINS);
        assert!(input[..6 * mel::BINS].iter().all(|value| *value == 0.0));
        assert!(input[46 * mel::BINS..].iter().all(|value| *value == 0.0));
    }

    #[test]
    fn pieces_shorter_than_the_border_still_start_with_silent_padding() {
        assert_eq!(chunk_starts(2), [-6]);
        assert_eq!(chunk_starts(5), [-6]);
        let (input, count) = chunk(&[[1.0; mel::BINS]; 2], -6);
        assert_eq!(count, 14);
        assert!(input[..6 * mel::BINS].iter().all(|sample| *sample == 0.0));
        assert!(
            input[6 * mel::BINS..8 * mel::BINS]
                .iter()
                .all(|sample| (*sample - 1.0).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn empty_short_and_exact_chunk_length_audio_have_their_edges_padded() {
        assert!(chunk_starts(0).is_empty());
        assert_eq!(chunk_starts(1), [-6]);
        assert_eq!(chunk_starts(CHUNK), [-6, 6]);
        let (input, len) = chunk(&[[3.0; mel::BINS]], -6);
        assert_eq!(len, 13);
        assert!(input[..6 * mel::BINS].iter().all(|value| *value == 0.0));
        assert!(
            input[6 * mel::BINS..7 * mel::BINS]
                .iter()
                .all(|value| (*value - 3.0).abs() < f32::EPSILON)
        );
        assert!(input[7 * mel::BINS..].iter().all(|value| *value == 0.0));

        let spect = vec![[2.0; mel::BINS]; CHUNK];
        let (first, first_len) = chunk(&spect, -6);
        let (last, last_len) = chunk(&spect, 6);
        assert_eq!((first_len, last_len), (CHUNK, CHUNK));
        assert!(first[..6 * mel::BINS].iter().all(|value| *value == 0.0));
        assert!(
            first[6 * mel::BINS..]
                .iter()
                .all(|value| (*value - 2.0).abs() < f32::EPSILON)
        );
        assert!(
            last[..(CHUNK - 6) * mel::BINS]
                .iter()
                .all(|value| (*value - 2.0).abs() < f32::EPSILON)
        );
        assert!(
            last[(CHUNK - 6) * mel::BINS..]
                .iter()
                .all(|value| *value == 0.0)
        );
    }

    #[test]
    fn peaks_are_local_maxima_above_one_half_and_downbeats_sit_on_beats() {
        let mut beat = vec![-5.0; 60];
        let mut downbeat = vec![-5.0; 60];
        for frame in [10, 35] {
            beat[frame] = 2.0;
        }
        beat[20] = 1.0;
        beat[21] = 1.0;
        beat[50] = -0.5;
        downbeat[11] = 3.0;
        let found = peaks(beat, downbeat);
        assert_eq!(found.beats, [0.2, 0.41, 0.7]);
        assert_eq!(found.downbeats, [0.2]);
    }

    #[test]
    fn adjacent_equal_peaks_merge_but_zero_logits_do_not_peak() {
        let found = peaks(vec![0.0, 1.0, 1.0, -1.0, 0.0, 0.0, -1.0, 2.0], vec![0.0; 8]);
        assert_eq!(found.beats, [0.03, 0.14]);
        assert!(found.downbeats.is_empty());
        let without_beats = peaks(vec![0.0, -1.0], vec![3.0, 0.0]);
        assert!(without_beats.beats.is_empty());
        assert!(without_beats.downbeats.is_empty());
    }

    #[test]
    fn close_downbeats_snap_to_one_beat_without_duplicates() {
        let mut beat = vec![-1.0; 30];
        beat[2] = 2.0;
        beat[20] = 2.0;
        let mut downbeat = vec![-1.0; 30];
        downbeat[3] = 1.0;
        downbeat[9] = 1.0;
        downbeat[21] = 1.0;
        let found = peaks(beat, downbeat);
        assert_eq!(found.beats, [0.04, 0.4]);
        assert_eq!(found.downbeats, [0.04, 0.4]);
    }

    #[test]
    fn tracking_empty_and_short_audio_uses_the_resampled_frame_count() {
        let tracker = tiny();
        assert_eq!(tracker.track(&[], mel::RATE).unwrap(), Beats::default());
        let found = tracker.track(&vec![0.0; 882], 44_100).unwrap();
        assert_eq!(found.beat_logits.len(), 2);
        assert_eq!(found.downbeat_logits.len(), 2);
        assert!(matches!(tracker.track(&[0.5], 0), Err(Error::Resample(_))));
    }

    #[test]
    fn a_whole_piece_is_tracked_a_chunk_at_a_time() {
        let tracker = tiny();
        let spect = vec![[0.5; mel::BINS]; 1600];
        let (beat, downbeat) = tracker.logits(&spect);
        assert_eq!((beat.len(), downbeat.len()), (1600, 1600));
        assert!(beat.iter().all(|logit| *logit > -1000.0));
    }
}
