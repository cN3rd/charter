//! A summary of a track for drawing its waveform: the lowest and highest sample over each block
//! of frames, at resolutions from fine to coarse, so any stretch of time reads in a few steps.

use crate::Track;

/// Frames a block holds at the finest resolution.
const FINEST: usize = 64;

/// The lowest and highest sample of a stretch of audio, both channels together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub low: f32,
    pub high: f32,
}

impl Extent {
    const SILENT: Self = Self {
        low: 0.0,
        high: 0.0,
    };

    /// The extent covering both.
    fn and(self, other: Self) -> Self {
        Self {
            low: self.low.min(other.low),
            high: self.high.max(other.high),
        }
    }

    fn of(frames: &[[f32; 2]]) -> Self {
        let samples = frames.iter().flatten().copied();
        samples.fold(Self::SILENT, |extent, sample| Self {
            low: extent.low.min(sample),
            high: extent.high.max(sample),
        })
    }
}

/// A track's extents over blocks of frames, each level's blocks twice as long as the last's.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    rate: f64,
    /// How many frames the track has.
    frames: usize,
    levels: Vec<Vec<Extent>>,
}

impl Peaks {
    /// The peaks of `track`.
    #[must_use]
    pub fn of(track: &Track) -> Self {
        let finest: Vec<Extent> = track.frames().chunks(FINEST).map(Extent::of).collect();
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
            rate: f64::from(track.rate().get()),
            frames: track.frames().len(),
            levels,
        }
    }

    /// The extent of the audio from `from` to `to` seconds, silence outside the track. It may
    /// reach a little past either end, to the edges of the blocks it reads.
    #[must_use]
    pub fn between(&self, from: f64, to: f64) -> Extent {
        let (start, end) = (from * self.rate, to * self.rate);
        if !(start.is_finite() && end.is_finite()) || end < 0.0 {
            return Extent::SILENT;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to the track
        let frames = |at: f64| (at.max(0.0) as usize).min(self.frames);
        let (start, end) = (frames(start.floor()), frames(end.ceil()));
        if start >= self.frames {
            return Extent::SILENT;
        }
        let end = end.max(start + 1);
        // The coarsest level that still gives the stretch a few blocks.
        let span = end - start;
        let level = (0..self.levels.len())
            .rev()
            .find(|&level| span / (FINEST << level) >= 4)
            .unwrap_or(0);
        let block = FINEST << level;
        let Some(blocks) = self.levels.get(level) else {
            return Extent::SILENT;
        };
        let (first, last) = (start / block, end.div_ceil(block).min(blocks.len()));
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
