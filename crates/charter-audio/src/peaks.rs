//! A summary of a track for drawing its waveform: the lowest and highest sample over each block
//! of frames, at resolutions from fine to coarse, so any stretch of time reads in a few steps.

use crate::{Track, cast};

/// Frames a block holds at the finest resolution.
const FINEST: usize = 64;

/// The lowest and highest sample of a stretch of audio, both channels together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    /// Lowest sample across both channels.
    pub low: f32,
    /// Highest sample across both channels.
    pub high: f32,
}

impl Extent {
    /// The zero-amplitude extent used when no frames contribute.
    const SILENT: Self = Self {
        low: 0.0,
        high: 0.0,
    };

    /// The extent covering both.
    const fn and(self, other: Self) -> Self {
        Self {
            low: self.low.min(other.low),
            high: self.high.max(other.high),
        }
    }

    /// The minimum and maximum across both channels of `frames`, including zero.
    fn of(frames: &[[f32; 2]]) -> Self {
        let samples = frames.iter().flatten().copied();
        samples.fold(Self::SILENT, |extent, sample| Self {
            low: extent.low.min(sample),
            high: extent.high.max(sample),
        })
    }
}

/// What starts kept peaks, naming the layout: a change to it changes this.
const MAGIC: &[u8; 8] = b"CHPEAKS1";

/// Bytes before the extents: the magic, the source's fingerprint, the rate and the frame count.
const HEADER: usize = MAGIC.len() + 8 + 8 + 8;

/// A fingerprint of an audio file's `bytes`, 64-bit FNV-1a, to tell whether kept peaks are of
/// it. Not for security: a file made to collide only gets itself a wrong waveform.
#[must_use]
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A track's extents over blocks of frames, each level's blocks twice as long as the last's.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    /// Source frames per second, used to turn requested times into block indices.
    rate: f64,
    /// How many frames the track has.
    frames: usize,
    /// Cached extents from 64-frame blocks through successively doubled blocks.
    levels: Vec<Vec<Extent>>,
}

impl Peaks {
    /// The peaks of `track`.
    #[must_use]
    pub fn of(track: &Track) -> Self {
        let finest: Vec<Extent> = track.frames().chunks(FINEST).map(Extent::of).collect();
        Self::from_finest(f64::from(track.rate().get()), track.frames().len(), finest)
    }

    /// The peaks whose finest level is `finest`, of a track of `frames` frames at `rate`, with
    /// every coarser level worked out from it.
    fn from_finest(rate: f64, frames: usize, finest: Vec<Extent>) -> Self {
        let mut levels = vec![finest];
        while let Some(last) = levels.last()
            && last.len() > 1
        {
            let coarser = last
                .chunks(2)
                .map(|pair| pair.iter().fold(Extent::SILENT, |all, &one| all.and(one)))
                .collect();
            levels.push(coarser);
        }
        Self {
            rate,
            frames,
            levels,
        }
    }

    /// The peaks as bytes to keep, marked as the peaks of the audio whose [`fingerprint`] is
    /// `source`. Only the finest level is kept, since the rest follow from it.
    #[must_use]
    pub fn to_bytes(&self, source: u64) -> Vec<u8> {
        let finest = self.levels.first().map_or(&[][..], Vec::as_slice);
        let mut bytes = Vec::with_capacity(HEADER.saturating_add(finest.len().saturating_mul(8)));
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&source.to_le_bytes());
        bytes.extend_from_slice(&self.rate.to_le_bytes());
        bytes.extend_from_slice(&cast::usize_u64(self.frames).to_le_bytes());
        for extent in finest {
            bytes.extend_from_slice(&extent.low.to_le_bytes());
            bytes.extend_from_slice(&extent.high.to_le_bytes());
        }
        bytes
    }

    /// The peaks kept in `bytes` by [`Peaks::to_bytes`], if they are whole and of the audio
    /// whose fingerprint is `source`. Anything else, damaged, of other audio or of an older
    /// layout, is `None`, and the peaks are worked out again.
    #[must_use]
    pub fn from_bytes(bytes: &[u8], source: u64) -> Option<Self> {
        let (header, body) = bytes.split_at_checked(HEADER)?;
        let (magic, header) = header.split_at(MAGIC.len());
        let word =
            |at: usize| -> Option<[u8; 8]> { header.get(at..at.checked_add(8)?)?.try_into().ok() };
        if magic != MAGIC || u64::from_le_bytes(word(0)?) != source {
            return None;
        }
        let rate = f64::from_le_bytes(word(8)?);
        let frames = usize::try_from(u64::from_le_bytes(word(16)?)).ok()?;
        if !(rate.is_finite() && rate > 0.0)
            || Some(body.len()) != frames.div_ceil(FINEST).checked_mul(8)
        {
            return None;
        }
        let (pairs, _) = body.as_chunks::<8>();
        let finest = pairs
            .iter()
            .map(|&[a, b, c, d, e, f, g, h]| {
                let (low, high) = (
                    f32::from_le_bytes([a, b, c, d]),
                    f32::from_le_bytes([e, f, g, h]),
                );
                (low.is_finite() && high.is_finite() && low <= high).then_some(Extent { low, high })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self::from_finest(rate, frames, finest))
    }

    /// The extent of the audio from `from` to `to` seconds, silence outside the track. It may
    /// reach a little past either end, to the edges of the blocks it reads.
    #[must_use]
    pub fn between(&self, from: f64, to: f64) -> Extent {
        let (start, end) = (from * self.rate, to * self.rate);
        if !(start.is_finite() && end.is_finite()) || end < 0.0 {
            return Extent::SILENT;
        }
        let frames = |at: f64| cast::f64_usize(at.max(0.0)).min(self.frames);
        let (start, end) = (frames(start.floor()), frames(end.ceil()));
        if start >= self.frames {
            return Extent::SILENT;
        }
        let end = end.max(start.saturating_add(1));
        // The coarsest level that still gives the stretch a few blocks.
        let span = end.saturating_sub(start);
        let level = (0..self.levels.len())
            .rev()
            .find(|&level| {
                FINEST
                    .checked_shl(u32::try_from(level).unwrap_or(u32::MAX))
                    .is_some_and(|block| block != 0 && span.div_euclid(block) >= 4)
            })
            .unwrap_or(0);
        let block = FINEST
            .checked_shl(u32::try_from(level).unwrap_or(u32::MAX))
            .unwrap_or(FINEST);
        let Some(blocks) = self.levels.get(level) else {
            return Extent::SILENT;
        };
        let (first, last) = (
            start.div_euclid(block),
            end.div_ceil(block).min(blocks.len()),
        );
        let covered = blocks.get(first..last).unwrap_or_default();
        covered
            .iter()
            .fold(Extent::SILENT, |all, &one| all.and(one))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track at 1000 frames a second: silence, then a second of a square wave at `amplitude`,
    /// then silence, each a second long.
    fn burst(amplitude: f32) -> Track {
        let mut frames = vec![[0.0, 0.0]; 1000];
        frames.extend((0..1000).map(|frame| {
            let sample = if frame % 2 == 0 {
                amplitude
            } else {
                -amplitude
            };
            [sample, sample * 0.5]
        }));
        frames.extend(vec![[0.0, 0.0]; 1000]);
        Track::from_frames(1000, frames)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn saved_peaks_read_back_the_same_for_the_same_audio() {
        let peaks = Peaks::of(&burst(0.8));
        let saved = peaks.to_bytes(7);
        assert_eq!(Peaks::from_bytes(&saved, 7), Some(peaks));
        assert_eq!(Peaks::from_bytes(&saved, 8), None, "other audio");
    }

    #[test]
    fn damaged_saved_peaks_are_refused_without_panicking() {
        let saved = Peaks::of(&burst(0.8)).to_bytes(7);
        for cut in 0..saved.len() {
            assert_eq!(Peaks::from_bytes(&saved[..cut], 7), None, "cut at {cut}");
        }
        let mut longer = saved.clone();
        longer.push(0);
        assert_eq!(Peaks::from_bytes(&longer, 7), None);
        let mut other = saved.clone();
        other[0] = b'X';
        assert_eq!(Peaks::from_bytes(&other, 7), None);
        let mut not_a_number = saved;
        let last = not_a_number.len() - 4;
        not_a_number[last..].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Peaks::from_bytes(&not_a_number, 7), None);
    }

    #[test]
    fn a_fingerprint_tells_different_audio_apart() {
        assert_eq!(fingerprint(b"audio"), fingerprint(b"audio"));
        assert_ne!(fingerprint(b"audio"), fingerprint(b"audia"));
        assert_ne!(fingerprint(b""), fingerprint(b"\0"));
    }

    #[test]
    fn a_loud_stretch_reads_as_its_extent_and_a_quiet_one_as_silence() {
        let peaks = Peaks::of(&burst(0.8));
        let loud = peaks.between(1.2, 1.4);
        assert!(near(loud.low, -0.8) && near(loud.high, 0.8), "{loud:?}");
        let quiet = peaks.between(0.1, 0.5);
        assert!(near(quiet.low, 0.0) && near(quiet.high, 0.0), "{quiet:?}");
    }

    #[test]
    fn a_long_stretch_reads_as_quickly_as_a_short_one_and_as_truly() {
        let peaks = Peaks::of(&burst(0.5));
        let whole = peaks.between(0.0, 3.0);
        assert!(near(whole.low, -0.5) && near(whole.high, 0.5), "{whole:?}");
        assert!(peaks.levels.len() > 3);
    }

    #[test]
    fn time_outside_the_track_is_silent() {
        let peaks = Peaks::of(&burst(0.5));
        assert_eq!(peaks.between(-5.0, -1.0), Extent::SILENT);
        assert_eq!(peaks.between(10.0, 12.0), Extent::SILENT);
        assert_eq!(peaks.between(f64::NAN, 1.0), Extent::SILENT);
    }

    #[test]
    fn a_stretch_shorter_than_a_block_still_reads_the_block_it_falls_in() {
        let peaks = Peaks::of(&burst(0.5));
        let brief = peaks.between(1.5, 1.5001);
        assert!(near(brief.high, 0.5), "{brief:?}");
    }
}
