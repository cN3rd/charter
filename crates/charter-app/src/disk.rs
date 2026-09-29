//! The files the host reads and writes, behind a trait so the host's handling of them is tested
//! against files held in memory.

use std::io;
use std::path::{Path, PathBuf};

/// Where the host's files live.
pub trait Disk {
    /// # Errors
    /// Whatever keeps the file from being read.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// # Errors
    /// Whatever keeps the file from being written.
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// # Errors
    /// Whatever keeps the file from being moved.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// # Errors
    /// Whatever keeps the file from being removed, such as it not being there.
    fn remove(&self, path: &Path) -> io::Result<()>;
    /// # Errors
    /// Whatever keeps the file from being copied.
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;
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
}

/// The computer's own files.
#[derive(Debug)]
pub struct Real;

impl Disk for Real {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::copy(from, to).map(|_| ())
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
}

/// Writes `bytes` beside `path`, then moves them over the file at `path`, so a failed write never
/// leaves the file half written.
///
/// # Errors
///
/// Whatever kept the bytes from being written or moved; the file at `path` is as it was.
pub fn write_whole(disk: &dyn Disk, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".partial");
    let beside = PathBuf::from(beside);
    // ponytail: no fsync, since some filesystems refuse it. The autosave covers a crash; sync
    // here if saves on power loss turn out to matter.
    let written = disk
        .write(&beside, bytes)
        .and_then(|()| disk.rename(&beside, path));
    if written.is_err() {
        // The write already failed; a partial file left over is the lesser problem.
        drop(disk.remove(&beside));
    }
    written
}

/// Copies audio without leaving a failed partial copy at its destination. An existing file is
/// reused only if it is the same audio.
///
/// # Errors
///
/// The source cannot be read or copied, the destination differs, or the copy cannot be moved.
pub fn copy_whole(disk: &dyn Disk, from: &Path, to: &Path) -> io::Result<()> {
    if disk.exists(to) {
        // ponytail: read both files to compare; stream if large existing audio costs real memory.
        if disk.read(from)? == disk.read(to)? {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already holds different audio", to.display()),
        ));
    }
    if let Some(folder) = to.parent() {
        disk.make_folder(folder)?;
    }
    let mut beside = to.as_os_str().to_owned();
    beside.push(".partial");
    let beside = PathBuf::from(beside);
    let copied = disk
        .copy(from, &beside)
        .and_then(|()| disk.rename(&beside, to));
    if copied.is_err() {
        drop(disk.remove(&beside));
    }
    copied
}

#[cfg(test)]
pub mod memory {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    /// Files held in memory, some of which refuse to be written or moved.
    #[derive(Debug, Default)]
    pub struct Memory {
        pub files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
        /// Files that refuse writes.
        pub unwritable: Vec<PathBuf>,
        pub unreadable: Vec<PathBuf>,
        /// Whether every move fails.
        pub stuck: bool,
        pub partial_copy_fail: bool,
    }

    fn missing(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not there", path.display()),
        )
    }

    impl Memory {
        pub fn with(files: &[(&str, &[u8])]) -> Self {
            let files = files
                .iter()
                .map(|(path, bytes)| (PathBuf::from(path), bytes.to_vec()))
                .collect();
            Self {
                files: RefCell::new(files),
                ..Self::default()
            }
        }

        pub fn get(&self, path: &str) -> Option<Vec<u8>> {
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
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.stuck {
                return Err(io::Error::other("the move failed"));
            }
            let bytes = self
                .files
                .borrow_mut()
                .remove(from)
                .ok_or_else(|| missing(from))?;
            self.files.borrow_mut().insert(to.to_path_buf(), bytes);
            Ok(())
        }

        fn remove(&self, path: &Path) -> io::Result<()> {
            self.files
                .borrow_mut()
                .remove(path)
                .map(|_| ())
                .ok_or_else(|| missing(path))
        }

        fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
            let bytes = self.read(from)?;
            if self.partial_copy_fail {
                self.files.borrow_mut().insert(
                    to.to_path_buf(),
                    bytes[..bytes.len().div_euclid(2)].to_vec(),
                );
                return Err(io::Error::other("the disk is full"));
            }
            self.write(to, &bytes)
        }

        fn make_folder(&self, _: &Path) -> io::Result<()> {
            Ok(())
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
        }

        fn list(&self, folder: &Path) -> io::Result<Vec<PathBuf>> {
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
    use super::memory::Memory;
    use super::*;

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
        assert!(copy_whole(&disk, Path::new("/m/audio.ogg"), Path::new("/s/audio.ogg")).is_err());
        assert_eq!(disk.get("/s/audio.ogg"), None);
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }

    #[test]
    fn copying_audio_refuses_a_different_existing_file() {
        let disk = Memory::with(&[
            ("/m/audio.ogg", b"correct audio"),
            ("/s/audio.ogg", b"other audio"),
        ]);
        let error =
            copy_whole(&disk, Path::new("/m/audio.ogg"), Path::new("/s/audio.ogg")).unwrap_err();
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
        copy_whole(&disk, Path::new("/m/audio.ogg"), Path::new("/s/audio.ogg")).unwrap();
        assert_eq!(
            disk.get("/s/audio.ogg").as_deref(),
            Some(&b"same audio"[..])
        );
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }

    #[test]
    fn a_failed_audio_move_preserves_the_destination_and_cleans_up_the_copy() {
        let disk = Memory {
            stuck: true,
            ..Memory::with(&[("/m/audio.ogg", b"complete audio")])
        };
        assert!(copy_whole(&disk, Path::new("/m/audio.ogg"), Path::new("/s/audio.ogg")).is_err());
        assert_eq!(
            disk.get("/m/audio.ogg").as_deref(),
            Some(&b"complete audio"[..])
        );
        assert_eq!(disk.get("/s/audio.ogg"), None);
        assert_eq!(disk.get("/s/audio.ogg.partial"), None);
    }
}
