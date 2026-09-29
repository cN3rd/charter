//! A decoded audio file.

use std::io::Cursor;
use std::num::NonZeroU32;

use crate::cast;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia_wrap as symphonia;

/// Audio decoded to stereo frames of `f32` samples, left then right.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// Nonzero sample rate in frames per second.
    rate: NonZeroU32,
    /// Interleaved-in-time stereo samples, left then right per frame.
    frames: Vec<[f32; 2]>,
}

/// Why bytes did not decode.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The file could not be recognised or decoded.
    #[error("not audio charter can read: {0}")]
    Unreadable(String),
    /// The file contains no decodable audio track.
    #[error("the file has no audio track")]
    NoTrack,
}

impl Track {
    /// Decodes `bytes`, an audio file whose name ends in `extension`, if it is known.
    ///
    /// # Errors
    ///
    /// Returns why the bytes are not audio charter can read.
    pub fn decode(bytes: Vec<u8>, extension: Option<&str>) -> Result<Self, DecodeError> {
        let unreadable =
            |error: symphonia::core::errors::Error| DecodeError::Unreadable(error.to_string());
        let source = MediaSourceStream::new(
            Box::new(Cursor::new(bytes)),
            MediaSourceStreamOptions::default(),
        );
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }
        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                source,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(unreadable)?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or(DecodeError::NoTrack)?;
        let id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(DecodeError::NoTrack)?;
        let mut codec = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(unreadable)?;
        let mut rate = None;
        let mut frames = Vec::new();
        let mut samples: Vec<f32> = Vec::new();
        while let Some(packet) = format.next_packet().map_err(unreadable)? {
            if packet.track_id != id {
                continue;
            }
            let buffer = match codec.decode(&packet) {
                Ok(buffer) => buffer,
                // A damaged packet is skipped, as players do; the rest still plays.
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(error) => return Err(unreadable(error)),
            };
            let spec = buffer.spec();
            rate = rate.or_else(|| NonZeroU32::new(spec.rate()));
            let channels = spec.channels().count();
            buffer.copy_to_vec_interleaved(&mut samples);
            frames.extend(samples.chunks(channels.max(1)).map(|frame| match frame {
                [left, right, ..] => [*left, *right],
                [mono] => [*mono, *mono],
                [] => [0.0, 0.0],
            }));
        }
        let rate = rate.ok_or(DecodeError::NoTrack)?;
        Ok(Self { rate, frames })
    }

    /// `frames` at `rate` frames per second, using 44.1 kHz when the rate is zero.
    #[must_use]
    pub fn from_frames(rate: u32, frames: Vec<[f32; 2]>) -> Self {
        let rate = NonZeroU32::new(rate).unwrap_or(NonZeroU32::MIN.saturating_add(44_099));
        Self { rate, frames }
    }

    /// `frames` frames of silence at `rate` frames per second.
    #[must_use]
    pub fn silence(rate: u32, frames: usize) -> Self {
        Self::from_frames(rate, vec![[0.0, 0.0]; frames])
    }

    /// Frames per second.
    #[must_use]
    pub const fn rate(&self) -> NonZeroU32 {
        self.rate
    }

    /// Stereo frames in playback order.
    #[must_use]
    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }

    /// How long the track plays at its own speed, in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        let frames = cast::usize_f64(self.frames.len());
        frames / f64::from(self.rate.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A 16-bit PCM WAV file of `channels` interleaved channels at `rate` holding `samples`.
    #[expect(
        clippy::little_endian_bytes,
        reason = "WAV and RIFF chunk sizes and samples are little-endian"
    )]
    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data = u32::try_from(samples.len().saturating_mul(2)).unwrap();
        let block = channels.saturating_mul(2);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&36_u32.saturating_add(data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&rate.saturating_mul(u32::from(block)).to_le_bytes());
        bytes.extend_from_slice(&block.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn a_stereo_file_decodes_to_its_frames_at_its_rate() {
        let track = Track::decode(wav(8000, 2, &[16384, -16384, 0, 32767]), Some("wav")).unwrap();
        assert_eq!(track.rate().get(), 8000);
        let frames = track.frames();
        assert_eq!(frames.len(), 2);
        assert!(near(frames[0][0], 0.5) && near(frames[0][1], -0.5));
        assert!(near(frames[1][0], 0.0) && near(frames[1][1], 1.0));
    }

    #[test]
    fn a_mono_file_plays_the_same_in_both_ears() {
        let track = Track::decode(wav(44100, 1, &[8192, -8192, 0]), None).unwrap();
        let frames = track.frames();
        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|[left, right]| near(*left, *right)));
        assert!(near(frames[0][0], 0.25));
    }

    #[test]
    fn a_track_lasts_its_frames_over_its_rate() {
        let track = Track::decode(wav(4000, 1, &[0; 1000]), Some("wav")).unwrap();
        assert!((track.seconds() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn bytes_that_are_not_audio_are_refused() {
        let refused = Track::decode(b"not audio at all".to_vec(), Some("ogg")).unwrap_err();
        assert!(matches!(refused, DecodeError::Unreadable(_)));
    }

    #[test]
    fn frames_at_a_rate_of_zero_are_taken_as_cd_audio() {
        assert_eq!(Track::from_frames(0, Vec::new()).rate().get(), 44_100);
    }

    #[test]
    fn a_file_named_without_an_extension_still_decodes() {
        let track = Track::decode(wav(8000, 1, &[0, 0]), None).unwrap();
        assert_eq!(track.frames().len(), 2);
    }

    #[test]
    fn an_empty_audio_stream_does_not_make_a_track_without_a_sample_rate() {
        let result = Track::decode(wav(8000, 1, &[]), Some("wav"));
        assert!(matches!(result, Err(DecodeError::NoTrack)));
    }

    #[test]
    fn a_truncated_audio_header_is_refused_without_panicking() {
        let bytes = wav(8000, 1, &[0, 0]);
        for end in [0, 4, 16, 36] {
            assert!(
                Track::decode(bytes[..end].to_vec(), Some("wav")).is_err(),
                "truncation at {end}"
            );
        }
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_audio_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            drop(Track::decode(bytes, Some("wav")));
        }
    }
}
