//! What an audio file says about itself: the title and artist its tags carry.

use std::io::Cursor;

use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTag};
use symphonia_wrap as symphonia;

/// The song facts an audio file's tags carry, where it carries them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
}

// ponytail: symphonia 0.6.1's WAV reader drops the INFO tags it parses, so WAV files read as
// untagged; take them from the reader's metadata log once that is fixed upstream.
/// The tags of `bytes`, an audio file whose name ends in `extension`, if it is known. A file
/// that is not audio, or has no tags, has none.
#[must_use]
pub fn tags(bytes: Vec<u8>, extension: Option<&str>) -> Tags {
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe().probe(
        &hint,
        source,
        FormatOptions::default(),
        MetadataOptions::default(),
    );
    let Ok(mut format) = probed else {
        return Tags::default();
    };
    let mut found = Tags::default();
    let mut metadata = format.metadata();
    let Some(revision) = metadata.skip_to_latest() else {
        return found;
    };
    let every = revision.media.tags.iter().chain(
        revision
            .per_track
            .iter()
            .flat_map(|track| &track.metadata.tags),
    );
    for tag in every {
        let text = |text: &str| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        match &tag.std {
            Some(StandardTag::TrackTitle(title)) => found.title = found.title.or(text(title)),
            Some(StandardTag::Artist(artist)) => found.artist = found.artist.or(text(artist)),
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FLAC metadata block of `kind` holding `body`, the last one when `last` is set.
    fn block(kind: u8, last: bool, body: &[u8]) -> Vec<u8> {
        let length = u32::try_from(body.len()).unwrap().to_be_bytes();
        let mut bytes = vec![kind | if last { 0x80 } else { 0 }];
        bytes.extend_from_slice(&length[1..]);
        bytes.extend_from_slice(body);
        bytes
    }

    /// A FLAC file of no frames, mono at 8 kHz, whose Vorbis comments are `comments`, each
    /// written as `NAME=value`.
    fn tagged(comments: &[&str]) -> Vec<u8> {
        let mut info = Vec::new();
        info.extend_from_slice(&4096u16.to_be_bytes());
        info.extend_from_slice(&4096u16.to_be_bytes());
        info.extend_from_slice(&[0; 6]);
        // 8000 Hz in 20 bits, one channel as 0 in 3 bits, 16 bits a sample as 15 in 5 bits,
        // then 36 bits of total samples.
        let packed: u64 = (8000 << 44) | (15 << 36);
        info.extend_from_slice(&packed.to_be_bytes());
        info.extend_from_slice(&[0; 16]);
        let mut vorbis = Vec::new();
        let vendor = b"charter";
        vorbis.extend_from_slice(&u32::try_from(vendor.len()).unwrap().to_le_bytes());
        vorbis.extend_from_slice(vendor);
        vorbis.extend_from_slice(&u32::try_from(comments.len()).unwrap().to_le_bytes());
        for comment in comments {
            vorbis.extend_from_slice(&u32::try_from(comment.len()).unwrap().to_le_bytes());
            vorbis.extend_from_slice(comment.as_bytes());
        }
        let mut bytes = b"fLaC".to_vec();
        bytes.extend(block(0, false, &info));
        bytes.extend(block(4, true, &vorbis));
        bytes.extend(silent_frame());
        bytes
    }

    /// One FLAC frame of 16 silent mono samples: a frame header, a constant subframe of 0, and
    /// the checksums the format asks for.
    fn silent_frame() -> Vec<u8> {
        // Sync and fixed blocking; a block size given at the end as an 8-bit count less one, and
        // the sample rate the stream info gives; mono at 16 bits; frame 0; 16 samples.
        let mut frame = vec![0xFF, 0xF8, 0x60, 0x08, 0x00, 15];
        frame.push(crc8(&frame));
        // A constant subframe of 16-bit zero.
        frame.extend_from_slice(&[0x00, 0x00, 0x00]);
        let crc = crc16(&frame);
        frame.extend_from_slice(&crc.to_be_bytes());
        frame
    }

    fn crc8(bytes: &[u8]) -> u8 {
        bytes.iter().fold(0u8, |crc, byte| {
            (0..8).fold(crc ^ byte, |crc, _| {
                if crc & 0x80 == 0 {
                    crc << 1
                } else {
                    (crc << 1) ^ 0x07
                }
            })
        })
    }

    fn crc16(bytes: &[u8]) -> u16 {
        bytes.iter().fold(0u16, |crc, byte| {
            (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
                if crc & 0x8000 == 0 {
                    crc << 1
                } else {
                    (crc << 1) ^ 0x8005
                }
            })
        })
    }

    #[test]
    fn a_tagged_file_names_its_title_and_artist() {
        let bytes = tagged(&["TITLE=Starlight", "ARTIST=The Lanes"]);
        let tags = tags(bytes, Some("flac"));
        assert_eq!(tags.title.as_deref(), Some("Starlight"));
        assert_eq!(tags.artist.as_deref(), Some("The Lanes"));
    }

    #[test]
    fn a_file_without_tags_or_that_is_not_audio_has_none() {
        assert_eq!(tags(tagged(&[]), Some("flac")), Tags::default());
        assert_eq!(tags(b"not audio".to_vec(), Some("ogg")), Tags::default());
    }

    #[test]
    fn blank_tags_count_as_none() {
        let tags = tags(tagged(&["TITLE=  "]), Some("flac"));
        assert_eq!(tags.title, None);
    }
}
