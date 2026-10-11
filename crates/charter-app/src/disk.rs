//! The files the host reads and writes, behind a trait so the host's handling of them is tested
//! against files held in memory.

use std::io;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use charter_audio::AudioCodec;

/// Failure when neither the chosen `FFmpeg` nor the system one can run.
const MISSING_FFMPEG: &str = "ffmpeg was found neither at the configured path nor on the PATH";

/// How ffmpeg makes M4A audio when the source cannot be copied as a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum M4aConversion {
    /// Move the compressed audio frames into an MP4 container unchanged.
    Remux,
    /// Encode audio to AAC.
    Encode,
}

/// No conversion means the source is already AAC in MP4 and can be copied as a file.
const fn m4a_conversion(codec: Option<AudioCodec>, mp4: bool) -> Option<M4aConversion> {
    match codec {
        Some(AudioCodec::Aac) if mp4 => None,
        Some(AudioCodec::Aac | AudioCodec::Mp3) => Some(M4aConversion::Remux),
        Some(AudioCodec::Other) | None => Some(M4aConversion::Encode),
    }
}

/// The source and destination of a file operation.
#[derive(Clone, Copy)]
pub(crate) struct Transfer<'a> {
    /// The source file.
    pub(crate) from: &'a Path,
    /// The destination file.
    pub(crate) to: &'a Path,
}

/// Where the host's files live.
pub(crate) trait Disk {
    /// Reads all bytes at `path`.
    ///
    /// # Errors
    /// Whatever keeps the file from being read.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Replaces the bytes at `path`.
    ///
    /// # Errors
    /// Whatever keeps the file from being written.
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Moves a file from `transfer.from` to `transfer.to`.
    ///
    /// # Errors
    /// Whatever keeps the file from being moved.
    fn rename(&self, transfer: Transfer<'_>) -> io::Result<()>;
    /// Removes the file at `path`.
    ///
    /// # Errors
    /// Whatever keeps the file from being removed, such as it not being there.
    fn remove(&self, path: &Path) -> io::Result<()>;
    /// Copies a file from `transfer.from` to `transfer.to`.
    ///
    /// # Errors
    /// Whatever keeps the file from being copied.
    fn copy(&self, transfer: Transfer<'_>) -> io::Result<()>;
    /// Makes `path` and any missing parent directories.
    ///
    /// # Errors
    /// Whatever keeps the folder from being made.
    fn make_folder(&self, path: &Path) -> io::Result<()>;
    /// Whether `path` names an existing file or directory.
    fn exists(&self, path: &Path) -> bool;
    /// The files directly in `folder`, in no particular order.
    ///
    /// # Errors
    /// Whatever keeps the folder from being read, such as it not being there.
    fn list(&self, folder: &Path) -> io::Result<Vec<PathBuf>>;
    /// Remuxes the audio unchanged or encodes it as AAC in an MP4 container.
    ///
    /// # Errors
    /// Whatever keeps the audio from being converted, such as no ffmpeg binary being available.
    fn convert_m4a(
        &self,
        ffmpeg: Option<&Path>,
        transfer: Transfer<'_>,
        conversion: M4aConversion,
    ) -> io::Result<()>;
}

/// The computer's own files.
#[derive(Debug)]
pub(crate) struct Real;

impl Disk for Real {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    fn rename(&self, transfer: Transfer<'_>) -> io::Result<()> {
        std::fs::rename(transfer.from, transfer.to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn copy(&self, transfer: Transfer<'_>) -> io::Result<()> {
        std::fs::copy(transfer.from, transfer.to).map(|_| ())
    }

    fn make_folder(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list(&self, folder: &Path) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            #[expect(
                clippy::filetype_is_file,
                reason = "snapshot listings exclude symlinks"
            )]
            if entry.file_type()?.is_file() {
                files.push(entry.path());
            }
        }
        Ok(files)
    }

    fn convert_m4a(
        &self,
        ffmpeg: Option<&Path>,
        transfer: Transfer<'_>,
        conversion: M4aConversion,
    ) -> io::Result<()> {
        let ffmpeg =
            ffmpeg.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, MISSING_FFMPEG))?;
        // ponytail: blocks the window for the seconds a song takes to convert; run it on a thread
        // with progress if long songs make that felt.
        let converted = m4a_command(ffmpeg, transfer, conversion)
            .output()
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    io::Error::new(io::ErrorKind::NotFound, MISSING_FFMPEG)
                } else {
                    error
                }
            })?;
        if converted.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "ffmpeg could not convert the audio: {}",
                String::from_utf8_lossy(&converted.stderr).trim()
            )))
        }
    }
}

/// The `FFmpeg` invocation used for M4A audio in a game's export.
fn m4a_command(
    ffmpeg: &Path,
    transfer: Transfer<'_>,
    conversion: M4aConversion,
) -> std::process::Command {
    let mut command = std::process::Command::new(ffmpeg);
    command
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(transfer.from)
        .arg("-vn");
    match conversion {
        M4aConversion::Remux => {
            command.args(["-c:a", "copy", "-f", "mp4"]);
        }
        M4aConversion::Encode => {
            command.args(["-c:a", "aac", "-b:a", "320k", "-f", "ipod"]);
        }
    }
    command.arg(transfer.to);
    command
}

/// Writes `bytes` beside `path`, then moves them over the file at `path`, so a failed write never
/// leaves the file half written.
///
/// # Errors
///
/// Whatever kept the bytes from being written or moved; the file at `path` is as it was.
pub(crate) fn write_whole(disk: &impl Disk, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let beside = partial_path(path);
    // ponytail: no fsync, since some filesystems refuse it. The autosave covers a crash; sync
    // here if saves on power loss turn out to matter.
    let written = disk.write(&beside, bytes).and_then(|()| {
        disk.rename(Transfer {
            from: &beside,
            to: path,
        })
    });
    if written.is_err() {
        // The write already failed; a partial file left over is the lesser problem.
        drop(disk.remove(&beside));
    }
    written
}

/// The partial file used for a write-then-rename update of `path`.
pub(crate) fn partial_path(path: &Path) -> PathBuf {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".partial");
    PathBuf::from(beside)
}

/// Creates `path`'s parent folder, then writes the bytes atomically enough for discardable caches.
///
/// # Errors
///
/// Whatever kept the folder, partial file or final rename from being written.
pub(crate) fn write_cache(disk: &impl Disk, path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        disk.make_folder(folder)?;
    }
    write_whole(disk, path, bytes)
}

/// Copies audio without leaving a failed partial copy at its destination. An existing file is
/// reused only if it is the same audio.
///
/// # Errors
///
/// The source cannot be read or copied, the destination differs, or the copy cannot be moved.
pub(crate) fn copy_whole(disk: &impl Disk, transfer: Transfer<'_>) -> io::Result<()> {
    if disk.exists(transfer.to) {
        // ponytail: read both files to compare; stream if large existing audio costs real memory.
        if disk.read(transfer.from)? == disk.read(transfer.to)? {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already holds different audio", transfer.to.display()),
        ));
    }
    copy_over(disk, transfer)
}

/// Copies the source through a partial file beside the destination, replacing whatever is there.
///
/// # Errors
///
/// The source cannot be read or copied, or the copy cannot be moved; the destination is as it was.
pub(crate) fn copy_over(disk: &impl Disk, transfer: Transfer<'_>) -> io::Result<()> {
    if let Some(folder) = transfer.to.parent() {
        disk.make_folder(folder)?;
    }
    let beside = partial_path(transfer.to);
    let copied = disk
        .copy(Transfer {
            from: transfer.from,
            to: &beside,
        })
        .and_then(|()| {
            disk.rename(Transfer {
                from: &beside,
                to: transfer.to,
            })
        });
    if copied.is_err() {
        drop(disk.remove(&beside));
    }
    copied
}

/// Writes the source to an M4A file, copying AAC in MP4, remuxing AAC or MP3 in other containers,
/// and encoding other audio as AAC.
///
/// # Errors
///
/// The source cannot be read or the file cannot be written; the destination is as it was.
pub(crate) fn m4a_over(
    disk: &impl Disk,
    ffmpeg: Option<&Path>,
    transfer: Transfer<'_>,
) -> io::Result<()> {
    // ponytail: Disk::read loads the whole song to probe it; stream the probe if large songs
    // make export memory-heavy.
    let source = disk.read(transfer.from)?;
    let mp4 = source.get(4..8) == Some(&b"ftyp"[..]);
    let extension = transfer.from.extension().and_then(std::ffi::OsStr::to_str);
    match m4a_conversion(charter_audio::audio_codec(source, extension), mp4) {
        None => copy_over(disk, transfer),
        Some(conversion) => convert_over(disk, ffmpeg, transfer, conversion),
    }
}

/// Fits the source picture within a transparent square and writes it as a PNG.
///
/// # Errors
///
/// The picture cannot be read or decoded, or the PNG cannot be written; the destination is as
/// it was.
pub(crate) fn square_artwork_over(
    disk: &impl Disk,
    transfer: Transfer<'_>,
    side: NonZeroU32,
) -> io::Result<()> {
    let source = disk.read(transfer.from)?;
    let side = side.get();
    let fitted = image::load_from_memory(&source)
        .map_err(io::Error::other)?
        .resize(side, side, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut canvas = image::RgbaImage::new(side, side);
    let x = i64::from(side.saturating_sub(fitted.width()).div_euclid(2));
    let y = i64::from(side.saturating_sub(fitted.height()).div_euclid(2));
    image::imageops::overlay(&mut canvas, &fitted, x, y);
    let mut output = io::Cursor::new(Vec::new());
    canvas
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(io::Error::other)?;
    if let Some(folder) = transfer.to.parent() {
        disk.make_folder(folder)?;
    }
    write_whole(disk, transfer.to, &output.into_inner())
}

/// Converts the source to M4A through a partial file, then replaces the destination.
///
/// # Errors
///
/// The audio cannot be converted, or the result cannot be moved; the destination is as it was.
fn convert_over(
    disk: &impl Disk,
    ffmpeg: Option<&Path>,
    transfer: Transfer<'_>,
    conversion: M4aConversion,
) -> io::Result<()> {
    if let Some(folder) = transfer.to.parent() {
        disk.make_folder(folder)?;
    }
    let beside = partial_path(transfer.to);
    let converted = disk
        .convert_m4a(
            ffmpeg,
            Transfer {
                from: transfer.from,
                to: &beside,
            },
            conversion,
        )
        .and_then(|()| {
            disk.rename(Transfer {
                from: &beside,
                to: transfer.to,
            })
        });
    if converted.is_err() {
        drop(disk.remove(&beside));
    }
    converted
}

#[cfg(test)]
pub(crate) mod memory {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// Files held in memory, some of which refuse to be written or moved. As on a real disk, a
    /// file goes only into a folder that exists: the root, one made, or one holding a file it
    /// started with.
    #[derive(Debug, Default)]
    pub(crate) struct Memory {
        /// File contents keyed by path.
        pub(crate) files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
        /// Folders that exist, besides the root.
        pub(crate) folders: RefCell<BTreeSet<PathBuf>>,
        /// Files that refuse writes.
        pub(crate) unwritable: Vec<PathBuf>,
        /// Files that refuse reads.
        pub(crate) unreadable: Vec<PathBuf>,
        /// Folders that refuse creation.
        pub(crate) uncreatable: Vec<PathBuf>,
        /// Whether every move fails.
        pub(crate) stuck: bool,
        /// Whether a copy writes half its bytes before failing.
        pub(crate) partial_copy_fail: bool,
        /// Binary the host passed to ffmpeg, if any.
        pub(crate) converted_with: RefCell<Option<PathBuf>>,
    }

    fn missing(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not there", path.display()),
        )
    }

    impl Memory {
        /// Starts with the given path-and-content pairs, and the folders holding them.
        pub(crate) fn with(files: &[(&str, &[u8])]) -> Self {
            let memory = Self::default();
            for (path, bytes) in files {
                let path = PathBuf::from(path);
                if let Some(folder) = path.parent() {
                    memory.add_folder(folder);
                }
                memory.files.borrow_mut().insert(path, bytes.to_vec());
            }
            memory
        }

        /// Makes `folder` and every folder above it.
        fn add_folder(&self, folder: &Path) {
            self.folders
                .borrow_mut()
                .extend(folder.ancestors().map(Path::to_path_buf));
        }

        /// Whether `folder` exists: the root, a relative path's base, or a folder made.
        fn has_folder(&self, folder: &Path) -> bool {
            folder.parent().is_none() || self.folders.borrow().contains(folder)
        }

        /// Refuses, as a real disk does, a file whose folder does not exist.
        fn folder_of(&self, path: &Path) -> io::Result<()> {
            match path.parent() {
                Some(folder) if !self.has_folder(folder) => Err(missing(folder)),
                _ => Ok(()),
            }
        }

        /// Returns the bytes at `path`, if present.
        pub(crate) fn get(&self, path: &str) -> Option<Vec<u8>> {
            self.files.borrow().get(Path::new(path)).cloned()
        }
    }

    impl Disk for Memory {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            if self.unreadable.iter().any(|refused| refused == path) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "access denied",
                ));
            }
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| missing(path))
        }

        fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            if self.unwritable.iter().any(|refused| refused == path) {
                return Err(io::Error::other("the disk is full"));
            }
            self.folder_of(path)?;
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn rename(&self, transfer: Transfer<'_>) -> io::Result<()> {
            if self.stuck {
                return Err(io::Error::other("the move failed"));
            }
            self.folder_of(transfer.to)?;
            let bytes = self
                .files
                .borrow_mut()
                .remove(transfer.from)
                .ok_or_else(|| missing(transfer.from))?;
            self.files
                .borrow_mut()
                .insert(transfer.to.to_path_buf(), bytes);
            Ok(())
        }

        fn remove(&self, path: &Path) -> io::Result<()> {
            self.files
                .borrow_mut()
                .remove(path)
                .map(|_| ())
                .ok_or_else(|| missing(path))
        }

        fn copy(&self, transfer: Transfer<'_>) -> io::Result<()> {
            let bytes = self.read(transfer.from)?;
            self.folder_of(transfer.to)?;
            if self.partial_copy_fail {
                self.files.borrow_mut().insert(
                    transfer.to.to_path_buf(),
                    bytes[..bytes.len().div_euclid(2)].to_vec(),
                );
                return Err(io::Error::other("the disk is full"));
            }
            self.write(transfer.to, &bytes)
        }

        fn make_folder(&self, path: &Path) -> io::Result<()> {
            if self.uncreatable.iter().any(|refused| refused == path) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "folder cannot be created",
                ));
            }
            self.add_folder(path);
            Ok(())
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path) || self.folders.borrow().contains(path)
        }

        /// Writes a prefix identifying the ffmpeg operation and the source's bytes.
        fn convert_m4a(
            &self,
            ffmpeg: Option<&Path>,
            transfer: Transfer<'_>,
            conversion: M4aConversion,
        ) -> io::Result<()> {
            let ffmpeg =
                ffmpeg.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, MISSING_FFMPEG))?;
            *self.converted_with.borrow_mut() = Some(ffmpeg.to_path_buf());
            let mut converted = match conversion {
                M4aConversion::Remux => b"remux:".to_vec(),
                M4aConversion::Encode => b"aac:".to_vec(),
            };
            converted.extend(self.read(transfer.from)?);
            self.write(transfer.to, &converted)
        }

        fn list(&self, folder: &Path) -> io::Result<Vec<PathBuf>> {
            if !self.has_folder(folder) {
                return Err(missing(folder));
            }
            Ok(self
                .files
                .borrow()
                .keys()
                .filter(|path| path.parent() == Some(folder))
                .cloned()
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::memory::Memory;
    use super::*;

    /// A solid red picture encoded without using the filesystem.
    fn picture_bytes(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([255, 0, 0]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    /// Exports an in-memory picture as a square PNG and decodes it for pixel assertions.
    fn square(width: u32, height: u32, format: image::ImageFormat, side: u32) -> image::RgbaImage {
        let source = picture_bytes(width, height, format);
        let disk = Memory::with(&[("/songs/cover.jpg", &source)]);
        square_artwork_over(
            &disk,
            Transfer {
                from: Path::new("/songs/cover.jpg"),
                to: Path::new("/out/artwork.png"),
            },
            NonZeroU32::new(side).unwrap(),
        )
        .unwrap();
        let saved = disk.get("/out/artwork.png").unwrap();
        assert!(saved.starts_with(b"\x89PNG\r\n\x1a\n"));
        image::load_from_memory(&saved).unwrap().to_rgba8()
    }

    #[test]
    fn wide_png_fits_an_80_pixel_square_with_transparent_top_and_bottom() {
        let canvas = square(400, 100, image::ImageFormat::Png, 80);
        assert_eq!(canvas.dimensions(), (80, 80));
        assert_eq!(canvas.get_pixel(40, 29).0, [0, 0, 0, 0]);
        assert_eq!(canvas.get_pixel(40, 79).0, [0, 0, 0, 0]);
        assert_eq!(canvas.get_pixel(40, 40).0, [255, 0, 0, 255]);
    }

    #[test]
    fn tall_jpeg_fits_a_160_pixel_square_with_transparent_sides() {
        let canvas = square(100, 400, image::ImageFormat::Jpeg, 160);
        assert_eq!(canvas.dimensions(), (160, 160));
        assert_eq!(canvas.get_pixel(59, 80).0, [0, 0, 0, 0]);
        assert_eq!(canvas.get_pixel(159, 80).0, [0, 0, 0, 0]);
        let centre = canvas.get_pixel(80, 80).0;
        assert!(centre[0] > 240 && centre[3] == 255, "{centre:?}");
    }

    #[test]
    fn square_picture_fills_either_canvas_without_padding() {
        for side in [80, 160] {
            let canvas = square(40, 40, image::ImageFormat::Png, side);
            assert_eq!(canvas.dimensions(), (side, side));
            assert_eq!(canvas.get_pixel(0, 0).0, [255, 0, 0, 255]);
            assert_eq!(canvas.get_pixel(side - 1, side - 1).0, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn unreadable_artwork_does_not_replace_an_existing_export() {
        let disk = Memory::with(&[
            ("/songs/cover.jpg", b"not a picture"),
            ("/out/artwork.png", b"old artwork"),
        ]);
        let result = square_artwork_over(
            &disk,
            Transfer {
                from: Path::new("/songs/cover.jpg"),
                to: Path::new("/out/artwork.png"),
            },
            NonZeroU32::new(80).unwrap(),
        );
        assert!(result.is_err());
        assert_eq!(
            disk.get("/out/artwork.png").as_deref(),
            Some(&b"old artwork"[..])
        );
        assert_eq!(disk.get("/out/artwork.png.partial"), None);
    }

    #[test]
    fn a_whole_write_replaces_the_file_and_leaves_nothing_beside_it() {
        let disk = Memory::with(&[("/s/song.json", b"old")]);
        write_whole(&disk, Path::new("/s/song.json"), b"new").unwrap();
        assert_eq!(disk.get("/s/song.json").as_deref(), Some(&b"new"[..]));
        assert_eq!(disk.files.borrow().len(), 1);
    }

    #[test]
    fn a_write_that_fails_leaves_the_file_as_it_was() {
        let disk = Memory {
            unwritable: vec![PathBuf::from("/s/song.json.partial")],
            ..Memory::with(&[("/s/song.json", b"old")])
        };
        assert!(write_whole(&disk, Path::new("/s/song.json"), b"new").is_err());
        assert_eq!(disk.get("/s/song.json").as_deref(), Some(&b"old"[..]));
    }

    #[test]
    fn cached_writes_create_their_folder_and_use_the_standard_partial_name() {
        let disk = Memory::default();
        write_cache(&disk, Path::new("/s/cache/song.ogg.peaks"), b"peaks").unwrap();
        assert_eq!(
            disk.get("/s/cache/song.ogg.peaks").as_deref(),
            Some(&b"peaks"[..])
        );
        assert_eq!(
            partial_path(Path::new("/s/cache/song.ogg.peaks")),
            PathBuf::from("/s/cache/song.ogg.peaks.partial")
        );
    }

    #[test]
    fn a_move_that_fails_removes_what_was_written_beside_the_file() {
        let disk = Memory {
            stuck: true,
            ..Memory::with(&[("/s/song.json", b"old")])
        };
        assert!(write_whole(&disk, Path::new("/s/song.json"), b"new").is_err());
        assert_eq!(disk.get("/s/song.json").as_deref(), Some(&b"old"[..]));
        assert_eq!(disk.get("/s/song.json.partial"), None);
    }

    #[test]
    fn copying_audio_cleans_up_a_partial_file_on_failure() {
        let disk = Memory {
            partial_copy_fail: true,
            ..Memory::with(&[("/m/audio.ogg", b"complete audio")])
        };
        assert!(
            copy_whole(
                &disk,
                Transfer {
                    from: Path::new("/m/audio.ogg"),
                    to: Path::new("/s/audio.ogg"),
                },
            )
            .is_err()
        );
        assert_eq!(disk.get("/s/audio.ogg"), None);
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }

    #[test]
    fn copying_audio_refuses_a_different_existing_file() {
        let disk = Memory::with(&[
            ("/m/audio.ogg", b"correct audio"),
            ("/s/audio.ogg", b"other audio"),
        ]);
        let error = copy_whole(
            &disk,
            Transfer {
                from: Path::new("/m/audio.ogg"),
                to: Path::new("/s/audio.ogg"),
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            disk.get("/s/audio.ogg").as_deref(),
            Some(&b"other audio"[..])
        );
    }

    #[test]
    fn copying_identical_audio_reuses_the_destination_without_a_move() {
        let disk = Memory {
            stuck: true,
            ..Memory::with(&[
                ("/m/audio.ogg", b"same audio"),
                ("/s/audio.ogg", b"same audio"),
            ])
        };
        copy_whole(
            &disk,
            Transfer {
                from: Path::new("/m/audio.ogg"),
                to: Path::new("/s/audio.ogg"),
            },
        )
        .unwrap();
        assert_eq!(
            disk.get("/s/audio.ogg").as_deref(),
            Some(&b"same audio"[..])
        );
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }

    #[test]
    fn copying_audio_never_reuses_a_destination_when_the_source_is_missing() {
        let disk = Memory::with(&[("/s/audio.ogg", b"earlier audio")]);
        let error = copy_whole(
            &disk,
            Transfer {
                from: Path::new("/m/missing.ogg"),
                to: Path::new("/s/audio.ogg"),
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(
            disk.get("/s/audio.ogg").as_deref(),
            Some(&b"earlier audio"[..])
        );
    }

    #[test]
    fn copying_audio_never_replaces_an_existing_unreadable_destination() {
        let disk = Memory {
            unreadable: vec![PathBuf::from("/s/audio.ogg")],
            ..Memory::with(&[
                ("/m/audio.ogg", b"new audio"),
                ("/s/audio.ogg", b"earlier audio"),
            ])
        };
        let error = copy_whole(
            &disk,
            Transfer {
                from: Path::new("/m/audio.ogg"),
                to: Path::new("/s/audio.ogg"),
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            disk.get("/s/audio.ogg").as_deref(),
            Some(&b"earlier audio"[..])
        );
    }

    #[test]
    fn copying_audio_into_an_uncreatable_folder_leaves_no_partial_file() {
        let disk = Memory {
            uncreatable: vec![PathBuf::from("/s")],
            ..Memory::with(&[("/m/audio.ogg", b"audio")])
        };
        let error = copy_whole(
            &disk,
            Transfer {
                from: Path::new("/m/audio.ogg"),
                to: Path::new("/s/audio.ogg"),
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(disk.get("/s/audio.ogg"), None);
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
        assert_eq!(disk.get("/m/audio.ogg").as_deref(), Some(&b"audio"[..]));
    }

    #[test]
    fn copying_over_replaces_only_the_destination() {
        let disk = Memory::with(&[
            ("/m/audio.ogg", b"new audio"),
            ("/s/audio.ogg", b"old audio"),
        ]);
        copy_over(
            &disk,
            Transfer {
                from: Path::new("/m/audio.ogg"),
                to: Path::new("/s/audio.ogg"),
            },
        )
        .unwrap();
        assert_eq!(disk.get("/m/audio.ogg").as_deref(), Some(&b"new audio"[..]));
        assert_eq!(disk.get("/s/audio.ogg").as_deref(), Some(&b"new audio"[..]));
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }

    #[test]
    fn a_failed_audio_move_preserves_the_destination_and_cleans_up_the_copy() {
        let disk = Memory {
            stuck: true,
            ..Memory::with(&[("/m/audio.ogg", b"complete audio")])
        };
        assert!(
            copy_whole(
                &disk,
                Transfer {
                    from: Path::new("/m/audio.ogg"),
                    to: Path::new("/s/audio.ogg"),
                },
            )
            .is_err()
        );
        assert_eq!(
            disk.get("/m/audio.ogg").as_deref(),
            Some(&b"complete audio"[..])
        );
        assert_eq!(disk.get("/s/audio.ogg"), None);
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }

    #[test]
    fn an_audio_export_runs_the_selected_ffmpeg_binary() {
        let command = m4a_command(
            Path::new("/tools/ffmpeg"),
            Transfer {
                from: Path::new("/songs/song.ogg"),
                to: Path::new("/out/audio.m4a.partial"),
            },
            M4aConversion::Encode,
        );
        assert_eq!(command.get_program(), "/tools/ffmpeg");
        let arguments: Vec<_> = command.get_args().collect();
        assert_eq!(
            arguments,
            [
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-i",
                "/songs/song.ogg",
                "-vn",
                "-c:a",
                "aac",
                "-b:a",
                "320k",
                "-f",
                "ipod",
                "/out/audio.m4a.partial",
            ]
        );
    }

    #[test]
    fn remuxing_mp3_to_m4a_copies_the_audio_stream() {
        let command = m4a_command(
            Path::new("/tools/ffmpeg"),
            Transfer {
                from: Path::new("/songs/song.mp3"),
                to: Path::new("/out/audio.m4a.partial"),
            },
            M4aConversion::Remux,
        );
        assert_eq!(command.get_program(), "/tools/ffmpeg");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-i",
                "/songs/song.mp3",
                "-vn",
                "-c:a",
                "copy",
                "-f",
                "mp4",
                "/out/audio.m4a.partial",
            ]
        );
    }

    #[test]
    fn aac_in_mp4_is_copied_without_ffmpeg() {
        assert_eq!(
            m4a_conversion(Some(charter_audio::AudioCodec::Aac), true),
            None
        );
    }

    #[test]
    fn raw_aac_and_mp3_are_remuxed_even_if_they_are_named_m4a() {
        for codec in [
            charter_audio::AudioCodec::Aac,
            charter_audio::AudioCodec::Mp3,
        ] {
            assert_eq!(
                m4a_conversion(Some(codec), false),
                Some(M4aConversion::Remux)
            );
        }
        assert_eq!(
            m4a_conversion(Some(charter_audio::AudioCodec::Mp3), true),
            Some(M4aConversion::Remux)
        );
    }

    #[test]
    fn other_or_unrecognised_audio_is_encoded() {
        assert_eq!(
            m4a_conversion(Some(charter_audio::AudioCodec::Other), false),
            Some(M4aConversion::Encode)
        );
        assert_eq!(m4a_conversion(None, false), Some(M4aConversion::Encode));
    }

    #[test]
    fn m4a_export_remuxes_mp3_and_raw_aac_despite_their_m4a_names() {
        let mut mp3_frame = vec![0xff, 0xfb, 0x90, 0x64];
        mp3_frame.resize(417, 0);
        let mp3 = [mp3_frame.clone(), mp3_frame].concat();
        let aac_header = [0xff, 0xf1, 0x50, 0x80, 0x00, 0xff, 0xfc];
        let aac = [aac_header, aac_header].concat();
        for source in [&mp3, &aac] {
            let disk = Memory::with(&[("/songs/song.m4a", source)]);
            let to = Path::new("/out/audio.m4a");
            m4a_over(
                &disk,
                Some(Path::new("/tools/ffmpeg")),
                Transfer {
                    from: Path::new("/songs/song.m4a"),
                    to,
                },
            )
            .unwrap();
            let mut remuxed = b"remux:".to_vec();
            remuxed.extend(source);
            assert_eq!(disk.get("/out/audio.m4a"), Some(remuxed));
            assert_eq!(disk.get("/out/audio.m4a.partial"), None);
            assert_eq!(
                disk.converted_with.borrow().as_deref(),
                Some(Path::new("/tools/ffmpeg"))
            );
        }
    }
}
