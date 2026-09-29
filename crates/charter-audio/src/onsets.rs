//! Onsets: where transients start in a track, found the way tempo mappers find them. The track
//! is mixed to mono and cut into short frames; the spectral flux, how much louder each frame is
//! than the one before across all frequencies, peaks where something new starts. Peaks that
//! stand out from their neighbourhood are onsets, placed between frames by fitting a parabola
//! to the peak, then moved back to where the hit starts: where the track's loudness first climbs
//! a tenth of the way from the quiet before the peak to the peak, which is where a tempo mapper
//! puts a hit's beat, and within a fraction of a millisecond of a sharp hit's first sample.

use std::sync::Arc;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::{Track, cast};

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
/// How far after a peak, in seconds, the prominence neighbourhood reaches.
const AFTER: f64 = 0.030;
/// How far before and after the peak's time, in seconds, the hit's start is looked for.
const BACK: f64 = 0.030;
/// How far after a peak, in seconds, to search for the start of a hit.
const AHEAD: f64 = 0.010;
/// How far from the quiet before a hit to its loudest, as a share, the loudness has climbed where
/// the hit starts.
const START: f32 = 0.1;
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
        let frames = cast::f64_usize((seconds * rate / HOP_SAMPLES).ceil());
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
            .get(
                index.saturating_sub(apart)
                    ..index
                        .saturating_add(apart)
                        .saturating_add(1)
                        .min(flux.len()),
            )
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
            .get(
                index.saturating_sub(before)
                    ..index
                        .saturating_add(after)
                        .saturating_add(1)
                        .min(flux.len()),
            )
            .unwrap_or_default();
        let mean = around.iter().sum::<f32>() / cast::usize_f32(around.len());
        if value < FLOOR || value < mean + DELTA * loudest {
            continue;
        }
        let at = cast::usize_f64(index) + parabola(&flux, index);
        // The flux of a frame rises as a hit reaches the middle of the window, at its hop.
        let sample = at * HOP_SAMPLES - HOP_SAMPLES / 2.0;
        peaks.push((sample, value - mean));
    }
    let strongest = peaks.iter().map(|(_, height)| *height).fold(0.0, f32::max);
    let envelope = envelope(&mono, rate);
    let mut found: Vec<Onset> = peaks
        .into_iter()
        .map(|(sample, height)| {
            let refined = attack_start(&envelope, (sample - BACK * rate, sample + AHEAD * rate))
                .unwrap_or(sample);
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
    let window: Vec<f32> = (0..FRAME)
        .map(|n| {
            let phase = std::f32::consts::TAU * cast::usize_f32(n) / cast::usize_f32(FRAME);
            0.5 - 0.5 * phase.cos()
        })
        .collect();
    let bins = FRAME.div_euclid(2).saturating_add(1);
    let mut previous = vec![0.0f32; bins];
    let mut buffer = vec![Complex::new(0.0, 0.0); FRAME];
    let mut scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    // Each frame is centred on its hop, with silence before the track, so a hit on the first
    // sample sits in the middle of a frame rather than at the edge of its window. None reaches
    // past the end, where the cut to silence would read as a hit.
    let frames = mono
        .len()
        .saturating_sub(FRAME.div_euclid(2))
        .div_euclid(HOP)
        .saturating_add(1);
    let mut values = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = frame.saturating_mul(HOP).checked_sub(FRAME.div_euclid(2));
        let skip = start.map_or_else(
            || {
                FRAME
                    .div_euclid(2)
                    .saturating_sub(frame.saturating_mul(HOP))
            },
            |_| 0,
        );
        let from = start.unwrap_or(0);
        buffer.fill(Complex::new(0.0, 0.0));
        let samples = mono.get(from..).unwrap_or_default().iter();
        for ((slot, weight), sample) in buffer.iter_mut().zip(&window).skip(skip).zip(samples) {
            *slot = Complex::new(sample * weight, 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        let mut sum = 0.0;
        for (bin, old) in buffer.iter().take(bins).zip(previous.iter_mut()) {
            let magnitude = (COMPRESSION * bin.norm()).ln_1p();
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
        index.checked_add(1).and_then(|next| values.get(next)),
    ) else {
        return 0.0;
    };
    let curve = before - 2.0 * peak + after;
    if curve.abs() <= f32::EPSILON {
        return 0.0;
    }
    f64::from((0.5 * (before - after) / curve).clamp(-0.5, 0.5))
}

/// The loudness of `mono` up to each sample: its rectified samples averaged over the 5 ms before
/// it, long enough to span a low note's cycle and not reaching ahead of what has sounded.
fn envelope(mono: &[f32], rate: f64) -> Vec<f32> {
    let span = cast::f64_usize(rate * 0.005).max(1);
    let running: Vec<f64> = std::iter::once(0.0)
        .chain(mono.iter().scan(0.0, |sum, sample| {
            *sum += f64::from(sample.abs());
            Some(*sum)
        }))
        .collect();
    (0..mono.len())
        .map(|at| {
            let to = at.saturating_add(1);
            let from = to.saturating_sub(span);
            let sum = running
                .get(to)
                .zip(running.get(from))
                .map_or(0.0, |(a, b)| a - b);
            let mean = cast::f64_f32(sum / cast::usize_f64(to.saturating_sub(from)));
            mean
        })
        .collect()
}

/// Where a hit between the samples `from` and `to` starts: the first sample after the quietest
/// point before the loudest where `envelope` has climbed [`START`] of the way from the one to
/// the other. None where it does not climb.
fn attack_start(envelope: &[f32], (from, to): (f64, f64)) -> Option<f64> {
    let (from, to) = (
        cast::f64_usize(from.max(0.0)),
        cast::f64_usize(to.max(0.0)).min(envelope.len()),
    );
    let span = envelope.get(from..to)?;
    let (peak_at, &peak) = span.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    let (quiet_at, &quiet) = span
        .get(..=peak_at)?
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))?;
    if peak <= quiet {
        return None;
    }
    let threshold = quiet + START * (peak - quiet);
    let start = span
        .get(quiet_at..=peak_at)?
        .iter()
        .position(|level| *level >= threshold)?;
    let start = cast::usize_f64(from.saturating_add(quiet_at).saturating_add(start));
    Some(start)
}

/// What starts kept onsets, naming the layout and the detector: a change to either changes
/// this, so onsets found another way are found again.
const MAGIC: &[u8; 8] = b"CHONSET3";

/// `onsets` as bytes to keep, marked as those of the audio whose
/// [fingerprint](crate::fingerprint) is `source`.
#[must_use]
pub fn onsets_to_bytes(onsets: &[Onset], source: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24usize.saturating_add(onsets.len().saturating_mul(12)));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&source.to_le_bytes());
    bytes.extend_from_slice(&cast::usize_u64(onsets.len()).to_le_bytes());
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
    let word =
        |at: usize| -> Option<[u8; 8]> { header.get(at..at.checked_add(8)?)?.try_into().ok() };
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
        let length = cast::f64_usize(seconds * rate);
        let mut frames = vec![[0.0f32; 2]; length];
        for &(at, level) in times {
            let start = cast::f64_usize((at * rate).round());
            for (offset, frame) in frames.iter_mut().skip(start).take(2000).enumerate() {
                let t = cast::usize_f64(offset) / rate;
                let sample =
                    cast::f64_f32((std::f64::consts::TAU * 2000.0 * t).sin() * (-t / 0.008).exp());
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

    /// Silence with a kick at each of `times`: a 60 Hz sine swelling over 15 ms and dying away,
    /// the kind of hit whose waveform is still climbing well after it starts.
    fn kicks(times: &[f64], seconds: f64) -> Track {
        let rate = f64::from(RATE);
        let length = cast::f64_usize(seconds * rate);
        let mut frames = vec![[0.0f32; 2]; length];
        for &at in times {
            let start = cast::f64_usize((at * rate).round());
            for (offset, frame) in frames.iter_mut().skip(start).take(19_800).enumerate() {
                let t = cast::usize_f64(offset) / rate;
                let swell = (t / 0.015).min(1.0);
                let sample = cast::f64_f32(
                    (std::f64::consts::TAU * 60.0 * t).sin() * swell * (-t / 0.050).exp() * 0.8,
                );
                *frame = [sample; 2];
            }
        }
        Track::from_frames(RATE, frames)
    }

    #[test]
    fn a_hit_that_swells_is_found_within_a_few_milliseconds_of_where_it_starts() {
        let times: Vec<f64> = (0..8).map(|beat| 0.3 + f64::from(beat) * 0.5).collect();
        let found = onsets(&kicks(&times, 4.5));
        assert_eq!(found.len(), times.len(), "{found:?}");
        for (onset, at) in found.iter().zip(&times) {
            let early = at - onset.seconds;
            assert!(
                (-0.003..=0.003).contains(&early),
                "{} for {at}",
                onset.seconds
            );
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
                let sample =
                    (std::f32::consts::TAU * 440.0 * cast::f64_f32(f64::from(n)) / 44_100.0).sin()
                        * 0.5;
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
