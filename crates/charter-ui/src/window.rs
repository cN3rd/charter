//! The editor window's state, and how actions from its panels, keys and gestures change it.

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

/// Something only the program hosting the window can do. The host answers a save through
/// [`Window::saved`], and an open by passing the file to [`Window::open`].
#[derive(Debug, PartialEq)]
pub enum Request {
    /// Write `bytes`, the song as it stands, to `path`.
    Write {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    /// Ask where to save the song, suggesting the file name `name`, then write `bytes` there.
    SaveAs {
        name: String,
        bytes: Vec<u8>,
    },
    /// Ask for a song to open.
    Open,
    /// Keep `bytes`, the song's unsaved changes, where the next run finds them, in place of any
    /// kept before. The host passes them to [`Window::recover`] when it starts, and says through
    /// [`Window::autosave_failed`] if they could not be kept.
    Autosave {
        bytes: Vec<u8>,
    },
    /// Remove the kept changes: they were saved or thrown away.
    DropAutosave,
    /// Copy the song's audio from `from` to `to`, beside the song, unless a file is there. The
    /// host says through [`Window::audio_not_copied`] if it could not.
    CopyAudio {
        from: PathBuf,
        to: PathBuf,
    },
    /// Play the song's audio from `from` seconds, at `rate` times its speed. The host reports
    /// where it is through [`Window::playhead`], and says when it ends through
    /// [`Window::playback_ended`].
    /// `clicks` are the tones to mix over it, each at its time in seconds into the audio.
    /// `volumes` say how loud the audio and each kind of click play, and `looping` the stretch
    /// to play over and over, in seconds, if any.
    Play {
        from: f64,
        rate: f64,
        clicks: Vec<(f64, charter_audio::Tone)>,
        volumes: charter_audio::Volumes,
        looping: Option<(f64, f64)>,
        /// Whether audio played faster or slower than its speed keeps its pitch.
        keep_pitch: bool,
    },
    /// Carry on playing with the audio at this volume.
    Volume(charter_audio::Gain),
    Pause,
    /// Carry on playing from this many seconds into the audio.
    Seek(f64),
    /// Carry on playing at this many times the audio's speed.
    Rate(f64),
    /// Play `clicks` over silence, each at its time in seconds from now, alongside anything
    /// playing: a taste of a metronome sound.
    Preview {
        clicks: Vec<(f64, charter_audio::Tone)>,
        volumes: charter_audio::Volumes,
    },
    /// Ask the user to choose an audio file for the open song, and pass it to
    /// [`Window::use_audio`].
    ChooseAudio,
    /// Ask the user to choose a picture for the open song's artwork, and pass it to
    /// [`Window::use_artwork`].
    ChooseArtwork,
    /// Read and decode the picture at `file`, the song's artwork, and pass it to
    /// [`Window::artwork_read`].
    ReadArtwork {
        file: PathBuf,
    },
    /// Ask the user to choose a file holding timing in another program's format, such as a
    /// `.osu` file, and pass its text to [`Window::timing_file_read`].
    ChooseTimingFile,
    /// Read the tags of the audio file at `file`, and pass them to [`Window::tags_read`].
    ReadTags {
        file: PathBuf,
    },
    /// Find the timing of the audio in `file` at `ticks_per_beat` and pass it to
    /// [`Window::timing_found`], or call [`Window::weights_needed`] when the beat tracker's
    /// weights are not there yet.
    FindTiming {
        file: PathBuf,
        ticks_per_beat: charter_core::TicksPerBeat,
    },
    /// Download the beat tracker's weights, the user having agreed, telling
    /// [`Window::weights_progress`] how far it has got, then find the timing asked for.
    DownloadWeights,
}

/// Where finding the song's timing from its audio has got to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Finding {
    #[default]
    Idle,
    /// Waiting on the user to agree to download the beat tracker's weights.
    Asking,
    /// Downloading the weights, this share of them so far.
    Downloading(f32),
    /// The host is finding it.
    Running,
}

/// What tags read from an audio file would change, waiting on whether to take them.
#[derive(Debug)]
struct Offer {
    /// Whether they go to the new-song dialog rather than the open song.
    draft: bool,
    title: Option<String>,
    artist: Option<String>,
}

/// How fast the audio plays, as a factor of its own speed. The chart is untouched by it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate(f64);

impl Default for Rate {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Rate {
    const SLOWEST: f64 = 0.1;
    const FASTEST: f64 = 4.0;

    /// This rate changed by `step`, to the nearest percent, and kept between a tenth and four
    /// times the audio's speed, as ArrowVortex allows.
    #[must_use]
    pub fn by(self, step: f64) -> Self {
        Self::of(self.0 + step)
    }

    #[must_use]
    pub fn of(factor: f64) -> Self {
        let percent = (factor * 100.0).round() / 100.0;
        Self(percent.clamp(Self::SLOWEST, Self::FASTEST))
    }

    #[must_use]
    pub fn factor(self) -> f64 {
        self.0
    }

    #[must_use]
    pub fn percent(self) -> f64 {
        (self.0 * 100.0).round()
    }
}

/// How the audio the song wants stands with the host.
#[derive(Debug, PartialEq)]
enum Loaded {
    /// Ready to play, this many seconds long.
    Ready(f64),
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
        path: Option<PathBuf>,
        song: Box<Song>,
        /// Whether the song is the autosave, restored.
        restored: bool,
        /// The audio file dropped on the song, when an autosave kept it.
        audio: Option<PathBuf>,
    },
}

/// Unsaved changes from an earlier run, waiting on whether to restore them.
#[derive(Debug)]
struct Restoring {
    path: Option<PathBuf>,
    audio: Option<PathBuf>,
    song: Box<Song>,
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
    Tap,
    Mine,
}

impl Note {
    fn kind(self) -> Kind {
        match self {
            Self::Tap => Kind::Tap,
            Self::Mine => Kind::Mine,
        }
    }
}

/// Something a panel, key or gesture asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    OpenDemo,
    OpenChart(usize),
    Move(Step),
    MoveMeasure(Step),
    FinerSnap,
    CoarserSnap,
    ZoomBy(f32),
    /// Places a note on the lane at the cursor, or removes what starts there.
    Toggle(LaneIndex, Note),
    /// The lane key of a lane that toggled came up.
    LaneUp(LaneIndex),
    /// Every lane key that toggled is up again.
    LanesReleased,
    /// Places a note at `tick` on `lane`, or removes what starts there.
    Place {
        tick: Tick,
        lane: LaneIndex,
        note: Note,
    },
    /// Makes the tap or hold starting on `lane` at `from` end at `to`.
    Stretch {
        lane: LaneIndex,
        from: Tick,
        to: Tick,
    },
    /// Meets the selection with a note, or with none: nothing was under the pointer.
    Select(Option<NoteAt>, Selecting),
    /// Moves the selected notes this many ticks later and lanes right, earlier and left when
    /// negative.
    MoveSelection {
        ticks: i32,
        lanes: i32,
    },
    /// Meets the selection with the notes starting within `ticks` on a lane within `lanes`.
    SelectBetween {
        ticks: (Tick, Tick),
        lanes: (LaneIndex, LaneIndex),
        how: Selecting,
    },
    Undo,
    Redo,
    SelectAll,
    ClearSelection,
    DeleteSelection,
    /// Moves the end of the selected notes, or those at the cursor, a snap later or earlier.
    ResizeNotes(Step),
    /// Asks the host for a song to open.
    Open,
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
    SetVolumes(charter_audio::Volumes),
    /// Gives the timing point on the tick this tempo.
    SetBpmAt(Tick, charter_core::Bpm),
    /// Gives the song this title, artist, audio and preview.
    SetMetadata(Box<charter_core::Metadata>),
    /// Sets a text setting of the song or the open chart, or takes it away without a value.
    SetSetting {
        scope: charter_core::SettingScope,
        key: String,
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
    ShowSync,
    ShowImport,
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
        from: Tick,
        to: Tick,
    },
    /// Switch between the usual playback speed and the alternate one.
    ToggleRate,
    /// Mute the metronome, or unmute it, while a key is held.
    MuteMetronome(bool),
    ToggleSync,
    /// Moves every timing event at `from` to `to`, as one timing point.
    MoveTiming {
        from: Tick,
        to: Tick,
    },
    /// Moves the song's marker at `index` to `to`.
    MoveMarker {
        index: usize,
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
    /// Finds the song's timing from its audio.
    FindTiming,
    /// Draws the notefield's grid down to this note value.
    SetGrid(std::num::NonZeroU16),
}

/// The editor window's state.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // independent flags, not a state machine
pub struct Window {
    /// The ruleset plugins every song is checked against.
    plugins: &'static [&'static dyn RulesetPlugin],
    song: Option<Song>,
    /// The file the open song was read from or last saved to.
    path: Option<PathBuf>,
    /// What the host has yet to do.
    requests: Vec<Request>,
    /// The title last given to the host's window.
    shown_title: String,
    /// A save asked for this frame, made once every edit of the frame is in.
    saving: Option<Saving>,
    /// What waits on the answer to whether to save the open song's changes.
    discard: Option<Discard>,
    /// What to do once the save the user asked for is done.
    after_save: Option<Discard>,
    /// How far the window is in closing.
    leaving: Leaving,
    settings: Settings,
    /// Whether the settings window is showing.
    settings_open: bool,
    /// The timing window.
    sync: charter_timing::TimingWindow,
    /// The ticks being looped, while a loop plays.
    looping: Option<(Tick, Tick)>,
    /// The usual playback speed, while the alternate one plays.
    usual_rate: Option<Rate>,
    /// Whether the metronome is muted for as long as a key is held.
    muted: bool,
    /// The frame clock this frame, in seconds.
    frame_time: f64,
    /// The latency calibration window.
    calibration: latency::Calibration,
    /// Whether the window asks whether to reset the timing.
    resetting: bool,
    /// Where finding the timing from the audio has got to.
    finding: Finding,
    /// The import timing window.
    import: import::Import,
    /// How tall a second of the open song is on screen.
    zoom: Zoom,
    /// Why the last file failed to open, or the last edit was refused.
    failure: Option<String>,
    /// Touchpad scrolling not yet turned into cursor steps.
    scrolled: f32,
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
    /// A drag across the notefield in progress.
    gesture: Option<notefield::Gesture>,
    /// Whether the host may be keeping an autosave.
    autosave_kept: bool,
    /// The revision of the open song the autosave holds, when it holds one.
    autosaved: Option<u64>,
    /// When the open song's changes are next autosaved, in seconds on the frame clock.
    autosave_due: Option<f64>,
    /// Unsaved changes from an earlier run, waiting on whether to restore them.
    restoring: Option<Restoring>,
    /// The dialog that starts a song, while it shows.
    new_song: Option<new_song::Draft>,
    /// Tags that would change the title and artist, waiting on whether to take them.
    offer: Option<Offer>,
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
    /// The volumes the playback under way plays at, and whether clicks are mixed into it, which
    /// bakes every volume in.
    played: Option<(charter_audio::Volumes, bool)>,
    /// Whether files dropped on the window never arrive, as under Wayland, so the window offers
    /// only choosing them.
    no_drops: bool,
    /// What the keyboard shortcuts are filtered to.
    help_filter: String,
    /// The waveform of an audio file, once the host has summarized it.
    waveform: Option<(PathBuf, Arc<Peaks>)>,
    /// The frequencies of an audio file, once the host has analysed them.
    spectrum: Option<(PathBuf, Arc<Spectrum>)>,
    /// The onsets of an audio file the host found, and which file.
    onsets: Option<(PathBuf, Arc<[charter_audio::Onset]>)>,
    /// The audio file the host last reported on, and how it went.
    audio: Option<(PathBuf, Loaded)>,
    /// Whether the audio is playing.
    playing: bool,
    rate: Rate,
    /// The audio file dropped on the song, which is copied beside the song when it is saved
    /// elsewhere.
    audio_file: Option<PathBuf>,
    /// The song's cover artwork, as read, chosen or taken from its audio.
    artwork: Option<Artwork>,
    /// The artwork the audio of the song being started holds, and which file, to take when the
    /// song starts.
    embedded: Option<(PathBuf, String, Picture)>,
}

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

    /// The folder of the file the open song was read from or saved to: its project folder, where
    /// the host keeps what it works out from the song's audio.
    #[must_use]
    pub fn song_folder(&self) -> Option<PathBuf> {
        self.path.as_ref()?.parent().map(Path::to_path_buf)
    }

    /// Takes the text of the file chosen for [`Request::ChooseTimingFile`] into the import timing
    /// window, which shows what it holds.
    pub fn timing_file_read(&mut self, text: &str) {
        text.clone_into(&mut self.import.text);
        self.import.open = true;
    }

    /// Says a file chosen for [`Request::ChooseTimingFile`] could not be read, and `why`.
    pub fn timing_file_failed(&mut self, why: &str) {
        self.failure = Some(format!("Could not read the timing: {why}"));
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

    #[must_use]
    pub fn settings(&self) -> &Settings {
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

    /// Takes how a save and its audio preflight went. Only success makes the song clean.
    pub fn saved(&mut self, saved: Saved) {
        match saved {
            Saved::To(path) => {
                if let Some(song) = &mut self.song {
                    song.mark_saved();
                }
                self.audio_file = None;
                self.path = Some(path);
                if let Some(artwork) = &mut self.artwork {
                    artwork.saved();
                }
                self.failure = None;
                if let Some(discard) = self.after_save.take() {
                    self.carry_on(discard);
                }
            }
            Saved::Cancelled => self.after_save = None,
            Saved::Failed(why) => {
                self.after_save = None;
                self.failure = Some(format!("Could not save: {why}"));
            }
        }
    }

    /// Takes the autosave an earlier run kept, and asks whether to restore it. One that cannot be
    /// read is removed, saying so.
    pub fn recover(&mut self, bytes: &[u8]) {
        self.autosave_kept = true;
        match recovery::read(bytes, self.plugins) {
            Ok(recovery::Kept { path, audio, song }) => {
                self.restoring = Some(Restoring {
                    path,
                    audio,
                    song: Box::new(song),
                });
            }
            Err(error) => {
                self.failure = Some(format!(
                    "Unsaved changes from last time could not be restored: {error}"
                ));
                self.drop_autosave();
            }
        }
    }

    /// The audio file the open song plays: the one dropped on it while the song still names
    /// it, or else the file the song names beside the song's own. None for a song without a
    /// file or without audio. A host loads it whenever it changes, and says how that went
    /// through [`Window::audio_ready`] or [`Window::audio_failed`].
    #[must_use]
    pub fn audio_file(&self) -> Option<PathBuf> {
        let song = self.song.as_ref()?;
        let audio = song.document().document().metadata.audio.as_str();
        if audio == NO_AUDIO {
            return None;
        }
        let dropped = self.audio_file.as_ref().filter(|file| {
            file.file_name()
                .is_some_and(|name| name.to_str() == Some(audio))
        });
        dropped.cloned().or_else(|| {
            let folder = self.path.as_ref()?.parent()?;
            Some(folder.join(audio))
        })
    }

    /// Takes that `file`, `seconds` long, is ready to play.
    pub fn audio_ready(&mut self, file: &Path, seconds: f64) {
        self.audio = Some((file.to_path_buf(), Loaded::Ready(seconds)));
    }

    /// Takes that `file` could not be loaded, and why.
    pub fn audio_failed(&mut self, file: &Path, why: &str) {
        self.audio = Some((file.to_path_buf(), Loaded::Failed));
        self.failure = Some(format!("{} cannot play: {why}", file.display()));
    }

    /// Takes where playback has got to, in seconds into the audio, and puts the cursor there.
    pub fn playhead(&mut self, seconds: f64) {
        if !self.playing {
            return;
        }
        // What plays now is heard a moment later: the cursor shows what is heard.
        let seconds = seconds - self.settings.audio_latency_ms * self.rate.factor() / 1000.0;
        let tapped = seconds - self.input_latency();
        if let Some(song) = &mut self.song {
            let tick = song.timing().latest_tick_by(seconds);
            song.jump_to(tick);
            if let Some(why) = grow(song, &self.holding, &self.pressed, tapped) {
                self.failure = Some(why);
            }
        }
    }

    /// The calibration's click track: the open song's first tempo and meter, or 120 BPM in 4/4
    /// without one.
    fn calibration_beat(&self) -> latency::Beat {
        let Some(song) = &self.song else {
            return latency::Beat::default();
        };
        let timing = song.timing();
        let first = timing
            .anchors
            .first()
            .map_or(Tick::new(0), |anchor| anchor.tick);
        let per_measure = timing
            .measures_from(first)
            .next()
            .map_or(4, |measure| measure.meter.numerator.get());
        latency::Beat::of(timing.tempo_at(first), per_measure)
    }

    /// How much earlier in the audio than the cursor a lane key pressed now was meant, in seconds
    /// of audio: the input latency, scaled to the playback rate.
    fn input_latency(&self) -> f64 {
        self.settings.input_latency_ms * self.rate.factor() / 1000.0
    }

    /// Takes that the audio played to its end.
    pub fn playback_ended(&mut self) {
        self.playing = false;
    }

    /// What the status bar says of the audio.
    fn audio_state(&self) -> String {
        match self.finding {
            Finding::Downloading(share) => {
                return format!("Downloading the Beat This! weights, {:.0}%", share * 100.0);
            }
            Finding::Running => return "Finding the timing…".to_owned(),
            Finding::Idle | Finding::Asking => {}
        }
        match (self.audio_file(), self.loaded()) {
            (None, _) => "No audio",
            (Some(_), Some(Loaded::Ready(_))) => "Audio ready",
            (Some(_), Some(Loaded::Failed)) => "Audio failed",
            (Some(_), None) => "Audio loading",
        }
        .to_owned()
    }

    /// Asks the host to find the song's timing in its audio.
    fn find_timing(&mut self) {
        let (Some(song), Some(file)) = (&self.song, self.audio_file()) else {
            self.failure = Some("Finding the timing needs the song's audio".to_owned());
            return;
        };
        let ticks_per_beat = song.document().document().ticks_per_beat;
        self.requests.push(Request::FindTiming {
            file,
            ticks_per_beat,
        });
        self.finding = Finding::Running;
    }

    /// Takes that the beat tracker's weights are not downloaded yet, so the user is asked.
    pub fn weights_needed(&mut self) {
        self.finding = Finding::Asking;
    }

    /// Takes that `share` of the beat tracker's weights has downloaded.
    pub fn weights_progress(&mut self, share: f32) {
        self.finding = Finding::Downloading(share.clamp(0.0, 1.0));
    }

    /// Takes the timing found in the song's audio, which goes in place of the open chart's as
    /// one step, or why none was found.
    pub fn timing_found(&mut self, found: Result<charter_core::Timing, String>) {
        self.finding = Finding::Idle;
        let Some(song) = &mut self.song else {
            return;
        };
        self.failure = Some(match found.map(|timing| song.replace_timing(timing)) {
            Ok(Ok(())) => found_message(song),
            Ok(Err(refusal)) => format!("The timing found was refused: {refusal}"),
            Err(why) => format!("No timing found: {why}"),
        });
    }

    /// How the audio the song wants stands, if the host has said.
    fn loaded(&self) -> Option<&Loaded> {
        let wanted = self.audio_file()?;
        self.audio
            .as_ref()
            .filter(|(file, _)| *file == wanted)
            .map(|(_, loaded)| loaded)
    }

    /// Plays the audio from the cursor, or pauses it, or says why it cannot play.
    /// Brings the playback under way to the volumes the settings now hold. With no clicks mixed
    /// in, the host turns the audio up or down as it plays. Clicks are mixed in before playing,
    /// the music's volume with them, so with clicks any change plays again from where it is, once
    /// the pointer lets go of whatever set it.
    fn follow_volumes(&mut self, ui: &Ui) {
        let (Some((played, mixed)), true) = (self.played, self.playing) else {
            return;
        };
        let wanted = self.settings.volumes;
        if wanted == played {
            return;
        }
        if !mixed {
            if wanted.music != played.music {
                self.requests.push(Request::Volume(wanted.music));
            }
            self.played = Some((wanted, false));
        } else if !ui.input(|input| input.pointer.any_down()) {
            self.toggle_play();
            self.toggle_play();
        }
    }

    /// Plays from `from` seconds with the metronome on, whatever the settings say, so an edit to
    /// the timing is heard at once. Does nothing until the audio is ready.
    fn audition(&mut self, from: f64) {
        self.looping = None;
        self.start(from, None, true);
    }

    /// Loops the ticks from `from` to `to` with the metronome, or stops when they loop already.
    fn toggle_loop(&mut self, from: Tick, to: Tick) {
        if self.looping == Some((from, to)) && self.playing {
            self.looping = None;
            self.playing = false;
            self.requests.push(Request::Pause);
            return;
        }
        self.looping = Some((from, to));
        self.play_loop();
    }

    /// Plays the loop from its start, its ends where the timing puts them now, so an edit to it is
    /// heard on the next pass.
    fn play_loop(&mut self) {
        let (Some(song), Some((from, to))) = (&self.song, self.looping) else {
            return;
        };
        let timing = song.timing();
        let span = (timing.time_at(from).seconds(), timing.time_at(to).seconds());
        self.start(span.0, Some(span), true);
    }

    /// Plays again from where the cursor is, or the loop from its start, so a change to what is
    /// mixed in is heard.
    fn replay(&mut self) {
        if !self.playing {
            return;
        }
        if self.looping.is_some() {
            self.play_loop();
        } else if let Some(song) = &self.song {
            let from = song.timing().time_at(song.cursor()).seconds();
            self.start(from, None, false);
        }
    }

    /// Plays from `from` seconds, looping `looping` when given, with the metronome on when
    /// `metronome` whatever the settings say. Does nothing until the audio is ready.
    fn start(&mut self, from: f64, looping: Option<(f64, f64)>, metronome: bool) {
        let (Some(song), Some(&Loaded::Ready(seconds))) = (&self.song, self.loaded()) else {
            return;
        };
        if self.playing {
            self.requests.push(Request::Pause);
        }
        let from = from.max(0.0);
        let settings = Settings {
            metronome: (self.settings.metronome || metronome) && !self.muted,
            ..self.settings.clone()
        };
        let until = looping.map_or(seconds, |(_, end)| end.min(seconds));
        // From the song's start, so a seek back while playing still ticks.
        let clicks = clicks(song, &settings, (0.0, until), self.sync.grid);
        let volumes = self.settings.volumes;
        self.playing = true;
        self.played = Some((volumes, !clicks.is_empty()));
        self.requests.push(Request::Play {
            from,
            rate: self.rate.factor(),
            clicks,
            volumes,
            looping,
            keep_pitch: self.settings.keep_pitch,
        });
    }

    /// Switches between the usual speed and the alternate one.
    fn toggle_rate(&mut self) {
        if let Some(usual) = self.usual_rate.take() {
            self.set_rate(usual);
        } else {
            self.usual_rate = Some(self.rate);
            self.set_rate(Rate::of(self.settings.rate_b));
        }
    }

    fn toggle_play(&mut self) {
        if std::mem::take(&mut self.playing) {
            self.requests.push(Request::Pause);
            return;
        }
        let Some(song) = &self.song else {
            return;
        };
        let from = song.timing().time_at(song.cursor()).seconds();
        match self.loaded() {
            Some(&Loaded::Ready(_)) => {
                self.looping = None;
                self.start(from, None, false);
            }
            Some(Loaded::Failed) => {
                self.failure = Some(
                    "The audio could not be played. Choose another audio file with File > \
                     Choose audio… and try again."
                        .to_owned(),
                );
            }
            None if self.audio_file().is_none() => {
                let name = song.document().document().metadata.audio.as_str();
                self.failure = Some(match (name == NO_AUDIO, self.no_drops) {
                    (true, false) => "This song has no audio yet: drop an audio file on the \
                        window, or choose one."
                        .to_owned(),
                    (true, true) => "This song has no audio yet: choose one.".to_owned(),
                    (false, false) => format!(
                        "Save the song beside {name}, or drop {name} on the window, to play it."
                    ),
                    (false, true) => format!(
                        "Save the song beside {name}, or choose {name} with File > Choose \
                         audio, to play it."
                    ),
                });
            }
            None => self.failure = Some("The audio is still loading.".to_owned()),
        }
    }

    fn set_rate(&mut self, rate: Rate) {
        self.rate = rate;
        // Audio that keeps its pitch is stretched for its speed, so a new speed plays it again.
        if self.playing && self.settings.keep_pitch {
            self.replay();
        } else if self.playing {
            self.requests.push(Request::Rate(rate.factor()));
        }
    }

    /// Makes the file at `file`, which the host let the user choose, the audio of the song
    /// being started, or else of the open song.
    pub fn use_audio(&mut self, file: &Path) {
        if self.new_song.is_some() {
            self.draft_audio(file);
        } else {
            self.add_audio(file);
        }
    }

    /// Makes the picture the host read from `file`, which the user chose, the open song's
    /// artwork, written beside the song when it is saved. Says so when it could not be read as
    /// a picture.
    pub fn use_artwork(&mut self, file: &Path, picture: Option<Picture>) {
        let Some(picture) = picture else {
            self.failure = Some(format!(
                "{} cannot be the song's artwork: it is not a PNG or JPEG picture",
                file.display()
            ));
            return;
        };
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        self.take_artwork(name.unwrap_or_default(), picture);
    }

    /// Takes the picture the audio `file` holds, with the extension a file of it is named with.
    /// The song, or the song being started, takes it as `cover` beside it when it has no artwork
    /// yet and plays that file.
    pub fn artwork_embedded(&mut self, file: &Path, extension: &str, picture: Picture) {
        if self
            .new_song
            .as_ref()
            .is_some_and(|draft| draft.audio.as_deref() == Some(file))
        {
            self.embedded = Some((file.to_path_buf(), extension.to_owned(), picture));
            return;
        }
        let bare = self
            .song
            .as_ref()
            .is_some_and(|song| song.document().document().metadata.artwork.is_none());
        if bare && self.audio_file.as_deref() == Some(file) {
            self.take_artwork(format!("cover.{extension}"), picture);
        }
    }

    /// Takes what the host read of `file`, the song's artwork, or that it could not.
    pub fn artwork_read(&mut self, file: &Path, picture: Option<Picture>) {
        if self.artwork_file().as_deref() == Some(file)
            && let Some(artwork) = &mut self.artwork
        {
            artwork.read(picture);
        }
    }

    /// Where and what the song's artwork writes when the song is saved to `path`, if it is not
    /// there already. The host writes it before reporting [`Saved::To`].
    #[must_use]
    pub fn artwork_write_for(&self, path: &Path) -> Option<(PathBuf, Vec<u8>)> {
        self.artwork
            .as_ref()?
            .write_for(path, self.artwork_file().as_deref())
    }

    /// The file beside the saved song its artwork names.
    fn artwork_file(&self) -> Option<PathBuf> {
        let name = self
            .song
            .as_ref()?
            .document()
            .document()
            .metadata
            .artwork
            .as_ref()?;
        Some(self.song_folder()?.join(name.as_str()))
    }

    /// Names `picture` the open song's artwork as `name`.
    fn take_artwork(&mut self, name: String, picture: Picture) {
        let Some(song) = &mut self.song else {
            self.failure = Some("Open or start a song before adding its artwork.".to_owned());
            return;
        };
        let path = match charter_core::ResourcePath::new(&name) {
            Ok(path) => path,
            Err(error) => {
                self.failure = Some(format!("{name} cannot be a song's artwork: {error}"));
                return;
            }
        };
        let old = &song.document().document().metadata;
        let metadata = charter_core::Metadata {
            artwork: Some(path),
            ..old.clone()
        };
        self.failure = refused(song.set_metadata(metadata).err());
        if self.failure.is_none() {
            self.artwork = Some(Artwork::taken(name, picture));
        }
    }

    /// Keeps the artwork in step with what the song names: artwork named anew is read from beside
    /// the song, and none is none.
    fn follow_artwork(&mut self) {
        let named = self.song.as_ref().and_then(|song| {
            let artwork = song.document().document().metadata.artwork.as_ref();
            artwork.map(ToString::to_string)
        });
        let Some(named) = named else {
            self.artwork = None;
            return;
        };
        if self
            .artwork
            .as_ref()
            .is_some_and(|artwork| artwork.name == named)
        {
            return;
        }
        let file = self.artwork_file();
        self.artwork = Some(Artwork::named(&named, file.as_deref()));
        if let Some(file) = file {
            self.requests.push(Request::ReadArtwork { file });
        }
    }

    /// Takes the title and artist the tags of `file` carry. Where the song, or the song being
    /// started, still has no title and artist of its own they are taken at once; otherwise the
    /// user is asked. Tags of a file the window no longer wants are dropped.
    pub fn tags_read(&mut self, file: &Path, title: Option<String>, artist: Option<String>) {
        if title.is_none() && artist.is_none() {
            return;
        }
        let (named, by) = (title.as_deref(), artist.as_deref());
        if let Some(draft) = &mut self.new_song {
            if draft.audio.as_deref() != Some(file) || !draft.differs(named, by) {
                return;
            }
            if draft.blank() {
                draft.fill(named, by);
            } else {
                self.offer = Some(Offer {
                    draft: true,
                    title,
                    artist,
                });
            }
            return;
        }
        let Some(song) = &mut self.song else {
            return;
        };
        if self.audio_file.as_deref() != Some(file) {
            return;
        }
        let metadata = &song.document().document().metadata;
        let untouched =
            metadata.title == new_song::UNTITLED && metadata.artist == new_song::UNKNOWN_ARTIST;
        let differs = named.is_some_and(|named| named != metadata.title)
            || by.is_some_and(|by| by != metadata.artist);
        if !differs {
            return;
        }
        if untouched {
            self.take_tags(false, named, by);
        } else {
            self.offer = Some(Offer {
                draft: false,
                title,
                artist,
            });
        }
    }

    /// Gives the song, or the song being started when `draft` is set, the title and artist
    /// given.
    fn take_tags(&mut self, draft: bool, title: Option<&str>, artist: Option<&str>) {
        if draft {
            if let Some(draft) = &mut self.new_song {
                draft.fill(title, artist);
            }
            return;
        }
        let Some(song) = &mut self.song else {
            return;
        };
        let metadata = &song.document().document().metadata;
        let title = title.unwrap_or(&metadata.title).to_owned();
        let artist = artist.unwrap_or(&metadata.artist).to_owned();
        self.failure = refused(song.set_song_facts(&title, &artist).err());
    }

    /// Gives the song being started the audio at `file`, and asks for its tags.
    fn draft_audio(&mut self, file: &Path) {
        if let Some(draft) = &mut self.new_song {
            draft.audio = Some(file.to_path_buf());
            self.requests.push(Request::ReadTags {
                file: file.to_path_buf(),
            });
        }
    }

    /// Takes the peaks of `file`, to draw its waveform while the song plays it.
    pub fn waveform_ready(&mut self, file: &Path, peaks: Arc<Peaks>) {
        self.waveform = Some((file.to_path_buf(), peaks));
    }

    /// Takes the frequencies of `file`, to draw them in place of its waveform when the settings
    /// ask.
    pub fn spectrum_ready(&mut self, file: &Path, spectrum: Arc<Spectrum>) {
        self.spectrum = Some((file.to_path_buf(), spectrum));
    }

    /// Takes the onsets the host found in the audio `file`, which the timing window snaps to
    /// and measures the grid against.
    pub fn onsets_ready(&mut self, file: &Path, onsets: Arc<[charter_audio::Onset]>) {
        self.onsets = Some((file.to_path_buf(), onsets));
    }

    /// The onsets of the audio the open song plays, once the host has found them.
    #[must_use]
    pub fn onsets(&self) -> Option<&[charter_audio::Onset]> {
        let file = self.audio_file();
        let (of, onsets) = self.onsets.as_ref()?;
        (Some(of.as_path()) == file.as_deref()).then_some(onsets.as_ref())
    }

    /// What the notefield's audio strip shows: nothing unless the settings want it and the host
    /// has summarized the audio the song plays.
    #[must_use]
    pub fn strip(&self) -> Option<notefield::Strip<'_>> {
        shown_strip(
            &self.settings,
            self.waveform.as_ref(),
            self.spectrum.as_ref(),
            self.audio_file().as_deref(),
        )
    }

    /// Whether the window takes the audio to be playing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// What the window says went wrong last, while it says it.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Takes why the song's audio could not be copied beside it.
    pub fn audio_not_copied(&mut self, why: &str) {
        self.failure = Some(format!("Audio not copied: {why}"));
    }

    /// The source and destination of audio a save to `path` needs copied. The host can copy it
    /// before reporting [`Saved::To`], so a failed copy leaves the song unsaved.
    #[must_use]
    pub fn audio_copy_for(&self, path: &Path) -> Option<(PathBuf, PathBuf)> {
        let song = self.song.as_ref()?;
        let from = self.audio_file()?;
        let to = path
            .parent()?
            .join(song.document().document().metadata.audio.as_str());
        (from != to).then_some((from, to))
    }

    /// Forgets how audio the song no longer plays stood, stopping it if it was playing.
    fn forget_stale_audio(&mut self) {
        let wanted = self.audio_file();
        if self
            .audio
            .as_ref()
            .is_some_and(|(file, _)| Some(file) != wanted.as_ref())
        {
            self.audio = None;
            if std::mem::take(&mut self.playing) {
                self.requests.push(Request::Pause);
            }
        }
    }

    /// Makes the file at `path` the open song's audio.
    fn add_audio(&mut self, path: &Path) {
        let Some(song) = &mut self.song else {
            self.failure = Some("Open or start a song before adding its audio.".to_owned());
            return;
        };
        let name = path.file_name().map(|name| name.to_string_lossy());
        let audio = match charter_core::ResourcePath::new(name.as_deref().unwrap_or_default()) {
            Ok(audio) => audio,
            Err(error) => {
                self.failure = Some(format!(
                    "{} cannot be a song's audio: {error}",
                    path.display()
                ));
                return;
            }
        };
        self.failure = refused(song.set_audio(audio).err());
        if self.failure.is_none() {
            self.audio_file = Some(path.to_path_buf());
            self.requests.push(Request::ReadTags {
                file: path.to_path_buf(),
            });
        }
    }

    /// Starts the song the new-song dialog asks for.
    fn create(&mut self, plugin: &dyn charter_core::RulesetPlugin, song: &charter_core::NewSong) {
        match plugin.new_song(song) {
            Ok(document) => {
                let audio = self.new_song.as_ref().and_then(|draft| draft.audio.clone());
                let song = Box::new(Song::new(document, self.plugins));
                self.after_asking(Discard::Open {
                    path: None,
                    song,
                    restored: false,
                    audio,
                });
            }
            Err(error) => {
                let why = format!("Could not start the song: {error}");
                if let Some(draft) = &mut self.new_song {
                    draft.error = Some(why.clone());
                }
                self.failure = Some(why);
            }
        }
    }

    /// Takes why the last autosave could not be kept. It is tried again later.
    pub fn autosave_failed(&mut self, why: &str) {
        self.autosaved = None;
        self.failure = Some(format!("Autosave failed: {why}"));
    }

    /// Asks the host to remove the autosave, if it may be keeping one.
    fn drop_autosave(&mut self) {
        self.autosaved = None;
        self.autosave_due = None;
        if std::mem::take(&mut self.autosave_kept) {
            self.requests.push(Request::DropAutosave);
        }
    }

    /// Asks the host to keep the open song's unsaved changes once they have waited long enough,
    /// and to remove them once there are none.
    fn autosave(&mut self, ctx: &egui::Context) {
        if self.leaving != Leaving::Staying || self.restoring.is_some() {
            return;
        }
        let Some(song) = &self.song else {
            return;
        };
        if !song.is_modified() {
            self.drop_autosave();
            return;
        }
        let revision = song.revision();
        if self.autosaved == Some(revision) {
            return;
        }
        let now = ctx.input(|input| input.time);
        let Some(after) = self.settings.autosave_after() else {
            self.autosave_due = None;
            return;
        };
        let due = *self.autosave_due.get_or_insert(now + after);
        if now < due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            return;
        }
        self.autosave_due = None;
        let audio = self.audio_file.as_deref();
        match recovery::write(self.path.as_deref(), audio, song.document().document()) {
            Ok(bytes) => {
                self.requests.push(Request::Autosave { bytes });
                self.autosave_kept = true;
                self.autosaved = Some(revision);
            }
            Err(error) => self.failure = Some(format!("Autosave failed: {error}")),
        }
    }

    /// Takes the answer to whether to restore the changes from an earlier run.
    fn answer_restore(&mut self, restore: bool) {
        let Some(Restoring {
            path,
            audio,
            mut song,
        }) = self.restoring.take()
        else {
            return;
        };
        if restore {
            song.mark_unsaved();
            self.after_asking(Discard::Open {
                path,
                song,
                restored: true,
                audio,
            });
        } else {
            self.drop_autosave();
        }
    }

    /// The name the host's window goes by: the open song, starred while it has unsaved changes.
    #[must_use]
    pub fn title(&self) -> String {
        let Some(song) = &self.song else {
            return "charter".to_owned();
        };
        let star = if song.is_modified() { "*" } else { "" };
        format!("{star}{} - charter", self.file_name(song))
    }

    /// The open song's file name, or one made from its title when it has no file.
    fn file_name(&self, song: &Song) -> String {
        file_name(self.path.as_deref(), song)
    }

    /// Asks the host to write the song to its file, or to ask where when `saving` says so or the
    /// song has no file.
    fn save(&mut self, saving: Saving) {
        let Some(song) = &self.song else {
            return;
        };
        let bytes = match song.document().to_json() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.failure = Some(format!("Could not save: {error}"));
                return;
            }
        };
        let request = match &self.path {
            Some(path) if saving == Saving::InPlace => Request::Write {
                path: path.clone(),
                bytes,
            },
            _ => Request::SaveAs {
                name: self.file_name(song),
                bytes,
            },
        };
        self.requests.push(request);
    }

    /// Opens the song in `bytes`, read from the file at `path`. A file that fails leaves the
    /// open song as it was and says why.
    ///
    /// Over a song with unsaved changes, it first asks whether to save them.
    pub fn open(&mut self, path: &Path, bytes: &[u8]) {
        match Song::open(bytes, self.plugins) {
            Ok(song) => self.after_asking(Discard::Open {
                path: Some(path.to_path_buf()),
                song: Box::new(song),
                restored: false,
                audio: None,
            }),
            Err(error) => self.refuse(path, &error),
        }
    }

    /// Says that the file at `path` did not open, and why.
    pub fn refuse(&mut self, path: &Path, why: &dyn std::fmt::Display) {
        self.failure = Some(format!("{} did not open: {why}", path.display()));
    }

    /// Opens the demo song, first asking whether to save unsaved changes to the song open.
    pub fn open_demo(&mut self) {
        let song = Box::new(Song::new(demo::song(), self.plugins));
        self.after_asking(Discard::Open {
            path: None,
            song,
            restored: false,
            audio: None,
        });
    }

    /// Does what would lose the open song's unsaved changes, once the user says what to do with
    /// them; at once when there are none.
    fn after_asking(&mut self, discard: Discard) {
        if self.song.as_ref().is_some_and(Song::is_modified) {
            self.discard = Some(discard);
        } else {
            self.carry_on(discard);
        }
    }

    fn carry_on(&mut self, discard: Discard) {
        match discard {
            Discard::Close => {
                self.drop_autosave();
                self.leaving = Leaving::Now;
            }
            Discard::Open {
                path,
                mut song,
                restored,
                audio,
            } => {
                if restored {
                    self.autosave_due = None;
                    self.autosaved = Some(song.revision());
                } else {
                    self.drop_autosave();
                }
                self.saving = None;
                song.set_snap(self.settings.snap);
                self.new_song = None;
                self.song = Some(*song);
                self.import.invalidate();
                self.path = path;
                self.zoom = Zoom::START.by(self.settings.zoom / 100.0);
                self.failure = None;
                self.placed = false;
                self.holding.clear();
                self.pressed.clear();
                self.audio_file = audio;
                self.artwork = None;
                if std::mem::take(&mut self.playing) {
                    self.requests.push(Request::Pause);
                }
                let embedded = self.embedded.take();
                if let Some((file, extension, picture)) = embedded
                    && self.audio_file.as_ref() == Some(&file)
                {
                    self.artwork_embedded(&file, &extension, picture);
                }
            }
        }
    }

    /// Takes the answer to whether to save before the changes are lost.
    fn answer(&mut self, answer: panels::Answer) {
        let Some(discard) = self.discard.take() else {
            return;
        };
        match answer {
            panels::Answer::Save => {
                self.after_save = Some(discard);
                self.saving = Some(Saving::InPlace);
            }
            panels::Answer::DontSave => self.carry_on(discard),
            panels::Answer::Cancel => {}
        }
    }

    /// Does what the window does whether or not a song is open.
    fn apply_to_window(&mut self, action: &Action) {
        match *action {
            Action::OpenDemo => self.open_demo(),
            Action::Open => self.requests.push(Request::Open),
            Action::Save => self.saving = Some(Saving::InPlace),
            Action::SaveAs => self.saving = Some(Saving::Elsewhere),
            Action::Settings => self.settings_open = !self.settings_open,
            Action::Calibrate => self.calibration.open = true,
            Action::ToggleSync => self.sync.open = !self.sync.open,
            Action::ShowSync => self.sync.open = true,
            Action::ShowImport => self.import.open = true,
            Action::ChooseTimingFile => self.requests.push(Request::ChooseTimingFile),
            Action::Help => self.help_open = !self.help_open,
            Action::ToggleSongPanel => self.settings.song_panel = !self.settings.song_panel,
            Action::ToggleMetronome => self.settings.metronome = !self.settings.metronome,
            Action::ToggleTicks => self.settings.tick_sound = self.settings.tick_sound.toggled(),
            Action::ToggleTunedLanes => {
                self.settings.tuned_lanes = !self.settings.tuned_lanes;
                // Heard at once while the ticks play.
                if self.settings.tick_sound != settings::Sound::Off {
                    self.replay();
                }
            }
            Action::SetGrid(grid) => self.settings.grid = grid,
            Action::TogglePlay => self.toggle_play(),
            Action::Audition(from) => self.audition(from),
            Action::AskResetTiming => self.resetting = true,
            Action::FindTiming => self.find_timing(),
            Action::Loop { from, to } => self.toggle_loop(from, to),
            Action::ToggleRate => self.toggle_rate(),
            Action::MuteMetronome(muted) => {
                self.muted = muted;
                self.replay();
            }
            Action::ChooseAudio => self.requests.push(Request::ChooseAudio),
            Action::ChooseArtwork => self.requests.push(Request::ChooseArtwork),
            Action::NudgeRate(step) => self.set_rate(self.rate.by(step)),
            Action::SetRate(factor) => self.set_rate(Rate::of(factor)),
            Action::SetVolumes(volumes) => self.settings.volumes = volumes,
            Action::NewSong => {
                self.new_song.get_or_insert_default();
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)] // one arm for every action
    fn apply(&mut self, action: Action) {
        self.apply_to_window(&action);
        let input_latency = self.input_latency();
        let Some(song) = &mut self.song else {
            return;
        };
        song.keep_notes(if self.settings.notes_keep_time {
            KeepNotes::InTime
        } else {
            KeepNotes::OnBeats
        });
        match action {
            Action::OpenChart(index) => {
                song.open_chart(index);
                self.holding.clear();
                self.pressed.clear();
            }
            Action::Move(step) => {
                song.step(step);
                self.failure = stretch(song, &self.holding).or(self.failure.take());
            }
            Action::MoveMeasure(step) => {
                song.step_measure(step);
                self.failure = stretch(song, &self.holding).or(self.failure.take());
            }
            Action::FinerSnap => song.finer_snap(),
            Action::CoarserSnap => song.coarser_snap(),
            Action::ZoomBy(factor) => self.zoom = self.zoom.by(factor),
            Action::Toggle(lane, note) => {
                let at = placing_at(song, self.playing.then_some(input_latency));
                let toggled = song.toggle(at, lane, note.kind());
                if let Ok(Toggled::Placed) = toggled {
                    self.placed = true;
                    if note == Note::Tap {
                        self.holding.push((lane, at));
                        if self.playing {
                            let now = song.timing().time_at(song.cursor()).seconds();
                            self.pressed.push((lane, now - input_latency));
                        }
                    }
                }
                self.failure = refused(toggled.err());
            }
            Action::LaneUp(lane) => {
                if self.playing {
                    // Where the key came up is where the hold ends.
                    let now = song.timing().time_at(song.cursor()).seconds() - input_latency;
                    let held: Vec<_> = self
                        .holding
                        .iter()
                        .copied()
                        .filter(|(held, _)| *held == lane)
                        .collect();
                    self.failure = grow(song, &held, &self.pressed, now).or(self.failure.take());
                }
                self.holding.retain(|(held, _)| *held != lane);
                self.pressed.retain(|(held, _)| *held != lane);
            }
            // Playback moves the cursor on already; a jump would seek the audio ahead.
            Action::LanesReleased => {
                if std::mem::take(&mut self.placed) && self.settings.auto_jump && !self.playing {
                    song.step(Step::Later);
                }
            }
            Action::Undo => {
                self.placed = false;
                self.holding.clear();
                self.pressed.clear();
                self.failure = refused(song.undo().err());
            }
            Action::Redo => {
                self.placed = false;
                self.holding.clear();
                self.pressed.clear();
                self.failure = refused(song.redo().err());
            }
            Action::Place { tick, lane, note } => {
                self.failure = refused(song.toggle(tick, lane, note.kind()).err());
            }
            Action::Stretch { lane, from, to } => {
                self.failure = refused(song.stretch(lane, from, to).err());
            }
            Action::Select(Some(note), how) => song.select([note], how),
            Action::Select(None, Selecting::Replace) | Action::ClearSelection => {
                song.clear_selection();
            }
            Action::OpenDemo
            | Action::Open
            | Action::Save
            | Action::SaveAs
            | Action::Settings
            | Action::Calibrate
            | Action::ToggleSync
            | Action::ShowSync
            | Action::ShowImport
            | Action::ChooseTimingFile
            | Action::Help
            | Action::ToggleSongPanel
            | Action::ToggleMetronome
            | Action::ToggleTicks
            | Action::ToggleTunedLanes
            | Action::FindTiming
            | Action::SetGrid(_)
            | Action::TogglePlay
            | Action::Audition(_)
            | Action::AskResetTiming
            | Action::Loop { .. }
            | Action::ToggleRate
            | Action::MuteMetronome(_)
            | Action::ChooseAudio
            | Action::ChooseArtwork
            | Action::NudgeRate(_)
            | Action::SetRate(_)
            | Action::SetVolumes(_)
            | Action::NewSong
            | Action::Select(None, Selecting::Add | Selecting::Subtract) => {}
            Action::SelectBetween { ticks, lanes, how } => {
                song.select_between(ticks.0..=ticks.1, lanes.0..=lanes.1, how);
            }
            Action::SetBpmAt(..)
            | Action::MovePointTo(..)
            | Action::SetMetadata(_)
            | Action::SetSetting { .. }
            | Action::SetMeterAt(..)
            | Action::RemoveTimingAt(_)
            | Action::SetOffset(_)
            | Action::AddBpm
            | Action::NudgeOffset(_)
            | Action::ImportTiming(_)
            | Action::ResetTiming
            | Action::MoveTiming { .. } => self.failure = retime(song, &action),
            // The timing window says itself why its edits were refused.
            Action::Timing(edit) => match edit.apply(song) {
                Err(why) => {
                    let now = self.frame_time;
                    self.sync.refused(&edit, &why, song.timing(), now);
                }
                // A loop plays again from its start, so the edit is heard on this pass.
                Ok(()) if self.looping.is_some() => self.play_loop(),
                Ok(()) => {}
            },
            Action::JumpTo(tick) => song.jump_to(tick),
            Action::JumpEnd => song.jump_to(song.end()),
            Action::MoveMarker { index, to } => {
                self.failure = refused(song.move_marker(index, to).err());
            }
            Action::JumpSection(step) => {
                if let Some(tick) = song.section_from(step) {
                    song.jump_to(tick);
                }
            }
            Action::ToggleBookmark => self.failure = refused(song.toggle_bookmark().err()),
            Action::NudgeBpm(by) => self.failure = nudge_bpm(song, by),
            Action::SetSection(text) => self.failure = refused(song.set_section(&text).err()),
            Action::RemoveMarker(index) => self.failure = refused(song.remove_marker(index).err()),
            Action::SelectAll => song.select_all(),
            Action::DeleteSelection => self.failure = refused(song.delete_selection().err()),
            Action::ResizeNotes(step) => self.failure = refused(song.resize_notes(step).err()),
            Action::MoveSelection { ticks, lanes } => {
                self.failure = refused(song.move_selection(ticks, lanes).err());
            }
        }
    }

    /// Opens every song dropped on the window this frame, in order, so the last one that opens
    /// wins, and gives the open song the audio file dropped on it.
    fn open_dropped(&mut self, ui: &Ui) {
        let dropped = ui.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path();
            let extension = path.extension().and_then(|extension| extension.to_str());
            if extension.is_some_and(|extension| AUDIO.contains(&extension.to_lowercase().as_str()))
            {
                self.use_audio(path);
                continue;
            }
            match file.bytes() {
                Ok(bytes) => self.open(path, &bytes),
                Err(error) => self.refuse(path, &error),
            }
        }
    }

    /// Shows whichever question or dialog is waiting on the user, and takes the answer.
    fn dialogs(&mut self, ui: &Ui) {
        if let Some(draft) = &mut self.new_song {
            match new_song::show(ui, self.plugins, draft, self.no_drops) {
                Some(new_song::Answer::Create { plugin, song }) => {
                    self.create(plugin, &song);
                }
                Some(new_song::Answer::ChooseAudio) => self.requests.push(Request::ChooseAudio),
                Some(new_song::Answer::Cancel) => self.new_song = None,
                None => {}
            }
        }
        if let Some(Offer {
            draft,
            title,
            artist,
        }) = &self.offer
            && let Some(take) = panels::offer_tags(ui, title.as_deref(), artist.as_deref())
        {
            let (draft, title, artist) = (*draft, title.clone(), artist.clone());
            self.offer = None;
            if take {
                self.take_tags(draft, title.as_deref(), artist.as_deref());
            }
        }
        if let Some(Restoring { path, song, .. }) = &self.restoring
            && let Some(restore) = panels::ask_to_restore(ui, &file_name(path.as_deref(), song))
        {
            self.answer_restore(restore);
        }
        if self.discard.is_some() {
            let name = self.song.as_ref().map(|song| self.file_name(song));
            if let Some(answer) = panels::ask_to_save(ui, name.as_deref().unwrap_or_default()) {
                self.answer(answer);
            }
        }
    }

    /// What this frame's keys ask for. Lane keys edit only a song open for editing. Floating
    /// windows such as the settings leave the keys to the editor; only a text field being typed
    /// in, a question, or the timing window once clicked takes them.
    fn keyboard(&mut self, ui: &Ui) -> Vec<Action> {
        let lanes = if self.calibration.open {
            0
        } else {
            self.song.as_ref().map_or(0, editable_lanes)
        };
        let lane_keys = &mut self.lane_keys;
        let asked = ui.input(|input| keys::key_actions(input, lanes, lane_keys));
        // A focused widget or a question takes the keyboard, but a lane key coming up still ends
        // its chord.
        let asking = self.new_song.is_some()
            || self.discard.is_some()
            || self.restoring.is_some()
            || self.offer.is_some()
            || self.calibration.open
            || self.resetting
            || self.finding == Finding::Asking;
        // A button that Tab focused wants no letters, so only a text field counts.
        let typing = ui
            .ctx()
            .memory(egui::Memory::focused)
            .is_some_and(|id| egui::text_edit::TextEditState::load(ui.ctx(), id).is_some());
        // The timing window, once clicked, takes the keys it has a use for; undo and redo still
        // reach the song.
        let timing = self.sync.has_keyboard();
        let history = |action: &Action| matches!(action, Action::Undo | Action::Redo);
        let ends_a_chord =
            |action: &Action| matches!(action, Action::LaneUp(_) | Action::LanesReleased);
        // Commands a text field has no use for of its own still work while one has the keyboard.
        let global = |action: &Action| {
            matches!(
                action,
                Action::Save
                    | Action::SaveAs
                    | Action::Open
                    | Action::NewSong
                    | Action::Settings
                    | Action::ToggleSync
                    | Action::Help
            )
        };
        asked
            .into_iter()
            .filter(|action| {
                let open = if typing {
                    global(action)
                } else if timing {
                    global(action) || history(action)
                } else {
                    true
                };
                ends_a_chord(action) || !asking && open
            })
            .collect()
    }

    /// Draws the window into `ui` and answers this frame's input.
    #[allow(clippy::too_many_lines)] // one frame, panel by panel in the order they draw
    pub fn ui(&mut self, ui: &mut Ui) {
        style::apply(ui.ctx(), &self.fallback_fonts);
        self.frame_time = ui.input(|input| input.time);
        let scale = self.settings.scale();
        if (ui.ctx().zoom_factor() - scale).abs() > f32::EPSILON {
            ui.ctx().set_zoom_factor(scale);
        }
        if self.leaving == Leaving::Now {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            self.leaving = Leaving::Going;
        }
        if ui.input(|input| input.viewport().close_requested()) && self.leaving == Leaving::Staying
        {
            if self.song.as_ref().is_some_and(Song::is_modified) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.discard = Some(Discard::Close);
            } else {
                self.leaving = Leaving::Going;
            }
        }
        self.dialogs(ui);
        self.open_dropped(ui);
        self.forget_stale_audio();
        let mut actions = self.keyboard(ui);
        let song = self.song.as_ref();
        let audio_state = self.audio_state();
        let mut dismissed = false;
        let playing = self.playing;
        egui::Panel::top("menu")
            .frame(style::chrome())
            .show(ui, |ui| {
                let importable = import::offered(self.plugins);
                let findable = self.audio_file().is_some() && self.finding == Finding::Idle;
                panels::menu_bar(ui, song, (playing, importable, findable), &mut actions);
            });
        let sounds = (
            self.settings.metronome,
            (
                self.settings.tick_sound != settings::Sound::Off,
                self.settings.tuned_lanes,
            ),
            self.settings.volumes,
        );
        let view = (
            self.zoom,
            self.rate,
            self.playing,
            self.settings.song_panel,
            self.settings.grid,
        );
        egui::Panel::top("transport")
            .frame(style::toolbar())
            .show(ui, |ui| {
                panels::transport_bar(ui, self.song.as_ref(), view, sounds, &mut actions);
            });
        egui::Panel::bottom("status")
            .frame(style::chrome())
            .show(ui, |ui| {
                panels::status_bar(ui, self.song.as_ref(), (&audio_state, self.playing));
            });
        self.follow_artwork();
        let artwork = self.artwork.as_mut().map(|artwork| artwork.shown(ui.ctx()));
        if let Some(song) = self.song.as_ref().filter(|_| self.settings.song_panel) {
            let timing = &mut self.timing_drafts;
            let facts = &mut self.song_drafts;
            let section = &mut self.section_draft;
            let refused = egui::Panel::left("song")
                .frame(style::panel())
                .default_size(300.0)
                .show(ui, |ui| {
                    style::solid_scroll_bars(ui);
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| {
                            let named =
                                panels::song_panel(ui, song, (facts, artwork), &mut actions);
                            let refused = timing_panel::section(ui, song, timing, &mut actions);
                            let refused = named.or(refused);
                            panels::markers(ui, song, section, &mut actions);
                            panels::details(ui, song);
                            refused
                        })
                        .inner
                })
                .inner;
            if refused.is_some() {
                self.failure = refused;
            }
        }
        egui::CentralPanel::default()
            .frame(style::canvas())
            .show(ui, |ui| {
                if let Some(failure) = &self.failure {
                    // Over the canvas rather than in its layout, so the notefield never moves
                    // under a drag when a message comes or goes.
                    dismissed = panels::toast(ui, failure);
                }
                if let Some(why) = self.song.as_ref().and_then(Song::read_only) {
                    panels::read_only(ui, why);
                }
                if self.song.as_ref().is_some_and(|song| {
                    song.document().document().metadata.audio.as_str() == NO_AUDIO
                }) {
                    panels::no_audio(ui, self.no_drops, &mut actions);
                }
                match &self.song {
                    Some(song) => {
                        let shown = notefield::Shown {
                            zoom: self.zoom,
                            direction: self.settings.direction,
                            strip: shown_strip(
                                &self.settings,
                                self.waveform.as_ref(),
                                self.spectrum.as_ref(),
                                self.audio_file().as_deref(),
                            ),
                            grid: self.settings.grid,
                        };
                        let map = self.settings.minimap && song.chart().is_some();
                        let length = match self.loaded() {
                            Some(Loaded::Ready(seconds)) => Some(*seconds),
                            _ => None,
                        };
                        let reversed = self.settings.direction == Direction::Down;
                        let (scrolled, gesture) = (&mut self.scrolled, &mut self.gesture);
                        style::stage().show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            let whole = ui.available_rect_before_wrap();
                            let (field, beside) = if map {
                                let gap = minimap::WIDTH + 8.0;
                                let (field, beside) =
                                    whole.split_left_right_at_x(whole.right() - gap);
                                (field, Some(beside.with_min_x(beside.left() + 8.0)))
                            } else {
                                (whole, None)
                            };
                            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(field));
                            let visible = notefield::show(
                                &mut inner,
                                song,
                                shown,
                                scrolled,
                                gesture,
                                &mut actions,
                            );
                            if let (Some(beside), Some(visible)) = (beside, visible) {
                                minimap::show(
                                    ui,
                                    beside,
                                    song,
                                    (visible, length, reversed),
                                    &mut actions,
                                );
                            }
                            ui.allocate_rect(whole, egui::Sense::hover());
                        });
                    }
                    None => panels::empty(ui, self.no_drops, &mut actions),
                }
            });
        let asked = settings::show(ui.ctx(), &mut self.settings, &mut self.settings_open);
        if asked == Some(settings::Asked::Calibrate) {
            self.calibration.open = true;
        }
        let kit = self.settings.metronome_sound;
        let time = (self.frame_time, kit, self.calibration_beat());
        if let Some(request) =
            latency::show(ui.ctx(), &mut self.calibration, time, &mut self.settings)
        {
            self.requests.push(request);
        }
        if self.finding == Finding::Asking
            && let Some(download) = panels::ask_to_download_weights(ui)
        {
            if download {
                self.requests.push(Request::DownloadWeights);
                self.finding = Finding::Downloading(0.0);
            } else {
                self.finding = Finding::Idle;
            }
        }
        if self.resetting
            && let Some(answer) = panels::ask_to_reset_timing(ui)
        {
            self.resetting = false;
            if answer {
                actions.push(Action::ResetTiming);
            }
        }
        if let Some(settings::Asked::Preview(kit)) = asked {
            let clicks = (0..4u8).map(|beat| {
                let tone = if beat == 0 {
                    charter_audio::Tone::Accent(kit)
                } else {
                    charter_audio::Tone::Beat(kit)
                };
                (f64::from(beat) * 0.5, tone)
            });
            self.requests.push(Request::Preview {
                clicks: clicks.collect(),
                volumes: self.settings.volumes,
            });
        }
        import::show(
            ui.ctx(),
            self.song.as_ref(),
            self.plugins,
            &mut self.import,
            &mut actions,
        );
        if self.sync.open {
            // The timing window shows the audio whether or not the notefield's waveform is on.
            let file = self.audio_file();
            let peaks = self
                .waveform
                .as_ref()
                .filter(|(of, _)| Some(of.as_path()) == file.as_deref())
                .map(|(_, peaks)| peaks.as_ref());
            let onsets = self
                .onsets
                .as_ref()
                .filter(|(of, _)| Some(of.as_path()) == file.as_deref())
                .map_or(&[][..], |(_, onsets)| onsets.as_ref());
            let inputs = charter_timing::Inputs {
                song: self.song.as_ref(),
                peaks,
                onsets,
                playing: self.playing,
                latency_ms: self.settings.audio_latency_ms,
            };
            let mut commands = Vec::new();
            charter_timing::show(ui.ctx(), inputs, &mut self.sync, &mut commands);
            for command in commands {
                match command {
                    charter_timing::Command::Edit(edit) => actions.push(Action::Timing(edit)),
                    charter_timing::Command::PlayFrom(tick) => {
                        actions.push(Action::JumpTo(tick));
                        if !self.playing {
                            actions.push(Action::TogglePlay);
                        }
                    }
                    charter_timing::Command::Audition(from) => actions.push(Action::Audition(from)),
                    charter_timing::Command::TogglePlay => actions.push(Action::TogglePlay),
                    charter_timing::Command::Loop { from, to } => {
                        actions.push(Action::Loop { from, to });
                    }
                    charter_timing::Command::ToggleRate => actions.push(Action::ToggleRate),
                    charter_timing::Command::ToggleMetronome => {
                        actions.push(Action::ToggleMetronome);
                    }
                    charter_timing::Command::MuteMetronome(muted) => {
                        actions.push(Action::MuteMetronome(muted));
                    }
                }
            }
        }
        if dismissed {
            self.failure = None;
        }
        panels::shortcuts(ui.ctx(), &mut self.help_open, &mut self.help_filter);
        let before = self.song.as_ref().map(Song::cursor);
        for action in actions {
            self.apply(action);
        }
        if self.playing
            && let Some(song) = &self.song
            && Some(song.cursor()) != before
        {
            let to = song.timing().time_at(song.cursor()).seconds();
            self.requests.push(Request::Seek(to));
        }
        self.follow_volumes(ui);
        if let Some(saving) = self.saving.take() {
            self.save(saving);
        }
        self.autosave(ui.ctx());
        let title = self.title();
        if title != self.shown_title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }
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
    refused(refusals.into_iter().last())
}

fn stretch(song: &mut Song, holding: &[(LaneIndex, Tick)]) -> Option<String> {
    let cursor = song.cursor();
    let refusals = holding
        .iter()
        .filter_map(|&(lane, from)| song.stretch(lane, from, cursor).err());
    refused(refusals.last())
}

// ponytail: the clicks are worked out once, when playback starts, so notes placed while playing
// click from the next play; schedule them live if that gets in the way of recording.
/// The tones `settings` ask for over the open chart of `song`, playing from `from` seconds to
/// `until`: a tick on each note, and the metronome on each beat.
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
    #[allow(clippy::cast_possible_truncation)] // within an octave either way
    let cents = cents as i16;
    cents
}

fn clicks(
    song: &Song,
    settings: &Settings,
    (from, until): (f64, f64),
    grid: charter_timing::grid::Grid,
) -> Vec<(f64, charter_audio::Tone)> {
    use charter_audio::Tone;
    use charter_timing::grid::Line;
    let timing = song.timing();
    let mut clicks = Vec::new();
    if let (Some(tick), Some(chart)) = (settings.tick_sound.tick(), song.chart()) {
        let lanes = chart.lanes.map_or(0, u16::from);
        let heads = chart.objects().map(|object| {
            let cents = if settings.tuned_lanes {
                lane_cents(object.position, lanes)
            } else {
                0
            };
            (timing.time_at(object.tick).seconds(), tick.pitched(cents))
        });
        clicks.extend(heads.filter(|(at, _)| (from..=until).contains(at)));
    }
    if settings.metronome {
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
            for (tick, line) in grid.lines(&measure, resolution) {
                let at = timing.time_at(tick).seconds();
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
    waveform: Option<&'a (PathBuf, Arc<Peaks>)>,
    spectrum: Option<&'a (PathBuf, Arc<Spectrum>)>,
    file: Option<&Path>,
) -> Option<notefield::Strip<'a>> {
    let of_file = |of: &PathBuf| settings.waveform && Some(of.as_path()) == file;
    let frequencies = spectrum
        .filter(|(of, _)| settings.frequencies && of_file(of))
        .map(|(_, spectrum)| notefield::Strip::Frequencies(spectrum));
    frequencies.or_else(|| {
        waveform
            .filter(|(of, _)| of_file(of))
            .map(|(_, peaks)| notefield::Strip::Waveform(peaks))
    })
}

/// The timing a reset leaves: 120 BPM in 4/4 from 0 ms, as a new song starts with.
fn fresh_timing() -> charter_core::Timing {
    let fresh = serde_json::json!({
        "anchors": [ { "tick": 0, "seconds": 0.0 } ],
        "tempo": 120.0,
        "events": []
    });
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
    refused(done.err())
}

/// Raises the tempo at the cursor by `by` BPM, and says why if it cannot.
fn nudge_bpm(song: &mut Song, by: f64) -> Option<String> {
    let (_, bpm) = song.bpm()?;
    let nudged = f64::from(bpm) + by;
    match charter_core::Bpm::try_from(nudged) {
        Ok(bpm) => refused(song.set_bpm(bpm).err()),
        Err(_) => Some(format!("{nudged} is not a tempo")),
    }
}

/// What to say about a refused edit, or nothing when the edit went through.
fn refused(refusal: Option<Refusal>) -> Option<String> {
    refusal.map(|refusal| format!("Refused: {refusal}"))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use std::error::Error;

    use charter_core::{
        Chart, LaneCount, LaneIndex, LaneSpan, Position, Tick, Valid, ValidDocument,
    };
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use crate::flip::Flip;
    use crate::recovery;
    use crate::settings::Sound;
    use charter_audio::{Gain, Kit, Tone, Volumes};

    /// The format chapter, embedded at compile time so the tests do no I/O.
    const SPEC: &str = include_str!("../../../docs/src/format.md");

    /// The format chapter's full example song.
    fn example() -> &'static [u8] {
        let start = SPEC.rfind("```json\n").unwrap() + "```json\n".len();
        let end = start + SPEC[start..].find("```").unwrap();
        &SPEC.as_bytes()[start..end]
    }

    fn window(app: Window) -> Harness<'static, Window> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut Window| app.ui(ui), app);
        harness.run();
        harness
    }

    fn with_demo() -> Harness<'static, Window> {
        let mut app = Window::default();
        app.open_demo();
        window(app)
    }

    /// A file dropped on the window, held in memory.
    #[derive(Debug)]
    struct Dropped(&'static str, Vec<u8>);

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
    struct AnySm;

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
    struct NoFourthLane;

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

    const PLUGINS: &[&dyn RulesetPlugin] = &[&AnySm];

    fn editable_demo() -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        window(app)
    }

    /// The lanes of the open chart's objects on `tick`, lowest first.
    fn lanes_at(harness: &Harness<'_, Window>, tick: i32) -> Vec<u16> {
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

    fn cursor_shows(harness: &Harness<'_, Window>, position: &str) -> bool {
        harness
            .query_by_label(&format!("Cursor at {position}"))
            .is_some()
    }

    #[test]
    fn a_song_its_plugin_accepts_opens_for_editing() {
        let harness = editable_demo();
        assert!(harness.query_by_label("Editing").is_some());
        assert!(harness.query_by_label_contains("Read only").is_none());
    }

    #[test]
    fn a_song_without_its_plugin_is_read_only_and_names_the_plugin() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        assert!(
            harness
                .query_by_label("Read only: charter has no arcaea plugin")
                .is_some()
        );
        assert!(harness.query_by_label("Read only").is_some());
    }

    #[test]
    fn read_only_songs_disable_menu_items_that_would_edit_them() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("example.json"), example());
        let mut harness = window(app);

        harness.get_by_label("File").click();
        harness.run();
        assert!(
            harness
                .get_by_label("Choose audio…")
                .accesskit_node()
                .is_disabled()
        );
        harness.get_by_label("Timing").click();
        harness.run();
        assert!(
            harness
                .get_by_label_contains("Raise the tempo")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            !harness
                .get_by_label_contains("Timing window")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn a_lane_key_places_a_tap_at_the_cursor_and_moves_on_a_snap() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn lane_keys_held_together_place_a_chord_and_move_on_once_all_are_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_down(egui::Key::Num2);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
        harness.key_up(egui::Key::Num2);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 1, 3]);
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    /// Presses and releases each of `keys` in turn, all in the next frame, as a fast typist does
    /// when frames are slow. Queued events would each get a frame of their own.
    fn tap_in_one_frame(harness: &mut Harness<'_, Window>, keys: &[egui::Key]) {
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

    #[test]
    fn a_lane_key_tapped_twice_in_one_frame_places_two_notes_down_the_lane() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num1]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [0]);
        assert!(cursor_shows(&harness, "0:0:480"));
    }

    #[test]
    fn two_lane_keys_tapped_in_one_frame_make_a_stream_not_a_chord() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num2]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [1]);
    }

    #[test]
    fn keys_in_one_frame_act_in_the_order_they_were_pressed() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::ArrowDown]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:480"));
    }

    #[test]
    fn a_lane_key_released_under_ctrl_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        harness.event(egui::Event::Key {
            key: egui::Key::Num1,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
            physical_key: None,
        });
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    /// A key event as a keyboard sends it: what the layout makes of the key under `modifiers`,
    /// and where the key sits.
    fn keyboard(
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

    #[test]
    fn a_lane_key_released_under_shift_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        let shift = egui::Modifiers::SHIFT;
        harness.event(keyboard(
            egui::Key::Num1,
            egui::Key::Num1,
            true,
            egui::Modifiers::NONE,
        ));
        harness.run();
        harness.event(egui::Event::ModifiersChanged(shift));
        harness.event(keyboard(
            egui::Key::Exclamationmark,
            egui::Key::Num1,
            false,
            shift,
        ));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn a_lane_key_held_through_shift_coming_up_repeats_without_toggling() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let none = egui::Modifiers::NONE;
        harness.event(keyboard(egui::Key::Num1, egui::Key::Num1, true, none));
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    }

    #[test]
    fn ctrl_minus_zooms_out_where_the_layout_puts_minus_on_a_digit_key() {
        let mut harness = editable_demo();
        let command = egui::Modifiers::COMMAND;
        harness.event(egui::Event::ModifiersChanged(command));
        harness.event(keyboard(egui::Key::Minus, egui::Key::Num6, true, command));
        harness.run();
        assert!(harness.query_by_label("Zoom 80%").is_some());
    }

    #[test]
    fn a_numpad_arrow_moves_the_cursor_rather_than_placing_a_note() {
        let mut harness = editable_demo();
        let none = egui::Modifiers::NONE;
        harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, true, none));
        harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, false, none));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
        assert_eq!(lanes_at(&harness, 0), [3]);
    }

    #[test]
    fn a_lane_key_released_while_a_widget_has_focus_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert!(harness.ctx.egui_wants_keyboard_input());
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn losing_focus_with_a_lane_key_down_moves_on_as_if_it_came_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.event(egui::Event::WindowFocused(false));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
        harness.event(egui::Event::WindowFocused(true));
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [0]);
    }

    #[test]
    fn undoing_a_note_whose_key_is_still_down_keeps_the_cursor_when_it_comes_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn opening_a_song_with_a_lane_key_down_leaves_its_cursor_alone() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.state_mut().open_demo();
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn keys_wait_while_the_window_asks_about_unsaved_changes() {
        let mut harness = edited_four_taps();
        harness
            .state_mut()
            .open(Path::new("example.json"), example());
        harness.run();
        harness.key_press(egui::Key::Num3);
        harness.run();
        assert_eq!(note_count(&harness), 5);
    }

    /// What starts on `lane` at `tick` in the open chart, as the format writes it.
    fn note_at(harness: &Harness<'_, Window>, tick: i32, lane: u16) -> Option<serde_json::Value> {
        let song = harness.state().song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        chart
            .objects
            .iter()
            .find(|object| {
                object.tick == Tick::new(tick)
                    && matches!(object.position, Position::Lane { index, .. } if index == LaneIndex::new(lane))
            })
            .map(|object| serde_json::json!({ "kind": object.kind.to_string(), "shape": object.shape }))
    }

    fn tap() -> serde_json::Value {
        serde_json::json!({ "kind": "tap", "shape": "instant" })
    }

    fn hold(duration: u32) -> serde_json::Value {
        serde_json::json!({ "kind": "hold", "shape": { "sustain": { "duration": duration } } })
    }

    fn mine() -> serde_json::Value {
        serde_json::json!({ "kind": "mine", "shape": "instant" })
    }

    /// Presses the lane key for `lane` under Shift, which a US layout reports as a symbol.
    fn shift_down(harness: &Harness<'_, Window>, symbol: egui::Key, place: egui::Key) {
        let shift = egui::Modifiers::SHIFT;
        harness.event(egui::Event::ModifiersChanged(shift));
        harness.event(keyboard(symbol, place, true, shift));
    }

    #[test]
    fn shift_and_a_lane_key_place_a_mine_and_move_on() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(keyboard(
            egui::Key::Exclamationmark,
            egui::Key::Num1,
            false,
            egui::Modifiers::SHIFT,
        ));
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn holding_a_lane_key_while_the_cursor_moves_draws_a_hold() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(hold(480)));
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:720"));
    }

    #[test]
    fn a_hold_drawn_with_a_lane_key_undoes_in_one_step() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_up(egui::Key::Num1);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), None);
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn moving_back_to_where_the_key_went_down_makes_the_hold_a_tap_again() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowUp);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    }

    #[test]
    fn a_mine_does_not_stretch() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    }

    /// An `sm` song with one four-lane chart of taps walking across the lanes a sixteenth apart.
    fn four_taps() -> Vec<u8> {
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

    fn editable_four_taps() -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("four.json"), &four_taps());
        window(app)
    }

    fn note_count(harness: &Harness<'_, Window>) -> usize {
        let song = harness.state().song.as_ref().unwrap();
        song.chart().unwrap().objects.len()
    }

    #[test]
    fn brackets_lengthen_the_note_at_the_cursor_to_a_hold_and_shorten_it_back() {
        let mut harness = editable_four_taps();
        let first = |harness: &Harness<'_, Window>| {
            let song = harness.state().song.as_ref().unwrap();
            song.chart().unwrap().objects.first().unwrap().kind.clone()
        };
        harness.key_press(egui::Key::CloseBracket);
        harness.run();
        assert_eq!(first(&harness), charter_core::Kind::Hold);
        harness.key_press(egui::Key::OpenBracket);
        harness.run();
        assert_eq!(first(&harness), charter_core::Kind::Tap);
    }

    #[test]
    fn clicking_the_minimap_moves_the_cursor_there_and_the_setting_hides_it() {
        let mut harness = editable_four_taps();
        let map = harness.get_by_label("Minimap").rect();
        // The last tap starts at tick 720 of a map running to its end.
        click(
            &mut harness,
            egui::pos2(map.center().x, map.bottom() - 1.0),
            egui::Modifiers::NONE,
        );
        let cursor = harness.state().song.as_ref().unwrap().cursor();
        assert!(cursor > Tick::new(0), "{cursor:?}");
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            minimap: false,
            ..Settings::default()
        });
        app.open(Path::new("four.json"), &four_taps());
        let harness = window(app);
        assert!(harness.query_by_label("Minimap").is_none());
    }

    #[test]
    fn ctrl_a_selects_the_chart_and_delete_removes_it_in_one_step() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert!(harness.query_by_label("4 notes selected").is_some());
        harness.key_press(egui::Key::Delete);
        harness.run();
        assert_eq!(note_count(&harness), 0);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }

    fn nothing_selected(harness: &Harness<'_, Window>) -> bool {
        harness.query_by_label_contains("selected").is_none()
    }

    #[test]
    fn the_status_bar_counts_the_selection() {
        let mut harness = editable_four_taps();
        assert!(nothing_selected(&harness));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert!(harness.query_by_label("4 notes selected").is_some());
    }

    /// Types `text` over the song panel's field `label`, then presses Enter.
    fn retype(harness: &mut Harness<'_, Window>, label: &str, text: &str) {
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

    fn metadata(harness: &Harness<'_, Window>) -> charter_core::Metadata {
        let song = harness.state().song.as_ref().unwrap();
        song.document().document().metadata.clone()
    }

    fn picture(bytes: &[u8]) -> Picture {
        (
            bytes.to_vec(),
            egui::ColorImage::filled([4, 4], egui::Color32::RED),
        )
    }

    fn artwork_name(harness: &Harness<'_, Window>) -> Option<String> {
        metadata(harness).artwork.map(|path| path.to_string())
    }

    #[test]
    fn chosen_artwork_previews_and_is_written_beside_the_song_on_save() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        harness.get_by_label("Choose the song's artwork").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseArtwork);
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("art.png"));
        assert!(
            harness
                .query_by_label("Not a picture beside the song")
                .is_none()
        );
        let saving = Path::new("/songs/Four/four.json");
        assert_eq!(
            harness.state().artwork_write_for(saving),
            Some((PathBuf::from("/songs/Four/art.png"), b"png".to_vec()))
        );
        harness.state_mut().saved(Saved::To(saving.to_path_buf()));
        assert_eq!(harness.state().artwork_write_for(saving), None);
    }

    #[test]
    fn a_file_that_is_not_a_picture_is_refused_as_artwork() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/notes.txt"), None);
        harness.run();
        assert_eq!(artwork_name(&harness), None);
        assert!(
            harness
                .query_by_label_contains("is not a PNG or JPEG picture")
                .is_some()
        );
    }

    #[test]
    fn new_audio_brings_its_embedded_cover_unless_the_song_has_artwork() {
        let mut harness = four_taps_at("/songs/four.json");
        let audio = Path::new("/music/song.flac");
        harness.state_mut().use_audio(audio);
        harness.run();
        harness
            .state_mut()
            .artwork_embedded(Path::new("/music/other.flac"), "jpg", picture(b"x"));
        assert_eq!(artwork_name(&harness), None);
        harness
            .state_mut()
            .artwork_embedded(audio, "jpg", picture(b"jpeg"));
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
        harness
            .state_mut()
            .artwork_embedded(audio, "png", picture(b"png"));
        assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
    }

    #[test]
    fn a_song_naming_artwork_asks_for_it_beside_the_song_and_previews_it() {
        let mut harness = four_taps_at("/songs/four.json");
        retype(&mut harness, "Artwork", "art.png");
        let requests = harness.state_mut().take_requests();
        let file = PathBuf::from("/songs/art.png");
        assert!(
            requests.contains(&Request::ReadArtwork { file: file.clone() }),
            "{requests:?}"
        );
        assert!(harness.query_by_label("Reading…").is_some());
        harness.state_mut().artwork_read(&file, None);
        harness.run();
        assert!(
            harness
                .query_by_label("Not a picture beside the song")
                .is_some()
        );
    }

    #[test]
    fn the_song_panel_renames_the_song_and_sets_its_preview() {
        let mut harness = editable_demo();
        retype(&mut harness, "Title", "Mine");
        retype(&mut harness, "Artist", "Me");
        assert_eq!(
            (metadata(&harness).title, metadata(&harness).artist),
            ("Mine".to_owned(), "Me".to_owned())
        );
        retype(&mut harness, "Preview", "0:45.5");
        retype(&mut harness, "Preview length", "15");
        let preview = metadata(&harness).preview.unwrap();
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            serde_json::json!({ "start_seconds": 45.5, "length_seconds": 15.0 })
        );
        retype(&mut harness, "Preview", "");
        assert_eq!(metadata(&harness).preview, None);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(metadata(&harness).preview.is_some());
        harness.get_by_label("Choose the song's audio").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseAudio);
    }

    #[test]
    fn the_song_panel_sets_the_genre_and_artwork_and_clears_them() {
        let mut harness = editable_demo();
        retype(&mut harness, "Genre", "Dance");
        retype(&mut harness, "Artwork", "art/artwork.png");
        assert_eq!(metadata(&harness).genre.as_deref(), Some("Dance"));
        assert_eq!(
            metadata(&harness).artwork.map(|path| path.to_string()),
            Some("art/artwork.png".to_owned())
        );
        retype(&mut harness, "Artwork", "../outside.png");
        assert!(
            harness
                .query_by_label_contains("is not a file beside the song")
                .is_some()
        );
        retype(&mut harness, "Genre", "");
        retype(&mut harness, "Artwork", "");
        assert_eq!(
            (metadata(&harness).genre, metadata(&harness).artwork),
            (None, None)
        );
    }

    #[test]
    fn a_games_settings_show_as_fields_for_the_song_and_its_open_chart() {
        const TTR: &[&dyn RulesetPlugin] = &[&charter_ttr::Plugin];
        let draft = charter_core::NewSong {
            metadata: charter_editor::demo::song().document().metadata.clone(),
            layout: 0,
            charts: vec![charter_core::NewChart::named("Extreme")],
        };
        let song = charter_ttr::Plugin.new_song(&draft).unwrap();
        let mut app = Window::new(TTR);
        app.open(Path::new("/songs/ttr.json"), &song.to_json().unwrap());
        let mut harness = window(app);
        retype(&mut harness, "Theme", "DeltaHeavyTheme");
        retype(&mut harness, "Chart theme", "Neon");
        let song = harness.state().song.as_ref().unwrap();
        let document = song.document().document();
        assert_eq!(
            document.settings["theme"],
            serde_json::json!("DeltaHeavyTheme")
        );
        assert_eq!(
            document.charts[0].settings["theme"],
            serde_json::json!("Neon")
        );
        retype(&mut harness, "Theme", "");
        let song = harness.state().song.as_ref().unwrap();
        assert!(song.document().document().settings.is_empty());
        let sm = editable_demo();
        assert!(sm.query_by_label("Chart theme").is_none());
    }

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
    fn the_song_panel_folds_away_and_comes_back() {
        let mut harness = editable_demo();
        assert!(harness.query_by_label("Verse").is_some());
        harness.get_by_label("Hide the song panel").click();
        harness.run();
        assert!(harness.query_by_label("Verse").is_none());
        assert!(!harness.state().settings().song_panel);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Backslash);
        harness.run();
        assert!(harness.query_by_label("Verse").is_some());
        harness.get_by_label("View").click();
        harness.run();
        harness.get_by_label_contains("Song panel").click();
        harness.run();
        assert!(harness.query_by_label("Verse").is_none());
    }

    #[test]
    fn escape_clears_the_selection() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(nothing_selected(&harness));
        harness.key_press(egui::Key::Backspace);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }

    /// Where the middle of `lane` meets the line of `tick` on screen, `below` points lower.
    fn spot(harness: &Harness<'_, Window>, lane: u16, tick: i32, below: f32) -> egui::Pos2 {
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

    fn press(
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

    fn click(harness: &mut Harness<'_, Window>, pos: egui::Pos2, modifiers: egui::Modifiers) {
        harness.hover_at(pos);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        press(harness, pos, true, modifiers);
        press(harness, pos, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
    }

    fn drag(
        harness: &mut Harness<'_, Window>,
        from: egui::Pos2,
        to: egui::Pos2,
        modifiers: egui::Modifiers,
    ) {
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        press(harness, from, true, modifiers);
        for step in 1..=4 {
            #[allow(clippy::cast_precision_loss)] // four steps
            harness.hover_at(from.lerp(to, step as f32 / 4.0));
        }
        press(harness, to, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
    }

    #[test]
    fn ctrl_clicking_a_lane_places_a_tap_on_the_nearest_snap_line() {
        let mut harness = editable_four_taps();
        let pos = spot(&harness, 3, 240, 4.0);
        click(&mut harness, pos, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(tap()));
    }

    #[test]
    fn ctrl_clicking_a_lane_number_does_not_place_a_hidden_note() {
        let mut harness = editable_four_taps();
        let x = spot(&harness, 0, 0, 0.0).x;
        let top = harness.get_by_label("Notefield").rect().top();
        click(
            &mut harness,
            egui::pos2(x, top + 13.0),
            egui::Modifiers::COMMAND,
        );

        assert_eq!(note_count(&harness), 4);
        assert!(!harness.state().song.as_ref().unwrap().is_modified());
    }

    #[test]
    fn ctrl_shift_clicking_places_a_mine() {
        let mut harness = editable_four_taps();
        let pos = spot(&harness, 1, 480, 0.0);
        click(
            &mut harness,
            pos,
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        );
        assert_eq!(note_at(&harness, 480, 1), Some(mine()));
    }

    #[test]
    fn ctrl_dragging_down_a_lane_draws_a_hold_that_undoes_in_one_step() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 240, 3), None);
    }

    #[test]
    fn clicking_notes_selects_them_as_shift_adds_and_alt_takes_away() {
        let mut harness = editable_four_taps();
        let (first, second) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 240, 2.0));
        click(&mut harness, first, egui::Modifiers::NONE);
        assert!(harness.query_by_label("1 note selected").is_some());
        click(&mut harness, second, egui::Modifiers::SHIFT);
        assert!(harness.query_by_label("2 notes selected").is_some());
        click(&mut harness, first, egui::Modifiers::ALT);
        assert!(harness.query_by_label("1 note selected").is_some());
        let empty = spot(&harness, 3, 0, 0.0);
        click(&mut harness, empty, egui::Modifiers::NONE);
        assert!(nothing_selected(&harness));
    }

    #[test]
    fn dragging_a_box_selects_the_notes_whose_heads_it_covers() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert!(harness.query_by_label("2 notes selected").is_some());
        let song = harness.state().song.as_ref().unwrap();
        let ticks: Vec<_> = song.selection().map(|note| note.tick).collect();
        assert_eq!(ticks, [Tick::new(240), Tick::new(480)]);
    }

    #[test]
    fn dragging_a_note_moves_it_along_the_snap_and_across_lanes_in_one_step() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 480, 3.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(note_at(&harness, 0, 0), None);
        assert_eq!(note_at(&harness, 480, 1), Some(tap()));
        assert_eq!(note_count(&harness), 4);
        assert!(harness.query_by_label("1 note selected").is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
        assert_eq!(note_at(&harness, 480, 1), None);
    }

    #[test]
    fn dragging_a_selected_note_moves_the_selection_and_keeps_it_on_the_lanes() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        let (from, to) = (spot(&harness, 1, 240, 0.0), spot(&harness, 2, 480, 0.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        for (tick, lane) in [(240, 0), (480, 1), (720, 2), (960, 3)] {
            assert_eq!(note_at(&harness, tick, lane), Some(tap()), "{tick} {lane}");
        }
        assert!(harness.query_by_label("4 notes selected").is_some());
    }

    #[test]
    fn the_mouse_places_nothing_in_a_read_only_song() {
        let mut app = Window::default();
        app.open(Path::new("four.json"), &four_taps());
        let mut harness = window(app);
        let pos = spot(&harness, 3, 240, 0.0);
        click(&mut harness, pos, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), None);
        assert!(harness.query_by_label_contains("Refused").is_none());
    }

    /// The one request the window made since the last were taken.
    fn request(harness: &mut Harness<'_, Window>) -> Request {
        let mut requests = harness.state_mut().take_requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        requests.remove(0)
    }

    /// Whether `bytes` hold a song whose open chart has a tap on `lane` at `tick`.
    fn holds_tap(bytes: &[u8], tick: i32, lane: u16) -> bool {
        let song = charter_core::ValidDocument::from_json(bytes).unwrap();
        let chart = song.charts().next().unwrap();
        chart.objects().any(|object| {
            object.tick == Tick::new(tick)
                && object.position.lanes()
                    == Some(LaneSpan::new(LaneIndex::new(lane), LaneCount::ONE))
        })
    }

    #[test]
    fn ctrl_s_writes_the_song_back_to_its_file() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, bytes } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("four.json"));
        assert!(holds_tap(&bytes, 0, 3));
    }

    #[test]
    fn saving_a_song_without_a_file_asks_where() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::SaveAs { name, .. } = request(&mut harness) else {
            panic!("expected save as");
        };
        assert_eq!(name, "Demo.json");
    }

    #[test]
    fn ctrl_shift_s_asks_where_even_for_a_song_with_a_file() {
        let mut harness = editable_four_taps();
        let command_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(command_shift, egui::Key::S);
        harness.run();
        let Request::SaveAs { name, .. } = request(&mut harness) else {
            panic!("expected save as");
        };
        assert_eq!(name, "four.json");
    }

    #[test]
    fn ctrl_o_and_the_file_menu_ask_for_a_song_to_open() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("Open…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }

    #[test]
    fn the_title_names_the_song_and_marks_unsaved_changes() {
        let mut harness = editable_four_taps();
        assert_eq!(harness.state().title(), "four.json - charter");
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("four.json")));
        harness.run();
        assert_eq!(harness.state().title(), "four.json - charter");
    }

    #[test]
    fn a_song_saved_elsewhere_is_saved_there_from_then_on() {
        let mut harness = editable_demo();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("mine.json")));
        harness.run();
        assert_eq!(harness.state().title(), "mine.json - charter");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("mine.json"));
    }

    #[test]
    fn a_failed_save_says_why_and_keeps_the_changes_unsaved() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.run();
        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        assert!(
            harness
                .query_by_label_contains("the disk is full")
                .is_some()
        );
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    /// The four taps with one more placed, so there is something to lose.
    fn edited_four_taps() -> Harness<'static, Window> {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.run();
        harness
    }

    /// Asks to close the window as its close button does, and runs the one frame that answers.
    fn close(harness: &mut Harness<'_, Window>) {
        let root = harness.input_mut().viewports.entry(egui::ViewportId::ROOT);
        root.or_default().events.push(egui::ViewportEvent::Close);
        harness.step();
    }

    /// Whether the last frame sent `command` to the host's window.
    fn sent(harness: &Harness<'_, Window>, command: &egui::ViewportCommand) -> bool {
        let output = harness
            .output()
            .viewport_output
            .get(&egui::ViewportId::ROOT);
        output.is_some_and(|viewport| viewport.commands.contains(command))
    }

    fn asks_to_save(harness: &Harness<'_, Window>) -> bool {
        harness
            .query_by_label("Save changes to four.json?")
            .is_some()
    }

    #[test]
    fn closing_with_unsaved_changes_asks_first() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        assert!(sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();
        assert!(asks_to_save(&harness));
    }

    #[test]
    fn closing_without_changes_does_not_ask() {
        let mut harness = editable_four_taps();
        close(&mut harness);
        assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();
        assert!(!asks_to_save(&harness));
    }

    #[test]
    fn dont_save_closes_and_lets_the_close_through() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.step();
        harness.step();
        assert!(sent(&harness, &egui::ViewportCommand::Close));
        close(&mut harness);
        assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
    }

    #[test]
    fn save_writes_and_closes_once_the_host_says_it_saved() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        harness.state_mut().saved(Saved::To(path));
        harness.step();
        assert!(sent(&harness, &egui::ViewportCommand::Close));
    }

    #[test]
    fn cancel_keeps_the_window_and_its_changes() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(!asks_to_save(&harness));
        assert!(!sent(&harness, &egui::ViewportCommand::Close));
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    #[test]
    fn opening_another_song_over_unsaved_changes_asks_first() {
        let mut harness = edited_four_taps();
        harness
            .state_mut()
            .open(Path::new("example.json"), example());
        harness.run();
        assert!(asks_to_save(&harness));
        assert_eq!(harness.state().title(), "*four.json - charter");
        harness.get_by_label("Don't save").click();
        harness.run();
        assert_eq!(harness.state().title(), "example.json - charter");
    }

    #[test]
    fn a_save_that_fails_leaves_everything_as_it_was() {
        let mut harness = edited_four_taps();
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label("Open the demo song").click();
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        assert!(!asks_to_save(&harness));
    }

    /// Shows the settings page named `name`.
    fn settings_page(harness: &mut Harness<'_, Window>, name: &str) {
        harness.get_by_label(name).click();
        harness.run();
    }

    #[test]
    fn the_settings_window_shows_one_page_at_a_time_starting_with_editing() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
                .is_some()
        );
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "Notefield")
                .is_none()
        );
        settings_page(&mut harness, "Display");
        assert!(
            harness
                .query_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
                .is_none()
        );
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "Notefield")
                .is_some()
        );
    }

    #[test]
    fn the_settings_window_closes_from_its_own_button() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness.get_by_label("Close settings").click();
        harness.run();
        assert!(harness.query_all_by_label("Settings").next().is_none());
    }

    #[test]
    fn the_settings_window_opens_from_the_file_menu_and_with_ctrl_comma() {
        let mut harness = editable_demo();
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("Settings…").click();
        harness.run();
        assert!(harness.query_all_by_label("Settings").next().is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        assert!(harness.query_all_by_label("Settings").next().is_none());
    }

    #[test]
    fn lane_keys_and_commands_still_work_with_the_settings_open() {
        let mut harness = editable_demo();
        let before = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .chart()
            .unwrap()
            .objects
            .clone();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        let after = &harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .chart()
            .unwrap()
            .objects;
        assert_ne!(after, &before);
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(harness.state().song.as_ref().unwrap().cursor() > Tick::new(0));
    }

    #[test]
    fn a_button_tab_focused_leaves_the_keys_to_the_editor() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert!(harness.ctx.memory(egui::Memory::focused).is_some());
        let before = harness.state().song.as_ref().unwrap().cursor();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(harness.state().song.as_ref().unwrap().cursor() > before);
    }

    #[test]
    fn the_settings_window_turns_the_notefield_and_picks_sounds() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Display");
        harness
            .get_by_role_and_label(Role::ComboBox, "Notefield")
            .click();
        harness.run();
        harness
            .get_by_label("Horizontal, notes travel left")
            .click();
        harness.run();
        assert_eq!(harness.state().settings().direction, Direction::Left);
        settings_page(&mut harness, "Sounds");
        harness
            .get_by_role_and_label(Role::CheckBox, "Metronome")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "Metronome sound")
            .click();
        harness.run();
        harness.get_by_label("Shaker burst").click();
        harness.run();
        assert!(harness.state().settings().metronome);
        assert_eq!(harness.state().settings().metronome_sound, Kit::ShakerBurst);
        harness.get_by_label("Preview the metronome").click();
        harness.run();
        let preview = harness.state_mut().take_requests();
        let Some(Request::Preview { clicks, .. }) = preview.last() else {
            panic!("expected a preview, got {preview:?}");
        };
        let tones: Vec<Tone> = clicks.iter().map(|(_, tone)| *tone).collect();
        let (accent, beat) = (Tone::Accent(Kit::ShakerBurst), Tone::Beat(Kit::ShakerBurst));
        assert_eq!(tones, [accent, beat, beat, beat]);
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "Note ticks")
                .is_some()
        );
    }

    /// The demo, open for editing with the notefield running `direction`.
    fn turned_demo(direction: Direction) -> Harness<'static, Window> {
        let settings = Settings {
            direction,
            ..Settings::default()
        };
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open_demo();
        window(app)
    }

    /// Where the middle of `lane` meets the line of `tick` on screen, however the notefield runs.
    fn turned_spot(harness: &Harness<'_, Window>, lane: u16, tick: i32) -> egui::Pos2 {
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

    #[test]
    fn ctrl_clicking_places_a_tap_however_the_notefield_runs() {
        for direction in Direction::ALL {
            let mut harness = turned_demo(direction);
            let at = turned_spot(&harness, 1, 240);
            click(&mut harness, at, egui::Modifiers::COMMAND);
            assert_eq!(lanes_at(&harness, 240), [1], "{direction:?}");
        }
    }

    #[test]
    fn clicking_a_note_selects_it_however_the_notefield_runs() {
        for direction in [Direction::Down, Direction::Right] {
            let mut harness = turned_demo(direction);
            let at = turned_spot(&harness, 3, 0);
            click(&mut harness, at, egui::Modifiers::NONE);
            let song = harness.state().song.as_ref().unwrap();
            assert_eq!(song.selection().count(), 1, "{direction:?}");
        }
    }

    /// Clicks what `label` names in the song panel, scrolled into view first, since the panel
    /// runs past the bottom of the test's window.
    fn click_in_panel(harness: &mut Harness<'_, Window>, label: &str) {
        harness.get_by_label(label).scroll_to_me();
        harness.run();
        harness.get_by_label(label).click();
        harness.run();
    }

    fn bpm_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "BPM")
    }

    /// Types `text` over whatever the BPM field holds, then presses `key`.
    fn enter_bpm(harness: &mut Harness<'_, Window>, text: &str, key: egui::Key) {
        bpm_field(harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        bpm_field(harness).type_text(text);
        harness.run();
        harness.key_press(key);
        harness.run();
    }

    /// The tick and BPM of the change in effect at the cursor.
    fn bpm_at_cursor(harness: &Harness<'_, Window>) -> Option<(i64, String)> {
        let song = harness.state().song.as_ref().unwrap();
        let (tick, bpm) = song.bpm()?;
        let tick = serde_json::to_value(tick).unwrap().as_i64().unwrap();
        let value = f64::from(bpm);
        let one = (value * 10.0).round() / 10.0;
        let shown = if (value - one).abs() < 1e-5 {
            charter_core::Bpm::try_from(one).map_or_else(|_| bpm.to_string(), |bpm| bpm.to_string())
        } else {
            bpm.to_string()
        };
        Some((tick, shown))
    }

    #[test]
    fn the_tempo_at_the_cursor_shows_beside_the_song_and_in_the_status_bar() {
        let harness = editable_demo();
        assert_eq!(bpm_field(&harness).value().as_deref(), Some("150"));
        assert!(harness.query_by_label("150 BPM").is_some());
    }

    #[test]
    fn a_bpm_typed_changes_the_tempo_of_the_section_the_cursor_is_in() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        enter_bpm(&mut harness, "175.5", egui::Key::Enter);
        assert_eq!(bpm_at_cursor(&harness), Some((0, "175.5".to_owned())));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }

    /// The tick and time of the first note after the start of the open chart.
    fn first_note(harness: &Harness<'_, Window>) -> (Tick, f64) {
        let song = harness.state().song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let note = chart
            .objects
            .iter()
            .find(|note| note.tick > Tick::new(0))
            .unwrap();
        (note.tick, song.timing().time_at(note.tick).seconds())
    }

    #[test]
    fn a_bpm_keeps_notes_where_they_sound_unless_the_setting_keeps_them_on_their_beats() {
        let mut harness = editable_demo();
        let (tick, seconds) = first_note(&harness);
        enter_bpm(&mut harness, "175.5", egui::Key::Enter);
        let (moved, heard) = first_note(&harness);
        assert_ne!(moved, tick);
        assert!((heard - seconds).abs() < 1e-3, "{heard} {seconds}");

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.state_mut().settings.notes_keep_time = false;
        harness.run();
        enter_bpm(&mut harness, "175.5", egui::Key::Enter);
        assert_eq!(first_note(&harness).0, tick);
    }

    #[test]
    fn a_bpm_typed_applies_when_the_field_is_left() {
        let mut harness = editable_demo();
        bpm_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        bpm_field(&harness).type_text("160");
        harness.run();
        offset_field(&harness).focus();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "160".to_owned())));
    }

    #[test]
    fn the_bpm_steps_up_and_down_a_whole_bpm_at_a_time() {
        let mut harness = editable_demo();
        click_in_panel(&mut harness, "Raise the BPM by 1");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "151".to_owned())));
        harness.get_by_label("Lower the BPM by 1").click();
        click_in_panel(&mut harness, "Lower the BPM by 1");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "149".to_owned())));
    }

    #[test]
    fn a_bpm_left_with_escape_changes_nothing() {
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "90", egui::Key::Escape);
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
        assert_eq!(bpm_field(&harness).value().as_deref(), Some("150"));
    }

    #[test]
    fn a_bpm_that_is_not_a_tempo_says_so() {
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "fast", egui::Key::Enter);
        assert!(
            harness
                .query_by_label_contains("fast is not a tempo")
                .is_some()
        );
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }

    #[test]
    fn a_timing_point_is_added_at_the_cursor_and_removed_but_not_the_first() {
        let mut harness = editable_demo();
        assert!(harness.query_by_label("Remove this timing point").is_none());
        let add = |harness: &Harness<'_, Window>| {
            harness
                .get_by_label("Add a timing point here")
                .accesskit_node()
                .is_disabled()
        };
        assert!(add(&harness), "the cursor is on the first point");
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(!add(&harness));
        harness.get_by_label("Add a timing point here").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((240, "150".to_owned())));
        assert!(add(&harness));
        click_in_panel(&mut harness, "Remove this timing point");
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }

    #[test]
    fn the_timing_points_list_shows_every_point_and_jumps_to_one_clicked() {
        let mut harness = editable_demo();
        // Tall enough that the whole song panel, the list with it, is on screen.
        harness.set_size(egui::vec2(1000.0, 1100.0));
        harness.run();
        let rows = harness
            .query_all_by_label_contains("Timing point at")
            .count();
        assert_eq!(rows, 5);
        harness.get_by_label("Timing point at 00:26.100").click();
        harness.run();
        assert!(cursor_shows(&harness, "16:0:0"));
        assert_eq!(bpm_field(&harness).value().as_deref(), Some("180"));
    }

    #[test]
    fn a_time_typed_for_a_point_moves_it_in_the_audio() {
        let mut harness = editable_demo();
        harness.get_by_label("Timing point at 00:26.100").click();
        harness.run();
        let time = harness.get_by_role_and_label(Role::TextInput, "Time");
        assert_eq!(time.value().as_deref(), Some("00:26.100"));
        time.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Time")
            .type_text("0:26.25");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(anchor_seconds(&harness), [0.0, 26.25]);
        harness.get_by_label("Timing point at 00:26.250").click();
        harness.run();
    }

    #[test]
    fn the_starting_bpm_sets_the_first_points_tempo_from_anywhere() {
        let mut harness = editable_demo();
        harness.get_by_label("Timing point at 00:26.100").click();
        harness.run();
        let start = harness.get_by_role_and_label(Role::TextInput, "Starting BPM");
        assert_eq!(start.value().as_deref(), Some("150"));
        start.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Starting BPM")
            .type_text("160");
        harness.key_press(egui::Key::Enter);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(
            song.timing().bpm_at(Tick::new(0)).map(f64::from),
            Some(160.0)
        );
        assert_eq!(anchor_seconds(&harness), [0.0, 26.1]);
    }

    fn meter_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "Meter")
    }

    #[test]
    fn a_meter_typed_starts_measures_of_it_at_the_timing_point() {
        let mut harness = editable_demo();
        assert_eq!(meter_field(&harness).value().as_deref(), Some("4/4"));
        meter_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        meter_field(&harness).type_text("3/4");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        let meter = song
            .timing()
            .measures_from(Tick::new(0))
            .next()
            .unwrap()
            .meter;
        assert_eq!(meter.to_string(), "3/4");
        meter_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        meter_field(&harness).type_text("waltz");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(
            harness
                .query_by_label_contains("waltz is not a meter")
                .is_some()
        );
    }

    #[test]
    fn a_read_only_song_shows_its_tempo_without_changing_it() {
        let mut harness = window(Window::new(&[]));
        harness.state_mut().open_demo();
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "BPM")
                .is_none()
        );
        assert!(harness.query_by_label("150 BPM").is_some());
    }

    fn section_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "Section here")
    }

    /// Types `text` over whatever the section field holds, then presses `key`.
    fn enter_section(harness: &mut Harness<'_, Window>, text: &str, key: egui::Key) {
        section_field(harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        section_field(harness).type_text(text);
        harness.run();
        harness.key_press(key);
        harness.run();
    }

    fn marker_texts(harness: &Harness<'_, Window>) -> Vec<String> {
        let song = harness.state().song.as_ref().unwrap();
        let markers = &song.document().document().markers;
        markers.iter().map(|marker| marker.text.clone()).collect()
    }

    #[test]
    fn clicking_a_marker_in_the_list_jumps_there() {
        let mut harness = editable_demo();
        // The list sits below the tempo controls, past the bottom of a short window.
        harness.get_by_label("Chorus").scroll_to_me();
        harness.run();
        harness.get_by_label("Chorus").click();
        harness.run();
        assert!(cursor_shows(&harness, "16:0:0"));
        assert_eq!(section_field(&harness).value().as_deref(), Some("Chorus"));
    }

    #[test]
    fn a_section_named_at_the_cursor_is_added_and_listed() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        assert_eq!(section_field(&harness).value().as_deref(), Some(""));
        enter_section(&mut harness, "Build", egui::Key::Enter);
        assert_eq!(marker_texts(&harness)[1], "Build");
        assert!(harness.query_by_label("Build").is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(!marker_texts(&harness).contains(&"Build".to_owned()));
    }

    #[test]
    fn a_section_renamed_at_the_cursor_keeps_its_place() {
        let mut harness = editable_demo();
        enter_section(&mut harness, "Opening", egui::Key::Enter);
        assert_eq!(marker_texts(&harness)[0], "Opening");
    }

    #[test]
    fn a_section_name_left_with_escape_changes_nothing() {
        let mut harness = editable_demo();
        enter_section(&mut harness, "Opening", egui::Key::Escape);
        assert_eq!(marker_texts(&harness)[0], "Intro");
    }

    #[test]
    fn a_marker_can_be_removed_from_the_list() {
        let mut harness = editable_demo();
        let remove = "Remove marker Check the stream";
        harness.get_by_label(remove).scroll_to_me();
        harness.run();
        harness.get_by_label(remove).click();
        harness.run();
        assert!(!marker_texts(&harness).contains(&"Check the stream".to_owned()));
        assert!(harness.query_by_label("Check the stream").is_none());
    }

    #[test]
    fn a_read_only_song_jumps_to_markers_without_editing_them() {
        let mut harness = window(Window::new(&[]));
        harness.state_mut().open_demo();
        harness.run();
        click_in_panel(&mut harness, "Verse");
        assert!(cursor_shows(&harness, "8:0:0"));
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "Section here")
                .is_none()
        );
        assert!(harness.query_by_label("Remove marker Verse").is_none());
    }

    #[test]
    fn home_and_end_jump_to_the_start_and_to_where_the_last_note_ends() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::End);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.cursor(), song.end());
        assert_ne!(song.cursor(), Tick::new(0));
        harness.key_press(egui::Key::Home);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn alt_up_and_down_jump_between_sections() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
        harness.run();
        assert!(cursor_shows(&harness, "8:0:0"));
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
        harness.run();
        assert!(cursor_shows(&harness, "16:0:0"));
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowUp);
        harness.run();
        assert!(cursor_shows(&harness, "8:0:0"));
    }

    #[test]
    fn ctrl_b_toggles_a_bookmark_at_the_cursor() {
        let mut harness = editable_demo();
        let before = marker_texts(&harness).len();
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
        harness.run();
        assert_eq!(marker_texts(&harness).len(), before + 1);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
        harness.run();
        assert_eq!(marker_texts(&harness).len(), before);
    }

    #[test]
    fn f8_and_f7_raise_and_lower_the_tempo_at_the_cursor_a_bpm_at_a_time() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F8);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "151".to_owned())));
        harness.key_press(egui::Key::F7);
        harness.key_press(egui::Key::F7);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "149".to_owned())));
    }

    #[test]
    fn f1_shows_the_keyboard_shortcuts() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F1);
        harness.run();
        let shown = |harness: &Harness<'_, Window>| {
            harness
                .query_all_by_label("Keyboard shortcuts")
                .next()
                .is_some()
        };
        assert!(shown(&harness));
        harness.key_press(egui::Key::F1);
        harness.run();
        assert!(!shown(&harness));
    }

    fn four_taps_at(path: &str) -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new(path), &four_taps());
        window(app)
    }

    /// Four taps whose audio the host has loaded.
    fn playable() -> Harness<'static, Window> {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .audio_ready(Path::new("/songs/a.ogg"), 60.0);
        harness.run();
        harness
    }

    fn space(harness: &mut Harness<'_, Window>) {
        harness.key_press(egui::Key::Space);
        harness.run();
    }

    #[test]
    fn a_song_from_a_file_wants_the_audio_beside_it() {
        let harness = four_taps_at("/songs/four.json");
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/songs/a.ogg"))
        );
    }

    #[test]
    fn a_new_song_wants_no_audio_until_some_is_dropped_on_it() {
        let mut harness = created();
        assert_eq!(harness.state().audio_file(), None);
        drop_file(&mut harness, "/music/track.ogg");
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/music/track.ogg"))
        );
    }

    #[test]
    fn play_after_an_audio_load_failure_explains_how_to_try_again() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .audio_failed(Path::new("/songs/a.ogg"), "unsupported codec");
        harness.run();
        harness.get_by_label("Dismiss").click();
        harness.run();
        space(&mut harness);
        assert!(
            harness
                .query_by_label_contains("Choose another audio file")
                .is_some()
        );
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn space_plays_from_the_cursor_and_pauses() {
        let mut harness = playable();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        space(&mut harness);
        let Request::Play { from, rate, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert!((from - 0.125).abs() < 1e-9, "{from}");
        assert!((rate - 1.0).abs() < f64::EPSILON);
        space(&mut harness);
        assert_eq!(request(&mut harness), Request::Pause);
    }

    #[test]
    fn space_before_the_audio_is_ready_says_so() {
        let mut harness = four_taps_at("/songs/four.json");
        space(&mut harness);
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(harness.query_by_label_contains("still loading").is_some());
    }

    #[test]
    fn audio_that_failed_to_load_says_why_and_does_not_play() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .audio_failed(Path::new("/songs/a.ogg"), "no such file");
        harness.run();
        assert!(harness.query_by_label_contains("no such file").is_some());
        space(&mut harness);
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn the_cursor_follows_the_playhead() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playhead(0.5);
        harness.run();
        assert!(cursor_shows(&harness, "0:1:0"));
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn a_lane_key_while_playing_places_on_the_nearest_snap_line_without_seeking() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        // 0.63 s is tick 1209 at 120 BPM; the nearest sixteenth is 1200.
        harness.state_mut().playhead(0.63);
        harness.run();
        assert!(lanes_at(&harness, 1200).is_empty());
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 1200), [0]);
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn a_lane_key_while_playing_places_the_input_latency_before_the_cursor() {
        let mut harness = playable_with(Settings {
            input_latency_ms: 250.0,
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        // Heard at 0.88 s, meant at 0.63 s: tick 1209, the nearest sixteenth 1200.
        harness.state_mut().playhead(0.88);
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 1200), [0]);
    }

    /// Moves the playhead of a playing song to `seconds`, as the host reports each frame.
    fn play_to(harness: &mut Harness<'_, Window>, seconds: f64) {
        harness.state_mut().playhead(seconds);
        harness.run();
    }

    #[test]
    fn a_lane_key_held_while_playing_grows_a_hold_until_it_comes_up() {
        let mut harness = playable();
        space(&mut harness);
        play_to(&mut harness, 0.5);
        harness.key_down(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 0.6);
        assert_eq!(note_at(&harness, 960, 0), Some(tap()));
        play_to(&mut harness, 0.9);
        assert_eq!(note_at(&harness, 960, 0), Some(hold(720)));
        play_to(&mut harness, 1.13);
        harness.key_up(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 1.6);
        assert_eq!(note_at(&harness, 960, 0), Some(hold(1200)));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 960, 0), None);
    }

    #[test]
    fn a_lane_key_let_go_quickly_while_playing_stays_a_tap() {
        let mut harness = playable();
        space(&mut harness);
        play_to(&mut harness, 0.5);
        harness.key_down(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 0.62);
        harness.key_up(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 1.0);
        assert_eq!(note_at(&harness, 960, 0), Some(tap()));
    }

    #[test]
    fn moving_the_cursor_while_playing_seeks_there() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        let Request::Seek(to) = request(&mut harness) else {
            panic!("expected a seek");
        };
        assert!((to - 2.0).abs() < 1e-9, "{to}");
    }

    #[test]
    fn playback_that_ends_stops() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playback_ended();
        harness.run();
        space(&mut harness);
        assert!(matches!(request(&mut harness), Request::Play { .. }));
    }

    #[test]
    fn shift_left_and_right_change_the_playback_rate_without_touching_the_chart() {
        let mut harness = playable_with(Settings {
            keep_pitch: false,
            ..Settings::default()
        });
        let before = harness.state().song.as_ref().unwrap().document().clone();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        harness.run();
        let Request::Rate(rate) = request(&mut harness) else {
            panic!("expected a rate");
        };
        assert!((rate - 0.9).abs() < 1e-9, "{rate}");
        assert!(harness.query_by_label("Speed 90%").is_some());
        let command = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(command, egui::Key::ArrowRight);
        harness.run();
        let Request::Rate(rate) = request(&mut harness) else {
            panic!("expected a rate");
        };
        assert!((rate - 0.91).abs() < 1e-9, "{rate}");
        let after = harness.state().song.as_ref().unwrap().document();
        assert_eq!(after.document(), before.document());
    }

    #[test]
    fn audio_keeping_its_pitch_plays_again_at_a_new_speed() {
        let mut harness = playable();
        space(&mut harness);
        let Request::Play { keep_pitch, .. } = request(&mut harness) else {
            panic!("expected playback");
        };
        assert!(keep_pitch);
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        harness.run();
        let requests = harness.state_mut().take_requests();
        let Some(Request::Play { rate, .. }) = requests.last() else {
            panic!("expected playback again, got {requests:?}");
        };
        assert!((rate - 0.9).abs() < 1e-9, "{rate}");
    }

    #[test]
    fn playback_menu_exposes_the_fine_speed_shortcuts() {
        let mut harness = playable();
        harness.get_by_label("Playback").click();
        harness.run();

        harness.get_by_label_contains("Slower by 1%").click();
        harness.run();

        assert!(harness.query_by_label("Speed 99%").is_some());
    }

    #[test]
    fn the_playback_rate_stays_between_a_tenth_and_four_times() {
        let mut harness = playable();
        for _ in 0..40 {
            harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        }
        harness.run();
        assert!(harness.query_by_label("Speed 10%").is_some());
        space(&mut harness);
        let Request::Play { rate, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert!((rate - 0.1).abs() < 1e-9, "{rate}");
    }

    fn playable_with(settings: Settings) -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness
            .state_mut()
            .audio_ready(Path::new("/songs/a.ogg"), 2.0);
        harness.run();
        harness
    }

    fn clicks_played(harness: &mut Harness<'_, Window>) -> Vec<(f64, Tone)> {
        space(harness);
        let Request::Play { clicks, .. } = request(harness) else {
            panic!("expected play");
        };
        clicks
    }

    fn quieter() -> Volumes {
        Volumes {
            music: Gain::from(0.5),
            ticks: Gain::from(0.25),
            metronome: Gain::from(0.75),
            subdivision: Gain::from(0.5),
        }
    }

    #[test]
    fn playback_plays_at_the_volumes_the_settings_hold() {
        let mut harness = playable_with(Settings {
            volumes: quieter(),
            ..Settings::default()
        });
        space(&mut harness);
        let Request::Play { volumes, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert_eq!(volumes, quieter());
    }

    #[test]
    fn with_no_clicks_the_music_volume_changes_live() {
        let mut harness = playable_with(Settings::default());
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().settings.volumes.music = Gain::from(0.3);
        harness.run();
        assert_eq!(request(&mut harness), Request::Volume(Gain::from(0.3)));
    }

    #[test]
    fn with_clicks_mixed_in_any_volume_change_plays_on_from_the_cursor() {
        let mut harness = playable_with(Settings {
            metronome: true,
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        for change in [
            |volumes: &mut Volumes| volumes.music = Gain::from(0.3),
            |volumes: &mut Volumes| volumes.metronome = Gain::from(0.2),
        ] {
            change(&mut harness.state_mut().settings.volumes);
            harness.run();
            let requests = harness.state_mut().take_requests();
            let wanted = harness.state().settings.volumes;
            assert!(
                matches!(
                    requests.as_slice(),
                    [Request::Pause, Request::Play { volumes, .. }] if *volumes == wanted
                ),
                "{requests:?}"
            );
        }
    }

    #[test]
    fn the_toolbar_and_the_sounds_page_both_set_the_volumes() {
        let mut harness = playable_with(Settings::default());
        harness.get_by_label("Volume").click();
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::Slider, "Music volume")
                .is_some()
        );
        harness.key_press(egui::Key::Escape);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Sounds");
        for name in ["Music volume", "Note tick volume", "Metronome volume"] {
            assert!(
                harness
                    .query_by_role_and_label(Role::Slider, name)
                    .is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn with_no_sounds_chosen_playback_clicks_nothing() {
        let mut harness = playable_with(Settings::default());
        assert_eq!(clicks_played(&mut harness), []);
    }

    #[test]
    fn tuned_lanes_step_a_whole_tone_from_the_middle_and_span_an_octave_at_most() {
        let at = |index: u16, width: u16, lanes: u16| {
            let position = charter_core::Position::Lane {
                index: LaneIndex::new(index),
                width: LaneCount::new(width).unwrap(),
            };
            lane_cents(position, lanes)
        };
        let row = |lanes: u16| {
            (0..lanes)
                .map(|lane| at(lane, 1, lanes))
                .collect::<Vec<_>>()
        };
        assert_eq!(row(1), [0]);
        assert_eq!(row(3), [-200, 0, 200]);
        assert_eq!(row(4), [-300, -100, 100, 300]);
        let wide = row(13);
        assert_eq!(
            (wide.first(), wide.get(6), wide.last()),
            (Some(&-600), Some(&0), Some(&600))
        );
        assert_eq!(at(0, 2, 4), -200);
    }

    #[test]
    fn shift_f4_tunes_the_note_ticks_by_lane() {
        let mut harness = playable_with(Settings {
            tick_sound: Sound::Clap,
            ..Settings::default()
        });
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F4);
        harness.run();
        assert!(harness.state().settings().tuned_lanes);
        let clicks = clicks_played(&mut harness);
        let tones: Vec<_> = clicks.iter().map(|(_, tone)| *tone).collect();
        let clap = |cents| charter_audio::Tick::Clap.pitched(cents);
        assert_eq!(tones, [clap(-300), clap(-100), clap(100), clap(300)]);
        space(&mut harness);
        harness.get_by_label("Tuned lanes").click();
        harness.run();
        assert!(!harness.state().settings().tuned_lanes);
    }

    #[test]
    fn the_tick_sound_plays_on_every_note_even_before_the_cursor() {
        let mut harness = playable_with(Settings {
            tick_sound: Sound::Beep,
            ..Settings::default()
        });
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        let clicks = clicks_played(&mut harness);
        let times: Vec<f64> = clicks.iter().map(|(at, _)| *at).collect();
        assert_eq!(times, [0.0, 0.125, 0.25, 0.375]);
        assert!(
            clicks
                .iter()
                .all(|(_, tone)| *tone == charter_audio::Tick::Beep.tone())
        );
    }

    #[test]
    fn the_metronome_plays_on_every_beat_until_the_audio_ends_accenting_each_measure() {
        let mut harness = playable_with(Settings {
            metronome: true,
            metronome_sound: Kit::PureSine,
            ..Settings::default()
        });
        let clicks = clicks_played(&mut harness);
        let times: Vec<f64> = clicks.iter().map(|(at, _)| *at).collect();
        assert_eq!(times, [0.0, 0.5, 1.0, 1.5, 2.0]);
        let tones: Vec<Tone> = clicks.iter().map(|(_, tone)| *tone).collect();
        let (accent, beat) = (Tone::Accent(Kit::PureSine), Tone::Beat(Kit::PureSine));
        assert_eq!(tones, [accent, beat, beat, beat, accent]);
    }

    #[test]
    fn the_metronome_follows_every_timing_point_offset_tempo_meter_and_stop() {
        let song = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Timed", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.5, "events": [
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 3840, "kind": { "bpm": 240.0 } },
                { "tick": 3840, "kind": { "time_signature": [3, 4] } },
                { "tick": 4800, "kind": { "stop": 0.5 } }
            ] },
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let settings = Settings {
            metronome: true,
            metronome_sound: Kit::PureSine,
            ..Settings::default()
        };
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open(
            Path::new("/songs/timed.json"),
            &serde_json::to_vec(&song).unwrap(),
        );
        let mut harness = window(app);
        harness
            .state_mut()
            .audio_ready(Path::new("/songs/a.ogg"), 4.2);
        harness.run();
        let clicks = clicks_played(&mut harness);
        let (a, b) = (Tone::Accent(Kit::PureSine), Tone::Beat(Kit::PureSine));
        // 120 BPM in 4/4 from 0.5 s, its grid carried back into the lead-in; from 2.5 s,
        // 240 BPM in 3/4, with half a second stopped after the beat at 2.75 s.
        let expected = [
            (0.0, b),
            (0.5, a),
            (1.0, b),
            (1.5, b),
            (2.0, b),
            (2.5, a),
            (2.75, b),
            (3.5, b),
            (3.75, a),
            (4.0, b),
        ];
        assert_eq!(clicks.len(), expected.len(), "{clicks:?}");
        for ((at, tone), (want_at, want_tone)) in clicks.iter().zip(expected) {
            assert!((at - want_at).abs() < 1e-9, "{clicks:?}");
            assert_eq!(*tone, want_tone, "{clicks:?}");
        }
    }

    /// Four taps whose two seconds of audio have loaded, with the timing window open.
    fn listening(settings: Settings) -> Harness<'static, Window> {
        let mut harness = playable_with(settings);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        harness.state_mut().take_requests();
        harness
    }

    /// Where a play started, what it loops and what clicks over it.
    type Played = (f64, Option<(f64, f64)>, Vec<(f64, Tone)>);

    fn played(harness: &mut Harness<'_, Window>) -> Played {
        let requests = harness.state_mut().take_requests();
        let play = requests
            .into_iter()
            .rev()
            .find_map(|request| match request {
                Request::Play {
                    from,
                    looping,
                    clicks,
                    ..
                } => Some((from, looping, clicks)),
                _ => None,
            });
        play.expect("a play")
    }

    #[test]
    fn l_loops_the_bar_under_the_cursor_with_the_metronome() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::L);
        harness.run();
        let (from, looping, clicks) = played(&mut harness);
        assert!(from.abs() < 1e-9);
        assert_eq!(looping, Some((0.0, 2.0)));
        assert!(
            clicks
                .iter()
                .any(|(_, tone)| matches!(tone, Tone::Accent(_)))
        );
        harness.key_press(egui::Key::L);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [Request::Pause]);
    }

    #[test]
    fn an_edit_while_looping_plays_the_loop_again_from_its_start() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::L);
        harness.run();
        played(&mut harness);
        harness.key_press(egui::Key::Plus);
        harness.run();
        let (from, looping, _) = played(&mut harness);
        assert!(from.abs() < 1e-9);
        let (_, end) = looping.expect("looping");
        assert!(
            (end - 2.0).abs() > 1e-3,
            "the bar follows the new tempo: {end}"
        );
    }

    #[test]
    fn r_switches_to_the_alternate_speed_and_back() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::R);
        harness.run();
        assert!((harness.state().rate.factor() - 0.6).abs() < 1e-9);
        harness.key_press(egui::Key::R);
        harness.run();
        assert!((harness.state().rate.factor() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_metronome_following_the_grid_clicks_between_beats_too() {
        let mut harness = listening(Settings {
            metronome: true,
            metronome_grid: true,
            ..Settings::default()
        });
        harness.key_press(egui::Key::Num6);
        harness.run();
        space(&mut harness);
        let (_, _, clicks) = played(&mut harness);
        let subs = clicks
            .iter()
            .filter(|(_, tone)| matches!(tone, Tone::Sub(_)))
            .count();
        let beats = clicks
            .iter()
            .filter(|(_, tone)| matches!(tone, Tone::Beat(_)))
            .count();
        assert_eq!((beats, subs), (3, 4), "{clicks:?}");
    }

    #[test]
    fn c_toggles_the_metronome_and_shift_c_mutes_it_while_held() {
        let mut harness = listening(Settings {
            metronome: true,
            ..Settings::default()
        });
        space(&mut harness);
        played(&mut harness);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        harness.key_down_modifiers(egui::Modifiers::SHIFT, egui::Key::C);
        harness.run();
        let (_, _, muted) = played(&mut harness);
        assert!(muted.is_empty(), "{muted:?}");
        harness.key_up_modifiers(egui::Modifiers::SHIFT, egui::Key::C);
        harness.run();
        let (_, _, back) = played(&mut harness);
        assert!(!back.is_empty());
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.key_press(egui::Key::C);
        harness.run();
        assert!(!harness.state().settings().metronome);
    }

    #[test]
    fn calibrating_latency_plays_clicks_at_the_songs_first_tempo() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Sounds");
        harness.get_by_label("Calibrate…").scroll_to_me();
        harness.run();
        harness.get_by_label("Calibrate…").click();
        harness.run();
        harness.get_by_label("Start").click();
        // The flash keeps the window drawing while the clicks play.
        harness.run_steps(2);
        let requests = harness.state_mut().take_requests();
        let Some(Request::Preview { clicks, .. }) = requests.last() else {
            panic!("expected clicks, got {requests:?}");
        };
        assert_eq!(clicks.len(), 32);
        let gap = clicks.get(1).unwrap().0 - clicks.first().unwrap().0;
        assert!(
            (gap - 0.4).abs() < 1e-9,
            "150 BPM, the demo's first tempo: {gap}"
        );
        assert!(harness.query_by_label_contains("of 16 taps").is_some());
    }

    #[test]
    fn the_cursor_shows_what_is_heard_by_the_calibrated_latency() {
        let mut harness = playable_with(Settings {
            audio_latency_ms: 250.0,
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playhead(0.75);
        harness.run();
        assert!(cursor_shows(&harness, "0:1:0"));
    }

    #[test]
    fn the_cursor_scales_calibrated_latency_to_slowed_audio_time() {
        let mut harness = playable_with(Settings {
            audio_latency_ms: 250.0,
            ..Settings::default()
        });
        harness.state_mut().set_rate(Rate::of(0.5));
        space(&mut harness);
        let Request::Play { rate, .. } = request(&mut harness) else {
            panic!("expected playback");
        };
        assert!((rate - 0.5).abs() < f64::EPSILON);

        harness.state_mut().playhead(0.75);
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().cursor(),
            Tick::new(1200)
        );
    }

    #[test]
    fn the_toolbar_draws_grid_lines_between_the_beats() {
        let mut harness = editable_demo();
        harness
            .get_by_role_and_label(Role::ComboBox, "Grid lines")
            .click();
        harness.run();
        harness.get_by_label("16th notes").click();
        harness.run();
        assert_eq!(harness.state().settings().grid.get(), 16);
        let grid = harness.get_by_role_and_label(Role::ComboBox, "Grid lines");
        assert_eq!(grid.value().as_deref(), Some("16th notes"));
    }

    #[test]
    fn f3_and_f4_turn_the_metronome_and_the_note_ticks_on_and_off() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F3);
        harness.key_press(egui::Key::F4);
        harness.run();
        let settings = harness.state().settings();
        assert_eq!(
            (settings.metronome, settings.tick_sound),
            (true, Sound::Click)
        );
        harness.key_press(egui::Key::F3);
        harness.run();
        assert!(!harness.state().settings().metronome);
    }

    #[test]
    fn saving_a_song_into_another_folder_plans_its_audio_copy() {
        let mut harness = four_taps_at("/songs/four.json");
        let path = PathBuf::from("/elsewhere/four.json");
        assert_eq!(
            harness.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/songs/a.ogg"),
                PathBuf::from("/elsewhere/a.ogg"),
            ))
        );
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn saving_elsewhere_keeps_the_audios_relative_subdirectory() {
        let mut document: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
        document["metadata"]["audio"] = serde_json::json!("assets/track.ogg");
        let mut app = Window::new(PLUGINS);
        app.open(
            Path::new("/songs/four.json"),
            &serde_json::to_vec(&document).unwrap(),
        );
        let mut harness = window(app);

        harness.key_press_modifiers(
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
            egui::Key::S,
        );
        harness.run();
        let Request::SaveAs { bytes, .. } = request(&mut harness) else {
            panic!("expected Save As");
        };
        let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["metadata"]["audio"], "assets/track.ogg");
        let path = PathBuf::from("/elsewhere/four.json");
        assert_eq!(
            harness.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/songs/assets/track.ogg"),
                PathBuf::from("/elsewhere/assets/track.ogg"),
            ))
        );
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn a_host_can_preflight_audio_without_marking_a_failed_save_as_saved() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::SaveAs { .. }));

        let original = harness.state().audio_file();
        assert_eq!(
            harness
                .state()
                .audio_copy_for(Path::new("/songs/mine.json")),
            Some((
                PathBuf::from("/music/track.ogg"),
                PathBuf::from("/songs/track.ogg"),
            ))
        );
        assert_eq!(harness.state().audio_file(), original);
        harness
            .state_mut()
            .saved(Saved::Failed("the copy failed".to_owned()));
        harness.run();
        assert_eq!(harness.state().audio_file(), original);
        assert!(harness.state().song.as_ref().unwrap().is_modified());
        assert_eq!(harness.state().path, None);
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn a_confirmed_save_after_audio_preflight_queues_no_second_copy() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::SaveAs { .. }));
        let path = PathBuf::from("/songs/mine.json");
        assert!(harness.state().audio_copy_for(&path).is_some());

        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/songs/track.ogg"))
        );
        assert!(!harness.state().song.as_ref().unwrap().is_modified());
    }

    #[test]
    fn saving_a_song_in_its_own_folder_copies_nothing() {
        let mut harness = four_taps_at("/songs/four.json");
        let path = PathBuf::from("/songs/renamed.json");
        assert_eq!(harness.state().audio_copy_for(&path), None);
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn audio_that_moves_under_the_song_stops_playing_until_it_loads_again() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("/elsewhere/four.json")));
        harness.run();
        harness.state_mut().take_requests();
        space(&mut harness);
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(harness.query_by_label_contains("still loading").is_some());
    }

    #[test]
    fn ctrl_dragging_from_a_note_stretches_it_rather_than_removing_it() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 240, 0.0), spot(&harness, 1, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 1), Some(hold(480)));
    }

    #[test]
    fn opening_another_song_while_playing_pauses_first() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().open_demo();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [Request::Pause]);
        assert!(!harness.state().is_playing());
    }

    #[test]
    fn saving_while_playing_saves_and_keeps_playing() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.state_mut().take_requests();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        assert!(harness.state().is_playing());
    }

    #[test]
    fn a_restore_offered_while_starting_a_song_can_still_be_answered() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = new_song_dialog();
        harness.state_mut().recover(&bytes);
        harness.run();
        harness.get_by_label("Restore").click();
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    #[test]
    fn audio_whose_name_cannot_be_a_song_path_is_refused() {
        let mut harness = created();
        drop_file(&mut harness, "/music/a\\b.ogg");
        assert_eq!(audio(&harness), NO_AUDIO);
        assert!(
            harness
                .query_by_label_contains("cannot be a song's audio")
                .is_some()
        );
    }

    #[test]
    fn audio_dropped_then_undone_is_not_copied_on_save() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("/songs/mine.json")));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn lowering_the_tempo_below_nothing_says_so() {
        let mut harness = editable_demo();
        enter_bpm(&mut harness, "0.5", egui::Key::Enter);
        harness.key_press(egui::Key::F7);
        harness.run();
        assert!(harness.query_by_label_contains("is not a tempo").is_some());
        assert_eq!(bpm_at_cursor(&harness), Some((0, "0.5".to_owned())));
    }

    #[test]
    fn a_box_and_a_dragged_hold_work_however_the_notefield_runs() {
        for direction in Direction::ALL {
            let mut harness = turned_demo(direction);
            let (from, to) = (turned_spot(&harness, 0, 480), turned_spot(&harness, 0, 960));
            drag(&mut harness, from, to, egui::Modifiers::COMMAND);
            assert_eq!(note_at(&harness, 480, 0), Some(hold(480)), "{direction:?}");
            let corner = |lane, tick| turned_spot(&harness, lane, tick);
            let (a, b) = (corner(0, 470), corner(3, 490));
            let (a, b) = (
                a + (a - b).normalized() * 30.0,
                b + (b - a).normalized() * 30.0,
            );
            drag(&mut harness, a, b, egui::Modifiers::NONE);
            let song = harness.state().song.as_ref().unwrap();
            assert!(song.selection().count() >= 1, "{direction:?}");
        }
    }

    #[test]
    fn the_empty_window_opens_a_song_from_a_file() {
        let mut harness = window(Window::new(GAMES));
        harness.get_by_label("Open song…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }

    #[test]
    fn a_song_whose_audio_cannot_be_found_says_how_to_play_it() {
        let mut harness = editable_demo();
        space(&mut harness);
        assert!(
            harness
                .query_by_label_contains("drop demo.ogg on the window")
                .is_some()
        );
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

    #[test]
    fn a_new_song_asks_the_host_to_choose_its_audio() {
        let mut harness = created();
        harness.get_by_label("Choose audio…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseAudio);
        harness.state_mut().use_audio(Path::new("/music/track.ogg"));
        harness.run();
        assert_eq!(audio(&harness), "track.ogg");
        assert!(!no_audio(&harness));
    }

    #[test]
    fn where_files_cannot_be_dropped_the_window_offers_choosing_them_and_says_why() {
        let mut harness = window(Window::new(GAMES).without_file_drops());
        assert!(
            harness
                .query_by_label_contains("drop a charter song")
                .is_none()
        );
        harness.get_by_label("New song").click();
        harness.run();
        assert!(harness.query_by_label_contains("Drop an audio").is_none());
        assert!(
            harness
                .query_by_label_contains("doesn't work under Wayland yet")
                .is_some()
        );
        harness.get_by_label("Create").click();
        harness.run();
        assert!(harness.query_by_label_contains("Drop an audio").is_none());
        harness.get_by_label("Choose audio…").click();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [Request::ChooseAudio]);
        harness.get_by_label("File").click();
        harness.run();
        // The banner's button, and now the File menu's.
        assert_eq!(harness.query_all_by_label("Choose audio…").count(), 2);
    }

    #[test]
    fn the_new_song_dialog_says_where_the_audio_comes_from() {
        let harness = new_song_dialog();
        assert!(
            harness
                .query_by_label_contains("Drop an audio file here")
                .is_some()
        );
    }

    #[test]
    fn saving_opening_and_help_work_while_a_field_has_the_keyboard() {
        let mut harness = editable_four_taps();
        bpm_field(&harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness.key_press(egui::Key::Space);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(!harness.state().is_playing());
    }

    #[test]
    fn the_timing_menu_adds_a_bookmark_and_jumps_between_sections() {
        let mut harness = editable_demo();
        let before = marker_texts(&harness).len();
        harness.get_by_label("Timing").click();
        harness.run();
        harness
            .get_by_label_contains("Add or remove a bookmark")
            .click();
        harness.run();
        assert_eq!(marker_texts(&harness).len(), before + 1);
        harness.get_by_label("Timing").click();
        harness.run();
        harness.get_by_label_contains("Next section").click();
        harness.run();
        assert!(cursor_shows(&harness, "8:0:0"));
    }

    #[test]
    fn the_edit_menu_selects_and_deletes() {
        let mut harness = editable_four_taps();
        harness.get_by_label("Edit").click();
        harness.run();
        harness.get_by_label_contains("Select all").click();
        harness.run();
        harness.get_by_label("Edit").click();
        harness.run();
        harness
            .get_by_label_contains("Delete the selection")
            .click();
        harness.run();
        assert_eq!(note_count(&harness), 0);
    }

    #[test]
    fn the_shortcuts_filter_to_what_is_typed() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F1);
        harness.run();
        let filter = harness.get_by_role_and_label(Role::TextInput, "Filter");
        filter.focus();
        filter.type_text("bookmark");
        harness.run();
        assert!(harness.query_by_label("Ctrl+B").is_some());
        assert!(harness.query_by_label("Ctrl+Z").is_none());
    }

    #[test]
    fn the_status_bar_says_how_the_audio_stands() {
        let harness = created();
        assert!(harness.query_by_label("No audio").is_some());
        let harness = four_taps_at("/songs/four.json");
        assert!(harness.query_by_label("Audio loading").is_some());
        let mut harness = playable();
        assert!(harness.query_by_label("Audio ready").is_some());
        space(&mut harness);
        assert!(harness.query_by_label("Playing").is_some());
    }

    /// Four lanes at 120 BPM that change to 150 BPM at the second beat, with a section at the
    /// first eighth.
    fn timed_song() -> Harness<'static, Window> {
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

    fn field(harness: &Harness<'_, Window>) -> notefield::Field {
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
    fn on_line(harness: &Harness<'_, Window>, x: f32, tick: i32) -> egui::Pos2 {
        let field = field(harness);
        let song = harness.state().song.as_ref().unwrap();
        let y = field.view.y(song.timing().time_at(Tick::new(tick)));
        egui::pos2(x, y)
    }

    fn event_ticks(harness: &Harness<'_, Window>) -> Vec<Tick> {
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
    fn syncing() -> Harness<'static, Window> {
        let mut harness = timed_song();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        harness.state_mut().sync.layout = charter_timing::rows::Layout::Bars;
        harness.run();
        harness
    }

    #[test]
    fn the_timing_window_opens_on_rows_of_seconds() {
        let mut harness = timed_song();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
        assert!(harness.query_by_label("Measure 0").is_none());
    }

    #[test]
    fn the_timing_windows_start_bpm_sets_the_first_anchors_tempo() {
        let mut harness = syncing();
        let start = harness.get_by_role_and_label(Role::TextInput, "Start BPM");
        let before = start.value();
        start.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Start BPM")
            .type_text("140");
        harness.key_press(egui::Key::Enter);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(
            song.timing().bpm_at(Tick::new(0)).map(f64::from),
            Some(140.0)
        );
        assert_ne!(before.as_deref(), Some("140"));
    }

    /// Where `tick` lies across its bar's row in the timing window, a bar a row with a tenth of
    /// the bar before showing ahead of its downbeat.
    fn in_row(harness: &Harness<'_, Window>, tick: i32) -> egui::Pos2 {
        let song = harness.state().song.as_ref().unwrap();
        let measure = song.timing().measures_from(Tick::new(tick)).next().unwrap();
        let row = harness
            .get_by_label(&format!("Measure {}", measure.number))
            .rect();
        let lead = charter_timing::rows::LEAD_IN;
        #[allow(clippy::cast_possible_truncation)] // a fraction of a row
        let across = ((measure.fraction(Tick::new(tick)) + lead) / (1.0 + lead)) as f32;
        egui::pos2(row.left() + across * row.width(), row.top() + 30.0)
    }

    fn seconds_at(harness: &Harness<'_, Window>, tick: i32) -> f64 {
        let song = harness.state().song.as_ref().unwrap();
        song.timing().time_at(Tick::new(tick)).seconds()
    }

    fn anchor_seconds(harness: &Harness<'_, Window>) -> Vec<f64> {
        let song = harness.state().song.as_ref().unwrap();
        song.anchors().iter().map(|pin| pin.seconds).collect()
    }

    fn reset_from_the_menu(harness: &mut Harness<'_, Window>) {
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Reset timing…").click();
        harness.run();
    }

    fn find_from_the_menu(harness: &mut Harness<'_, Window>) {
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Find timing from the audio…").click();
        harness.run();
    }

    #[test]
    fn finding_timing_asks_the_host_to_find_it_in_the_songs_audio() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::FindTiming {
                file: PathBuf::from("/songs/a.ogg"),
                ticks_per_beat: charter_core::DEFAULT_TICKS_PER_BEAT,
            }]
        );
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_some()
        );
    }

    #[test]
    fn the_weights_are_downloaded_only_once_the_user_says_so() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().weights_needed();
        harness.run();
        assert!(harness.query_by_label_contains("81 MB").is_some());
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_none()
        );

        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().weights_needed();
        harness.run();
        harness.get_by_label("Download").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::DownloadWeights]
        );
        harness.state_mut().weights_progress(0.42);
        harness.run();
        assert!(harness.query_by_label_contains("42%").is_some());
    }

    #[test]
    fn timing_found_takes_the_songs_place_in_one_step_and_says_what_it_is() {
        let mut harness = playable();
        find_from_the_menu(&mut harness);
        harness.state_mut().take_requests();
        let before = anchor_seconds(&harness);
        let found: charter_core::Timing = serde_json::from_value(serde_json::json!({
            "anchors": [ { "tick": 0, "seconds": 3.616 } ],
            "tempo": 132.0,
            "events": []
        }))
        .unwrap();
        harness.state_mut().timing_found(Ok(found));
        harness.run();
        assert_eq!(anchor_seconds(&harness), [3.616]);
        assert!(
            harness
                .query_by_label_contains("Timing found: 132 BPM")
                .is_some()
        );
        assert!(
            harness
                .query_by_label_contains("Finding the timing")
                .is_none()
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(anchor_seconds(&harness), before);
        harness
            .state_mut()
            .timing_found(Err("the tracker would not load".to_owned()));
        harness.run();
        assert!(harness.query_by_label_contains("would not load").is_some());
    }

    #[test]
    fn resetting_the_timing_asks_first_then_leaves_120_bpm_from_zero_in_one_step() {
        let mut harness = editable_demo();
        let before = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .timing()
            .events
            .clone();
        reset_from_the_menu(&mut harness);
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().timing().events,
            before
        );
        reset_from_the_menu(&mut harness);
        harness.get_by_label("Reset").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.timing().tempo.to_string(), "120");
        assert_eq!(
            serde_json::to_value(song.timing().anchors[0].seconds).unwrap(),
            serde_json::json!(0.0)
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().timing().events,
            before
        );
    }

    #[test]
    fn v_lays_the_rows_out_in_seconds_and_a_click_anchors_the_moment_under_it() {
        let mut harness = syncing();
        harness.key_press(egui::Key::V);
        harness.run();
        assert!(harness.query_by_label("Measure 0").is_none());
        let row = harness.get_by_label("Row at 0.0 s").rect();
        // Two seconds a row, a tenth of that from before it: 0.95 s is 1.15 of 2.2 across.
        #[allow(clippy::cast_possible_truncation)]
        let across = (1.15 / 2.2) as f32;
        let at = egui::pos2(row.left() + across * row.width(), row.top() + 30.0);
        click(&mut harness, at, egui::Modifiers::NONE);
        let seconds = anchor_seconds(&harness);
        assert_eq!(seconds.len(), 3, "{seconds:?}");
        assert!((seconds[2] - 0.95).abs() < 5e-3, "{seconds:?}");
        assert!(
            (seconds[1] - 0.5).abs() < 1e-9,
            "the other anchors keep their moments"
        );
        harness.key_press(egui::Key::V);
        harness.run();
        assert!(harness.query_by_label("Measure 0").is_some());
    }

    #[test]
    fn ctrl_t_opens_and_closes_the_timing_window() {
        let mut harness = timed_song();
        assert!(harness.query_by_label("Row at 0.0 s").is_none());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        assert!(harness.query_by_label("Row at 0.0 s").is_none());
    }

    #[test]
    fn clicking_a_row_adds_an_anchor_pinning_the_nearest_grid_line_to_the_moment_clicked() {
        let mut harness = syncing();
        let heard = seconds_at(&harness, 1950);
        let near = in_row(&harness, 1950);
        click(&mut harness, near, egui::Modifiers::NONE);
        assert_eq!(
            event_ticks(&harness),
            [Tick::new(0), Tick::new(960), Tick::new(1920)]
        );
        assert!((seconds_at(&harness, 1920) - heard).abs() < 5e-3);
        assert!((seconds_at(&harness, 960) - 0.5).abs() < 1e-9);
    }

    /// The timed song with its audio loaded and one strong onset `seconds` in.
    fn syncing_with_an_onset(seconds: f64) -> Harness<'static, Window> {
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

    #[test]
    fn a_click_ten_ms_from_an_onset_anchors_on_it_and_auditions_a_beat_before() {
        // Tick 1920 falls 0.9 s in; the onset is a little later.
        let mut harness = syncing_with_an_onset(0.912);
        let pos = in_row(&harness, 1920);
        assert!((seconds_at(&harness, 1920) - 0.9).abs() < 1e-9);
        click(&mut harness, pos, egui::Modifiers::NONE);
        let seconds = anchor_seconds(&harness);
        assert_eq!(event_ticks(&harness)[2], Tick::new(1920));
        assert!((seconds[2] - 0.912).abs() < 1e-4, "{seconds:?}");
        let requests = harness.state_mut().take_requests();
        let from = requests.iter().find_map(|request| match request {
            Request::Play { from, .. } => Some(*from),
            _ => None,
        });
        assert!((from.expect("an audition") - (0.912 - 0.4)).abs() < 1e-6);
    }

    #[test]
    fn a_click_forty_ms_from_any_onset_stays_where_it_was_clicked() {
        let mut harness = syncing_with_an_onset(0.94);
        let heard = seconds_at(&harness, 1920);
        let pos = in_row(&harness, 1920);
        click(&mut harness, pos, egui::Modifiers::NONE);
        let seconds = anchor_seconds(&harness);
        assert!((seconds[2] - heard).abs() < 5e-3, "{seconds:?}");
    }

    #[test]
    fn an_anchor_is_not_added_within_half_a_grid_step_of_another_and_says_why() {
        let mut harness = syncing();
        let near = in_row(&harness, 1150);
        click(&mut harness, near, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        assert!(
            harness
                .query_by_label_contains("Too close to the anchor")
                .is_some()
        );
    }

    #[test]
    fn dragging_an_anchor_moves_it_on_the_grid_keeping_its_moment() {
        let mut harness = syncing();
        let (from, to) = (in_row(&harness, 960), in_row(&harness, 1900));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(1920)]);
        assert!((seconds_at(&harness, 1920) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ctrl_dragging_an_anchor_moves_it_in_the_audio_keeping_its_beat() {
        let mut harness = syncing();
        let heard = seconds_at(&harness, 1200);
        let (from, to) = (in_row(&harness, 960), in_row(&harness, 1200));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        assert!((seconds_at(&harness, 960) - heard).abs() < 5e-3);
    }

    #[test]
    fn an_anchor_dragged_into_its_neighbour_stops_short_of_it_in_one_step() {
        let mut harness = syncing();
        // Into the tenth of the bar before, shown ahead of the first downbeat.
        let (from, to) = (
            in_row(&harness, 960),
            in_row(&harness, 0) - egui::vec2(30.0, 0.0),
        );
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        let seconds = anchor_seconds(&harness);
        assert!(seconds[1] > seconds[0] && seconds[1] < 0.01, "{seconds:?}");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn dragging_across_a_row_selects_the_anchors_inside_and_delete_removes_them() {
        let mut harness = syncing();
        let (from, to) = (in_row(&harness, 200), in_row(&harness, 1500));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        harness.key_press(egui::Key::Delete);
        harness.run();
        assert_eq!(event_ticks(&harness), [Tick::new(0)]);
    }

    #[test]
    fn a_double_click_on_an_anchor_opens_its_moment_to_type_not_a_removal() {
        let mut harness = syncing();
        harness
            .ctx
            .options_mut(|options| options.input_options.max_double_click_delay = 2.0);
        let at = in_row(&harness, 960);
        harness.hover_at(at);
        for _ in 0..2 {
            press(&harness, at, true, egui::Modifiers::NONE);
            press(&harness, at, false, egui::Modifiers::NONE);
            harness.step();
        }
        harness.run();
        harness.run();
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        let field = harness.get_by_role_and_label(Role::TextInput, "t");
        assert_eq!(field.value().as_deref(), Some("0.5000"));
        field.type_text("0.6");
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "t")
            .type_text("0.6");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn ctrl_wheel_nudges_the_focused_anchor_by_a_millisecond() {
        let mut harness = syncing();
        let at = in_row(&harness, 960);
        click(&mut harness, at, egui::Modifiers::NONE);
        harness.hover_at(in_row(&harness, 1000));
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            modifiers: egui::Modifiers::COMMAND,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
        assert!((seconds_at(&harness, 960) - 0.501).abs() < 1e-6);
    }

    #[test]
    fn twenty_arrow_presses_nudge_twenty_milliseconds_as_one_step() {
        let mut harness = syncing();
        let at = in_row(&harness, 960);
        click(&mut harness, at, egui::Modifiers::NONE);
        for _ in 0..20 {
            harness.key_press(egui::Key::ArrowRight);
        }
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.52).abs() < 1e-6);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!((anchor_seconds(&harness)[1] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn number_keys_set_the_grid_while_the_timing_window_has_the_keyboard() {
        let mut harness = syncing();
        let at = in_row(&harness, 960);
        click(&mut harness, at, egui::Modifiers::NONE);
        harness.key_press(egui::Key::Num8);
        harness.run();
        assert_eq!(harness.state().sync.grid.resolution.get(), 16);
        assert!(lanes_at(&harness, 0).is_empty());
        let grid = harness.get_by_role_and_label(Role::ComboBox, "Grid");
        assert_eq!(grid.value().as_deref(), Some("1/16"));
    }

    #[test]
    fn the_end_of_a_row_in_the_timing_window_is_clear_of_its_scroll_bar() {
        // The demo runs to more bars than the window shows, so its rows scroll.
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
        harness.run();
        harness.state_mut().sync.layout = charter_timing::rows::Layout::Bars;
        harness.run();
        let heard = seconds_at(&harness, 3830);
        let at = in_row(&harness, 3830);
        click(&mut harness, at, egui::Modifiers::NONE);
        assert!(
            event_ticks(&harness).contains(&Tick::new(3840)),
            "{:?}",
            event_ticks(&harness)
        );
        assert!((seconds_at(&harness, 3840) - heard).abs() < 5e-3);
    }

    /// Reads `offset,bpm` as a timing, standing in for a game's timing format.
    #[derive(Debug)]
    struct Importer;

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
    struct ContextImporter;

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

    const IMPORTING: &[&dyn RulesetPlugin] = &[&AnySm, &Importer];
    const CONTEXT_IMPORTING: &[&dyn RulesetPlugin] = &[
        &charter_stepmania::Plugin,
        &charter_ttr::Plugin,
        &ContextImporter,
    ];

    fn importing() -> Harness<'static, Window> {
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

    #[test]
    fn reopening_import_after_opening_another_game_previews_for_that_song() {
        let draft = charter_core::NewSong {
            metadata: charter_editor::demo::song().document().metadata.clone(),
            layout: 0,
            charts: vec![charter_core::NewChart::named("Easy")],
        };
        let first = charter_stepmania::Plugin.new_song(&draft).unwrap();
        let second = charter_ttr::Plugin.new_song(&draft).unwrap();
        let mut app = Window::new(CONTEXT_IMPORTING);
        app.open(
            Path::new("/songs/stepmania.json"),
            &first.to_json().unwrap(),
        );
        let mut harness = window(app);

        harness.state_mut().timing_file_read("0.895");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("150 BPM at the start")
                .is_some()
        );
        harness.get_by_label("Close window").click();
        harness.run();
        harness
            .state_mut()
            .open(Path::new("/songs/ttr.json"), &second.to_json().unwrap());
        harness.run();
        harness.query_all_by_label("Timing").next().unwrap().click();
        harness.run();
        harness.get_by_label("Import timing…").click();
        harness.run();

        assert!(
            harness
                .query_by_label_contains("208.1 BPM at the start")
                .is_some()
        );
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "208.1".to_owned())));
    }

    #[test]
    fn imported_timing_shows_what_it_holds_and_replaces_the_songs_in_one_step() {
        let mut harness = importing();
        assert!(harness.query_by_label_contains("test timing").is_some());
        harness.state_mut().timing_file_read("0.895,208.1");
        harness.run();
        assert!(harness.query_by_label_contains("208.1 BPM").is_some());
        assert!(harness.query_by_label_contains("895 ms").is_some());
        assert!(harness.query_by_label_contains("kiai time").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "208.1".to_owned())));
        assert_eq!(offset(&harness), serde_json::json!(0.895));
        assert!(harness.query_by_label("Replace the timing").is_none());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }

    #[test]
    fn timing_that_does_not_read_says_why_and_imports_nothing() {
        let mut harness = importing();
        harness.state_mut().timing_file_read("nonsense");
        harness.run();
        assert!(harness.query_by_label_contains("no comma").is_some());
        harness.get_by_label("Replace the timing").click();
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }

    #[test]
    fn the_import_window_reads_timing_from_a_file_the_host_opens() {
        let mut harness = importing();
        harness.get_by_label("Read a file…").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::ChooseTimingFile]
        );
    }

    #[test]
    fn the_import_window_can_be_cancelled_without_importing() {
        let mut harness = importing();
        harness.state_mut().timing_file_read("0.895,208.1");
        harness.run();

        harness.get_by_label("Cancel").click();
        harness.run();

        assert!(harness.query_by_label("Replace the timing").is_none());
        assert_eq!(bpm_at_cursor(&harness), Some((0, "150".to_owned())));
    }

    fn moments(harness: &Harness<'_, Window>) -> Vec<f64> {
        let song = harness.state().song.as_ref().unwrap();
        song.anchors().iter().map(|pin| pin.seconds).collect()
    }

    #[test]
    fn dragging_a_tempo_point_moves_it_to_the_moment_under_the_pointer() {
        let mut harness = timed_song();
        let before = moments(&harness);
        let heard = seconds_at(&harness, 1920);
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        let after = moments(&harness);
        assert!((after[1] - heard).abs() < 0.01, "{after:?} {heard}");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(moments(&harness), before);
    }

    #[test]
    fn a_bpm_change_and_a_meter_at_one_tick_drag_as_one_timing_point() {
        let mut harness = timed_song();
        let meter = serde_json::json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
        let mut document = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .document()
            .document()
            .clone();
        document
            .timing
            .events
            .push(serde_json::from_value(meter).unwrap());
        let bytes = serde_json::to_vec(&document).unwrap();
        harness.state_mut().open(Path::new("timed.json"), &bytes);
        harness.run();
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(
            event_ticks(&harness),
            [Tick::new(0), Tick::new(960), Tick::new(960)]
        );
    }

    #[test]
    fn dragging_the_first_tempo_point_moves_the_whole_grid() {
        let mut harness = timed_song();
        let before = moments(&harness);
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 0), on_line(&harness, x, 480));
        let later = seconds_at(&harness, 480) - seconds_at(&harness, 0);
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
        let after = moments(&harness);
        for (was, is) in before.iter().zip(&after) {
            assert!((is - was - later).abs() < 0.01, "{before:?} {after:?}");
        }
    }

    #[test]
    fn dragging_a_marker_in_the_ruler_moves_it() {
        let mut harness = timed_song();
        let x = field(&harness).columns.ruler_x();
        let (from, to) = (on_line(&harness, x, 480), on_line(&harness, x, 1440));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.document().document().markers[0].tick, Tick::new(1440));
    }

    #[test]
    fn dragging_the_cursor_handle_scrubs_through_the_song() {
        let mut harness = timed_song();
        let field = field(&harness);
        let from = egui::pos2(field.columns.handle_x(), field.view.now_y);
        let beat = field.view.y_of(0.5) - field.view.y_of(0.0);
        drag(
            &mut harness,
            from,
            from + egui::vec2(0.0, beat),
            egui::Modifiers::NONE,
        );
        assert!(cursor_shows(&harness, "0:1:0"));
    }

    #[test]
    fn the_handles_of_a_read_only_song_stay_put() {
        let mut harness = timed_song();
        harness.state_mut().plugins = &[];
        let song = harness.state().song.as_ref().unwrap().document().clone();
        harness.state_mut().song = Some(Song::new(song, &[]));
        harness.run();
        let x = field(&harness).columns.timing_x();
        let (from, to) = (on_line(&harness, x, 960), on_line(&harness, x, 1920));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(event_ticks(&harness), [Tick::new(0), Tick::new(960)]);
    }

    fn peaks() -> Arc<charter_audio::Peaks> {
        let frames = vec![[0.25, -0.25]; 4000];
        Arc::new(charter_audio::Peaks::of(
            &charter_audio::Track::from_frames(1000, frames),
        ))
    }

    #[test]
    fn the_waveform_of_the_songs_audio_shows_and_that_of_other_audio_does_not() {
        let mut harness = four_taps_at("/songs/four.json");
        assert!(harness.state().strip().is_none());
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/other.ogg"), peaks());
        harness.run();
        assert!(harness.state().strip().is_none());
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/a.ogg"), peaks());
        harness.run();
        assert!(harness.state().strip().is_some());
        let lanes = field(&harness)
            .columns
            .span(LaneIndex::new(0), LaneCount::ONE);
        let tap = on_line(&harness, lanes.center(), 0);
        click(&mut harness, tap, egui::Modifiers::NONE);
        assert!(harness.query_by_label("1 note selected").is_some());
    }

    #[test]
    fn the_strip_shows_the_frequencies_once_ready_when_the_settings_ask() {
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            frequencies: true,
            ..Settings::default()
        });
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/a.ogg"), peaks());
        harness.run();
        assert!(matches!(
            harness.state().strip(),
            Some(notefield::Strip::Waveform(_))
        ));
        let track = charter_audio::Track::from_frames(1000, vec![[0.25, -0.25]; 4000]);
        harness
            .state_mut()
            .spectrum_ready(Path::new("/songs/a.ogg"), Arc::new(Spectrum::of(&track)));
        harness.run();
        assert!(matches!(
            harness.state().strip(),
            Some(notefield::Strip::Frequencies(_))
        ));
    }

    #[test]
    fn the_waveform_can_be_turned_off_in_the_settings() {
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            waveform: false,
            ..Settings::default()
        });
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/a.ogg"), peaks());
        harness.run();
        assert!(harness.state().strip().is_none());
    }

    #[test]
    fn turning_auto_jump_off_keeps_the_cursor_where_notes_go() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness
            .get_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
            .click();
        harness.run();
        assert!(!harness.state().settings().auto_jump);
        harness.get_by_label("Close settings").click();
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn a_song_opens_on_the_snap_and_zoom_the_settings_choose() {
        let settings = Settings {
            snap: NonZeroU16::new(32).unwrap(),
            zoom: 200.0,
            ..Settings::default()
        };
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open_demo();
        let harness = window(app);
        assert!(harness.query_by_label("Snap 32nd").is_some());
        assert!(harness.query_by_label("Zoom 200%").is_some());
    }

    #[test]
    fn the_interface_size_setting_scales_the_whole_window() {
        let settings = Settings {
            scale: 1.5,
            ..Settings::default()
        };
        let harness = window(Window::new(PLUGINS).with_settings(settings));
        assert!((harness.ctx.zoom_factor() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_ctrl_drag_draws_its_hold_while_a_message_comes_and_goes() {
        let mut harness = editable_four_taps();
        harness
            .state_mut()
            .refuse(Path::new("other.json"), &"it is not a song");
        harness.run();
        let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
    }

    #[test]
    fn a_hold_reaches_where_the_drag_lets_go() {
        let mut harness = editable_four_taps();
        let (from, near, far) = (
            spot(&harness, 3, 240, 0.0),
            spot(&harness, 3, 480, 0.0),
            spot(&harness, 3, 960, 0.0),
        );
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        press(&harness, from, true, egui::Modifiers::COMMAND);
        harness.hover_at(from.lerp(near, 0.5));
        harness.hover_at(near);
        press(&harness, far, false, egui::Modifiers::COMMAND);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
        assert_eq!(note_at(&harness, 240, 3), Some(hold(720)));
    }

    #[test]
    fn escape_during_a_box_selection_cancels_it() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
        harness.hover_at(from);
        press(&harness, from, true, egui::Modifiers::NONE);
        harness.hover_at(from.lerp(to, 0.5));
        harness.hover_at(to);
        harness.key_press(egui::Key::Escape);
        press(&harness, to, false, egui::Modifiers::NONE);
        harness.run();
        assert!(nothing_selected(&harness));
    }

    #[test]
    fn a_save_holds_every_edit_made_in_its_frame() {
        let mut harness = editable_four_taps();
        let key = |key, pressed, modifiers| egui::Event::Key {
            key,
            pressed,
            repeat: false,
            modifiers,
            physical_key: None,
        };
        let command = egui::Modifiers::COMMAND;
        let events = &mut harness.input_mut().events;
        events.push(key(egui::Key::S, true, command));
        events.push(key(egui::Key::S, false, command));
        events.push(key(egui::Key::Num4, true, egui::Modifiers::NONE));
        events.push(key(egui::Key::Num4, false, egui::Modifiers::NONE));
        harness.run();
        let Request::Write { path, bytes } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert!(holds_tap(&bytes, 0, 3));
        harness.state_mut().saved(Saved::To(path));
        assert_eq!(harness.state().title(), "four.json - charter");
    }

    #[test]
    fn a_lane_key_held_across_a_chart_switch_leaves_the_other_chart_alone() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Hard").click();
        harness.run();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    }

    /// Runs the window with its clock at `seconds`.
    fn at_time(harness: &mut Harness<'_, Window>, seconds: f64) {
        harness.input_mut().time = Some(seconds);
        harness.run();
    }

    /// The four taps edited at one second in, and left long enough to be autosaved.
    fn autosaved_four_taps() -> (Harness<'static, Window>, Vec<u8>) {
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

    fn four_taps_with(settings: Settings) -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open(Path::new("four.json"), &four_taps());
        window(app)
    }

    #[test]
    fn unsaved_changes_wait_as_long_as_the_settings_say_before_they_are_autosaved() {
        let mut harness = four_taps_with(Settings {
            autosave_seconds: 10,
            ..Settings::default()
        });
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 10.5);
        assert_eq!(harness.state_mut().take_requests(), []);
        at_time(&mut harness, 11.5);
        assert!(matches!(request(&mut harness), Request::Autosave { .. }));
    }

    #[test]
    fn with_autosave_off_nothing_is_autosaved() {
        let mut harness = four_taps_with(Settings {
            autosave: false,
            ..Settings::default()
        });
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 120.0);
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn the_settings_window_turns_autosave_off() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Autosave");
        harness
            .get_by_role_and_label(Role::CheckBox, "Autosave unsaved changes")
            .click();
        harness.run();
        assert!(!harness.state().settings().autosave);
    }

    #[test]
    fn unsaved_changes_are_autosaved_half_a_minute_after_they_are_made() {
        let mut harness = editable_four_taps();
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 20.0);
        assert_eq!(harness.state_mut().take_requests(), []);
        at_time(&mut harness, 31.5);
        let Request::Autosave { bytes } = request(&mut harness) else {
            panic!("expected an autosave");
        };
        let kept = recovery::read(&bytes, PLUGINS).unwrap();
        assert_eq!(kept.path.as_deref(), Some(Path::new("four.json")));
        assert!(holds_tap(&kept.song.document().to_json().unwrap(), 0, 3));
        at_time(&mut harness, 90.0);
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn saving_removes_the_autosave() {
        let (mut harness, _) = autosaved_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("four.json")));
        harness.run();
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }

    #[test]
    fn undoing_back_to_the_saved_song_removes_the_autosave() {
        let (mut harness, _) = autosaved_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }

    #[test]
    fn closing_without_saving_removes_the_autosave() {
        let (mut harness, _) = autosaved_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.run();
        let requests = harness.state_mut().take_requests();
        assert_eq!(requests, [Request::DropAutosave]);
    }

    fn restore_prompt(harness: &Harness<'_, Window>) -> bool {
        harness
            .query_by_label("Restore unsaved changes to four.json?")
            .is_some()
    }

    #[test]
    fn an_autosave_restored_remembers_the_audio_dropped_on_the_song() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        at_time(&mut harness, 100.0);
        at_time(&mut harness, 131.0);
        let Request::Autosave { bytes } = request(&mut harness) else {
            panic!("expected an autosave");
        };
        let mut restored = window(Window::new(GAMES));
        restored.state_mut().recover(&bytes);
        restored.run();
        restored.get_by_label("Restore").click();
        restored.run();
        assert_eq!(
            restored.state().audio_file(),
            Some(PathBuf::from("/music/track.ogg"))
        );
        let path = PathBuf::from("/songs/mine.json");
        assert_eq!(
            restored.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/music/track.ogg"),
                PathBuf::from("/songs/track.ogg"),
            ))
        );
        restored.state_mut().saved(Saved::To(path));
        restored.run();
        let copies: Vec<_> = restored
            .state_mut()
            .take_requests()
            .into_iter()
            .filter(|request| matches!(request, Request::CopyAudio { .. }))
            .collect();
        assert_eq!(copies, []);
    }

    #[test]
    fn an_autosave_restored_brings_the_changes_back_unsaved_for_their_file() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = window(Window::new(PLUGINS));
        harness.state_mut().recover(&bytes);
        harness.run();
        assert!(restore_prompt(&harness));
        harness.get_by_label("Restore").click();
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        assert_eq!(note_at(&harness, 0, 3), Some(tap()));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("four.json"));
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }

    #[test]
    fn an_autosave_discarded_is_removed() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = window(Window::new(PLUGINS));
        harness.state_mut().recover(&bytes);
        harness.run();
        harness.get_by_label("Discard").click();
        harness.run();
        assert!(!restore_prompt(&harness));
        assert_eq!(request(&mut harness), Request::DropAutosave);
        assert!(harness.state().song.is_none());
    }

    #[test]
    fn a_damaged_autosave_says_so_and_is_removed() {
        let mut harness = window(Window::new(PLUGINS));
        harness.state_mut().recover(b"{ not json");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("could not be restored")
                .is_some()
        );
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }

    #[test]
    fn a_failed_autosave_says_why_and_tries_again_later() {
        let (mut harness, _) = autosaved_four_taps();
        harness.state_mut().autosave_failed("the disk is full");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Autosave failed: the disk is full")
                .is_some()
        );
        at_time(&mut harness, 70.0);
        assert!(matches!(request(&mut harness), Request::Autosave { .. }));
    }

    #[test]
    fn a_lane_key_on_a_note_removes_it_and_leaves_the_cursor_there() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn lane_keys_leave_a_read_only_song_alone() {
        let mut harness = with_demo();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(harness.query_by_label_contains("Refused").is_none());
    }

    #[test]
    fn a_key_past_the_last_lane_does_nothing() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num5);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
        assert!(harness.query_by_label_contains("Refused").is_none());
    }

    #[test]
    fn an_edit_the_game_refuses_says_why() {
        const PICKY: &[&dyn RulesetPlugin] = &[&NoFourthLane];
        let small = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Small", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let mut app = Window::new(PICKY);
        app.open(
            Path::new("small.json"),
            &serde_json::to_vec(&small).unwrap(),
        );
        let mut harness = window(app);
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert!(
            harness
                .query_by_label("Refused: a note on lane 4")
                .is_some()
        );
        assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
    }

    #[test]
    fn ctrl_z_takes_an_edit_back_and_ctrl_shift_z_or_ctrl_y_makes_it_again() {
        let mut harness = editable_demo();
        let command = egui::Modifiers::COMMAND;
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.key_press_modifiers(command, egui::Key::Z);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
        harness.key_press_modifiers(command.plus(egui::Modifiers::SHIFT), egui::Key::Z);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        harness.key_press_modifiers(command, egui::Key::Z);
        harness.run();
        harness.key_press_modifiers(command, egui::Key::Y);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
    }

    #[test]
    fn the_edit_menu_undoes_and_redoes_what_it_can() {
        let mut harness = editable_demo();
        harness.get_by_label("Edit").click();
        harness.run();
        let disabled = |harness: &Harness<'_, Window>, item: &str| {
            harness
                .get_by_label_contains(item)
                .accesskit_node()
                .is_disabled()
        };
        assert!(disabled(&harness, "Undo") && disabled(&harness, "Redo"));
        harness.key_press(egui::Key::Escape);
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Edit").click();
        harness.run();
        assert!(!disabled(&harness, "Undo"));
        harness.get_by_label_contains("Undo").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        harness.get_by_label("Edit").click();
        harness.run();
        harness.get_by_label_contains("Redo").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
    }

    #[test]
    fn an_empty_window_offers_the_demo_song() {
        let mut harness = window(Window::default());
        harness.get_by_label("Open the demo song").click();
        harness.run();
        assert!(harness.query_by_label("Demo").is_some());
    }

    #[test]
    fn an_opened_song_lists_its_charts() {
        let mut app = Window::default();
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        assert!(harness.query_all_by_label("Future").next().is_some());
        assert!(harness.query_all_by_label("Beyond").next().is_some());
    }

    #[test]
    fn a_chart_says_what_it_cannot_draw_without_its_ruleset() {
        let mut app = Window::default();
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        let note = "1 object on the plane needs the arcaea plugin to be drawn";
        assert!(harness.query_by_label(note).is_some());
    }

    #[test]
    fn clicking_a_chart_opens_it() {
        let mut harness = with_demo();
        harness.get_by_label("Hard").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.chart_index(), 1);
    }

    #[test]
    fn a_file_that_breaks_a_rule_says_what_and_where() {
        let broken = String::from_utf8(example().to_vec())
            .unwrap()
            .replace("\"Beyond\"", "\"Future\"");
        let mut app = Window::default();
        app.open(Path::new("broken.json"), broken.as_bytes());
        let harness = window(app);
        assert!(
            harness
                .query_by_label_contains("broken.json did not open")
                .is_some()
        );
        assert!(harness.query_by_label_contains("charts[1]").is_some());
    }

    #[test]
    fn the_keyboard_moves_the_cursor_and_changes_the_snap() {
        let mut harness = with_demo();
        for _ in 0..4 {
            harness.key_press(egui::Key::ArrowDown);
        }
        harness.run();
        assert!(cursor_shows(&harness, "0:1:0"));
        harness.key_press(egui::Key::ArrowRight);
        harness.run();
        assert!(harness.query_by_label("Snap 24th").is_some());
        harness.key_press(egui::Key::PageDown);
        harness.run();
        assert!(cursor_shows(&harness, "1:0:0"));
    }

    #[test]
    fn ctrl_plus_zooms_the_notefield_and_leaves_the_window_its_size() {
        let mut harness = with_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
        harness.run();
        assert!(harness.query_by_label("Zoom 125%").is_some());
        assert!((harness.ctx.zoom_factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dropping_a_song_on_the_window_opens_it() {
        let mut harness = window(Window::default());
        let file = Arc::new(Dropped("example.json", example().to_vec()));
        harness.input_mut().dropped_files.push(file);
        harness.run();
        assert!(harness.query_all_by_label("Future").next().is_some());
    }

    /// The StepMania and Tap Tap Revenge plugins, which start songs.
    const GAMES: &[&dyn RulesetPlugin] = &[&charter_stepmania::Plugin, &charter_ttr::Plugin];

    fn new_song_dialog() -> Harness<'static, Window> {
        let mut harness = window(Window::new(GAMES));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        harness
    }

    fn dialog_open(harness: &Harness<'_, Window>) -> bool {
        harness.query_by_label("Create").is_some()
    }

    fn charts(harness: &Harness<'_, Window>) -> Vec<(String, Option<LaneCount>)> {
        let song = harness.state().song.as_ref().unwrap();
        let charts = &song.document().document().charts;
        charts
            .iter()
            .map(|chart| (chart.name.clone(), chart.lanes))
            .collect()
    }

    fn lanes(n: u16) -> Option<LaneCount> {
        LaneCount::new(n)
    }

    #[test]
    fn the_file_menu_and_ctrl_n_start_a_new_song_and_cancel_leaves_it() {
        let mut harness = window(Window::new(GAMES));
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("New song…").click();
        harness.run();
        assert!(dialog_open(&harness));
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(!dialog_open(&harness));
        assert!(harness.state().song.is_none());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        assert!(dialog_open(&harness));
    }

    #[test]
    fn shortcuts_work_with_no_song_open() {
        let mut harness = window(Window::new(GAMES));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }

    #[test]
    fn the_empty_window_offers_a_new_song() {
        let mut harness = window(Window::new(GAMES));
        harness.get_by_label("New song").click();
        harness.run();
        assert!(dialog_open(&harness));
    }

    #[test]
    fn a_new_song_starts_as_one_empty_chart_of_the_first_games_usual_layout() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Create").click();
        harness.run();
        assert!(!dialog_open(&harness));
        assert_eq!(charts(&harness), [("Easy".to_owned(), lanes(4))]);
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.document().document().ruleset.id.as_str(), "sm");
        assert!(song.read_only().is_none());
        assert_eq!(harness.state().title(), "Untitled.json - charter");
    }

    #[test]
    fn a_new_tap_tap_revenge_song_takes_any_levels_named_as_its_game_names_them() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Tap Tap Revenge").click();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "Lanes")
            .click();
        harness.run();
        harness.get_by_label("5 lanes").click();
        harness.run();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();
        let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
        assert_eq!(preset.value().as_deref(), Some("Custom, Extreme only"));
        preset.click();
        harness.run();
        harness.get_by_label("Tap Tap Reloaded").click();
        harness.run();
        harness
            .get_by_role_and_label(Role::CheckBox, "Easy")
            .click();
        harness.run();
        let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
        assert_eq!(preset.value().as_deref(), Some("Your own"));
        harness.get_by_label("Create").click();
        harness.run();
        let named = |name: &str| (name.to_owned(), lanes(5));
        assert_eq!(
            charts(&harness),
            [
                named("Easy"),
                named("Metallica"),
                named("Downloads"),
                named("Customs")
            ]
        );
        let song = harness.state().song.as_ref().unwrap();
        let levels: Vec<_> = song
            .document()
            .document()
            .charts
            .iter()
            .map(|chart| chart.settings["difficulty"].clone())
            .collect();
        assert_eq!(levels, ["easy", "medium", "hard", "extreme"]);
        assert_eq!(harness.state().title(), "Mine.json - charter");
    }

    #[test]
    fn the_new_song_dialog_keeps_its_top_and_its_fields_one_edge() {
        let mut harness = new_song_dialog();
        let top = |harness: &Harness<'_, Window>| {
            harness.get_by_label_contains("Pick the game").rect().top()
        };
        let left = |harness: &Harness<'_, Window>, label: &str| {
            harness
                .get_by_role_and_label(Role::TextInput, label)
                .rect()
                .left()
        };
        let before = top(&harness);
        let edge = left(&harness, "Title");
        assert!((left(&harness, "Chart 1") - edge).abs() < 0.5);
        harness.get_by_label("Tap Tap Revenge").click();
        harness.run();
        harness.run();
        assert!((top(&harness) - before).abs() < 0.5);
        let extreme = left(&harness, "Extreme chart name");
        assert!((extreme - edge).abs() < 0.5, "{extreme} {edge}");
        let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
        assert!((preset.rect().left() - edge).abs() < 0.5);
    }

    #[test]
    fn a_new_song_needs_a_chart() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Remove Easy").click();
        harness.run();
        assert!(harness.query_by_label("Remove Easy").is_none());
        harness.get_by_label("Create").click();
        harness.run();
        assert!(dialog_open(&harness));
        assert!(harness.state().song.is_none());
    }

    #[test]
    fn a_rejected_new_song_keeps_its_fields_and_shows_why() {
        let mut harness = new_song_dialog();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();
        harness.get_by_label("Add a chart").click();
        harness.run();
        let second = harness.get_by_role_and_label(Role::TextInput, "Chart 2");
        second.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Chart 2")
            .type_text("Easy");
        harness.run();

        harness.get_by_label("Create").click();
        harness.run();
        assert!(harness.state().song.is_none());
        assert!(dialog_open(&harness));
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Title")
                .value()
                .as_deref(),
            Some("Mine")
        );
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Chart 2")
                .value()
                .as_deref(),
            Some("Easy")
        );
        assert!(
            harness
                .query_all_by_label_contains("Could not start the song")
                .next()
                .is_some()
        );
    }

    #[test]
    fn cancelling_the_unsaved_song_prompt_keeps_the_new_song_draft() {
        let mut app = Window::new(GAMES);
        app.open(Path::new("/songs/four.json"), &four_taps());
        app.song.as_mut().unwrap().mark_unsaved();
        let mut harness = window(app);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();

        harness.get_by_label("Create").click();
        harness.run();
        assert!(asks_to_save(&harness));
        harness.state_mut().answer(panels::Answer::Cancel);
        harness.run();

        assert!(dialog_open(&harness));
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Title")
                .value()
                .as_deref(),
            Some("Mine")
        );
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    #[test]
    fn lane_keys_wait_while_the_new_song_dialog_shows() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
    }

    fn created() -> Harness<'static, Window> {
        let mut harness = new_song_dialog();
        harness.get_by_label("Create").click();
        harness.run();
        harness
    }

    /// A file dropped on the window that only a path stands for.
    #[derive(Debug)]
    struct DroppedPath(&'static str);

    impl egui::DroppedFile for DroppedPath {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            Err("not read in tests".to_owned())
        }
    }

    /// Drops the file at `path` on the window, and forgets the tags it asks the host for.
    fn drop_file(harness: &mut Harness<'_, Window>, path: &'static str) {
        drop_keeping_requests(harness, path);
        let requests = &mut harness.state_mut().requests;
        requests.retain(|request| !matches!(request, Request::ReadTags { .. }));
    }

    fn drop_keeping_requests(harness: &mut Harness<'_, Window>, path: &'static str) {
        harness
            .input_mut()
            .dropped_files
            .push(Arc::new(DroppedPath(path)));
        harness.run();
    }

    fn read_tags(harness: &mut Harness<'_, Window>, path: &str, title: &str, artist: &str) {
        let file = Path::new(path);
        let (title, artist) = (Some(title.to_owned()), Some(artist.to_owned()));
        harness.state_mut().tags_read(file, title, artist);
        harness.run();
    }

    fn field_value(harness: &Harness<'_, Window>, label: &str) -> Option<String> {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .value()
    }

    fn facts(harness: &Harness<'_, Window>) -> (String, String) {
        let song = harness.state().song.as_ref().unwrap();
        let metadata = &song.document().document().metadata;
        (metadata.title.clone(), metadata.artist.clone())
    }

    const OFFER: &str = "Do you want to import song metadata from tags?";

    #[test]
    fn audio_dropped_on_the_new_song_dialog_fills_a_blank_title_and_artist_from_its_tags() {
        let mut harness = new_song_dialog();
        drop_keeping_requests(&mut harness, "/music/track.ogg");
        assert_eq!(
            request(&mut harness),
            Request::ReadTags {
                file: PathBuf::from("/music/track.ogg")
            }
        );
        assert!(harness.query_by_label("track.ogg").is_some());
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert_eq!(field_value(&harness, "Title").as_deref(), Some("Starlight"));
        assert!(harness.query_by_label_contains(OFFER).is_none());
        harness.get_by_label("Create").click();
        harness.run();
        assert_eq!(
            facts(&harness),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
        assert_eq!(audio(&harness), "track.ogg");
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/music/track.ogg"))
        );
        assert!(!harness.state().song.as_ref().unwrap().is_modified());
    }

    #[test]
    fn a_new_song_takes_the_cover_its_audio_holds_and_writes_it_into_its_folder() {
        let mut harness = new_song_dialog();
        drop_file(&mut harness, "/music/track.flac");
        let (bytes, image) = (
            b"jpeg".to_vec(),
            egui::ColorImage::filled([2, 2], egui::Color32::RED),
        );
        harness
            .state_mut()
            .artwork_embedded(Path::new("/music/track.flac"), "jpg", (bytes, image));
        harness.get_by_label("Create").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        let artwork = song.document().document().metadata.artwork.clone();
        assert_eq!(
            artwork.map(|path| path.to_string()).as_deref(),
            Some("cover.jpg")
        );
        assert_eq!(
            harness
                .state()
                .artwork_write_for(Path::new("/s/Track/Track.json")),
            Some((PathBuf::from("/s/Track/cover.jpg"), b"jpeg".to_vec()))
        );
    }

    #[test]
    fn tags_for_a_title_already_typed_are_offered_rather_than_taken() {
        let mut harness = new_song_dialog();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert!(harness.query_by_label_contains(OFFER).is_some());
        harness.get_by_label("Keep mine").click();
        harness.run();
        assert_eq!(field_value(&harness, "Title").as_deref(), Some("Mine"));
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        harness.get_by_label("Import").click();
        harness.run();
        assert_eq!(field_value(&harness, "Title").as_deref(), Some("Starlight"));
    }

    #[test]
    fn the_new_song_dialog_chooses_its_audio_through_the_host() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Choose audio…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseAudio);
        harness
            .state_mut()
            .use_audio(Path::new("/music/picked.flac"));
        harness.run();
        assert!(harness.query_by_label("picked.flac").is_some());
        assert!(matches!(request(&mut harness), Request::ReadTags { .. }));
    }

    #[test]
    fn a_new_songs_title_and_artist_follow_the_tags_of_audio_dropped_on_it() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert_eq!(
            facts(&harness),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        let defaults = ("Untitled".to_owned(), "Unknown artist".to_owned());
        assert_eq!(facts(&harness), defaults);
    }

    #[test]
    fn a_song_with_a_title_of_its_own_asks_before_taking_the_tags() {
        let mut harness = editable_four_taps();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert_eq!(facts(&harness), ("Four".to_owned(), "a".to_owned()));
        harness.get_by_label("Import").click();
        harness.run();
        assert_eq!(
            facts(&harness),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
    }

    #[test]
    fn tags_for_audio_the_song_no_longer_plays_are_ignored() {
        let mut harness = editable_four_taps();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/other.ogg", "Starlight", "The Lanes");
        assert!(harness.query_by_label_contains(OFFER).is_none());
        assert_eq!(facts(&harness), ("Four".to_owned(), "a".to_owned()));
    }

    fn offset_field<'a>(harness: &'a Harness<'_, Window>) -> egui_kittest::Node<'a> {
        harness.get_by_role_and_label(Role::TextInput, "Offset")
    }

    fn offset(harness: &Harness<'_, Window>) -> serde_json::Value {
        let song = harness.state().song.as_ref().unwrap();
        serde_json::to_value(
            song.document()
                .timing()
                .time_at(charter_core::Tick::new(0))
                .seconds(),
        )
        .unwrap()
    }

    fn enter_offset(harness: &mut Harness<'_, Window>, text: &str) {
        offset_field(harness).focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        offset_field(harness).type_text(text);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
    }

    #[test]
    fn the_offset_is_typed_in_milliseconds() {
        let mut harness = editable_demo();
        assert_eq!(offset_field(&harness).value().as_deref(), Some("0"));
        enter_offset(&mut harness, "1500.25");
        assert_eq!(offset(&harness), serde_json::json!(1.50025));
        assert_eq!(offset_field(&harness).value().as_deref(), Some("1500.25"));
    }

    #[test]
    fn an_offset_that_is_not_a_time_says_so() {
        let mut harness = editable_demo();
        enter_offset(&mut harness, "soon");
        assert!(
            harness
                .query_by_label_contains("soon is not an offset in milliseconds")
                .is_some()
        );
    }

    #[test]
    fn alt_arrows_and_the_offset_steppers_nudge_it_by_a_millisecond_or_ten_with_shift() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowRight);
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(0.001));
        let coarse = egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(coarse, egui::Key::ArrowLeft);
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(-0.009));
        harness.get_by_label("Raise the offset by 1 ms").click();
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(-0.008));
        harness.get_by_label("Lower the offset by 1 ms").click();
        harness.run();
        assert_eq!(offset(&harness), serde_json::json!(-0.009));
    }

    #[test]
    fn the_tempo_section_opens_the_sync_window() {
        let mut harness = editable_demo();
        click_in_panel(&mut harness, "Sync to the audio");
        assert!(harness.query_by_label("Row at 0.0 s").is_some());
    }

    fn no_audio(harness: &Harness<'_, Window>) -> bool {
        harness.query_by_label_contains("No audio yet").is_some()
    }

    fn audio(harness: &Harness<'_, Window>) -> String {
        let song = harness.state().song.as_ref().unwrap();
        song.document()
            .document()
            .metadata
            .audio
            .as_str()
            .to_owned()
    }

    #[test]
    fn a_new_song_asks_for_audio_until_a_file_is_dropped_on_it() {
        let mut harness = created();
        assert!(no_audio(&harness));
        drop_file(&mut harness, "/music/track.ogg");
        assert!(!no_audio(&harness));
        assert_eq!(audio(&harness), "track.ogg");
        assert_eq!(harness.state().title(), "*Untitled.json - charter");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(no_audio(&harness));
    }

    #[test]
    fn saving_a_song_plans_to_copy_dropped_audio_beside_it() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::SaveAs { .. }));
        let path = PathBuf::from("/songs/mine.json");
        assert_eq!(
            harness.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/music/track.ogg"),
                PathBuf::from("/songs/track.ogg"),
            ))
        );
        harness.state_mut().saved(Saved::To(path.clone()));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert_eq!(harness.state().audio_copy_for(&path), None);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn audio_already_beside_the_song_is_not_copied() {
        let mut harness = created();
        drop_file(&mut harness, "/songs/track.ogg");
        let path = PathBuf::from("/songs/mine.json");
        assert_eq!(harness.state().audio_copy_for(&path), None);
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }

    #[test]
    fn audio_that_could_not_be_copied_says_why() {
        let mut harness = created();
        harness.state_mut().audio_not_copied("the disk is full");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Audio not copied: the disk is full")
                .is_some()
        );
    }

    #[test]
    fn audio_dropped_with_no_song_open_says_to_open_one() {
        let mut harness = window(Window::new(GAMES));
        drop_file(&mut harness, "/music/track.ogg");
        assert!(
            harness
                .query_by_label_contains("Open or start a song before adding its audio")
                .is_some()
        );
    }

    #[test]
    fn audio_dropped_on_a_read_only_song_is_refused() {
        let mut harness = window(Window::new(&[]));
        harness.state_mut().open_demo();
        harness.run();
        drop_file(&mut harness, "/music/track.ogg");
        assert_eq!(audio(&harness), "demo.ogg");
        assert!(harness.query_by_label_contains("Refused").is_some());
    }
}
