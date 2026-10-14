//! The state the window keeps between frames, beside the open song itself.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "window submodules share one private implementation vocabulary"
    )
)]
use super::*;

/// Playback that has been mixed by the host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PlayedAudio {
    /// Volumes baked into the current playback.
    pub(super) volumes: charter_audio::Volumes,
    /// Whether metronome or note clicks were mixed into the audio.
    pub(super) mixed_clicks: bool,
}

/// Host status for one audio file.
#[derive(Debug, PartialEq)]
pub(super) struct AudioStatus {
    /// File this status describes.
    pub(super) file: PathBuf,
    /// Whether the host loaded it.
    pub(super) loaded: Loaded,
}

/// A delayed paste waiting for the platform clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PendingPaste {
    /// Tick where the paste should land.
    pub(super) tick: Tick,
    /// Frames left before falling back to the in-memory copy.
    pub(super) wait: u8,
}

/// Analysis data for one audio file.
#[derive(Debug)]
pub(super) struct OfFile<T> {
    /// File the analysis was made from.
    pub(super) file: PathBuf,
    /// Analysis result.
    pub(super) value: T,
}

/// Cover artwork embedded in an audio file.
#[derive(Debug)]
pub(super) struct EmbeddedArtwork {
    /// Audio file that held this picture.
    pub(super) file: PathBuf,
    /// Picture file extension to use when saving.
    pub(super) extension: String,
    /// Decoded picture.
    pub(super) picture: Picture,
}

/// Playback state and audio host status.
#[derive(Debug, Default)]
pub(super) struct Playback {
    /// The usual playback speed, while the alternate one plays.
    pub(super) usual_rate: Option<Rate>,
    /// Whether the metronome is muted for as long as a key is held.
    pub(super) muted: bool,
    /// The volumes the playback under way plays at, and whether clicks are mixed into it, which
    /// bakes every volume in.
    pub(super) played: Option<PlayedAudio>,
    /// The audio file the host last reported on, and how it went.
    pub(super) audio: Option<AudioStatus>,
    /// What the audio playing was started for, while it plays.
    pub(super) playing: Option<Mode>,
    /// Current playback speed relative to the audio's own speed.
    pub(super) rate: Rate,
    /// Whether the audio playing was asked to keep its pitch.
    pub(super) played_pitch: bool,
    /// The settings the audio playing was mixed with, so a change to them plays it again.
    pub(super) played_sounds: Option<Sounds>,
}

/// What audio playing was started for, which playing it again keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Mode {
    /// From the cursor, which follows it.
    Cursor,
    /// From the cursor with the metronome on, to hear a timing edit.
    Audition,
    /// Between two audio moments, leaving the cursor where it is.
    Snippet {
        /// Where in the audio it starts.
        from: ResolvedTime,
        /// Where in the audio it stops.
        until: ResolvedTime,
    },
    /// The ticks from the first to the second, over and over with the metronome on.
    Loop(Tick, Tick),
}

/// The Help menu's windows, each showing or not.
#[derive(Debug, Default)]
pub(super) struct HelpWindows {
    /// Whether the keyboard shortcuts are showing.
    pub(super) shortcuts: bool,
    /// Whether the About window is showing.
    pub(super) about: bool,
}

/// Lane-key typing state.
#[derive(Debug, Default)]
pub(super) struct Typing {
    /// Which lane keys are down, and which of those placed or removed a note.
    pub(super) lane_keys: keys::LaneKeys,
    /// Whether a lane key placed a note that the cursor has not moved on from, which it does once
    /// every lane key is up, so the keys of a chord place it on one tick.
    pub(super) placed: bool,
    /// Taps placed by lane keys still down, by lane and tick. Each stretches into a hold to the
    /// cursor as it moves.
    pub(super) holding: Vec<(LaneIndex, Tick)>,
    /// When, in seconds into the audio, each lane key held down while playing went down: held
    /// long enough, its tap grows into a hold with the playback.
    pub(super) pressed: Vec<(LaneIndex, f64)>,
    /// The history step before the lane keys held now placed their first note.
    pub(super) chord_since: Option<usize>,
}

/// Clipboard and pending paste state.
#[derive(Debug, Default)]
pub(super) struct Clipboard {
    /// The notes copied last, their ticks counted from the earliest's.
    pub(super) clip: Vec<charter_core::Object>,
    /// Where the menu's paste goes, and how many frames are left to wait for the clipboard.
    pub(super) pasting: Option<PendingPaste>,
    /// Whether to ask the host for the clipboard's text this frame.
    pub(super) request_paste: bool,
    /// Text to put on the system clipboard this frame.
    pub(super) out: Option<String>,
}

/// Waveform, spectrum and onset analysis state.
#[derive(Debug, Default)]
pub(super) struct Analysis {
    /// The waveform of an audio file, once the host has summarized it.
    pub(super) waveform: Option<OfFile<Arc<Peaks>>>,
    /// The frequencies of an audio file, once the host has analysed them.
    pub(super) spectrum: Option<OfFile<notefield::Tiles>>,
    /// The onsets of an audio file the host found, and which file.
    pub(super) onsets: Option<OfFile<Arc<[charter_audio::Onset]>>>,
}

/// Artwork and dropped-audio state.
#[derive(Debug, Default)]
pub(super) struct ArtworkState {
    /// The audio file dropped on the song, which is copied beside the song when it is saved
    /// elsewhere.
    pub(super) audio_file: Option<PathBuf>,
    /// The song's cover artwork, as read, chosen or taken from its audio.
    pub(super) current: Option<Artwork>,
    /// The artwork file the song named when it was last opened or saved, which a save may write
    /// over; any other file there is someone else's.
    pub(super) on_disk: Option<PathBuf>,
    /// The artwork an autosave kept, to decode once its song opens.
    pub(super) restored: Option<Vec<u8>>,
    /// Pictures chosen but not saved whose names an undo took away, until another song opens.
    pub(super) parked: Vec<Artwork>,
    /// The artwork the audio of the song being started holds, and which file, to take when the
    /// song starts.
    pub(super) embedded: Option<EmbeddedArtwork>,
}

/// One recovery deadline: the wait before the open song is written somewhere it can be recovered
/// from, and the revision last written there.
///
/// The wait starts the first frame the song differs from what was written, and the revision stops
/// an unchanged song being written twice.
#[derive(Debug, Default)]
pub(super) struct Deadline {
    /// When the wait is up, in seconds on the frame clock.
    pub(super) due: Option<f64>,
    /// The revision last written, or opened, so an unchanged song is not written again.
    pub(super) done: Option<u64>,
}

impl Deadline {
    /// Whether `revision` is due to be written now, waiting `after` seconds from the first frame
    /// it was asked about. Asks egui to come back when the wait is up, and clears the wait when
    /// the setting is off.
    pub(super) fn ready(&mut self, ctx: &egui::Context, revision: u64, after: Option<f64>) -> bool {
        let Some(after) = after else {
            self.due = None;
            return false;
        };
        if self.done == Some(revision) {
            return false;
        }
        let now = ctx.input(|input| input.time);
        let due = *self.due.get_or_insert(now + after);
        if now < due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            return false;
        }
        self.due = None;
        true
    }

    /// Takes `revision` as written, so nothing is written again until the song changes.
    pub(super) const fn wrote(&mut self, revision: u64) {
        self.done = Some(revision);
        self.due = None;
    }

    /// Lets the next frame try again after a write failed, keeping the wait that led to it.
    pub(super) const fn retry(&mut self) {
        self.done = None;
    }

    /// Forgets the write and its wait, so the next change starts a fresh one.
    pub(super) const fn forget(&mut self) {
        self.done = None;
        self.due = None;
    }
}

/// Autosave and snapshot bookkeeping.
#[derive(Debug, Default)]
pub(super) struct Autosave {
    /// Whether the host may be keeping an autosave.
    pub(super) kept: bool,
    /// Charter's data folder, where snapshots go by default, when the host has one.
    pub(super) data_folder: Option<PathBuf>,
    /// When the open song's changes are next autosaved.
    pub(super) edits: Deadline,
    /// When the changed song is next copied to a dated snapshot.
    pub(super) snapshots: Deadline,
}

/// Dialogs and pending answers.
#[derive(Debug, Default)]
pub(super) struct Dialogs {
    /// Work held up by the open song's unsaved changes, and what it is waiting for.
    pub(super) pending: Option<Waiting>,
    /// How far the window is in closing.
    pub(super) leaving: Leaving,
    /// Whether the window asks whether to reset the timing.
    pub(super) resetting: bool,
    /// Whether installation help for a missing `FFmpeg` is open.
    pub(super) ffmpeg: bool,
    /// Where finding the timing from the audio has got to.
    pub(super) finding: Finding,
    /// The audio and resolution the timing last asked for is for, which a song open when it
    /// arrives must still have for it to take the timing.
    pub(super) finding_for: Option<(PathBuf, charter_core::TicksPerBeat)>,
    /// Timing the user has yet to accept or keep their current timing instead of.
    pub(super) found: Option<charter_core::Timing>,
    /// The model finding it, or last asked to.
    pub(super) tracker: Tracker,
    /// Unsaved changes from an earlier run, waiting on whether to restore them.
    pub(super) restoring: Option<Restoring>,
    /// The dialog that starts a song, while it shows.
    pub(super) new_song: Option<new_song::Draft>,
    /// Tags that would change the title and artist, waiting on whether to take them.
    pub(super) offer: Option<Offer>,
    /// A file to import or export, waiting on which of the formats sharing its extension it is.
    pub(super) choosing: Option<formats::Choosing>,
    /// An export whose files the host has yet to confirm were written.
    pub(super) exporting: Option<formats::Exporting>,
}

impl Dialogs {
    /// Whether a question is open and needs the keyboard.
    #[expect(
        clippy::unneeded_field_pattern,
        reason = "listing every field makes new dialogs require a keyboard-gate decision"
    )]
    pub(super) const fn asking(&self) -> bool {
        let Self {
            pending,
            leaving: _,
            resetting,
            ffmpeg,
            finding,
            finding_for: _,
            found,
            tracker: _,
            restoring,
            new_song,
            offer,
            choosing,
            exporting: _,
        } = self;
        new_song.is_some()
            || matches!(pending, Some(Waiting::Asking(_)))
            || restoring.is_some()
            || offer.is_some()
            || *resetting
            || *ffmpeg
            || choosing.is_some()
            || found.is_some()
            || matches!(finding, Finding::Asking | Finding::NoGpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_recovery_write_tries_again_without_restarting_its_wait() {
        let mut deadline = Deadline {
            due: Some(12.0),
            done: Some(7),
        };

        deadline.retry();
        assert_eq!(deadline.done, None);
        assert_eq!(
            deadline.due,
            Some(12.0),
            "the wait that led to the failed write still stands"
        );

        deadline.wrote(9);
        assert_eq!(deadline.done, Some(9));
        assert_eq!(
            deadline.due, None,
            "a write ends the wait it was waiting on"
        );

        deadline.forget();
        assert_eq!((deadline.done, deadline.due), (None, None));
    }
    #[test]
    fn pending_questions_take_keys_but_background_work_does_not() {
        let mut dialogs = Dialogs::default();
        assert!(!dialogs.asking());

        dialogs.pending = Some(Waiting::Asking(Discard::Close));
        assert!(dialogs.asking());
        dialogs.pending = None;
        dialogs.resetting = true;
        assert!(dialogs.asking());
        dialogs.resetting = false;
        dialogs.finding = Finding::Asking;
        assert!(dialogs.asking());
        dialogs.finding = Finding::NoGpu;
        assert!(dialogs.asking());
        dialogs.finding = Finding::Downloading(0.5);
        assert!(!dialogs.asking());
        // A save in flight holds the same work, but asks the user nothing, so it leaves the keys.
        dialogs.pending = Some(Waiting::Saving(Discard::Close));
        assert!(!dialogs.asking());
    }
}
