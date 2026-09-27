//! Onsets: where transients start in a track, found the way tempo mappers find them. The track
//! is mixed to mono and cut into short frames; the spectral flux, how much louder each frame is
//! than the one before across all frequencies, peaks where something new starts. Peaks that
//! stand out from their neighbourhood are onsets, placed between frames by fitting a parabola
//! to the peak, then moved to the steepest rise of the track's envelope within 5 ms, which puts
//! a sharp hit within a fraction of a millisecond.

use std::sync::Arc;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::Track;

/// Samples a frame holds, about 12 ms at 44.1 kHz: short enough that an onset's frame puts it
/// within 5 ms, where the envelope finds it exactly.
const FRAME: usize = 512;
/// Samples from one frame to the next.
const HOP: usize = 128;
/// The hop as a real, for turning frames into samples.
const HOP_SAMPLES: f64 = 128.0;
/// How close two onsets may be, in seconds: closer, they are one hit.
const APART: f64 = 0.030;
/// How far back and ahead of a peak, in seconds, the neighbourhood it has to stand out from
/// reaches.
const BEFORE: f64 = 0.100;
const AFTER: f64 = 0.030;
/// How far from the peak's time, in seconds, the steepest rise of the envelope is looked for.
const REFINE: f64 = 0.005;
/// How much of the loudest peak a peak has to stand above its neighbourhood by.
const DELTA: f32 = 0.05;
/// The least flux a peak needs at all. A steady tone's frames differ by about 5 from one to the
/// next; a quiet click's rise is hundreds.
const FLOOR: f32 = 20.0;
/// How much the magnitudes are compressed before their differences are summed, so quiet
/// instruments count as well as loud ones.
const COMPRESSION: f32 = 100.0;

/// Where a transient starts, and how strong it is next to the strongest in the track, from just
/// above 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Onset {
    pub seconds: f64,
    pub strength: f32,
}

/// The onsets of `track`, earliest first.
#[must_use]
pub fn onsets(track: &Track) -> Vec<Onset> {
    let rate = f64::from(track.rate().get());
    let mono: Vec<f32> = track
        .frames()
        .iter()
        .map(|[left, right]| (left + right) / 2.0)
        .collect();
    let flux = flux(&mono);
    let frames_per = |seconds: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few hundred
        let frames = (seconds * rate / HOP_SAMPLES).ceil() as usize;
        frames.max(1)
    };
    let (apart, before, after) = (frames_per(APART), frames_per(BEFORE), frames_per(AFTER));
    let loudest = flux.iter().copied().fold(0.0, f32::max);
    if loudest <= f32::EPSILON {
        return Vec::new();
    }
    let mut peaks: Vec<(f64, f32)> = Vec::new();
    for (index, &value) in flux.iter().enumerate() {
        let near = flux
            .get(index.saturating_sub(apart)..(index + apart + 1).min(flux.len()))
            .unwrap_or_default();
        // The first of equal highest values in reach is the peak.
        let highest = near.iter().copied().fold(0.0, f32::max);
        let earlier_equal = flux
            .get(index.saturating_sub(apart)..index)
            .unwrap_or_default()
            .contains(&value);
        if value < highest || earlier_equal || value <= 0.0 {
            continue;
        }
        let around = flux
            .get(index.saturating_sub(before)..(index + after + 1).min(flux.len()))
            .unwrap_or_default();
        #[allow(clippy::cast_precision_loss)] // a few hundred values
        let mean = around.iter().sum::<f32>() / around.len() as f32;
        if value < FLOOR || value < mean + DELTA * loudest {
            continue;
        }
        #[allow(clippy::cast_precision_loss)] // a frame of a track, far below 2^52
        let at = index as f64 + parabola(&flux, index);
        // The flux of a frame rises as a hit reaches the middle of the window, at its hop.
        let sample = at * HOP_SAMPLES - HOP_SAMPLES / 2.0;
        peaks.push((sample, value - mean));
    }
    let strongest = peaks.iter().map(|(_, height)| *height).fold(0.0, f32::max);
    let envelope = envelope(&mono, rate);
    let reach = REFINE * rate;
    let mut found: Vec<Onset> = peaks
        .into_iter()
        .map(|(sample, height)| {
            let refined = steepest_rise(&envelope, sample, reach).unwrap_or(sample);
            Onset {
                seconds: refined.max(0.0) / rate,
                strength: (height / strongest).clamp(f32::MIN_POSITIVE, 1.0),
            }
        })
        .collect();
    found.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
    found.dedup_by(|later, earlier| later.seconds - earlier.seconds < APART / 2.0);
    found
}

/// The spectral flux of `mono`, one value a hop: the sum over frequencies of how much each grew
/// since the frame before, on compressed magnitudes.
fn flux(mono: &[f32]) -> Vec<f32> {
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FRAME);
    #[allow(clippy::cast_precision_loss)] // 512 samples
    let window: Vec<f32> = (0..FRAME)
        .map(|n| {
            let phase = std::f32::consts::TAU * n as f32 / FRAME as f32;
            0.5 - 0.5 * phase.cos()
        })
        .collect();
    let bins = FRAME / 2 + 1;
    let mut previous = vec![0.0f32; bins];
    let mut buffer = vec![Complex::new(0.0, 0.0); FRAME];
    let mut scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    // Each frame is centred on its hop, with silence before the track, so a hit on the first
    // sample sits in the middle of a frame rather than at the edge of its window. None reaches
    // past the end, where the cut to silence would read as a hit.
    let frames = mono.len().saturating_sub(FRAME / 2) / HOP + 1;
    let mut values = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = (frame * HOP).checked_sub(FRAME / 2);
        let skip = start.map_or_else(|| FRAME / 2 - frame * HOP, |_| 0);
        let from = start.unwrap_or(0);
        buffer.fill(Complex::new(0.0, 0.0));
        let samples = mono.get(from..).unwrap_or_default().iter();
        for ((slot, weight), sample) in buffer.iter_mut().zip(&window).skip(skip).zip(samples) {
            *slot = Complex::new(sample * weight, 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        let mut sum = 0.0;
        for (bin, old) in buffer.iter().take(bins).zip(previous.iter_mut()) {
            let magnitude = (1.0 + COMPRESSION * bin.norm()).ln();
            sum += (magnitude - *old).max(0.0);
            *old = magnitude;
        }
        // The first frame rises from silence, so a hit on the first sample counts like any other.
        values.push(sum);
    }
    values
}

/// Where the top of a parabola through the peak at `index` of `values` and its neighbours lies,
/// from half a step before it to half a step after.
fn parabola(values: &[f32], index: usize) -> f64 {
    let (Some(&before), Some(&peak), Some(&after)) = (
        index.checked_sub(1).and_then(|at| values.get(at)),
        values.get(index),
        values.get(index + 1),
    ) else {
        return 0.0;
    };
    let curve = before - 2.0 * peak + after;
    if curve.abs() <= f32::EPSILON {
        return 0.0;
    }
    f64::from((0.5 * (before - after) / curve).clamp(-0.5, 0.5))
}

/// The loudness of `mono` about each sample: its rectified samples averaged over the
/// millisecond around it.
fn envelope(mono: &[f32], rate: f64) -> Vec<f32> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // tens of samples
    let half = ((rate * 0.0005) as usize).max(1);
    let running: Vec<f64> = std::iter::once(0.0)
        .chain(mono.iter().scan(0.0, |sum, sample| {
            *sum += f64::from(sample.abs());
            Some(*sum)
        }))
        .collect();
    (0..mono.len())
        .map(|at| {
            let (from, to) = (at.saturating_sub(half), (at + half + 1).min(mono.len()));
            let span = running
                .get(to)
                .zip(running.get(from))
                .map_or(0.0, |(a, b)| a - b);
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // a mean
            let mean = (span / (to - from) as f64) as f32;
            mean
        })
        .collect()
}

/// The sample within `reach` samples of `around` where `envelope` rises fastest, if it rises at
/// all there.
fn steepest_rise(envelope: &[f32], around: f64, reach: f64) -> Option<f64> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // within the track
    let (from, to) = (
        (around - reach).max(1.0) as usize,
        ((around + reach) as usize).min(envelope.len().saturating_sub(1)),
    );
    let mut best: Option<(usize, f32)> = None;
    for at in from..to {
        let (Some(&low), Some(&high)) = (envelope.get(at - 1), envelope.get(at + 1)) else {
            continue;
        };
        let rise = high - low;
        if rise > 0.0 && best.is_none_or(|(_, steepest)| rise > steepest) {
            best = Some((at, rise));
        }
    }
    #[allow(clippy::cast_precision_loss)] // a sample index
    best.map(|(at, _)| at as f64)
}

/// What starts kept onsets, naming the layout and the detector: a change to either changes
/// this, so onsets found another way are found again.
const MAGIC: &[u8; 8] = b"CHONSET2";

/// `onsets` as bytes to keep, marked as those of the audio whose
/// [fingerprint](crate::fingerprint) is `source`.
#[must_use]
pub fn onsets_to_bytes(onsets: &[Onset], source: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24 + onsets.len() * 12);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&source.to_le_bytes());
    bytes.extend_from_slice(&(onsets.len() as u64).to_le_bytes());
    for onset in onsets {
        bytes.extend_from_slice(&onset.seconds.to_le_bytes());
        bytes.extend_from_slice(&onset.strength.to_le_bytes());
    }
    bytes
}

/// The onsets kept in `bytes` by [`onsets_to_bytes`], if they are whole, in order and of the
/// audio whose fingerprint is `source`.
#[must_use]
pub fn onsets_from_bytes(bytes: &[u8], source: u64) -> Option<Arc<[Onset]>> {
    let (header, body) = bytes.split_at_checked(24)?;
    let (magic, header) = header.split_at(8);
    let word = |at: usize| -> Option<[u8; 8]> { header.get(at..at + 8)?.try_into().ok() };
    if magic != MAGIC || u64::from_le_bytes(word(0)?) != source {
        return None;
    }
    let count = usize::try_from(u64::from_le_bytes(word(8)?)).ok()?;
    let (records, rest) = body.as_chunks::<12>();
    if records.len() != count || !rest.is_empty() {
        return None;
    }
    let onsets: Vec<Onset> = records
        .iter()
        .map(|record| {
            let (seconds, strength) = record.split_at(8);
            let seconds = f64::from_le_bytes(seconds.try_into().ok()?);
            let strength = f32::from_le_bytes(strength.try_into().ok()?);
            let sane = seconds.is_finite() && seconds >= 0.0 && (0.0..=1.0).contains(&strength);
            sane.then_some(Onset { seconds, strength })
        })
        .collect::<Option<_>>()?;
    let ordered = onsets.windows(2).all(|pair| match pair {
        [a, b] => a.seconds <= b.seconds,
        _ => true,
    });
    ordered.then(|| onsets.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Silence with a decaying 2 kHz click at each of `times`, each at its `level`.
    fn clicks(times: &[(f64, f32)], seconds: f64) -> Track {
        let rate = f64::from(RATE);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let length = (seconds * rate) as usize;
        let mut frames = vec![[0.0f32; 2]; length];
        for &(at, level) in times {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let start = (at * rate).round() as usize;
            for (offset, frame) in frames.iter_mut().skip(start).take(2000).enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let t = offset as f64 / rate;
                #[allow(clippy::cast_possible_truncation)]
                let sample =
                    ((std::f64::consts::TAU * 2000.0 * t).sin() * (-t / 0.008).exp()) as f32;
                *frame = [sample * level; 2];
            }
        }
        Track::from_frames(RATE, frames)
    }

    #[test]
    fn clicks_are_found_within_a_millisecond_of_where_they_start() {
        let times: Vec<(f64, f32)> = (0..12)
            .map(|beat| (0.25 + f64::from(beat) * 0.428_571, 0.8))
            .collect();
        let found = onsets(&clicks(&times, 6.0));
        assert_eq!(found.len(), times.len(), "{found:?}");
        for (onset, (at, _)) in found.iter().zip(&times) {
            assert!(
                (onset.seconds - at).abs() < 0.001,
                "{} vs {at}",
                onset.seconds
            );
            assert!(onset.strength > 0.5);
        }
    }

    #[test]
    fn a_hit_on_the_very_first_sample_is_found() {
        let found = onsets(&clicks(&[(0.0, 0.8), (0.5, 0.8)], 1.0));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].seconds < 0.001, "{found:?}");
    }

    #[test]
    fn a_quieter_hit_is_weaker_and_silence_has_none() {
        let found = onsets(&clicks(&[(0.5, 0.9), (1.0, 0.2)], 1.5));
        assert_eq!(found.len(), 2);
        assert!(found[1].strength < found[0].strength);
        assert!((found[0].strength - 1.0).abs() < f32::EPSILON);
        assert!(onsets(&Track::silence(RATE, 44_100)).is_empty());
    }

    #[test]
    fn a_steady_tone_has_at_most_the_onset_where_it_begins() {
        let frames: Vec<[f32; 2]> = (0..88_200u32)
            .map(|n| {
                #[allow(clippy::cast_precision_loss)] // two seconds of samples
                let sample = (std::f32::consts::TAU * 440.0 * n as f32 / 44_100.0).sin() * 0.5;
                [sample; 2]
            })
            .collect();
        let found = onsets(&Track::from_frames(RATE, frames));
        assert!(found.len() <= 1, "{found:?}");
    }

    #[test]
    fn kept_onsets_read_back_and_damaged_ones_are_refused() {
        let found = onsets(&clicks(&[(0.5, 0.9), (1.0, 0.2)], 1.5));
        let bytes = onsets_to_bytes(&found, 9);
        assert_eq!(
            onsets_from_bytes(&bytes, 9).as_deref(),
            Some(found.as_slice())
        );
        assert_eq!(onsets_from_bytes(&bytes, 8), None);
        for cut in 0..bytes.len() {
            assert_eq!(onsets_from_bytes(&bytes[..cut], 9), None, "cut at {cut}");
        }
        let mut unordered = onsets_to_bytes(&[found[1], found[0]], 9);
        assert_eq!(onsets_from_bytes(&unordered, 9), None);
        unordered[0] = b'X';
        assert_eq!(onsets_from_bytes(&unordered, 9), None);
    }
}
