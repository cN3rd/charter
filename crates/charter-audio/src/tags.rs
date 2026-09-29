//! What an audio file says about itself: the title and artist its tags carry, and the cover
//! artwork it holds.

use std::io::Cursor;

use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTag, StandardVisualKey, Visual};
use symphonia_wrap as symphonia;

/// The song facts an audio file's tags carry, where it carries them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    /// Song title, if tagged.
    pub title: Option<String>,
    /// Performing artist, if tagged.
    pub artist: Option<String>,
    /// The picture the file holds, its front cover where it names one.
    pub artwork: Option<Artwork>,
}

/// A picture held inside an audio file, as it was encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artwork {
    /// Encoded PNG or JPEG data.
    pub bytes: Vec<u8>,
    /// What a file of it is named with: `png` or `jpg`.
    pub extension: &'static str,
}

impl Artwork {
    /// The artwork `visual` is, when it is a PNG or a JPEG.
    fn of(visual: &Visual) -> Option<Self> {
        let extension = match visual.media_type.as_deref()? {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            _ => return None,
        };
        Some(Self {
            bytes: visual.data.to_vec(),
            extension,
        })
    }
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
    let visuals: Vec<&Visual> = revision
        .media
        .visuals
        .iter()
        .chain(
            revision
                .per_track
                .iter()
                .flat_map(|track| &track.metadata.visuals),
        )
        .collect();
    let front = |visual: &&&Visual| visual.usage == Some(StandardVisualKey::FrontCover);
    found.artwork = visuals
        .iter()
        .find(front)
        .into_iter()
        .chain(&visuals)
        .find_map(|visual| Artwork::of(visual));
    for tag in every {
        let text = |text: &str| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        match &tag.std {
            Some(StandardTag::TrackTitle(title)) => {
                found.title = found.title.or_else(|| text(title));
            }
            Some(StandardTag::Artist(artist)) => {
                found.artist = found.artist.or_else(|| text(artist));
            }
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
        pictured(comments, &[])
    }

    /// A FLAC PICTURE block of picture `kind`, 3 for the front cover, holding `data` of `mime`.
    fn picture(kind: u32, mime: &str, data: &[u8]) -> Vec<u8> {
        let length = |n: usize| u32::try_from(n).unwrap().to_be_bytes();
        let mut body = kind.to_be_bytes().to_vec();
        body.extend_from_slice(&length(mime.len()));
        body.extend_from_slice(mime.as_bytes());
        body.extend_from_slice(&length(0));
        body.extend_from_slice(&[0; 16]);
        body.extend_from_slice(&length(data.len()));
        body.extend_from_slice(data);
        body
    }

    /// A FLAC file as [`tagged`] makes, with a PICTURE block for each of `pictures`.
    fn pictured(comments: &[&str], pictures: &[Vec<u8>]) -> Vec<u8> {
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
        bytes.extend(block(4, pictures.is_empty(), &vorbis));
        for (at, body) in pictures.iter().enumerate() {
            bytes.extend(block(6, at.saturating_add(1) == pictures.len(), body));
        }
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
    fn a_file_with_pictures_gives_its_front_cover_named_for_its_kind() {
        let back = picture(4, "image/png", b"back");
        let front = picture(3, "image/jpeg", b"front");
        let tags = tags(pictured(&[], &[back.clone(), front]), Some("flac"));
        let artwork = tags.artwork.unwrap();
        assert_eq!(
            (artwork.bytes.as_slice(), artwork.extension),
            (&b"front"[..], "jpg")
        );
        let only = tags_of(pictured(&[], &[back]));
        assert_eq!(only.artwork.map(|image| image.extension), Some("png"));
        let unknown = tags_of(pictured(&[], &[picture(3, "image/x-weird", b"?")]));
        assert_eq!(unknown.artwork, None);
    }

    fn tags_of(bytes: Vec<u8>) -> Tags {
        tags(bytes, Some("flac"))
    }

    #[test]
    fn blank_tags_count_as_none() {
        let tags = tags(tagged(&["TITLE=  "]), Some("flac"));
        assert_eq!(tags.title, None);
    }
}
