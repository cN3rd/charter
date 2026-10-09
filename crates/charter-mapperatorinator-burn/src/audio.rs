//! Audio in, as the model hears it: mono samples at 16 kHz scaled to a peak of 1, cut into
//! windows the length of the model's context. The caller decodes, resamples and scales the
//! samples, as upstream's `load_audio_file` and `normalize_audio_samples` do; charter does so
//! through `charter-audio`.
//!
//! Ported from OliBomby/Mapperatorinator (MIT), `osuT5/osuT5/inference/preprocessor.py`
//! (`Preprocessor.segment` on its parallel path, whose windows do not overlap).

/// Samples a second of the audio the model hears.
pub const SAMPLE_RATE: usize = 16000;
/// Samples from one mel frame to the next.
pub(crate) const HOP: usize = 128;
/// Mel frames a window holds, less the one `center=True` adds: `src_seq_len - 1`.
pub(crate) const FRAMES: usize = 2047;
/// Samples in a window: 262016, or 16376 ms.
pub(crate) const WINDOW: usize = FRAMES * HOP;
/// Milliseconds a window lasts.
pub(crate) const WINDOW_MS: f64 = count_float(WINDOW * 1000) / count_float(SAMPLE_RATE);

/// Windows of a song for one pass of super timing.
pub(crate) struct Segments {
    /// Each window's samples, `WINDOW` long.
    pub(crate) windows: Vec<Vec<f32>>,
    /// Each window's start in milliseconds, counted from the start of the padded audio.
    pub(crate) frame_times: Vec<i64>,
    /// The length of the audio before padding, in milliseconds.
    pub(crate) song_length: f64,
}

/// `samples` after `begin_pad` zeros, cut into back-to-back windows, the last padded with zeros.
pub(crate) fn segment(samples: &[f32], begin_pad: usize) -> Segments {
    let song_length = count_float(samples.len()) / count_float(SAMPLE_RATE) * 1000.0;
    let total = begin_pad.saturating_add(samples.len());
    let padding = if total < WINDOW {
        WINDOW.saturating_sub(total)
    } else {
        let remainder = total.saturating_sub(WINDOW) % WINDOW;
        if remainder == 0 {
            0
        } else {
            WINDOW.saturating_sub(remainder)
        }
    };
    let mut padded = vec![0.0_f32; begin_pad];
    padded.extend_from_slice(samples);
    padded.resize(total.saturating_add(padding), 0.0);
    let count = padded
        .len()
        .saturating_sub(WINDOW)
        .div_euclid(WINDOW)
        .saturating_add(1);
    Segments {
        // The padding makes the audio a whole number of windows, `count` of them.
        windows: padded
            .as_chunks::<WINDOW>()
            .0
            .iter()
            .map(|window| window.to_vec())
            .collect(),
        frame_times: (0..count)
            .map(|i| truncated_ms(count_float(i) * WINDOW_MS))
            .collect(),
        song_length,
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "sample and window counts of a song are far below 2^52, where f64 is exact"
)]
/// A count of samples or windows as a float.
const fn count_float(count: usize) -> f64 {
    count as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "song times in milliseconds are far inside i64; truncating toward zero is intended"
)]
/// `ms` truncated toward zero to whole milliseconds.
pub(crate) const fn truncated_ms(ms: f64) -> i64 {
    ms as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_back_to_back_and_the_last_is_padded() {
        let samples = vec![1.0_f32; WINDOW + 10];
        let s = segment(&samples, 5);
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.frame_times, vec![0, 16376]);
        assert_eq!(&s.windows[0][..6], &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.windows[1][14], 1.0);
        assert_eq!(s.windows[1][15], 0.0);
        assert_eq!(s.song_length, count_float(WINDOW + 10) / 16.0);
    }

    #[test]
    fn a_short_song_is_one_padded_window() {
        let s = segment(&[1.0; 10], 0);
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].len(), WINDOW);
    }
}
