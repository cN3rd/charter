//! Audio in, as the model hears it: mono samples at 16 kHz scaled to a peak of 1, cut into
//! windows the length of the model's context. The caller decodes, resamples and scales the
//! samples, as upstream's `load_audio_file` and `normalize_audio_samples` do; charter does so
//! through `charter-audio`.
//!
//! Ported from OliBomby/Mapperatorinator (MIT), `osuT5/osuT5/inference/preprocessor.py`
//! (`Preprocessor.segment` on its parallel path, whose windows do not overlap).

pub const SAMPLE_RATE: usize = 16000;
pub(crate) const HOP: usize = 128;
/// Mel frames a window holds, less the one `center=True` adds: `src_seq_len - 1`.
pub(crate) const FRAMES: usize = 2047;
/// Samples in a window: 262016, or 16376 ms.
pub(crate) const WINDOW: usize = FRAMES * HOP;
pub(crate) const WINDOW_MS: f64 = (WINDOW * 1000) as f64 / SAMPLE_RATE as f64;

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
    let song_length = samples.len() as f64 / SAMPLE_RATE as f64 * 1000.0;
    let total = begin_pad + samples.len();
    let padding = if total < WINDOW {
        WINDOW - total
    } else {
        let remainder = (total - WINDOW) % WINDOW;
        if remainder == 0 {
            0
        } else {
            WINDOW - remainder
        }
    };
    let mut padded = vec![0.0_f32; begin_pad];
    padded.extend_from_slice(samples);
    padded.resize(total + padding, 0.0);
    let count = (padded.len() - WINDOW) / WINDOW + 1;
    Segments {
        windows: (0..count)
            .map(|i| padded[i * WINDOW..(i + 1) * WINDOW].to_vec())
            .collect(),
        frame_times: (0..count).map(|i| (i as f64 * WINDOW_MS) as i64).collect(),
        song_length,
    }
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
        assert_eq!(s.song_length, (WINDOW + 10) as f64 / 16.0);
    }

    #[test]
    fn a_short_song_is_one_padded_window() {
        let s = segment(&[1.0; 10], 0);
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].len(), WINDOW);
    }
}
