#![expect(
    clippy::mod_module_files,
    reason = "the window refactor deliberately uses a module directory"
)]
//! The editor window's state and action routing.
//!
//! `mod.rs` keeps the public window types and shared helpers. Concern modules add
//! focused `Window` impl blocks for files, playback, typing actions, clipboard,
//! artwork, timing, action dispatch, and per-frame UI drawing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use charter_audio::{Peaks, Spectrum};

use charter_core::{Kind, LaneIndex, RulesetPlugin, Tick};
use egui::Ui;

use charter_editor::{KeepNotes, NoteAt, Refusal, Selecting, Song, Step, Toggled, demo};

use crate::artwork::{Artwork, Picture};
use crate::flip::Direction;
use crate::new_song::{self, NO_AUDIO};
use crate::notefield::{self, Zoom};
use crate::settings::{self, Settings};
use crate::{import, keys, latency, minimap, panels, recovery, style, timing_panel};

/// The extensions of the audio files a song can play.
const AUDIO: [&str; 4] = ["ogg", "mp3", "wav", "flac"];

/// Which model finds a song's timing from its audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tracker {
    /// Beat This! finds the beats on the CPU, and charter fits tempo sections to them.
    #[default]
    BeatThis,
    /// Mapperatorinator's timing model writes the timing points itself, on the graphics card.
    Mapperatorinator,
}

/// Something only the program hosting the window can do. The host answers a save through
/// [`Window::saved`], and an open by passing the file to [`Window::open`].
#[derive(Debug, PartialEq)]
pub enum Request {
    /// Write `bytes`, the song as it stands, to `path`.
    Write {
        /// Destination file of the open song.
        path: PathBuf,
        /// Serialised chart document to write.
        bytes: Vec<u8>,
    },
    /// Ask where to save the song, suggesting the file name `name`, then write `bytes` there.
    SaveAs {
        /// Suggested file name for the save dialog.
        name: String,
        /// Serialised chart document to write at the chosen location.
        bytes: Vec<u8>,
    },
    /// Ask for a song to open.
    Open,
    /// Read the song at `path` and pass it to [`Window::open`], or say why with
    /// [`Window::refuse`]: a song opened before.
    OpenPath {
        /// File last used to open this song.
        path: PathBuf,
    },
    /// Write `bytes`, the song as its file holds it, as a dated copy named after `name` into each
    /// of `folders`, keeping the newest `kept` copies of it there; say what fails with
    /// [`Window::snapshot_failed`].
    Snapshot {
        /// Name used for the dated copies.
        name: String,
        /// Serialised song to preserve.
        bytes: Vec<u8>,
        /// Destinations for the copies.
        folders: Vec<PathBuf>,
        /// Maximum number of copies to retain per destination.
        kept: u32,
    },
    /// Ask the user for a folder to keep snapshots in, and pass it to
    /// [`Window::use_snapshot_folder`].
    ChooseSnapshotFolder,
    /// Keep `bytes`, the song's unsaved changes, where the next run finds them, in place of any
    /// kept before. The host passes them to [`Window::recover`] when it starts, and says through
    /// [`Window::autosave_failed`] if they could not be kept.
    Autosave {
        /// Serialised autosave and its original file path.
        bytes: Vec<u8>,
    },
    /// Remove the kept changes: they were saved or thrown away.
    DropAutosave,
    /// Copy the song's audio from `from` to `to`, beside the song, unless a file is there. The
    /// host says through [`Window::audio_not_copied`] if it could not.
    CopyAudio {
        /// Original audio file.
        from: PathBuf,
        /// Destination beside the chart file.
        to: PathBuf,
    },
    /// Play the song's audio from `from` seconds, at `rate` times its speed. The host reports
    /// where it is through [`Window::playhead`], and says when it ends through
    /// [`Window::playback_ended`].
    /// `clicks` are the tones to mix over it, each at its time in seconds into the audio.
    /// `volumes` say how loud the audio and each kind of click play, and `looping` the stretch
    /// to play over and over, in seconds, if any.
    Play {
        /// Position in the audio where playback starts, in seconds.
        from: f64,
        /// Playback speed relative to the audio's usual speed.
        rate: f64,
        /// Tones timed from the start of the audio, in seconds.
        clicks: Vec<(f64, charter_audio::Tone)>,
        /// Levels for the music and each kind of click.
        volumes: charter_audio::Volumes,
        /// Interval in the audio to repeat, in seconds.
        looping: Option<(f64, f64)>,
        /// Whether audio played faster or slower than its speed keeps its pitch.
        keep_pitch: bool,
    },
    /// Carry on playing with the audio at this volume.
    Volume(charter_audio::Gain),
    /// Pause the song's audio at its current position.
    Pause,
    /// Carry on playing from this many seconds into the audio.
    Seek(f64),
    /// Carry on playing at this many times the audio's speed.
    Rate(f64),
    /// Play `clicks` over silence, each at its time in seconds from now, alongside anything
    /// playing: a taste of a metronome sound.
    Preview {
        /// Tones timed from now, in seconds.
        clicks: Vec<(f64, charter_audio::Tone)>,
        /// Levels for the preview's tones.
        volumes: charter_audio::Volumes,
    },
    /// Ask the user to choose an audio file for the open song, and pass it to
    /// [`Window::use_audio`].
    ChooseAudio,
    /// Ask the user to choose a picture for the open song's artwork, and pass it to
    /// [`Window::use_artwork`].
    ChooseArtwork,
    /// Decode `bytes`, the artwork an autosave kept, and pass the picture to
    /// [`Window::artwork_restored`].
    DecodeArtwork {
        /// Picture bytes kept in the autosave.
        bytes: Vec<u8>,
    },
    /// Read and decode the picture at `file`, the song's artwork, and pass it to
    /// [`Window::artwork_read`].
    ReadArtwork {
        /// Artwork file named by the song.
        file: PathBuf,
    },
    /// Ask the user to choose a file holding timing in another program's format, such as a
    /// `.osu` file, and pass its text to [`Window::timing_file_read`].
    ChooseTimingFile,
    /// Read the tags of the audio file at `file`, and pass them to [`Window::tags_read`].
    ReadTags {
        /// Audio file whose title and artist tags should be read.
        file: PathBuf,
    },
    /// Find the timing of the audio in `file` at `ticks_per_beat` and pass it to
    /// [`Window::timing_found`], or call [`Window::weights_needed`] when the beat tracker's
    /// weights are not there yet.
    FindTiming {
        /// Audio to analyse for beats.
        file: PathBuf,
        /// Resolution of the song receiving the timing.
        ticks_per_beat: charter_core::TicksPerBeat,
        /// The model to find it with.
        tracker: Tracker,
    },
    /// Ask the user for a song in one of `formats`, each a name and the extensions its files end
    /// in, and pass its path and bytes to [`Window::import_song`] with those of the `companions`
    /// found beside it, by name.
    ChooseImport {
        /// Formats a plugin reads songs from, with their extensions.
        formats: Vec<(String, Vec<String>)>,
        /// Files a format reads beside the song's, by name, such as `info.plist`.
        companions: Vec<String>,
    },
    /// Ask where to write the open song in one of `formats`, suggesting the name `name`, and pass
    /// the path to [`Window::export_song`].
    ChooseExport {
        /// Suggested file name, without an extension.
        name: String,
        /// Formats the song's plugin writes, with their extensions.
        formats: Vec<(String, Vec<String>)>,
    },
    /// Write `bytes`, the open song in another program's format, to `path`, the files `beside` it
    /// and copies of the song's own, each from its file to its place, saying through
    /// [`Window::export_failed`] what could not be.
    WriteExport {
        /// Destination of the main file.
        path: PathBuf,
        /// The exported file.
        bytes: Vec<u8>,
        /// Files made beside it, each with its place.
        beside: Vec<(PathBuf, Vec<u8>)>,
        /// The song's files copied beside it, each from where it is to its place.
        copies: Vec<(PathBuf, PathBuf)>,
        /// The song's audio encoded as AAC in an MP4 container beside it, from where it is to its
        /// place.
        encodes: Vec<(PathBuf, PathBuf)>,
    },
    /// Download the beat tracker's weights, the user having agreed, telling
    /// [`Window::weights_progress`] how far it has got, then find the timing asked for.
    DownloadWeights,
    /// Find the timing asked for on the CPU, the user having agreed after
    /// [`Window::gpu_missing`].
    FindTimingOnCpu,
}

/// Where finding the song's timing from its audio has got to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Finding {
    /// No automatic timing operation is pending.
    #[default]
    Idle,
    /// Waiting on the user to agree to download the beat tracker's weights.
    Asking,
    /// Downloading the weights, this share of them so far.
    Downloading(f32),
    /// Waiting on the user to agree to find it on the CPU, there being no graphics card.
    NoGpu,
    /// The host is finding it, this share of the way when it says.
    Running(Option<f32>),
}

/// What tags read from an audio file would change, waiting on whether to take them.
#[derive(Debug)]
struct Offer {
    /// Whether they go to the new-song dialog rather than the open song.
    draft: bool,
    /// Title from the audio tags, if it would change the song's title.
    title: Option<String>,
    /// Artist from the audio tags, if it would change the song's artist.
    artist: Option<String>,
}

/// How fast the audio plays, as a factor of its own speed. The chart is untouched by it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rate(f64);

impl Default for Rate {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Rate {
    /// Lowest allowed playback speed as a factor of the audio's own speed.
    const SLOWEST: f64 = 0.1;
    /// Highest allowed playback speed as a factor of the audio's own speed.
    const FASTEST: f64 = 4.0;

    /// This rate changed by `step`, to the nearest percent, and kept between a tenth and four
    /// times the audio's speed, as ArrowVortex allows.
    #[must_use]
    pub(crate) fn by(self, step: f64) -> Self {
        Self::of(self.0 + step)
    }

    /// The nearest whole-percent speed to `factor`, between one tenth and four times normal.
    #[must_use]
    pub(crate) fn of(factor: f64) -> Self {
        let percent = (factor * 100.0).round() / 100.0;
        Self(percent.clamp(Self::SLOWEST, Self::FASTEST))
    }

    /// The audio's current speed as a factor of its own speed.
    #[must_use]
    pub(crate) const fn factor(self) -> f64 {
        self.0
    }

    /// The audio's current speed as a whole percentage of its own speed.
    #[must_use]
    pub(crate) fn percent(self) -> f64 {
        (self.0 * 100.0).round()
    }
}

/// How the audio the song wants stands with the host.
#[derive(Debug, PartialEq)]
enum Loaded {
    /// Ready to play, this many seconds long.
    Ready(f64),
    /// The host could not load the audio file for playback.
    Failed,
}

/// How a save the window asked for went.
#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    /// The song and any audio it needs are in place at this file now.
    To(PathBuf),
    /// No file was chosen.
    Cancelled,
    /// The file could not be written, for this reason.
    Failed(String),
}

/// What would lose the open song's unsaved changes, waiting on whether to save them first.
#[derive(Debug)]
enum Discard {
    /// Closing the window.
    Close,
    /// Opening another song, read from `path` unless it is the demo.
    Open {
        /// The opened song's file, or none for the demo.
        path: Option<PathBuf>,
        /// The replacement song, held until the current song can be discarded.
        song: Box<Song>,
        /// Whether the song is the autosave, restored.
        restored: bool,
        /// The audio file dropped on the song, when an autosave kept it.
        audio: Option<PathBuf>,
        /// What to tell the user once it is open, such as what an import left out.
        note: Option<String>,
    },
}

/// Unsaved changes from an earlier run, waiting on whether to restore them.
#[derive(Debug)]
struct Restoring {
    /// File the autosaved song belongs to, if one was chosen.
    path: Option<PathBuf>,
    /// Audio file dropped onto the autosaved song, if any.
    audio: Option<PathBuf>,
    /// The autosaved song to offer for restoration.
    song: Box<Song>,
    /// Artwork bytes not yet saved beside the song, if any.
    artwork: Option<Vec<u8>>,
}

/// Where a save the user asked for goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saving {
    /// To the song's file, or wherever the user chooses when it has none.
    InPlace,
    /// Wherever the user chooses.
    Elsewhere,
}

/// How far the window is in closing, once the user chose to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Leaving {
    /// No close request is under way.
    #[default]
    Staying,
    /// The host's window is to be told to close.
    Now,
    /// Closing, whatever changes are left: a close request goes through.
    Going,
}

/// A note a lane key places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// A note that starts and ends at its tick.
    Tap,
    /// A mine placed at its tick rather than a playable note.
    Mine,
}

impl Note {
    /// The chart's note kind to store for this lane-key choice.
    const fn kind(self) -> Kind {
        match self {
            Self::Tap => Kind::Tap,
            Self::Mine => Kind::Mine,
        }
    }
}

/// Something a panel, key or gesture asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Make a layer above the active one.
    AddLayer,
    /// Choose the layer at this row.
    SetActiveLayer(usize),
    /// Choose the adjacent row.
    StepLayer(Step),
    /// Move the selected notes onto this row.
    MoveSelectionToLayer(usize),
    /// Select the notes on this row.
    SelectLayer(usize),
    /// Set the view of this row.
    SetLayerView(usize, charter_core::LayerView),
    /// Solo this row, or restore the preceding views.
    SoloLayer(usize),
    /// Set the lock on this row.
    SetLayerLocked(usize, bool),
    /// Set the palette colour of this row.
    RecolourLayer(usize, charter_core::LayerColour),
    /// Give this row a new name.
    RenameLayer(usize, String),
    /// Drag a row to a new position.
    MoveLayer {
        /// Row moved.
        from: usize,
        /// Row it moves to.
        to: usize,
    },
    /// Merge the active row into the next row.
    MergeActiveLayer,
    /// Merge this row into the next row.
    MergeLayer(usize),
    /// Remove the row, either discarding or moving its notes.
    DeleteLayer(usize, charter_editor::LayerNotes),
    /// Copy this row into another chart.
    CopyLayerToChart {
        /// Row whose notes are copied.
        layer: usize,
        /// Chart receiving the copied notes.
        chart: usize,
    },
    /// Show or hide the active row.
    ToggleActiveLayerView,
    /// Switch between note-kind and layer-colour tinting.
    ToggleLayerColours,
    /// Replace the open song with the built-in demo.
    OpenDemo,
    /// Show the chart at this index in the open song.
    OpenChart(usize),
    /// Move the cursor by a snap in the given direction.
    Move(Step),
    /// Move the cursor by a measure in the given direction.
    MoveMeasure(Step),
    /// Increase the snap's denominator to reach finer lines.
    FinerSnap,
    /// Decrease the snap's denominator to reach coarser lines.
    CoarserSnap,
    /// Multiply the notefield zoom by this factor.
    ZoomBy(f32),
    /// Places a note on the lane at the cursor, or removes what starts there.
    Toggle(LaneIndex, Note),
    /// The lane key of a lane that toggled came up.
    LaneUp(LaneIndex),
    /// Every lane key that toggled is up again.
    LanesReleased,
    /// Places a note at `tick` on `lane`, or removes what starts there.
    Place {
        /// Position along the chart where the note starts.
        tick: Tick,
        /// Lane on which the note is placed.
        lane: LaneIndex,
        /// Kind of note placed on that lane.
        note: Note,
    },
    /// Makes the tap or hold starting on `lane` at `from` end at `to`.
    Stretch {
        /// Lane whose tap or hold is stretched.
        lane: LaneIndex,
        /// Tick where the note starts.
        from: Tick,
        /// Tick where the note ends.
        to: Tick,
    },
    /// Meets the selection with a note, or with none: nothing was under the pointer.
    Select(Option<NoteAt>, Selecting),
    /// Moves the selected notes this many ticks later and lanes right, earlier and left when
    /// negative.
    MoveSelection {
        /// Signed distance to move along the chart, in ticks.
        ticks: i32,
        /// Signed distance to move across lanes.
        lanes: i32,
    },
    /// Meets the selection with the notes starting inside this tick and lane rectangle.
    SelectBetween {
        /// First tick covered by the selection.
        from_tick: Tick,
        /// Last tick covered by the selection.
        to_tick: Tick,
        /// First lane covered by the selection.
        from_lane: LaneIndex,
        /// Last lane covered by the selection.
        to_lane: LaneIndex,
        /// Whether to replace, add to or remove from the existing selection.
        how: Selecting,
    },
    /// Revert the latest editing step.
    Undo,
    /// Reapply the latest reverted editing step.
    Redo,
    /// Select every note in the open chart.
    SelectAll,
    /// Leave no notes selected.
    ClearSelection,
    /// Remove the selected notes from the open chart.
    DeleteSelection,
    /// Moves the end of the selected notes, or those at the cursor, a snap later or earlier.
    ResizeNotes(Step),
    /// Copies the selected notes.
    Copy,
    /// Copies the selected notes and removes them.
    Cut,
    /// Pastes the copied notes at the cursor: those the clipboard's text holds, when it holds
    /// some, and otherwise those copied last.
    Paste(Option<String>),
    /// Copies the selected notes to just after themselves.
    Duplicate,
    /// Moves the selected notes this many snap lines later and lanes right, back and left when
    /// negative.
    ShiftSelection {
        /// Signed number of snap lines to move the notes.
        snaps: i32,
        /// Signed number of lanes to move the notes.
        lanes: i32,
    },
    /// Flips the selected notes across the lanes.
    Mirror,
    /// Moves the selected notes to the snap's nearest lines.
    Quantize,
    /// Makes the selected notes what the song's game makes them by the change named.
    ChangeNotes(String),
    /// Places taps from the cursor on, one `per_beat`th of a beat apart, on the lanes `steps`
    /// counts from `lane`, wrapping past the last.
    Pattern {
        /// Lane offsets of the pattern, wrapping at the last lane.
        steps: &'static [u16],
        /// Number of pattern positions per beat.
        per_beat: u16,
        /// First lane of the pattern.
        lane: LaneIndex,
    },
    /// Selects every note on the lane.
    SelectLane(LaneIndex),
    /// Moves the cursor to the tick and then does the action there, for what a menu opened on
    /// that tick places.
    Here(Tick, Box<Self>),
    /// Pastes what the system clipboard holds at the tick, or with no text there the notes
    /// copied last.
    PasteHere(Tick),
    /// Asks the host for a song to open.
    Open,
    /// Opens a song opened before.
    OpenRecent(PathBuf),
    /// Forgets the songs opened before.
    ClearRecent,
    /// Saves the song to its file, or asks where when it has none.
    Save,
    /// Asks where to save the song.
    SaveAs,
    /// Shows or hides the settings window.
    Settings,
    /// Opens the latency calibration.
    Calibrate,
    /// Shows the dialog that starts a song.
    NewSong,
    /// Plays the audio from the cursor, or pauses it.
    TogglePlay,
    /// Asks the host to let the user choose the song's audio.
    ChooseAudio,
    /// Asks the host to let the user choose a picture for the song's artwork.
    ChooseArtwork,
    /// Makes the audio play this much faster, or slower when negative.
    NudgeRate(f64),
    /// Makes the audio play at this factor of its speed.
    SetRate(f64),
    /// Apply the user's playback volume settings.
    SetVolumes(charter_audio::Volumes),
    /// Gives the timing point on the tick this tempo.
    SetBpmAt(Tick, charter_core::Bpm),
    /// Gives the song this title, artist, audio and preview.
    SetMetadata(Box<charter_core::Metadata>),
    /// Sets a text setting of the song or the open chart, or takes it away without a value.
    SetSetting {
        /// Whether the setting belongs to the song or its open chart.
        scope: charter_core::SettingScope,
        /// The game's key for the text setting.
        key: String,
        /// Replacement text, or none to remove the setting.
        value: Option<String>,
    },
    /// Moves the tempo point on the tick to this many seconds into the audio, the first one
    /// taking the whole grid along.
    MovePointTo(Tick, f64),
    /// Gives the timing point on the tick this meter.
    SetMeterAt(Tick, charter_core::TimeSignature),
    /// Removes the timing point on the tick.
    RemoveTimingAt(Tick),
    /// Puts tick 0 of the open timing this far into the audio.
    SetOffset(charter_core::Origin),
    /// Starts a BPM change at the cursor, at the tempo in effect there.
    AddBpm,
    /// Moves the offset by this many seconds, later in the audio when positive.
    NudgeOffset(f64),
    /// Show the timing window for editing audio anchors.
    ShowSync,
    /// Show the dialog for reading another program's timing.
    ShowImport,
    /// Ask the host to choose a file to import timing from.
    ChooseTimingFile,
    /// Puts this timing in place of the one tempo edits go to.
    ImportTiming(Box<charter_core::Timing>),
    /// Puts the cursor on this tick.
    JumpTo(Tick),
    /// Names the section at the cursor this, starting one there if none does.
    SetSection(String),
    /// Removes the song's marker at this index.
    RemoveMarker(usize),
    /// Puts the cursor where the open chart's last note ends.
    JumpEnd,
    /// An edit the timing window asks for.
    Timing(charter_timing::Edit),
    /// Play from this many seconds into the audio with the metronome on, to hear an edit.
    Audition(f64),
    /// Ask whether to reset the open chart's timing.
    AskResetTiming,
    /// Put 120 BPM in 4/4 from 0 ms in place of the open chart's timing.
    ResetTiming,
    /// Loop the ticks from `from` to `to` with the metronome, or stop looping them.
    Loop {
        /// Tick at which the playback loop starts.
        from: Tick,
        /// Tick at which the playback loop ends.
        to: Tick,
    },
    /// Switch between the usual playback speed and the alternate one.
    ToggleRate,
    /// Mute the metronome, or unmute it, while a key is held.
    MuteMetronome(bool),
    /// Show or hide the timing window.
    ToggleSync,
    /// Moves every timing event at `from` to `to`, as one timing point.
    MoveTiming {
        /// Tick holding the events to move.
        from: Tick,
        /// Tick to which those events move.
        to: Tick,
    },
    /// Moves the song's marker at `index` to `to`.
    MoveMarker {
        /// Index of the marker in the song.
        index: usize,
        /// Tick at which to put the marker.
        to: Tick,
    },
    /// Puts the cursor on the nearest section that way.
    JumpSection(Step),
    /// Adds a bookmark at the cursor, or removes the one there.
    ToggleBookmark,
    /// Raises the tempo at the cursor by this many BPM, or lowers it when negative.
    NudgeBpm(f64),
    /// Shows or hides the keyboard shortcuts.
    Help,
    /// Shows or hides the song panel.
    ToggleSongPanel,
    /// Turns the metronome on or off.
    ToggleMetronome,
    /// Turns the tick on each note on or off.
    ToggleTicks,
    /// Tunes each lane's note ticks to a pitch of its own, or back.
    ToggleTunedLanes,
    /// Finds the song's timing from its audio with this model.
    FindTiming(Tracker),
    /// Asks for a song in another program's format to open.
    ImportSong,
    /// Asks where to write the open song in another program's format.
    ExportSong,
    /// Draws the notefield's grid down to this note value.
    SetGrid(std::num::NonZeroU16),
}

/// The editor window's state.
#[derive(Debug, Default)]
pub struct Window {
    /// The ruleset plugins every song is checked against.
    plugins: &'static [&'static dyn RulesetPlugin],
    /// The song whose chart and metadata the window currently draws.
    song: Option<Song>,
    /// The file the open song was read from or last saved to.
    path: Option<PathBuf>,
    /// What the host has yet to do.
    requests: Vec<Request>,
    /// The title last given to the host's window.
    shown_title: String,
    /// A save asked for this frame, made once every edit of the frame is in.
    saving: Option<Saving>,
    /// The user's editor choices, kept by the host between runs.
    settings: Settings,
    /// Whether the settings window is showing.
    settings_open: bool,
    /// The timing window.
    sync: charter_timing::TimingWindow,
    /// The frame clock this frame, in seconds.
    frame_time: f64,
    /// The latency calibration window.
    calibration: latency::Calibration,
    /// The import timing window.
    import: import::Import,
    /// How tall a second of the open song is on screen.
    zoom: Zoom,
    /// Why the last file failed to open, or the last edit was refused.
    failure: Option<String>,
    /// A drag across the notefield in progress.
    gesture: Option<notefield::Gesture>,
    /// Cached visible-object paint order for the notefield.
    object_order: notefield::ObjectOrder,
    /// What is being typed into the timing section of the song panel.
    timing_drafts: timing_panel::Drafts,
    /// What is being typed into the song panel's song fields.
    song_drafts: panels::SongDrafts,
    /// A section name being typed, while its field has the keyboard.
    section_draft: Option<String>,
    /// Whether the keyboard shortcuts are showing.
    help_open: bool,
    /// Fonts for what the bundled ones cannot draw, such as CJK titles, named.
    fallback_fonts: Vec<(String, Arc<egui::FontData>)>,
    /// Whether files dropped on the window never arrive, as under Wayland, so the window offers
    /// only choosing them.
    no_drops: bool,
    /// What the keyboard shortcuts are filtered to.
    help_filter: String,
    /// Playback state and audio host status.
    playback: Playback,
    /// Lane-key typing state.
    typing: Typing,
    /// Clipboard and pending paste state.
    clipboard: Clipboard,
    /// Waveform, spectrum and onset analysis state.
    analysis: Analysis,
    /// Artwork and dropped-audio state.
    artwork: ArtworkState,
    /// Autosave and snapshot bookkeeping.
    autosave: Autosave,
    /// Dialogs and pending answers.
    dialogs: Dialogs,
}

/// Playback that has been mixed by the host.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PlayedAudio {
    /// Volumes baked into the current playback.
    volumes: charter_audio::Volumes,
    /// Whether metronome or note clicks were mixed into the audio.
    mixed_clicks: bool,
}

/// Host status for one audio file.
#[derive(Debug, PartialEq)]
struct AudioStatus {
    /// File this status describes.
    file: PathBuf,
    /// Whether the host loaded it.
    loaded: Loaded,
}

/// A delayed paste waiting for the platform clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingPaste {
    /// Tick where the paste should land.
    tick: Tick,
    /// Frames left before falling back to the in-memory copy.
    wait: u8,
}

/// Analysis data for one audio file.
#[derive(Debug)]
struct OfFile<T> {
    /// File the analysis was made from.
    file: PathBuf,
    /// Analysis result.
    value: T,
}

/// Cover artwork embedded in an audio file.
#[derive(Debug)]
struct EmbeddedArtwork {
    /// Audio file that held this picture.
    file: PathBuf,
    /// Picture file extension to use when saving.
    extension: String,
    /// Decoded picture.
    picture: Picture,
}

/// Playback state and audio host status.
#[derive(Debug, Default)]
struct Playback {
    /// The ticks being looped, while a loop plays.
    looping: Option<(Tick, Tick)>,
    /// The usual playback speed, while the alternate one plays.
    usual_rate: Option<Rate>,
    /// Whether the metronome is muted for as long as a key is held.
    muted: bool,
    /// The volumes the playback under way plays at, and whether clicks are mixed into it, which
    /// bakes every volume in.
    played: Option<PlayedAudio>,
    /// The audio file the host last reported on, and how it went.
    audio: Option<AudioStatus>,
    /// Whether the audio is playing.
    playing: bool,
    /// Current playback speed relative to the audio's own speed.
    rate: Rate,
    /// Whether the audio playing was asked to keep its pitch.
    played_pitch: bool,
    /// The settings the audio playing was mixed with, so a change to them plays it again.
    played_sounds: Option<Sounds>,
}

/// Lane-key typing state.
#[derive(Debug, Default)]
struct Typing {
    /// Touchpad scrolling not yet turned into cursor steps, and into measures under Shift.
    scrolled: [f32; 2],
    /// Which lane keys are down, and which of those placed or removed a note.
    lane_keys: keys::LaneKeys,
    /// Whether a lane key placed a note that the cursor has not moved on from, which it does once
    /// every lane key is up, so the keys of a chord place it on one tick.
    placed: bool,
    /// Taps placed by lane keys still down, by lane and tick. Each stretches into a hold to the
    /// cursor as it moves.
    holding: Vec<(LaneIndex, Tick)>,
    /// When, in seconds into the audio, each lane key held down while playing went down: held
    /// long enough, its tap grows into a hold with the playback.
    pressed: Vec<(LaneIndex, f64)>,
    /// The history step before the lane keys held now placed their first note.
    chord_since: Option<usize>,
}

/// Clipboard and pending paste state.
#[derive(Debug, Default)]
struct Clipboard {
    /// The notes copied last, their ticks counted from the earliest's.
    clip: Vec<charter_core::Object>,
    /// Where the menu's paste goes, and how many frames are left to wait for the clipboard.
    pasting: Option<PendingPaste>,
    /// Whether to ask the host for the clipboard's text this frame.
    request_paste: bool,
    /// Text to put on the system clipboard this frame.
    out: Option<String>,
}

/// Waveform, spectrum and onset analysis state.
#[derive(Debug, Default)]
struct Analysis {
    /// The waveform of an audio file, once the host has summarized it.
    waveform: Option<OfFile<Arc<Peaks>>>,
    /// The frequencies of an audio file, once the host has analysed them.
    spectrum: Option<OfFile<notefield::Tiles>>,
    /// The onsets of an audio file the host found, and which file.
    onsets: Option<OfFile<Arc<[charter_audio::Onset]>>>,
}

/// Artwork and dropped-audio state.
#[derive(Debug, Default)]
struct ArtworkState {
    /// The audio file dropped on the song, which is copied beside the song when it is saved
    /// elsewhere.
    audio_file: Option<PathBuf>,
    /// The song's cover artwork, as read, chosen or taken from its audio.
    current: Option<Artwork>,
    /// The artwork file the song named when it was last opened or saved, which a save may write
    /// over; any other file there is someone else's.
    on_disk: Option<PathBuf>,
    /// The artwork an autosave kept, to decode once its song opens.
    restored: Option<Vec<u8>>,
    /// Pictures chosen but not saved whose names an undo took away, until another song opens.
    parked: Vec<Artwork>,
    /// The artwork the audio of the song being started holds, and which file, to take when the
    /// song starts.
    embedded: Option<EmbeddedArtwork>,
}

/// Autosave and snapshot bookkeeping.
#[derive(Debug, Default)]
struct Autosave {
    /// Whether the host may be keeping an autosave.
    kept: bool,
    /// The revision of the open song the autosave holds, when it holds one.
    autosaved: Option<u64>,
    /// When the open song's changes are next autosaved, in seconds on the frame clock.
    due: Option<f64>,
    /// Charter's data folder, where snapshots go by default, when the host has one.
    data_folder: Option<PathBuf>,
    /// When the next snapshot of the changed song is due, on the frame clock.
    snapshot_due: Option<f64>,
    /// The revision of the song last copied, or opened, so an unchanged song is not copied.
    snapshotted: Option<u64>,
}

/// Dialogs and pending answers.
#[derive(Debug, Default)]
struct Dialogs {
    /// What waits on the answer to whether to save the open song's changes.
    discard: Option<Discard>,
    /// What to do once the save the user asked for is done.
    after_save: Option<Discard>,
    /// How far the window is in closing.
    leaving: Leaving,
    /// Whether the window asks whether to reset the timing.
    resetting: bool,
    /// Where finding the timing from the audio has got to.
    finding: Finding,
    /// The model finding it, or last asked to.
    tracker: Tracker,
    /// Unsaved changes from an earlier run, waiting on whether to restore them.
    restoring: Option<Restoring>,
    /// The dialog that starts a song, while it shows.
    new_song: Option<new_song::Draft>,
    /// Tags that would change the title and artist, waiting on whether to take them.
    offer: Option<Offer>,
    /// A file to import or export, waiting on which of the formats sharing its extension it is.
    choosing: Option<formats::Choosing>,
}

mod apply;
mod artwork;
mod clipboard;
mod files;
mod formats;
mod frame;
mod playback;
mod timing;

impl Window {
    /// This window with the user's choices from an earlier run.
    #[must_use]
    pub fn with_settings(self, settings: Settings) -> Self {
        Self { settings, ..self }
    }

    /// This window drawing what its own fonts cannot, such as CJK song titles, in `fonts`: each
    /// a name and a font file's bytes, tried in order. The host finds them among the system's.
    #[must_use]
    pub fn with_fallback_fonts(self, fonts: Vec<(String, Vec<u8>)>) -> Self {
        let fallback_fonts = fonts
            .into_iter()
            .map(|(name, bytes)| (name, Arc::new(egui::FontData::from_owned(bytes))))
            .collect();
        Self {
            fallback_fonts,
            ..self
        }
    }

    /// This window where files dropped on it never arrive, as under Wayland, which winit does
    /// not take drops from yet: it offers choosing files instead, and says why.
    #[must_use]
    pub fn without_file_drops(self) -> Self {
        Self {
            no_drops: true,
            ..self
        }
    }

    /// The user's current editor choices, to be kept by the host between runs.
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// A window with no song open, which checks songs against `plugins`.
    #[must_use]
    pub fn new(plugins: &'static [&'static dyn RulesetPlugin]) -> Self {
        Self {
            plugins,
            ..Self::default()
        }
    }

    /// What the window needs its host to do, oldest first. A host takes these after every frame.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// Takes that the audio played to its end.
    pub const fn playback_ended(&mut self) {
        self.playback.playing = false;
    }

    /// Takes that the beat tracker's weights are not downloaded yet, so the user is asked.
    pub const fn weights_needed(&mut self) {
        self.dialogs.finding = Finding::Asking;
    }

    /// Takes that there is no graphics card to find the timing on, so the user is asked whether
    /// to find it on the CPU instead.
    pub const fn gpu_missing(&mut self) {
        self.dialogs.finding = Finding::NoGpu;
    }

    /// Takes that finding the timing is `share` of the way done.
    pub const fn finding_progress(&mut self, share: f32) {
        self.dialogs.finding = Finding::Running(Some(share.clamp(0.0, 1.0)));
    }

    /// Takes that `share` of the beat tracker's weights has downloaded.
    pub const fn weights_progress(&mut self, share: f32) {
        self.dialogs.finding = Finding::Downloading(share.clamp(0.0, 1.0));
    }

    /// Whether the window takes the audio to be playing.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playback.playing
    }
}

/// The file name of `song`, read from `path`, or one made from its title when it has no file.
fn file_name(path: Option<&Path>, song: &Song) -> String {
    path.and_then(Path::file_name).map_or_else(
        || format!("{}.json", song.document().document().metadata.title),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// How many lanes of the open chart lane keys can edit: none when the song is read-only.
fn editable_lanes(song: &Song) -> u16 {
    if song.read_only().is_some() {
        return 0;
    }
    song.chart()
        .and_then(|chart| chart.lanes)
        .map_or(0, u16::from)
}

/// Stretches each tap a held lane key placed into a hold to the cursor, and says why if the song
/// refuses one.
/// Where a lane key places a note: at the cursor, or while playing, on the snap line nearest
/// where the cursor was `input_latency` seconds of audio ago, when the key was meant.
fn placing_at(song: &Song, input_latency: Option<f64>) -> Tick {
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap());
    match step {
        Some(step) if let Some(input_latency) = input_latency => {
            let timing = song.timing();
            let now = timing.time_at(song.cursor()).seconds() - input_latency;
            notefield::nearest_line(timing, now, step)
        }
        _ => song.cursor(),
    }
}

/// How long a lane key stays down while playing, in seconds of audio, before its tap becomes a
/// hold: longer than a quick tap lasts.
const HOLD_AFTER: f64 = 0.2;

/// Stretches each tap in `holding` whose key went down, as `pressed` says, at least
/// [`HOLD_AFTER`] before `now` into a hold ending on the snap line nearest `now`, so holds grow
/// with the playback. A stretch that changes nothing is no edit.
fn grow(
    song: &mut Song,
    holding: &[(LaneIndex, Tick)],
    pressed: &[(LaneIndex, f64)],
    now: f64,
) -> Option<String> {
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap())?;
    let to = notefield::nearest_line(song.timing(), now, step);
    let long = |lane: LaneIndex| {
        pressed
            .iter()
            .any(|&(held, since)| held == lane && now - since >= HOLD_AFTER)
    };
    let refusals: Vec<_> = holding
        .iter()
        .filter(|(lane, _)| long(*lane))
        .filter_map(|&(lane, from)| song.stretch(lane, from, to.max(from)).err())
        .collect();
    refusals.last().map(refused)
}

/// Extends held lane-key notes to the cursor, or gives the last reason an edit failed.
fn stretch(song: &mut Song, holding: &[(LaneIndex, Tick)]) -> Option<String> {
    let cursor = song.cursor();
    let refusals = holding
        .iter()
        .filter_map(|&(lane, from)| song.stretch(lane, from, cursor).err());
    refusals.last().as_ref().map(refused)
}

// ponytail: the clicks are worked out once, when playback starts, so notes placed while playing
// click from the next play; schedule them live if that gets in the way of recording.
/// The tones `settings` ask for over the open chart of `song`, playing from `from` seconds to
/// `until`: a tick on each note, and the metronome on each beat.
/// How many songs opened before the File menu offers.
const RECENT: usize = 10;

/// Frames the menu's paste waits for the clipboard's text before pasting the notes copied last.
const PASTE_WAIT: u8 = 3;

/// The settings playback is mixed and stretched with: a change to any of them plays again.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent settings compared as a whole"
)]
struct Sounds {
    /// Whether changing playback speed preserves audio pitch.
    keep_pitch: bool,
    /// Whether note heads make a sound as playback passes them.
    note_ticks: bool,
    /// Which sound note heads play when note ticks are on.
    tick_sound: settings::Sound,
    /// Whether note ticks are pitched by their lane.
    tuned_lanes: bool,
    /// Whether the playback includes beat clicks.
    metronome: bool,
    /// Which set of sounds supplies accented and unaccented clicks.
    metronome_sound: charter_audio::Kit,
    /// Whether clicks play on the timing grid rather than just on beats.
    metronome_grid: bool,
}

impl Sounds {
    /// The playback settings that require a replay when changed.
    const fn of(settings: &Settings) -> Self {
        Self {
            keep_pitch: settings.keep_pitch,
            note_ticks: settings.note_ticks,
            tick_sound: settings.tick_sound,
            tuned_lanes: settings.tuned_lanes,
            metronome: settings.metronome,
            metronome_sound: settings.metronome_sound,
            metronome_grid: settings.metronome_grid,
        }
    }
}

/// What the system clipboard holds of copied notes: their JSON under a key of their own, so
/// other text is not taken for notes.
#[derive(serde::Serialize, serde::Deserialize)]
struct Clipped {
    /// Copied chart objects under a key distinct from ordinary clipboard text.
    charter_notes: Vec<charter_core::Object>,
}

/// Places the taps of a pattern from `song`'s cursor on, `per_beat` to a beat, on the lanes
/// `steps` counts from `lane`, and moves the cursor to the line after the last. Says why if the
/// song refuses them.
fn pattern(song: &mut Song, steps: &[u16], per_beat: u16, lane: LaneIndex) -> Option<String> {
    let lanes = song
        .chart()
        .and_then(|chart| chart.lanes)
        .map_or(0, u16::from);
    let note_value = per_beat.checked_mul(4).and_then(std::num::NonZeroU16::new);
    let resolution = song.document().document().ticks_per_beat;
    let Some(gap) = note_value.and_then(|value| resolution.note_length(value)) else {
        return Some(format!(
            "The song's resolution has no 1/{per_beat} of a beat"
        ));
    };
    if lanes == 0 {
        return None;
    }
    let start = i64::from(song.cursor());
    let gap = i64::from(u32::from(gap));
    let at = |index: usize| {
        let index = i64::try_from(index).ok()?;
        i32::try_from(start.checked_add(index.checked_mul(gap)?)?)
            .ok()
            .map(Tick::new)
    };
    let taps: Vec<(Tick, LaneIndex)> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let on = u16::from(lane).checked_add(*step)?.checked_rem(lanes)?;
            Some((at(index)?, LaneIndex::new(on)))
        })
        .collect();
    let why = song.add_taps(taps).err().as_ref().map(refused);
    if why.is_none()
        && let Some(after) = at(steps.len())
    {
        song.jump_to(after);
    }
    why
}

/// The notefield grid's finest lines for the chosen `grid` at `snap`: the coarsest lines holding
/// both the grid's and the snap's, so the cursor never steps onto a line the grid leaves out and
/// the grid keeps every line it was chosen for. Past 192nds, the snap's alone.
fn shown_grid(grid: std::num::NonZeroU16, snap: std::num::NonZeroU16) -> std::num::NonZeroU16 {
    let (a, b) = (
        std::num::NonZeroU32::from(grid).get(),
        std::num::NonZeroU32::from(snap).get(),
    );
    let (mut x, mut y) = (a, b);
    while y != 0 {
        (x, y) = (y, x.rem_euclid(y));
    }
    let both = a.div_euclid(x).saturating_mul(b);
    u16::try_from(both)
        .ok()
        .filter(|both| *both <= 192)
        .and_then(std::num::NonZeroU16::new)
        .unwrap_or(snap)
}

/// How far above its own pitch a note on `position` ticks with tuned lanes, in cents: a whole tone
/// a lane either side of the chart's middle, or closer on a chart too wide for an octave across,
/// so every chart spans an octave at most. A note across lanes ticks at their middle.
fn lane_cents(position: charter_core::Position, lanes: u16) -> i16 {
    let charter_core::Position::Lane { index, width } = position else {
        return 0;
    };
    let Some(gaps) = lanes.checked_sub(1).filter(|gaps| *gaps > 0) else {
        return 0;
    };
    let step = (1200.0 / f64::from(gaps)).min(200.0);
    let at = f64::from(u16::from(index)) + (f64::from(u16::from(width)) - 1.0) / 2.0;
    let cents = ((at - f64::from(gaps) / 2.0) * step)
        .round()
        .clamp(-1200.0, 1200.0);

    crate::cast::f64_i16(cents)
}

/// Note ticks and metronome clicks between `from` and `until` seconds, as the playback settings
/// request them.
fn clicks(
    song: &Song,
    settings: &Settings,
    metronome: bool,
    (from, until): (f64, f64),
    grid: charter_timing::grid::Grid,
) -> Vec<(f64, charter_audio::Tone)> {
    use charter_audio::Tone;
    use charter_timing::grid::Line;
    let timing = song.timing();
    let mut clicks = Vec::new();
    let tick = settings.note_ticks.then(|| settings.tick_sound.tick());
    if let (Some(tick), Some(chart)) = (tick, song.chart()) {
        let lanes = chart.lanes.map_or(0, u16::from);
        let heads = chart
            .objects()
            .filter(|object| song.layer_look(object).0 == charter_core::LayerView::Shown)
            .map(|object| {
                let cents = if settings.tuned_lanes {
                    lane_cents(object.position, lanes)
                } else {
                    0
                };
                (timing.time_at(object.tick).seconds(), tick.pitched(cents))
            });
        clicks.extend(heads.filter(|(at, _)| (from..=until).contains(at)));
    }
    if metronome {
        let kit = settings.metronome_sound;
        // On the beats, or on every line of the timing window's grid.
        let grid = if settings.metronome_grid {
            grid
        } else {
            charter_timing::grid::Grid::default()
        };
        let resolution = timing.ticks_per_beat();
        let first = timing.latest_tick_by(from);
        'measures: for measure in timing.measures_from(first) {
            for (line_tick, line) in grid.lines(&measure, resolution) {
                let at = timing.time_at(line_tick).seconds();
                if at > until {
                    break 'measures;
                }
                if at >= from {
                    let tone = match line {
                        Line::Downbeat => Tone::Accent(kit),
                        Line::Beat => Tone::Beat(kit),
                        Line::Sub { .. } => Tone::Sub(kit),
                    };
                    clicks.push((at, tone));
                }
            }
        }
    }
    clicks.sort_by(|a, b| a.0.total_cmp(&b.0));
    clicks
}

/// What the audio strip draws of `file`, the audio the song plays, when `settings` want the
/// strip: its frequencies if they ask for those and they are ready, else its waveform.
fn shown_strip<'a>(
    settings: &Settings,
    waveform: Option<&'a OfFile<Arc<Peaks>>>,
    spectrum: Option<&'a OfFile<notefield::Tiles>>,
    file: Option<&Path>,
) -> Option<notefield::Strip<'a>> {
    let frequencies = spectrum
        .filter(|analysis| {
            settings.frequencies && settings.waveform && Some(analysis.file.as_path()) == file
        })
        .map(|analysis| notefield::Strip::Frequencies(&analysis.value));
    frequencies.or_else(|| {
        waveform
            .filter(|analysis| settings.waveform && Some(analysis.file.as_path()) == file)
            .map(|analysis| notefield::Strip::Waveform(&analysis.value))
    })
}

/// The timing a reset leaves: 120 BPM in 4/4 from 0 ms, as a new song starts with.
fn fresh_timing() -> charter_core::Timing {
    let fresh = serde_json::json!({
        "anchors": [ { "tick": 0, "seconds": 0.0 } ],
        "tempo": 120.0,
        "events": []
    });
    #[expect(
        clippy::unreachable,
        reason = "the constant default timing is valid JSON"
    )]
    serde_json::from_value(fresh).unwrap_or_else(|_| unreachable!("the fresh timing reads"))
}

/// What the timing found and put in place is: how many sections, at what tempo from when.
fn found_message(song: &Song) -> String {
    let timing = song.timing();
    let Some(first) = timing.anchors.first() else {
        return "Timing found".to_owned();
    };
    let bpm = timing
        .bpm_at(first.tick)
        .map_or_else(String::new, |bpm| format!("{bpm} BPM "));
    let from = style::clock(charter_core::ResolvedTime::from(first.seconds).seconds());
    let sections = match timing.anchors.len() {
        1 => "one section".to_owned(),
        count => format!("{count} sections"),
    };
    format!("Timing found: {bpm}from {from}, {sections}. Ctrl+Z undoes it.")
}

/// Makes the timing edit `action` asks for, and says why if it cannot.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "retime is called from a timing-only dispatcher; other actions are a no-op"
)]
fn retime(song: &mut Song, action: &Action) -> Option<String> {
    let done = match action {
        Action::SetBpmAt(tick, bpm) => song.set_bpm_at(*tick, *bpm),
        Action::MovePointTo(tick, seconds) => song.move_point_to(*tick, *seconds),
        Action::SetMetadata(metadata) => song.set_metadata(metadata.as_ref().clone()),
        Action::SetSetting { scope, key, value } => song.set_setting(*scope, key, value.clone()),
        Action::SetMeterAt(tick, meter) => song.set_meter_at(*tick, *meter),
        Action::RemoveTimingAt(tick) => song.remove_timing_at(*tick),
        Action::SetOffset(offset) => song.set_offset(*offset),
        Action::AddBpm => song.add_bpm_change(),
        Action::ImportTiming(timing) => song.replace_timing(timing.as_ref().clone()),
        Action::ResetTiming => song.replace_timing(fresh_timing()),
        Action::NudgeOffset(by) => {
            let moved = song.timing().time_at(charter_core::Tick::new(0)).seconds() + by;
            let micros = (moved * 1e6).round() / 1e6;
            match charter_core::Origin::try_from(micros) {
                Ok(offset) => song.set_offset(offset),
                Err(_) => return Some(format!("{moved} s is not an offset")),
            }
        }
        Action::MoveTiming { from, to } => song.move_timing_at(*from, *to),
        _ => Ok(()),
    };
    done.err().as_ref().map(refused)
}

/// Raises the tempo at the cursor by `by` BPM, and says why if it cannot.
fn nudge_bpm(song: &mut Song, by: f64) -> Option<String> {
    let (_, bpm) = song.bpm()?;
    let nudged = f64::from(bpm) + by;
    match charter_core::Bpm::try_from(nudged) {
        Ok(new_bpm) => song.set_bpm(new_bpm).err().as_ref().map(refused),
        Err(_) => Some(format!("{nudged} is not a tempo")),
    }
}

/// What to say about a refused edit.
fn refused(refusal: &Refusal) -> String {
    format!("Refused: {refusal}")
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use std::num::NonZeroU16;
    pub(crate) use std::path::{Path, PathBuf};
    pub(crate) use std::sync::Arc;

    pub(crate) use std::error::Error;

    pub(crate) use charter_core::{
        Chart, LaneCount, LaneIndex, LaneSpan, Position, Tick, Valid, ValidDocument,
    };
    pub(crate) use egui::accesskit::Role;
    pub(crate) use egui_kittest::Harness;
    pub(crate) use egui_kittest::kittest::{NodeT, Queryable};

    pub(crate) use super::*;
    pub(crate) use crate::flip::Flip;
    pub(crate) use crate::recovery;
    pub(crate) use crate::settings::Sound;
    pub(crate) use charter_audio::{Gain, Kit, Tone, Volumes};

    /// The format chapter, embedded at compile time so the tests do no I/O.
    pub(crate) const SPEC: &str = include_str!("../../../../docs/src/format.md");

    /// The format chapter's full example song.
    pub(crate) fn example() -> &'static [u8] {
        let start = SPEC
            .rfind("```json\n")
            .unwrap()
            .saturating_add("```json\n".len());
        let end = start.saturating_add(
            SPEC.as_bytes()[start..]
                .windows(3)
                .position(|part| part == b"```")
                .unwrap(),
        );
        &SPEC.as_bytes()[start..end]
    }

    pub(crate) fn window(app: Window) -> Harness<'static, Window> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut Window| app.ui(ui), app);
        harness.run();
        harness
    }

    pub(crate) fn with_demo() -> Harness<'static, Window> {
        let mut app = Window::default();
        app.open_demo();
        window(app)
    }

    /// A file dropped on the window, held in memory.
    #[derive(Debug)]
    pub(crate) struct Dropped(pub(crate) &'static str, pub(crate) Vec<u8>);

    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(self.1.clone())
        }
    }

    /// Accepts every `sm` document, standing in for the StepMania plugin.
    #[derive(Debug)]
    pub(crate) struct AnySm;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for AnySm {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }
    }

    /// Refuses an `sm` document with a note on the fourth lane, which no format rule does.
    #[derive(Debug)]
    pub(crate) struct NoFourthLane;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for NoFourthLane {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            let fourth = Some(LaneSpan::new(LaneIndex::new(3), LaneCount::ONE));
            let on_fourth = |chart: Valid<'_, Chart>| {
                chart
                    .objects()
                    .any(|object| object.position.lanes() == fourth)
            };
            if document.charts().any(on_fourth) {
                return Err("a note on lane 4".into());
            }
            Ok(())
        }
    }

    pub(crate) const PLUGINS: &[&dyn RulesetPlugin] = &[&AnySm];

    pub(crate) fn editable_demo() -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        window(app)
    }

    /// The lanes of the open chart's objects on `tick`, lowest first.
    pub(crate) fn lanes_at(harness: &Harness<'_, Window>, tick: i32) -> Vec<u16> {
        let song = harness.state().song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let mut lanes: Vec<u16> = chart
            .objects
            .iter()
            .filter(|object| object.tick == Tick::new(tick))
            .filter_map(|object| match object.position {
                Position::Lane { index, .. } => Some(u16::from(index)),
                Position::Plane { .. } => None,
            })
            .collect();
        lanes.sort_unstable();
        lanes
    }

    pub(crate) fn cursor_shows(harness: &Harness<'_, Window>, position: &str) -> bool {
        harness
            .query_by_label(&format!("Cursor at {position}"))
            .is_some()
    }

    /// Presses and releases each of `keys` in turn, all in the next frame, as a fast typist does
    /// when frames are slow. Queued events would each get a frame of their own.
    pub(crate) fn tap_in_one_frame(harness: &mut Harness<'_, Window>, keys: &[egui::Key]) {
        for &key in keys {
            for pressed in [true, false] {
                harness.input_mut().events.push(egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                    physical_key: None,
                });
            }
        }
    }

    /// A key event as a keyboard sends it: what the layout makes of the key under `modifiers`,
    /// and where the key sits.
    pub(crate) fn keyboard(
        key: egui::Key,
        place: egui::Key,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) -> egui::Event {
        egui::Event::Key {
            key,
            pressed,
            repeat: false,
            modifiers,
            physical_key: Some(place),
        }
    }

    /// What starts on `lane` at `tick` in the open chart, as the format writes it.
    pub(crate) fn note_at(
        harness: &Harness<'_, Window>,
        tick: i32,
        lane: u16,
    ) -> Option<serde_json::Value> {
        let song = harness.state().song.as_ref()?;
        let chart = song.chart()?;
        chart
            .objects
            .iter()
            .find(|object| {
                object.tick == Tick::new(tick)
                    && matches!(object.position, Position::Lane { index, .. } if index == LaneIndex::new(lane))
            })
            .map(|object| serde_json::json!({ "kind": object.kind.to_string(), "shape": object.shape }))
    }

    pub(crate) fn tap() -> serde_json::Value {
        serde_json::json!({ "kind": "tap", "shape": "instant" })
    }

    pub(crate) fn hold(duration: u32) -> serde_json::Value {
        serde_json::json!({ "kind": "hold", "shape": { "sustain": { "duration": duration } } })
    }

    pub(crate) fn mine() -> serde_json::Value {
        serde_json::json!({ "kind": "mine", "shape": "instant" })
    }

    /// Presses the lane key for `lane` under Shift, which a US layout reports as a symbol.
    pub(crate) fn shift_down(harness: &Harness<'_, Window>, symbol: egui::Key, place: egui::Key) {
        let shift = egui::Modifiers::SHIFT;
        harness.event(egui::Event::ModifiersChanged(shift));
        harness.event(keyboard(symbol, place, true, shift));
    }

    /// An `sm` song with one four-lane chart of taps walking across the lanes a sixteenth apart.
    pub(crate) fn four_taps() -> Vec<u8> {
        let tap = |tick: i32, lane: u16| {
            serde_json::json!({ "tick": tick, "position": { "lane": { "index": lane } },
                                "kind": "tap", "shape": "instant" })
        };
        let song = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Four", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 4,
                          "objects": [tap(0, 0), tap(240, 1), tap(480, 2), tap(720, 3)] } ]
        });
        serde_json::to_vec(&song).unwrap()
    }

    pub(crate) fn editable_four_taps() -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("four.json"), &four_taps());
        window(app)
    }

    pub(crate) fn note_count(harness: &Harness<'_, Window>) -> usize {
        let song = harness.state().song.as_ref().unwrap();
        song.chart().unwrap().objects.len()
    }

    pub(crate) fn nothing_selected(harness: &Harness<'_, Window>) -> bool {
        harness.query_by_label_contains("selected").is_none()
    }

    /// Types `text` over the song panel's field `label`, then presses Enter.
    pub(crate) fn retype(harness: &mut Harness<'_, Window>, label: &str, text: &str) {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Backspace);
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .type_text(text);
        harness.key_press(egui::Key::Enter);
        harness.run();
    }

    pub(crate) fn metadata(harness: &Harness<'_, Window>) -> charter_core::Metadata {
        let song = harness.state().song.as_ref().unwrap();
        song.document().document().metadata.clone()
    }

    pub(crate) fn picture(bytes: &[u8]) -> Picture {
        (
            bytes.to_vec(),
            egui::ColorImage::filled([4, 4], egui::Color32::RED),
        )
    }

    pub(crate) fn artwork_name(harness: &Harness<'_, Window>) -> Option<String> {
        metadata(harness).artwork.map(|path| path.to_string())
    }

    /// Where the middle of `lane` meets the line of `tick` on screen, `below` points lower.
    pub(crate) fn spot(
        harness: &Harness<'_, Window>,
        lane: u16,
        tick: i32,
        below: f32,
    ) -> egui::Pos2 {
        let area = harness.get_by_label("Notefield").rect();
        let app = harness.state();
        let song = app.song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let timing = chart.timing();
        let lanes = chart.lanes.map_or(0, u16::from);
        let field = notefield::Field::new(
            area,
            lanes,
            timing.time_at(song.cursor()),
            app.zoom,
            app.strip().is_some(),
        );
        let x = field
            .columns
            .span(LaneIndex::new(lane), LaneCount::ONE)
            .center();
        egui::pos2(x, field.view.y(timing.time_at(Tick::new(tick))) + below)
    }

    pub(crate) fn press(
        harness: &Harness<'_, Window>,
        pos: egui::Pos2,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }

    pub(crate) fn click(
        harness: &mut Harness<'_, Window>,
        pos: egui::Pos2,
        modifiers: egui::Modifiers,
    ) {
        harness.hover_at(pos);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        press(harness, pos, true, modifiers);
        press(harness, pos, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
    }

    pub(crate) fn drag(
        harness: &mut Harness<'_, Window>,
        from: egui::Pos2,
        to: egui::Pos2,
        modifiers: egui::Modifiers,
    ) {
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        press(harness, from, true, modifiers);
        for step in 1..=4 {
            harness
                .hover_at(from.lerp(to, f32::from(u16::try_from(step).unwrap_or(u16::MAX)) / 4.0));
        }
        press(harness, to, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
    }

    pub(crate) fn lane_x(harness: &Harness<'_, Window>, lane: u16) -> f32 {
        field(harness)
            .columns
            .span(LaneIndex::new(lane), LaneCount::ONE)
            .center()
    }

    pub(crate) fn select_note(harness: &mut Harness<'_, Window>, lane: u16, tick: i32) {
        let pos = on_line(harness, lane_x(harness, lane), tick);
        click(harness, pos, egui::Modifiers::NONE);
    }

    /// The one request the window made since the last were taken.
    pub(crate) fn request(harness: &mut Harness<'_, Window>) -> Request {
        let mut requests = harness.state_mut().take_requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        requests.remove(0)
    }

    /// Whether `bytes` hold a song whose open chart has a tap on `lane` at `tick`.
    pub(crate) fn holds_tap(bytes: &[u8], tick: i32, lane: u16) -> bool {
        let song = charter_core::ValidDocument::from_json(bytes).unwrap();
        let chart = song.charts().next().unwrap();
        chart.objects().any(|object| {
            object.tick == Tick::new(tick)
                && object.position.lanes()
                    == Some(LaneSpan::new(LaneIndex::new(lane), LaneCount::ONE))
        })
    }

    /// The four taps with one more placed, so there is something to lose.
    pub(crate) fn edited_four_taps() -> Harness<'static, Window> {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.run();
        harness
    }

    /// Asks to close the window as its close button does, and runs the one frame that answers.
    pub(crate) fn close(harness: &mut Harness<'_, Window>) {
        let root = harness.input_mut().viewports.entry(egui::ViewportId::ROOT);
        root.or_default().events.push(egui::ViewportEvent::Close);
        harness.step();
    }

    /// Whether the last frame sent `command` to the host's window.
    pub(crate) fn sent(harness: &Harness<'_, Window>, command: &egui::ViewportCommand) -> bool {
        let output = harness
            .output()
            .viewport_output
            .get(&egui::ViewportId::ROOT);
        output.is_some_and(|viewport| viewport.commands.contains(command))
    }

    pub(crate) fn asks_to_save(harness: &Harness<'_, Window>) -> bool {
        harness
            .query_by_label("Save changes to four.json?")
            .is_some()
    }

    /// Shows the settings page named `name`.
    pub(crate) fn settings_page(harness: &mut Harness<'_, Window>, name: &str) {
        harness.get_by_label(name).click();
        harness.run();
    }

    /// The demo, open for editing with the notefield running `direction`.
    pub(crate) fn turned_demo(direction: Direction) -> Harness<'static, Window> {
        let settings = Settings {
            direction,
            ..Settings::default()
        };
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open_demo();
        window(app)
    }

    /// Where the middle of `lane` meets the line of `tick` on screen, however the notefield runs.
    pub(crate) fn turned_spot(harness: &Harness<'_, Window>, lane: u16, tick: i32) -> egui::Pos2 {
        let area = harness.get_by_label("Notefield").rect();
        let app = harness.state();
        let song = app.song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let timing = chart.timing();
        let lanes = chart.lanes.map_or(0, u16::from);
        let flip = Flip {
            area,
            direction: app.settings().direction,
        };
        let field = notefield::Field::new(
            flip.field(),
            lanes,
            timing.time_at(song.cursor()),
            app.zoom,
            app.strip().is_some(),
        );
        let x = field
            .columns
            .span(LaneIndex::new(lane), LaneCount::ONE)
            .center();
        flip.pos(egui::pos2(x, field.view.y(timing.time_at(Tick::new(tick)))))
    }

    /// Clicks what `label` names in the song panel, scrolled into view first, since the panel
    /// runs past the bottom of the test's window.
    pub(crate) fn click_in_panel(harness: &mut Harness<'_, Window>, label: &str) {
        harness.get_by_label(label).scroll_to_me();
        harness.run();
        harness.get_by_label(label).click();
        harness.run();
    }

    pub(crate) fn bpm_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "BPM")
    }

    /// Types `text` over whatever the BPM field holds, then presses `key`.
    pub(crate) fn enter_bpm(harness: &mut Harness<'_, Window>, text: &str, key: egui::Key) {
        bpm_field(harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        bpm_field(harness).type_text(text);
        harness.run();
        harness.key_press(key);
        harness.run();
    }

    /// The tick and BPM of the change in effect at the cursor.
    pub(crate) fn bpm_at_cursor(harness: &Harness<'_, Window>) -> Option<(i64, String)> {
        let song = harness.state().song.as_ref()?;
        let (tick, bpm) = song.bpm()?;
        let tick = serde_json::to_value(tick).ok()?.as_i64()?;
        let value = f64::from(bpm);
        let one = (value * 10.0).round() / 10.0;
        let shown = if (value - one).abs() < 1e-5 {
            charter_core::Bpm::try_from(one).map_or_else(|_| bpm.to_string(), |bpm| bpm.to_string())
        } else {
            bpm.to_string()
        };
        Some((tick, shown))
    }

    /// The tick and time of the first note after the start of the open chart.
    pub(crate) fn first_note(harness: &Harness<'_, Window>) -> (Tick, f64) {
        let song = harness.state().song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let note = chart
            .objects
            .iter()
            .find(|note| note.tick > Tick::new(0))
            .unwrap();
        (note.tick, song.timing().time_at(note.tick).seconds())
    }

    pub(crate) fn meter_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "Meter")
    }

    pub(crate) fn section_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "Section here")
    }

    /// Types `text` over whatever the section field holds, then presses `key`.
    pub(crate) fn enter_section(harness: &mut Harness<'_, Window>, text: &str, key: egui::Key) {
        section_field(harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        section_field(harness).type_text(text);
        harness.run();
        harness.key_press(key);
        harness.run();
    }

    pub(crate) fn marker_texts(harness: &Harness<'_, Window>) -> Vec<String> {
        let song = harness.state().song.as_ref().unwrap();
        let markers = &song.document().document().markers;
        markers.iter().map(|marker| marker.text.clone()).collect()
    }

    pub(crate) fn four_taps_at(path: &str) -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new(path), &four_taps());
        window(app)
    }

    /// Four taps whose audio the host has loaded.
    pub(crate) fn playable() -> Harness<'static, Window> {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .audio_ready(Path::new("/songs/a.ogg"), 60.0);
        harness.run();
        harness
    }

    pub(crate) fn space(harness: &mut Harness<'_, Window>) {
        harness.key_press(egui::Key::Space);
        harness.run();
    }

    /// Moves the playhead of a playing song to `seconds`, as the host reports each frame.
    pub(crate) fn play_to(harness: &mut Harness<'_, Window>, seconds: f64) {
        harness.state_mut().playhead(seconds);
        harness.run();
    }

    pub(crate) fn playable_with(settings: Settings) -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness
            .state_mut()
            .audio_ready(Path::new("/songs/a.ogg"), 2.0);
        harness.run();
        harness
    }

    pub(crate) fn clicks_played(harness: &mut Harness<'_, Window>) -> Vec<(f64, Tone)> {
        space(harness);
        let Request::Play { clicks, .. } = request(harness) else {
            panic!("expected play");
        };
        clicks
    }

    pub(crate) fn quieter() -> Volumes {
        Volumes {
            music: Gain::from(0.5),
            ticks: Gain::from(0.25),
            metronome: Gain::from(0.75),
            subdivision: Gain::from(0.5),
        }
    }

    /// Four taps whose two seconds of audio have loaded, with the timing window open.
    pub(crate) fn listening(settings: Settings) -> Harness<'static, Window> {
        let mut harness = playable_with(settings);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        harness.state_mut().take_requests();
        harness
    }

    /// Where a play started, what it loops and what clicks over it.
    type Played = (f64, Option<(f64, f64)>, Vec<(f64, Tone)>);

    pub(crate) fn played(harness: &mut Harness<'_, Window>) -> Played {
        let requests = harness.state_mut().take_requests();
        let play = requests.into_iter().rev().find_map(|request| {
            if let Request::Play {
                from,
                looping,
                clicks,
                ..
            } = request
            {
                Some((from, looping, clicks))
            } else {
                None
            }
        });
        play.expect("a play")
    }

    /// Four lanes at 120 BPM that change to 150 BPM at the second beat, with a section at the
    /// first eighth.
    pub(crate) fn timed_song() -> Harness<'static, Window> {
        let song = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "0.1.0" },
            "metadata": { "title": "Timed", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 960, "kind": { "bpm": 150.0 } }
            ] },
            "markers": [ { "tick": 480, "kind": "section", "text": "Drop" } ],
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("timed.json"), &serde_json::to_vec(&song).unwrap());
        window(app)
    }

    pub(crate) fn field(harness: &Harness<'_, Window>) -> notefield::Field {
        let area = harness.get_by_label("Notefield").rect();
        let app = harness.state();
        let song = app.song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let lanes = chart.lanes.map_or(0, u16::from);
        notefield::Field::new(
            area,
            lanes,
            chart.timing().time_at(song.cursor()),
            app.zoom,
            app.strip().is_some(),
        )
    }

    /// Where the line of `tick` crosses the column at `x`.
    pub(crate) fn on_line(harness: &Harness<'_, Window>, x: f32, tick: i32) -> egui::Pos2 {
        let field = field(harness);
        let song = harness.state().song.as_ref().unwrap();
        let y = field.view.y(song.timing().time_at(Tick::new(tick)));
        egui::pos2(x, y)
    }

    pub(crate) fn event_ticks(harness: &Harness<'_, Window>) -> Vec<Tick> {
        let song = harness.state().song.as_ref().unwrap();
        let timing = song.timing();
        let mut ticks: Vec<_> = timing
            .anchors
            .iter()
            .map(|anchor| anchor.tick)
            .chain(timing.events.iter().map(|event| event.tick))
            .collect();
        ticks.sort_unstable();
        ticks
    }

    /// The timing window open over `timed_song`, laid out a bar a row, as the gesture tests
    /// measure their spots.
    pub(crate) fn syncing() -> Harness<'static, Window> {
        let mut harness = timed_song();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        harness
            .state_mut()
            .sync
            .set_layout(charter_timing::rows::Layout::Bars);
        harness.run();
        harness
    }

    /// Where `tick` lies across its bar's row in the timing window, a bar a row with a tenth of
    /// the bar before showing ahead of its downbeat.
    pub(crate) fn in_row(harness: &Harness<'_, Window>, tick: i32) -> egui::Pos2 {
        let song = harness.state().song.as_ref().unwrap();
        let measure = song.timing().measures_from(Tick::new(tick)).next().unwrap();
        let row = harness
            .get_by_label(&format!("Measure {}", measure.number))
            .rect();
        let lead = charter_timing::rows::LEAD_IN;
        let across =
            crate::cast::f64_f32((measure.fraction(Tick::new(tick)) + lead) / (1.0 + lead));
        egui::pos2(row.left() + across * row.width(), row.top() + 30.0)
    }

    pub(crate) fn seconds_at(harness: &Harness<'_, Window>, tick: i32) -> f64 {
        let song = harness.state().song.as_ref().unwrap();
        song.timing().time_at(Tick::new(tick)).seconds()
    }

    pub(crate) fn anchor_seconds(harness: &Harness<'_, Window>) -> Vec<f64> {
        let song = harness.state().song.as_ref().unwrap();
        song.anchors().iter().map(|pin| pin.seconds).collect()
    }

    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the Reset timing menu label with its ellipsis"
    )]
    pub(crate) fn reset_from_the_menu(harness: &mut Harness<'_, Window>) {
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Reset timing…").click();
        harness.run();
    }

    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the Find timing menu label with its ellipsis"
    )]
    pub(crate) fn find_from_the_menu(harness: &mut Harness<'_, Window>) {
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Find timing with Beat This!…").click();
        harness.run();
    }

    /// The timed song with its audio loaded and one strong onset `seconds` in.
    pub(crate) fn syncing_with_an_onset(seconds: f64) -> Harness<'static, Window> {
        let mut harness = syncing();
        let file = harness.state().audio_file().unwrap();
        harness.state_mut().audio_ready(&file, 60.0);
        let onset = charter_audio::Onset {
            seconds,
            strength: 1.0,
        };
        harness.state_mut().onsets_ready(&file, vec![onset].into());
        harness.run();
        harness.state_mut().take_requests();
        harness
    }

    /// Reads `offset,bpm` as a timing, standing in for a game's timing format.
    #[derive(Debug)]
    pub(crate) struct Importer;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for Importer {
        fn id(&self) -> &'static str {
            "importer"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }

        fn timing_format(&self) -> Option<&str> {
            Some("test timing")
        }

        fn import_timing(
            &self,
            text: &str,
            _: &ValidDocument,
        ) -> Result<charter_core::ImportedTiming, Box<dyn Error + Send + Sync>> {
            let (offset, bpm) = text.trim().split_once(',').ok_or("no comma")?;
            let timing = serde_json::from_value(serde_json::json!({
                "anchors": [ { "tick": 0, "seconds": offset.parse::<f64>()? } ],
                "tempo": bpm.parse::<f64>()?,
                "events": []
            }))?;
            Ok(charter_core::ImportedTiming {
                timing,
                left_out: vec!["kiai time".to_owned()],
            })
        }
    }

    /// Reads the same text differently depending on the destination game's ruleset.
    #[derive(Debug)]
    pub(crate) struct ContextImporter;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for ContextImporter {
        fn id(&self) -> &'static str {
            "context-importer"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }

        fn timing_format(&self) -> Option<&str> {
            Some("context timing")
        }

        fn import_timing(
            &self,
            text: &str,
            into: &ValidDocument,
        ) -> Result<charter_core::ImportedTiming, Box<dyn Error + Send + Sync>> {
            let bpm = if into.document().ruleset.id.as_str() == "ttr" {
                "208.1"
            } else {
                "150"
            };
            Importer.import_timing(&format!("{text},{bpm}"), into)
        }
    }

    pub(crate) const IMPORTING: &[&dyn RulesetPlugin] = &[&AnySm, &Importer];
    pub(crate) const CONTEXT_IMPORTING: &[&dyn RulesetPlugin] = &[
        &charter_stepmania::Plugin,
        &charter_ttr::Plugin,
        &ContextImporter,
    ];

    #[expect(
        clippy::non_ascii_literal,
        reason = "the import test opens the Import timing menu label with its ellipsis"
    )]
    pub(crate) fn importing() -> Harness<'static, Window> {
        let mut app = Window::new(IMPORTING);
        app.open_demo();
        let mut harness = window(app);
        // The menu, which comes before the song panel's section of the same name.
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Import timing…").click();
        harness.run();
        harness
    }

    pub(crate) fn moments(harness: &Harness<'_, Window>) -> Vec<f64> {
        let song = harness.state().song.as_ref().unwrap();
        song.anchors().iter().map(|pin| pin.seconds).collect()
    }

    pub(crate) fn peaks() -> Arc<charter_audio::Peaks> {
        let frames = vec![[0.25, -0.25]; 4000];
        Arc::new(charter_audio::Peaks::of(
            &charter_audio::Track::from_frames(1000, frames),
        ))
    }

    /// Runs the window with its clock at `seconds`.
    pub(crate) fn at_time(harness: &mut Harness<'_, Window>, seconds: f64) {
        harness.input_mut().time = Some(seconds);
        harness.run();
    }

    /// The four taps edited at one second in, and left long enough to be autosaved.
    pub(crate) fn autosaved_four_taps() -> (Harness<'static, Window>, Vec<u8>) {
        let mut harness = editable_four_taps();
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 31.5);
        let Request::Autosave { bytes } = request(&mut harness) else {
            panic!("expected an autosave");
        };
        (harness, bytes)
    }

    pub(crate) fn four_taps_with(settings: Settings) -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open(Path::new("four.json"), &four_taps());
        window(app)
    }

    pub(crate) fn snapshots(harness: &mut Harness<'_, Window>) -> Vec<(String, Vec<PathBuf>)> {
        let requests = harness.state_mut().take_requests();
        requests
            .into_iter()
            .filter_map(|request| {
                if let Request::Snapshot { name, folders, .. } = request {
                    Some((name, folders))
                } else {
                    None
                }
            })
            .collect()
    }

    pub(crate) fn restore_prompt(harness: &Harness<'_, Window>) -> bool {
        harness
            .query_by_label("Restore unsaved changes to four.json?")
            .is_some()
    }

    /// The StepMania and Tap Tap Revenge plugins, which start songs.
    pub(crate) const GAMES: &[&dyn RulesetPlugin] =
        &[&charter_stepmania::Plugin, &charter_ttr::Plugin];

    pub(crate) fn new_song_dialog() -> Harness<'static, Window> {
        let mut harness = window(Window::new(GAMES));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        harness
    }

    pub(crate) fn dialog_open(harness: &Harness<'_, Window>) -> bool {
        harness.query_by_label("Create").is_some()
    }

    pub(crate) fn charts(harness: &Harness<'_, Window>) -> Vec<(String, Option<LaneCount>)> {
        let song = harness.state().song.as_ref().unwrap();
        let charts = &song.document().document().charts;
        charts
            .iter()
            .map(|chart| (chart.name.clone(), chart.lanes))
            .collect()
    }

    pub(crate) fn lanes(n: u16) -> Option<LaneCount> {
        LaneCount::new(n)
    }

    pub(crate) fn created() -> Harness<'static, Window> {
        let mut harness = new_song_dialog();
        harness.get_by_label("Create").click();
        harness.run();
        harness
    }

    /// A file dropped on the window that only a path stands for.
    #[derive(Debug)]
    pub(crate) struct DroppedPath(pub(crate) &'static str);

    impl egui::DroppedFile for DroppedPath {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            Err("not read in tests".to_owned())
        }
    }

    /// Drops the file at `path` on the window, and forgets the tags it asks the host for.
    pub(crate) fn drop_file(harness: &mut Harness<'_, Window>, path: &'static str) {
        drop_keeping_requests(harness, path);
        let requests = &mut harness.state_mut().requests;
        requests.retain(|request| !matches!(request, Request::ReadTags { .. }));
    }

    pub(crate) fn drop_keeping_requests(harness: &mut Harness<'_, Window>, path: &'static str) {
        harness
            .input_mut()
            .dropped_files
            .push(Arc::new(DroppedPath(path)));
        harness.run();
    }

    pub(crate) fn read_tags(
        harness: &mut Harness<'_, Window>,
        path: &str,
        title: &str,
        artist: &str,
    ) {
        let file = Path::new(path);
        let (title, artist) = (Some(title.to_owned()), Some(artist.to_owned()));
        harness.state_mut().tags_read(file, title, artist);
        harness.run();
    }

    pub(crate) fn field_value(harness: &Harness<'_, Window>, label: &str) -> Option<String> {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .value()
    }

    pub(crate) fn facts(harness: &Harness<'_, Window>) -> (String, String) {
        let song = harness.state().song.as_ref().unwrap();
        let metadata = &song.document().document().metadata;
        (metadata.title.clone(), metadata.artist.clone())
    }

    pub(crate) const OFFER: &str = "Do you want to import song metadata from tags?";

    pub(crate) fn offset_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "Offset")
    }

    pub(crate) fn offset(harness: &Harness<'_, Window>) -> serde_json::Value {
        let song = harness.state().song.as_ref().unwrap();
        serde_json::to_value(
            song.document()
                .timing()
                .time_at(charter_core::Tick::new(0))
                .seconds(),
        )
        .unwrap()
    }

    pub(crate) fn enter_offset(harness: &mut Harness<'_, Window>, text: &str) {
        offset_field(harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        offset_field(harness).type_text(text);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
    }

    pub(crate) fn no_audio(harness: &Harness<'_, Window>) -> bool {
        harness.query_by_label_contains("No audio yet").is_some()
    }

    pub(crate) fn audio(harness: &Harness<'_, Window>) -> String {
        let song = harness.state().song.as_ref().unwrap();
        song.document()
            .document()
            .metadata
            .audio
            .as_str()
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn a_preview_that_is_not_a_time_says_so() {
        let mut harness = editable_demo();
        retype(&mut harness, "Preview", "soon");
        assert!(
            harness
                .query_by_label_contains("soon is not a time")
                .is_some()
        );
        assert_eq!(metadata(&harness).preview, None);
    }
    #[test]
    fn space_before_the_audio_is_ready_says_so() {
        let mut harness = four_taps_at("/songs/four.json");
        space(&mut harness);
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(harness.query_by_label_contains("still loading").is_some());
    }
    #[test]
    fn a_message_can_be_dismissed() {
        let mut harness = editable_demo();
        space(&mut harness);
        harness.get_by_label("Dismiss").click();
        harness.run();
        assert_eq!(harness.state().message(), None);
        assert!(harness.query_by_label("Dismiss").is_none());
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use std::num::NonZeroU16;

    #[kani::proof]
    #[kani::unwind(34)]
    fn the_shown_grid_holds_both_requested_lines_or_falls_back_to_snap() {
        let a: u16 = u16::from(kani::any::<u8>());
        let b: u16 = u16::from(kani::any::<u8>());
        kani::assume(a > 0 && a <= 192 && b > 0 && b <= 192);
        let grid = NonZeroU16::new(a).expect("positive");
        let snap = NonZeroU16::new(b).expect("positive");
        let shown = shown_grid(grid, snap).get();
        if shown != b {
            assert!(shown <= 192);
            assert_eq!(shown % a, 0);
            assert_eq!(shown % b, 0);
        }
    }
}
