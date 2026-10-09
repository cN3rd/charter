//! Reporting what went wrong: an error with the causes behind it, for the window to show, and a
//! panic as color-eyre's report on the terminal, a crash file in the data folder and a dialog.

use std::error::Error;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `error` followed by each cause behind it, `: ` apart. A cause whose message the one before
/// it already includes is left out, since many errors repeat their source in their own.
pub(crate) fn chain<'a>(error: &'a (dyn Error + 'static)) -> impl Display + 'a {
    Chain(error)
}

/// An error shown with its causes.
struct Chain<'a>(&'a (dyn Error + 'static));

impl Display for Chain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut shown = self.0.to_string();
        f.write_str(&shown)?;
        let mut cause = self.0.source();
        while let Some(error) = cause {
            let message = error.to_string();
            if !shown.contains(&message) {
                write!(f, ": {message}")?;
            }
            shown = message;
            cause = error.source();
        }
        Ok(())
    }
}

/// The crash file for a panic `since_epoch` after the Unix epoch, in the data folder `folder`.
fn crash_file(folder: &Path, since_epoch: Duration) -> PathBuf {
    folder.join(format!("crash-{}.txt", since_epoch.as_secs()))
}

/// What the crash dialog says, naming the crash file when one was written.
fn crash_message(written: Option<&Path>) -> String {
    let report = written.map_or_else(String::new, |file| {
        format!(
            " A report of what happened is in {}; please attach it to an issue.",
            file.display()
        )
    });
    format!(
        "charter has crashed and will close.{report} The last autosave is offered for restoring on the next start."
    )
}

/// Prints `report` on standard error, where the default panic hook prints.
#[expect(
    clippy::print_stderr,
    reason = "a panic's report goes to the terminal as the default hook's would"
)]
fn to_terminal(report: impl Display) {
    eprintln!("{report}");
}

/// Reports panics from here on: color-eyre's report on standard error, a crash file in `folder`
/// and, on the main thread, a dialog.
pub(crate) fn install(folder: Option<PathBuf>) {
    let terminal = color_eyre::config::HookBuilder::default().into_hooks().0;
    std::panic::set_hook(Box::new(move |info| {
        to_terminal(terminal.panic_report(info));
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let report = format!("{info}\n\n{}", std::backtrace::Backtrace::force_capture());
        let written = folder.as_deref().and_then(|folder| {
            let file = crash_file(folder, since_epoch);
            std::fs::create_dir_all(folder)
                .and_then(|()| std::fs::write(&file, report))
                .ok()
                .map(|()| file)
        });
        if std::thread::current().name() == Some("main") {
            drop(
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("charter crashed")
                    .set_description(crash_message(written.as_deref()))
                    .show(),
            );
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Failed(&'static str, Option<Box<Self>>);

    impl Display for Failed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test error; the standard provided methods are right"
    )]
    impl Error for Failed {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.1
                .as_deref()
                .map(|cause| -> &(dyn Error + 'static) { cause })
        }
    }

    fn failed(messages: &[&'static str]) -> Failed {
        let (last, rest) = messages.split_last().unwrap();
        rest.iter()
            .rev()
            .fold(Failed(last, None), |cause, message| {
                Failed(message, Some(Box::new(cause)))
            })
    }

    #[test]
    fn an_error_is_followed_by_each_cause_behind_it() {
        let error = failed(&["opening the song", "reading song.ogg", "permission denied"]);
        assert_eq!(
            chain(&error).to_string(),
            "opening the song: reading song.ogg: permission denied"
        );
    }

    #[test]
    fn a_cause_the_message_before_already_repeats_is_left_out() {
        let error = failed(&["decoding: bad header", "bad header", "byte 4"]);
        assert_eq!(chain(&error).to_string(), "decoding: bad header: byte 4");
    }

    #[test]
    fn an_error_without_causes_is_its_own_message() {
        assert_eq!(chain(&failed(&["no audio"])).to_string(), "no audio");
    }

    #[test]
    fn crash_files_are_named_after_the_second_they_happened_in_the_data_folder() {
        assert_eq!(
            crash_file(Path::new("data"), Duration::from_millis(1_700_000_000_500)),
            Path::new("data").join("crash-1700000000.txt")
        );
    }

    #[test]
    fn the_crash_dialog_names_the_report_when_one_was_written() {
        let file = Path::new("data").join("crash-1.txt");
        let message = crash_message(Some(&file));
        assert!(message.contains(&file.display().to_string()));
        assert!(!crash_message(None).contains("report"));
    }
}
