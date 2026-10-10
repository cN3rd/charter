//! Finding the `FFmpeg` binary used when a game export needs converted audio.

use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use charter_ui::{FfmpegPlatform, Window};
use sha2::{Digest, Sha256};
use ureq::ResponseExt;

use crate::report::chain;

/// The binary the host last checked for the window's current `FFmpeg` setting.
#[derive(Debug, Default)]
pub(crate) struct Ffmpeg {
    /// Setting last checked, or none before the first frame.
    checked: Option<String>,
    /// Executable whose version check succeeded.
    pub(crate) binary: Option<PathBuf>,
    /// Charter's data folder, or none when the host has no place for a release.
    folder: Option<PathBuf>,
    /// Download worker, while one is running.
    download: Option<Receiver<Progress>>,
}

/// A download worker's progress or outcome.
#[derive(Debug)]
enum Progress {
    /// This share of the archive has arrived.
    Downloaded(f32),
    /// Installed binary, or why it could not be installed.
    Finished(Result<PathBuf, String>),
}

impl Ffmpeg {
    /// A host keeping optional release builds in `folder`.
    pub(crate) fn new(folder: Option<PathBuf>) -> Self {
        Self {
            folder,
            ..Self::default()
        }
    }

    /// Check on first draw and after a setting changes, reporting the result to the window.
    pub(crate) fn refresh(
        &mut self,
        window: &mut Window,
        find: impl FnOnce(&str) -> Option<PathBuf>,
    ) -> bool {
        let configured = &window.settings().ffmpeg_path;
        if self.checked.as_ref() == Some(configured) {
            return false;
        }
        self.binary = find(configured);
        self.checked = Some(configured.clone());
        window.ffmpeg_found(self.binary.clone());
        true
    }

    /// Install the release for `platform` on a worker after the user agreed.
    pub(crate) fn start(&mut self, window: &mut Window, platform: FfmpegPlatform) {
        if self.download.is_some() || self.binary.is_some() {
            return;
        }
        let Some(source) = source_for(platform, std::env::consts::ARCH) else {
            window.ffmpeg_failed("no ffmpeg download is available for this system");
            return;
        };
        let Some(folder) = self.folder.clone() else {
            window.ffmpeg_failed("charter has no data folder to keep ffmpeg in");
            return;
        };
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = fetch_and_install(&folder, source, &send);
            drop(send.send(Progress::Finished(result)));
        });
        self.download = Some(receive);
        window.ffmpeg_progress(0.0);
    }

    /// Relay worker progress and results; true while the worker is still active.
    pub(crate) fn update(&mut self, window: &mut Window) -> bool {
        let Some(work) = &self.download else {
            return false;
        };
        loop {
            match work.try_recv() {
                Ok(Progress::Downloaded(share)) => {
                    if self.binary.is_none() {
                        window.ffmpeg_progress(share);
                    }
                }
                Ok(Progress::Finished(Ok(path))) => {
                    self.download = None;
                    if let Some(binary) = &self.binary {
                        window.ffmpeg_found(Some(binary.clone()));
                    } else {
                        window.ffmpeg_installed(&path);
                    }
                    return false;
                }
                Ok(Progress::Finished(Err(why))) => {
                    self.download = None;
                    window.ffmpeg_failed(&why);
                    return false;
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => {
                    self.download = None;
                    window.ffmpeg_failed("the download stopped");
                    return false;
                }
            }
        }
    }
}

/// Check whether a candidate executable can print its version.
fn runs(candidate: &Path) -> bool {
    std::process::Command::new(candidate)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Check that a candidate file runs `-version`, preferring the setting over the PATH.
pub(crate) fn installed(configured: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let executable = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    resolve_ffmpeg(
        configured,
        std::env::split_paths(&path),
        Path::is_file,
        runs,
        executable,
    )
}

/// Prefer a working configured binary; otherwise find the first working `FFmpeg` on the PATH.
fn resolve_ffmpeg(
    configured: &str,
    paths: impl IntoIterator<Item = PathBuf>,
    exists: impl Fn(&Path) -> bool,
    runs: impl Fn(&Path) -> bool,
    executable: &str,
) -> Option<PathBuf> {
    if !configured.is_empty() {
        let path = Path::new(configured);
        if exists(path) && runs(path) {
            return Some(path.to_path_buf());
        }
    }
    paths
        .into_iter()
        .map(|folder| folder.join(executable))
        .find(|path| exists(path) && runs(path))
}

/// Install command for the first Linux package manager available on the PATH.
fn package_manager(available: impl Fn(&str) -> bool) -> Option<&'static str> {
    [
        ("apt", "sudo apt install ffmpeg"),
        ("dnf", "sudo dnf install ffmpeg"),
        ("pacman", "sudo pacman -S ffmpeg"),
        ("zypper", "sudo zypper install ffmpeg"),
        ("apk", "sudo apk add ffmpeg"),
        ("xbps-install", "sudo xbps-install -S ffmpeg"),
        ("emerge", "sudo emerge --ask media-video/ffmpeg"),
    ]
    .into_iter()
    .find_map(|(tool, command)| available(tool).then_some(command))
}

/// Only Linux offers an install command for a package manager on the PATH.
fn install_command(platform: FfmpegPlatform, available: impl Fn(&str) -> bool) -> Option<String> {
    (platform == FfmpegPlatform::Linux)
        .then(|| package_manager(available))
        .flatten()
        .map(str::to_owned)
}

/// Tell the window its host platform and, on Linux, an available install command.
pub(crate) fn native_help() -> (FfmpegPlatform, Option<String>) {
    let platform = native_platform();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let folders: Vec<_> = std::env::split_paths(&path).collect();
    let command = install_command(platform, |tool| {
        folders.iter().any(|folder| folder.join(tool).is_file())
    });
    (platform, command)
}

/// Platform this binary was built for.
pub(crate) const fn native_platform() -> FfmpegPlatform {
    if cfg!(target_os = "windows") {
        FfmpegPlatform::Windows
    } else if cfg!(target_os = "macos") {
        FfmpegPlatform::MacOs
    } else if cfg!(target_os = "linux") {
        FfmpegPlatform::Linux
    } else {
        FfmpegPlatform::Other
    }
}

/// The release archive the host's platform and processor can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// Gyan's Windows release essentials.
    Windows,
    /// Martin Riedl's Apple Silicon release.
    MacArm64,
    /// Martin Riedl's Intel Mac release.
    MacAmd64,
}

impl Source {
    /// Where to download its ZIP, with redirects followed by the HTTP client.
    const fn url(self) -> &'static str {
        match self {
            Self::Windows => "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
            Self::MacArm64 => {
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip"
            }
            Self::MacAmd64 => {
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip"
            }
        }
    }

    /// The checksum URL. macOS keeps it beside the *redirected* ZIP, not beside the alias.
    fn checksum_url(self, final_url: &str) -> Result<String, String> {
        match self {
            Self::Windows => Ok(
                "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256"
                    .to_owned(),
            ),
            Self::MacArm64 | Self::MacAmd64 => {
                let archive = final_url.rsplit('/').next();
                if !final_url.starts_with("https://")
                    || !archive.is_some_and(|name| name.eq_ignore_ascii_case("ffmpeg.zip"))
                {
                    return Err("the ffmpeg archive redirected to an unexpected address".into());
                }
                Ok(format!("{final_url}.sha256"))
            }
        }
    }

    /// Name of the binary to keep in charter's data folder.
    const fn binary_name(self) -> &'static str {
        match self {
            Self::Windows => "ffmpeg.exe",
            Self::MacArm64 | Self::MacAmd64 => "ffmpeg",
        }
    }
}

/// Release for this host, if a download is supported.
fn source_for(platform: FfmpegPlatform, arch: &str) -> Option<Source> {
    match (platform, arch) {
        (FfmpegPlatform::Windows, _) => Some(Source::Windows),
        (FfmpegPlatform::MacOs, "aarch64") => Some(Source::MacArm64),
        (FfmpegPlatform::MacOs, "x86_64") => Some(Source::MacAmd64),
        (FfmpegPlatform::MacOs | FfmpegPlatform::Linux | FfmpegPlatform::Other, _) => None,
    }
}

/// Read the SHA-256 digest from a bare line or a line followed by the ZIP's name.
fn parse_checksum(body: &str) -> Result<String, String> {
    let Some(digest) = body.split_whitespace().next() else {
        return Err("the ffmpeg checksum is empty".into());
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the ffmpeg checksum is invalid".into());
    }
    Ok(digest.to_ascii_lowercase())
}

/// Save an archive as it arrives, refusing an incomplete response or an untrusted checksum.
fn copy_verified(
    reader: &mut impl Read,
    writer: &mut impl Write,
    expected_sha256: &str,
    length: Option<u64>,
    max_bytes: u64,
    mut progress: impl FnMut(f32),
) -> Result<(), String> {
    let mut buffer = vec![0_u8; 1 << 16];
    let mut hash = Sha256::new();
    let (mut read, mut told) = (0_u64, 0_u64);
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|why| format!("reading the ffmpeg download: {why}"))?;
        if count == 0 {
            break;
        }
        read = read.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        if read > max_bytes {
            return Err("the ffmpeg archive is too large".into());
        }
        let chunk = buffer.get(..count).unwrap_or_default();
        writer
            .write_all(chunk)
            .map_err(|why| format!("writing the ffmpeg download: {why}"))?;
        hash.update(chunk);
        if let Some(length) = length.filter(|length| *length > 0)
            && read.saturating_mul(100).div_euclid(length) > told
        {
            told = read.saturating_mul(100).div_euclid(length);
            progress(download_share(read, length));
        }
    }
    if length.is_some_and(|length| length > 0 && read != length) {
        return Err("incomplete ffmpeg download".into());
    }
    if format!("{:x}", hash.finalize()) != expected_sha256 {
        return Err("the ffmpeg download checksum does not match the published value".into());
    }
    Ok(())
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "download progress is approximate"
)]
/// Share of the archive received when its length is known.
fn download_share(read: u64, length: u64) -> f32 {
    read as f32 / length as f32
}

/// Write only this platform's `FFmpeg` entry, never an archive-supplied path.
fn extract_binary(
    archive: impl Read + Seek,
    output: &mut impl Write,
    source: Source,
) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(archive)
        .map_err(|why| format!("the ffmpeg download is not a ZIP: {why}"))?;
    let wanted = match source {
        Source::Windows => "bin/ffmpeg.exe",
        Source::MacArm64 | Source::MacAmd64 => "ffmpeg",
    };
    let mut found = None;
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|why| format!("cannot read the ffmpeg ZIP: {why}"))?;
        let matches = match source {
            Source::Windows => entry.name().ends_with("/bin/ffmpeg.exe"),
            Source::MacArm64 | Source::MacAmd64 => entry.name() == "ffmpeg",
        };
        if matches {
            found = Some(index);
            break;
        }
    }
    let index = found.ok_or_else(|| format!("the ffmpeg ZIP does not contain {wanted}"))?;
    let mut entry = zip
        .by_index(index)
        .map_err(|why| format!("cannot read the ffmpeg binary in the ZIP: {why}"))?;
    if entry.size() > 512 * 1024 * 1024 {
        return Err("the ffmpeg binary in the ZIP is too large".into());
    }
    let copied = std::io::copy(&mut entry, output)
        .map_err(|why| format!("could not extract ffmpeg from the ZIP: {why}"))?;
    if copied != entry.size() {
        return Err("the ffmpeg binary in the ZIP is incomplete".into());
    }
    Ok(())
}

/// Discard the archive in either case and the binary partial when installation fails.
fn cleanup_partials<T>(
    result: Result<T, String>,
    archive: &Path,
    binary_partial: &Path,
    remove: impl Fn(&Path) -> io::Result<()>,
) -> Result<T, String> {
    drop(remove(archive));
    if result.is_err() {
        drop(remove(binary_partial));
    }
    result
}

/// Download, verify and extract only the executable into charter's data folder.
fn fetch_and_install(
    folder: &Path,
    source: Source,
    progress: &Sender<Progress>,
) -> Result<PathBuf, String> {
    const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

    let directory = folder.join("ffmpeg");
    std::fs::create_dir_all(&directory).map_err(|why| format!("{}: {why}", directory.display()))?;
    let archive = crate::disk::partial_path(&directory.join("archive.zip"));
    let binary = directory.join(source.binary_name());
    let binary_partial = crate::disk::partial_path(&binary);

    let result = (|| {
        let mut response = ureq::get(source.url())
            .call()
            .map_err(|why| format!("getting the ffmpeg archive: {}", chain(&why)))?;
        let checksum_url = source.checksum_url(&response.get_uri().to_string())?;
        let mut checksum_response = ureq::get(checksum_url.as_str())
            .call()
            .map_err(|why| format!("getting the ffmpeg checksum: {}", chain(&why)))?;
        let mut checksum_body = String::new();
        let size = checksum_response
            .body_mut()
            .as_reader()
            .take(1025)
            .read_to_string(&mut checksum_body)
            .map_err(|why| format!("reading the ffmpeg checksum: {why}"))?;
        if size > 1024 {
            return Err("the ffmpeg checksum response is too large".into());
        }
        let checksum = parse_checksum(&checksum_body)?;

        let body = response.body_mut();
        let length = body.content_length();
        if length.is_some_and(|length| length > MAX_ARCHIVE_BYTES) {
            return Err("the ffmpeg archive is too large".into());
        }
        let mut file = std::fs::File::create(&archive)
            .map_err(|why| format!("{}: {why}", archive.display()))?;
        copy_verified(
            &mut body.as_reader(),
            &mut file,
            &checksum,
            length,
            MAX_ARCHIVE_BYTES,
            |share| drop(progress.send(Progress::Downloaded(share))),
        )?;
        file.sync_all()
            .map_err(|why| format!("{}: {why}", archive.display()))?;
        drop(file);

        let zip =
            std::fs::File::open(&archive).map_err(|why| format!("{}: {why}", archive.display()))?;
        let mut executable = std::fs::File::create(&binary_partial)
            .map_err(|why| format!("{}: {why}", binary_partial.display()))?;
        extract_binary(zip, &mut executable, source)?;
        executable
            .sync_all()
            .map_err(|why| format!("{}: {why}", binary_partial.display()))?;
        drop(executable);

        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&binary_partial)
                .map_err(|why| format!("{}: {why}", binary_partial.display()))?
                .permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            std::fs::set_permissions(&binary_partial, permissions)
                .map_err(|why| format!("{}: {why}", binary_partial.display()))?;
        }
        #[cfg(target_os = "windows")]
        if binary.exists() {
            std::fs::remove_file(&binary).map_err(|why| format!("{}: {why}", binary.display()))?;
        }
        std::fs::rename(&binary_partial, &binary)
            .map_err(|why| format!("{}: {why}", binary.display()))?;
        if !runs(&binary) {
            drop(std::fs::remove_file(&binary));
            return Err("the downloaded ffmpeg could not run on this computer".into());
        }
        Ok(binary)
    })();
    cleanup_partials(result, &archive, &binary_partial, |path| {
        std::fs::remove_file(path)
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::disk::{Disk, memory::Memory};

    fn archive(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for &(name, bytes) in entries {
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(bytes).unwrap();
        }
        Cursor::new(zip.finish().unwrap().into_inner())
    }

    #[test]
    fn a_valid_configured_ffmpeg_takes_precedence_over_the_path() {
        let selected = resolve_ffmpeg(
            "/tools/ffmpeg",
            [PathBuf::from("/usr/bin")],
            |path| path == Path::new("/tools/ffmpeg") || path == Path::new("/usr/bin/ffmpeg"),
            |_| true,
            "ffmpeg",
        );
        assert_eq!(selected, Some(PathBuf::from("/tools/ffmpeg")));
    }

    #[test]
    fn an_unusable_configured_ffmpeg_falls_back_to_a_working_path_binary() {
        let selected = resolve_ffmpeg(
            "/old/ffmpeg",
            [PathBuf::from("/broken"), PathBuf::from("/working")],
            |path| path != Path::new("/broken/ffmpeg"),
            |path| path != Path::new("/old/ffmpeg"),
            "ffmpeg",
        );
        assert_eq!(selected, Some(PathBuf::from("/working/ffmpeg")));
    }

    #[test]
    fn no_valid_binary_on_the_path_or_in_settings_returns_none() {
        assert_eq!(
            resolve_ffmpeg(
                "/missing/ffmpeg",
                [PathBuf::from("/not-executable")],
                |path| path == Path::new("/not-executable/ffmpeg"),
                |_| false,
                "ffmpeg",
            ),
            None
        );
    }

    #[test]
    fn the_path_lookup_uses_the_platforms_binary_name() {
        let selected = resolve_ffmpeg(
            "",
            [PathBuf::from("/bin")],
            |path| path == Path::new("/bin/ffmpeg.exe"),
            |_| true,
            "ffmpeg.exe",
        );
        assert_eq!(selected, Some(PathBuf::from("/bin/ffmpeg.exe")));
    }

    #[test]
    fn the_host_checks_again_after_the_binary_setting_changes() {
        let mut window = charter_ui::Window::new(&[]);
        let mut ffmpeg = Ffmpeg::default();
        assert!(ffmpeg.refresh(&mut window, |configured| {
            assert!(configured.is_empty());
            Some(PathBuf::from("/usr/bin/ffmpeg"))
        }));
        assert_eq!(ffmpeg.binary, Some(PathBuf::from("/usr/bin/ffmpeg")));
        assert!(!ffmpeg.refresh(&mut window, |_| panic!("already checked")));

        window.use_ffmpeg_path(Path::new("/tools/ffmpeg"));
        assert!(ffmpeg.refresh(&mut window, |configured| {
            assert_eq!(configured, "/tools/ffmpeg");
            Some(PathBuf::from("/tools/ffmpeg"))
        }));
        assert_eq!(ffmpeg.binary, Some(PathBuf::from("/tools/ffmpeg")));
    }

    #[test]
    fn available_linux_package_managers_give_their_install_commands() {
        for (available, expected) in [
            (&["apt"][..], Some("sudo apt install ffmpeg")),
            (&["dnf"][..], Some("sudo dnf install ffmpeg")),
            (&["pacman"][..], Some("sudo pacman -S ffmpeg")),
            (&["zypper"][..], Some("sudo zypper install ffmpeg")),
            (&["apk"][..], Some("sudo apk add ffmpeg")),
            (&["xbps-install"][..], Some("sudo xbps-install -S ffmpeg")),
            (
                &["emerge"][..],
                Some("sudo emerge --ask media-video/ffmpeg"),
            ),
            (&["dnf", "apt"][..], Some("sudo apt install ffmpeg")),
            (&["other-tool"][..], None),
        ] {
            assert_eq!(
                package_manager(|tool| available.contains(&tool)),
                expected,
                "{available:?}"
            );
        }
    }

    #[test]
    fn linux_package_commands_are_not_offered_on_other_platforms() {
        for platform in [
            charter_ui::FfmpegPlatform::Windows,
            charter_ui::FfmpegPlatform::MacOs,
            charter_ui::FfmpegPlatform::Other,
        ] {
            assert_eq!(install_command(platform, |_| true), None, "{platform:?}");
        }
        assert_eq!(
            install_command(charter_ui::FfmpegPlatform::Linux, |tool| tool == "apt"),
            Some("sudo apt install ffmpeg".to_owned())
        );
    }

    #[test]
    fn release_sources_use_the_hosts_platform_and_macs_architecture() {
        let windows = source_for(FfmpegPlatform::Windows, "x86_64").unwrap();
        assert_eq!(
            windows.url(),
            "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
        );
        assert_eq!(
            windows
                .checksum_url("https://www.gyan.dev/ffmpeg/builds/ffmpeg-9.zip")
                .unwrap(),
            "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256"
        );
        assert_eq!(windows.binary_name(), "ffmpeg.exe");

        for (arch, url, checksum) in [
            (
                "aarch64",
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip",
                "https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip.sha256",
            ),
            (
                "x86_64",
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip",
                "https://ffmpeg.martin-riedl.de/download/macos/amd64/1789931890_9.0.2/ffmpeg.zip.sha256",
            ),
        ] {
            let mac = source_for(FfmpegPlatform::MacOs, arch).unwrap();
            assert_eq!(mac.url(), url);
            let final_url = checksum.strip_suffix(".sha256").unwrap();
            assert_eq!(mac.checksum_url(final_url).unwrap(), checksum);
            assert_eq!(mac.binary_name(), "ffmpeg");
        }
        let mac = source_for(FfmpegPlatform::MacOs, "aarch64").unwrap();
        assert!(
            mac.checksum_url("https://ffmpeg.martin-riedl.de/download/other.zip")
                .unwrap_err()
                .contains("unexpected")
        );
        assert!(source_for(FfmpegPlatform::Linux, "x86_64").is_none());
        assert!(source_for(FfmpegPlatform::Other, "x86_64").is_none());
        assert!(source_for(FfmpegPlatform::MacOs, "armv7").is_none());
    }

    #[test]
    fn checksum_responses_require_a_complete_hex_digest_with_optional_filename() {
        const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(parse_checksum(DIGEST).unwrap(), DIGEST);
        assert_eq!(
            parse_checksum(&format!("{DIGEST}  ffmpeg.zip\n")).unwrap(),
            DIGEST
        );
        assert!(parse_checksum("").unwrap_err().contains("empty"));
        assert!(
            parse_checksum("0123456789abcdef")
                .unwrap_err()
                .contains("invalid")
        );
        assert!(
            parse_checksum(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg ffmpeg.zip"
            )
            .unwrap_err()
            .contains("invalid")
        );
    }

    #[test]
    fn a_verified_ffmpeg_archive_stream_preserves_bytes_and_reports_progress() {
        let mut output = Vec::new();
        let mut progress = Vec::new();
        copy_verified(
            &mut Cursor::new(b"abc"),
            &mut output,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            Some(3),
            512 * 1024 * 1024,
            |share| progress.push(share),
        )
        .unwrap();
        assert_eq!(output, b"abc");
        assert_eq!(progress, [1.0]);
    }

    #[test]
    fn a_mismatched_or_incomplete_archive_is_refused() {
        let mut output = Vec::new();
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let mismatch = copy_verified(
            &mut Cursor::new(b"abd"),
            &mut output,
            expected,
            Some(3),
            512 * 1024 * 1024,
            |_| {},
        )
        .unwrap_err();
        assert!(mismatch.contains("checksum"), "{mismatch}");

        let incomplete = copy_verified(
            &mut Cursor::new(b"abc"),
            &mut output,
            expected,
            Some(4),
            512 * 1024 * 1024,
            |_| {},
        )
        .unwrap_err();
        assert!(incomplete.contains("incomplete"), "{incomplete}");
    }

    #[test]
    fn an_oversized_ffmpeg_archive_is_refused_while_streaming() {
        let error = copy_verified(
            &mut Cursor::new(b"abc"),
            &mut std::io::sink(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            Some(3),
            2,
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn windows_zip_extracts_only_the_versioned_bin_ffmpeg_exe() {
        let archive = archive(&[
            ("ffmpeg.exe", b"wrong file"),
            ("ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe", b"MZ ffmpeg"),
            ("ffmpeg-9.0.2-essentials_build/bin/ffplay.exe", b"MZ ffplay"),
        ]);
        let mut output = Vec::new();
        extract_binary(archive, &mut output, Source::Windows).unwrap();
        assert_eq!(output, b"MZ ffmpeg");
    }

    #[test]
    fn mac_zip_extracts_only_the_root_ffmpeg_binary() {
        let archive = archive(&[("docs/ffmpeg", b"wrong"), ("ffmpeg", b"Mach-O ffmpeg")]);
        for source in [Source::MacArm64, Source::MacAmd64] {
            let mut output = Vec::new();
            extract_binary(archive.clone(), &mut output, source).unwrap();
            assert_eq!(output, b"Mach-O ffmpeg");
        }
    }

    #[test]
    fn zip_without_the_required_ffmpeg_entry_is_refused() {
        let archive = archive(&[("readme.txt", b"no executable")]);
        let mut output = Vec::new();
        let error = extract_binary(archive, &mut output, Source::Windows).unwrap_err();
        assert!(error.contains("bin/ffmpeg.exe"), "{error}");
        assert!(output.is_empty());
    }

    #[test]
    fn a_failed_install_removes_both_partials_without_touching_an_old_binary() {
        let disk = Memory::with(&[
            ("/data/ffmpeg/archive.zip.partial", b"incomplete archive"),
            ("/data/ffmpeg/ffmpeg.partial", b"incomplete executable"),
            ("/data/ffmpeg/ffmpeg", b"earlier executable"),
        ]);
        let error = cleanup_partials::<()>(
            Err("checksum mismatch".into()),
            Path::new("/data/ffmpeg/archive.zip.partial"),
            Path::new("/data/ffmpeg/ffmpeg.partial"),
            |path| disk.remove(path),
        )
        .unwrap_err();
        assert_eq!(error, "checksum mismatch");
        assert!(!disk.exists(Path::new("/data/ffmpeg/archive.zip.partial")));
        assert!(!disk.exists(Path::new("/data/ffmpeg/ffmpeg.partial")));
        assert_eq!(
            disk.get("/data/ffmpeg/ffmpeg").as_deref(),
            Some(&b"earlier executable"[..])
        );
    }

    #[test]
    fn a_download_worker_reports_failure_and_stops_without_network_access() {
        let (send, receive) = std::sync::mpsc::channel();
        let mut ffmpeg = Ffmpeg {
            download: Some(receive),
            ..Ffmpeg::default()
        };
        let mut window = Window::new(&[]);
        window.ffmpeg_found(None);
        send.send(Progress::Downloaded(0.4)).unwrap();
        assert!(ffmpeg.update(&mut window));
        send.send(Progress::Finished(Err("checksum mismatch".into())))
            .unwrap();
        assert!(!ffmpeg.update(&mut window));
        assert!(ffmpeg.download.is_none());
        assert!(window.message().unwrap().contains("checksum mismatch"));
    }

    #[test]
    fn a_download_worker_keeps_the_installed_binary_as_the_setting() {
        let (send, receive) = std::sync::mpsc::channel();
        let mut ffmpeg = Ffmpeg {
            download: Some(receive),
            ..Ffmpeg::default()
        };
        let mut window = Window::new(&[]);
        window.ffmpeg_found(None);
        send.send(Progress::Finished(Ok("/data/ffmpeg/ffmpeg.exe".into())))
            .unwrap();
        assert!(!ffmpeg.update(&mut window));
        assert_eq!(window.settings().ffmpeg_path, "/data/ffmpeg/ffmpeg.exe");
        assert!(ffmpeg.download.is_none());
    }

    #[test]
    fn finishing_a_download_does_not_replace_a_valid_binary_chosen_in_the_meantime() {
        let (send, receive) = std::sync::mpsc::channel();
        let mut ffmpeg = Ffmpeg {
            binary: Some("/tools/ffmpeg".into()),
            download: Some(receive),
            ..Ffmpeg::default()
        };
        let mut window = Window::new(&[]);
        window.use_ffmpeg_path(Path::new("/tools/ffmpeg"));
        window.ffmpeg_found(Some("/tools/ffmpeg".into()));
        send.send(Progress::Finished(Ok("/data/ffmpeg/ffmpeg".into())))
            .unwrap();
        assert!(!ffmpeg.update(&mut window));
        assert_eq!(window.settings().ffmpeg_path, "/tools/ffmpeg");
        assert!(ffmpeg.download.is_none());
    }

    #[test]
    fn download_does_not_start_without_a_data_folder() {
        let mut ffmpeg = Ffmpeg::default();
        let mut window = Window::new(&[]);
        window.ffmpeg_found(None);
        ffmpeg.start(&mut window, FfmpegPlatform::Windows);
        assert!(window.message().unwrap().contains("no data folder"));
        assert!(ffmpeg.download.is_none());
    }
}
