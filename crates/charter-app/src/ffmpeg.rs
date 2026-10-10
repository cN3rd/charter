//! Finding the `FFmpeg` binary used when a game export needs converted audio.

use std::path::{Path, PathBuf};

use charter_ui::Window;

/// The binary the host last checked for the window's current `FFmpeg` setting.
#[derive(Debug, Default)]
pub(crate) struct Ffmpeg {
    /// Setting last checked, or none before the first frame.
    checked: Option<String>,
    /// Executable whose version check succeeded.
    pub(crate) binary: Option<PathBuf>,
}

impl Ffmpeg {
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
        |candidate| {
            std::process::Command::new(candidate)
                .arg("-version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        },
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
