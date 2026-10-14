#![expect(
    clippy::mod_module_files,
    reason = "the settings values and the page that edits them are one module directory"
)]

//! What the user chooses about the editor, and the window where they choose it.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use charter_audio::{Gain, Kit, Volumes};
use charter_core::NoteValue;
use charter_editor::SNAPS;

use crate::flip::Direction;
use crate::notefield::{GRIDS, Zoom};
use crate::panels::ordinal;
use crate::style::{
    ACCENT_FILL, ON_ACCENT, PANEL, TEXT, WEAK_TEXT, dialog_title, glyph, icon, quiet_icon, switch,
};
use crate::window::Rate;
use egui::RichText;

/// A sound the editor can make while a song plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    // Older versions kept the note ticks off as a sound of their own.
    /// A short, dry note click.
    #[default]
    #[serde(alias = "off")]
    Click,
    /// A pitched electronic note.
    Beep,
    /// A sharp impact.
    Hit,
    /// A handclap.
    Clap,
    /// A quieter handclap.
    SoftClap,
    /// A bright finishing accent.
    Finish,
}

/// A latency calibration value, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(from = "f64", into = "f64")]
pub struct LatencyMs(f64);

impl LatencyMs {
    /// Smallest latency, in milliseconds.
    pub(crate) const MIN: f64 = -500.0;
    /// Largest latency, in milliseconds.
    pub(crate) const MAX: f64 = 500.0;

    /// This latency in milliseconds.
    #[must_use]
    pub const fn ms(self) -> f64 {
        self.0
    }

    /// This latency as seconds at `rate`.
    #[must_use]
    pub fn seconds_at(self, rate: f64) -> f64 {
        self.0 * rate / 1000.0
    }
}

impl From<f64> for LatencyMs {
    fn from(ms: f64) -> Self {
        if ms.is_finite() {
            Self(ms.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<LatencyMs> for f64 {
    fn from(latency: LatencyMs) -> Self {
        latency.ms()
    }
}

/// A notefield zoom as a percentage of its usual size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f32", into = "f32")]
pub struct ZoomPercent(f32);

impl ZoomPercent {
    /// Smallest zoom percentage shared with the notefield.
    pub(crate) const MIN: f32 = Zoom::PERCENT_RANGE.0;
    /// Largest zoom percentage shared with the notefield.
    pub(crate) const MAX: f32 = Zoom::PERCENT_RANGE.1;

    /// Zoom as a percentage.
    #[must_use]
    pub const fn percent(self) -> f32 {
        self.0
    }
}

impl Default for ZoomPercent {
    fn default() -> Self {
        Self(100.0)
    }
}

impl From<f32> for ZoomPercent {
    fn from(percent: f32) -> Self {
        if percent.is_finite() {
            Self(percent.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<ZoomPercent> for f32 {
    fn from(zoom: ZoomPercent) -> Self {
        zoom.percent()
    }
}

/// The size of the editor window's interface, as a factor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f32", into = "f32")]
pub struct InterfaceScale(f32);

impl InterfaceScale {
    /// Smallest interface size the window can draw.
    const MIN: f32 = 0.5;
    /// Largest interface size the window can draw.
    const MAX: f32 = 3.0;

    /// Interface size as a factor.
    #[must_use]
    pub const fn factor(self) -> f32 {
        self.0
    }
}

impl Default for InterfaceScale {
    fn default() -> Self {
        Self(1.0)
    }
}

impl From<f32> for InterfaceScale {
    fn from(factor: f32) -> Self {
        if factor.is_finite() {
            Self(factor.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<InterfaceScale> for f32 {
    fn from(scale: InterfaceScale) -> Self {
        scale.factor()
    }
}

/// The alternate playback speed, as a factor of normal speed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f64", into = "f64")]
pub struct AlternateRate(f64);

impl AlternateRate {
    /// Slowest playback speed shared with the window.
    pub(crate) const MIN: f64 = Rate::SLOWEST;
    /// Fastest playback speed shared with the window.
    pub(crate) const MAX: f64 = Rate::FASTEST;

    /// Playback speed as a factor.
    #[must_use]
    pub const fn factor(self) -> f64 {
        self.0
    }
}

impl Default for AlternateRate {
    fn default() -> Self {
        Self(0.6)
    }
}

impl From<f64> for AlternateRate {
    fn from(factor: f64) -> Self {
        if factor.is_finite() {
            Self(factor.clamp(Self::MIN, Self::MAX))
        } else {
            Self::default()
        }
    }
}

impl From<AlternateRate> for f64 {
    fn from(rate: AlternateRate) -> Self {
        rate.factor()
    }
}

/// An integer setting kept between `MIN` and `MAX`, starting at `START`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "i128", into = "u32")]
pub struct BoundedU32<const MIN: u32, const MAX: u32, const START: u32>(u32);

impl<const MIN: u32, const MAX: u32, const START: u32> BoundedU32<MIN, MAX, START> {
    /// The lower bound.
    pub const MIN: u32 = MIN;
    /// The upper bound.
    pub const MAX: u32 = MAX;

    /// The bounded number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> Default for BoundedU32<MIN, MAX, START> {
    fn default() -> Self {
        Self::from(START)
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> From<u32> for BoundedU32<MIN, MAX, START> {
    fn from(value: u32) -> Self {
        Self(value.clamp(MIN, MAX))
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> From<i128> for BoundedU32<MIN, MAX, START> {
    fn from(value: i128) -> Self {
        let value = u32::try_from(value).unwrap_or(if value < 0 { MIN } else { MAX });
        Self::from(value)
    }
}

impl<const MIN: u32, const MAX: u32, const START: u32> From<BoundedU32<MIN, MAX, START>> for u32 {
    fn from(value: BoundedU32<MIN, MAX, START>) -> Self {
        value.get()
    }
}

/// Seconds between autosaves, from 5 to 600.
pub type AutosaveSeconds = BoundedU32<5, 600, 30>;
/// Minutes between snapshots, from 1 to 120.
pub type SnapshotMinutes = BoundedU32<1, 120, 5>;
/// Copies of a song to keep per folder, from 1 to 500.
pub type SnapshotsKept = BoundedU32<1, 500, 20>;
/// Milliseconds of audio before and after the cursor, from 50 to 1000.
pub type SnippetMs = BoundedU32<50, 1000, 200>;

impl From<SnapshotsKept> for NonZeroU32 {
    fn from(kept: SnapshotsKept) -> Self {
        Self::new(kept.get()).unwrap_or(Self::MIN)
    }
}

impl Sound {
    /// The note tick sounds offered in settings, in menu order.
    const ALL: [Self; 6] = [
        Self::Click,
        Self::Beep,
        Self::Hit,
        Self::Clap,
        Self::SoftClap,
        Self::Finish,
    ];

    /// What each note ticks with.
    pub(crate) const fn tick(self) -> charter_audio::Tick {
        use charter_audio::Tick;
        match self {
            Self::Click => Tick::Click,
            Self::Beep => Tick::Beep,
            Self::Hit => Tick::Hit,
            Self::Clap => Tick::Clap,
            Self::SoftClap => Tick::SoftClap,
            Self::Finish => Tick::Finish,
        }
    }

    /// The label used for this tick sound in the settings window.
    const fn name(self) -> &'static str {
        match self {
            Self::Click => "Click",
            Self::Beep => "Beep",
            Self::Hit => "Hit",
            Self::Clap => "Clap",
            Self::SoftClap => "Soft clap",
            Self::Finish => "Finish",
        }
    }
}

/// The user's choices, which the host keeps between runs. Choices missing from what the host
/// kept, such as ones newer than it, take their defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent choices, each its own switch"
)]
pub struct Settings {
    /// Whether the cursor moves on a snap once the lane keys that placed a note are up.
    pub auto_jump: bool,
    /// The snap a song opens on, where its resolution holds it.
    pub snap: NoteValue,
    /// Whether a BPM set on a timing point keeps the notes it times where they sound, rather
    /// than on their beats.
    pub notes_keep_time: bool,
    /// How close the notefield opens, as a percentage of its usual zoom.
    pub zoom: ZoomPercent,
    /// How large the whole window draws, as a factor.
    pub scale: InterfaceScale,
    /// Which way the notefield runs.
    pub direction: Direction,
    /// Whether a tick sounds as each note passes the now line during playback.
    #[serde(deserialize_with = "on_or_sound")]
    pub note_ticks: bool,
    /// What that tick sounds like, kept while the ticks are off.
    pub tick_sound: Sound,
    /// Whether each lane's note ticks in a pitch of its own, lowest on the left.
    pub tuned_lanes: bool,
    /// Whether the metronome sounds on each beat during playback.
    #[serde(deserialize_with = "on_or_sound")]
    pub metronome: bool,
    /// What the metronome sounds like.
    pub metronome_sound: Kit,
    /// How loud the music, the note ticks and the metronome play.
    pub volumes: Volumes,
    /// Whether unsaved changes are kept where the next run finds them.
    pub autosave: bool,
    /// How long unsaved changes wait before they are autosaved, in seconds.
    pub autosave_seconds: AutosaveSeconds,
    /// Whether a timestamped copy of a changed song is kept in the snapshot folder.
    pub snapshots_in_folder: bool,
    /// Where those copies go, each song in a folder of its own; none for `snapshots` in charter's
    /// data folder.
    pub snapshot_folder: Option<std::path::PathBuf>,
    /// Whether a timestamped copy of a changed song is kept in an `autosaves` folder beside it,
    /// once it has been saved.
    pub snapshots_beside_song: bool,
    /// How often a changed song is copied, in minutes.
    pub snapshot_minutes: SnapshotMinutes,
    /// How many copies of a song each folder keeps, the oldest going first.
    pub snapshots_kept: SnapshotsKept,
    /// The songs opened or saved last, the latest first.
    pub recent: Vec<std::path::PathBuf>,
    /// Whether the song panel shows beside the notefield.
    pub song_panel: bool,
    /// Whether the notefield draws the audio's waveform beside the lanes.
    pub waveform: bool,
    /// Whether that strip shows how loud each pitch is instead of the waveform.
    pub frequencies: bool,
    /// Whether the whole chart shows in a strip beside the notefield, to jump across it.
    pub minimap: bool,
    /// Tint whole notes with their layer's colour instead of marking their leading edge.
    pub colour_notes_by_layer: bool,
    /// The note value of the notefield grid's finest lines, one of [`GRIDS`]: 4 draws the beats
    /// alone. Older versions kept a sixteenths switch here.
    #[serde(alias = "sixteenths", deserialize_with = "grid_or_switch")]
    pub grid: NoteValue,
    /// Whether the metronome clicks on every line of the timing window's grid, not only on the
    /// beats.
    pub metronome_grid: bool,
    /// The alternate playback speed, as a factor, that R switches to in the timing window.
    pub rate_b: AlternateRate,
    /// How much audio Q and W play before and after the cursor, in milliseconds.
    pub snippet_ms: SnippetMs,
    /// Whether audio played slower or faster than its speed keeps its pitch, from a tenth to
    /// twice its speed.
    pub keep_pitch: bool,
    /// `FFmpeg` binary to try before looking on the PATH; empty to use the PATH alone.
    pub ffmpeg_path: String,
    /// Whether the first-startup `FFmpeg` help has already been offered.
    pub ffmpeg_offered: bool,
    /// How much later than it plays a sound is heard, in milliseconds, as the calibration's flash
    /// set it: the cursor takes it off the playhead to show what is heard.
    pub audio_latency_ms: LatencyMs,
    /// How much later than a sound is heard a tap along to it comes, in milliseconds: notes placed
    /// while playing go this much earlier than the cursor. Older versions kept the two latencies
    /// together here.
    #[serde(alias = "latency_ms")]
    pub input_latency_ms: LatencyMs,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_jump: true,
            snap: NoteValue::new(16).unwrap_or(NoteValue::QUARTER),
            notes_keep_time: true,
            zoom: ZoomPercent::default(),
            scale: InterfaceScale::default(),
            direction: Direction::Up,
            note_ticks: false,
            tick_sound: Sound::Click,
            tuned_lanes: false,
            metronome: false,
            metronome_sound: Kit::default(),
            volumes: Volumes::default(),
            autosave: true,
            autosave_seconds: AutosaveSeconds::default(),
            snapshots_in_folder: false,
            snapshot_folder: None,
            snapshots_beside_song: false,
            snapshot_minutes: SnapshotMinutes::default(),
            snapshots_kept: SnapshotsKept::default(),
            recent: Vec::new(),
            song_panel: true,
            waveform: true,
            frequencies: false,
            minimap: true,
            colour_notes_by_layer: false,
            grid: BEATS,
            metronome_grid: false,
            rate_b: AlternateRate::default(),
            snippet_ms: SnippetMs::default(),
            keep_pitch: true,
            ffmpeg_path: String::new(),
            ffmpeg_offered: false,
            audio_latency_ms: LatencyMs::default(),
            input_latency_ms: LatencyMs::default(),
        }
    }
}

/// What the settings window asks of the window around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// Play a measure of this metronome sound.
    Preview(Kit),
    /// Open the latency calibration.
    Calibrate,
    /// Ask the user for a folder to keep snapshots in.
    ChooseSnapshotFolder,
    /// Ask the host to choose an `FFmpeg` binary.
    ChooseFfmpeg,
    /// Show installation help for a missing `FFmpeg`.
    InstallFfmpeg,
}

/// Where the host found `FFmpeg`, or whether it has checked yet.
#[derive(Debug, Default)]
pub(crate) enum FfmpegStatus {
    /// The host has not checked yet.
    #[default]
    Unknown,
    /// No usable `FFmpeg` was found.
    Missing,
    /// A release download is this share of the way done.
    Downloading(f32),
    /// A binary whose version check passed.
    Found(std::path::PathBuf),
}

/// The grid of beats alone.
const BEATS: NoteValue = NoteValue::QUARTER;

/// The grid, read from a note value or, as older versions kept it, a sixteenths switch. A value
/// the notefield does not offer draws the beats alone.
fn grid_or_switch<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<NoteValue, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Kept {
        Grid(u16),
        Sixteenths(bool),
    }
    let grid = match Kept::deserialize(deserializer)? {
        Kept::Grid(grid) => grid,
        Kept::Sixteenths(true) => 16,
        Kept::Sixteenths(false) => 4,
    };
    Ok(NoteValue::new(grid)
        .filter(|value| GRIDS.contains(value))
        .unwrap_or(BEATS))
}

/// A grid's name in the settings and the toolbar.
pub(crate) fn grid_name(grid: NoteValue) -> String {
    if grid == BEATS {
        "Beats".to_owned()
    } else {
        format!("{} notes", ordinal(grid))
    }
}

/// Whether a sound is on, read from a switch or, as older versions kept it, the name of a
/// sound, `off` for none.
fn on_or_sound<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Kept {
        On(bool),
        Sound(String),
    }
    Ok(match Kept::deserialize(deserializer)? {
        Kept::On(on) => on,
        Kept::Sound(sound) => sound != "off",
    })
}

/// The interface sizes on offer, as factors.
/// Interface size factors offered in display settings.
const SCALES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

impl Settings {
    /// The settings as the host keeps them between runs, as JSON: only those that differ from
    /// the defaults, so a setting left at its default follows the default when a later version
    /// changes it.
    #[must_use]
    pub fn to_kept(&self) -> String {
        let (Ok(Value::Object(mine)), Ok(Value::Object(defaults))) = (
            serde_json::to_value(self),
            serde_json::to_value(Self::default()),
        ) else {
            return "{}".to_owned();
        };
        let note_ticks = mine.get("note_ticks").cloned();
        let mut changed: serde_json::Map<String, Value> = mine
            .into_iter()
            .filter(|(key, value)| defaults.get(key) != Some(value))
            .collect();
        // A tick sound kept alone reads as the ticks on, as older versions kept them, so the
        // switch is kept with it.
        if changed.contains_key("tick_sound")
            && let Some(on) = note_ticks
        {
            changed.insert("note_ticks".to_owned(), on);
        }
        Value::Object(changed).to_string()
    }

    /// The settings the host kept, from [`Settings::to_kept`] or an older whole copy: each kept
    /// setting that still reads, and the default for every other, so one setting a version can
    /// no longer read costs only itself.
    #[must_use]
    pub fn from_kept(kept: &str) -> Self {
        let Ok(Value::Object(mut kept)) = serde_json::from_str(kept) else {
            return Self::default();
        };
        // Older versions kept the ticks on as the sound they made.
        if !kept.contains_key("note_ticks")
            && let Some(Value::String(sound)) = kept.get("tick_sound")
        {
            let on = Value::Bool(sound != "off");
            kept.insert("note_ticks".to_owned(), on);
        }
        let mut taken = serde_json::Map::new();
        for (key, value) in kept {
            let mut trial = taken.clone();
            trial.insert(key.clone(), value.clone());
            if serde_json::from_value::<Self>(Value::Object(trial)).is_ok() {
                taken.insert(key, value);
            }
        }
        serde_json::from_value(Value::Object(taken)).unwrap_or_default()
    }

    /// How long a changed song waits between snapshots, in seconds; none when snapshots are off.
    pub(crate) fn snapshot_after(&self) -> Option<f64> {
        (self.snapshots_in_folder || self.snapshots_beside_song)
            .then(|| f64::from(self.snapshot_minutes.get()) * 60.0)
    }

    /// How long unsaved changes wait before they are autosaved; none when autosave is off.
    pub(crate) fn autosave_after(&self) -> Option<f64> {
        self.autosave
            .then(|| f64::from(self.autosave_seconds.get()))
    }

    /// How much audio Q and W play, in seconds.
    pub(crate) fn snippet_seconds(&self) -> f64 {
        f64::from(self.snippet_ms.get()) / 1000.0
    }
}

mod page;

pub(crate) use page::{grid_choice, show, volume};

#[cfg(test)]
mod tests {

    use super::*;

    use proptest::prelude::*;

    #[test]
    fn settings_open_on_sixteenths_at_the_usual_zoom_moving_on_after_notes() {
        let settings = Settings::default();
        assert!(settings.auto_jump);
        assert!(settings.notes_keep_time);
        assert_eq!(settings.snap.get(), 16);
        assert!((settings.zoom.percent() - 100.0).abs() < f32::EPSILON);
        assert!((settings.scale.factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn unsaved_changes_are_autosaved_after_half_a_minute_to_start_with() {
        let settings = Settings::default();
        assert!(settings.autosave);
        assert_eq!(settings.autosave_after(), Some(30.0));
        let off = Settings {
            autosave: false,
            ..Settings::default()
        };
        assert_eq!(off.autosave_after(), None);
    }

    #[test]
    fn an_autosave_wait_out_of_range_is_kept_within_it() {
        let settings = |seconds: u32| Settings {
            autosave_seconds: AutosaveSeconds::from(seconds),
            ..Settings::default()
        };
        assert_eq!(settings(0).autosave_after(), Some(5.0));
        assert_eq!(settings(100_000).autosave_after(), Some(600.0));
    }

    #[test]
    fn a_stored_interface_size_is_kept_to_what_the_window_can_draw() {
        let size = |scale: f32| {
            Settings {
                scale: scale.into(),
                ..Settings::default()
            }
            .scale
            .factor()
        };
        assert!((size(f32::INFINITY) - 1.0).abs() < f32::EPSILON);
        assert!((size(0.1) - 0.5).abs() < f32::EPSILON);
        assert!((size(9.0) - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn notes_travel_up_with_no_tick_or_metronome_sound_to_start_with() {
        let settings = Settings::default();
        assert_eq!(settings.direction, Direction::Up);
        assert!(!settings.note_ticks);
        assert!(!settings.metronome);
        assert_eq!(settings.metronome_sound, Kit::ToneGate);
    }

    #[test]
    fn a_metronome_stored_as_a_sound_by_an_older_version_comes_back_on_or_off() {
        let on: Settings = serde_json::from_str(r#"{ "metronome": "beep" }"#).unwrap();
        assert!(on.metronome);
        let off: Settings = serde_json::from_str(r#"{ "metronome": "off" }"#).unwrap();
        assert!(!off.metronome);
    }

    #[test]
    fn a_sixteenths_switch_stored_by_an_older_version_comes_back_as_the_grid() {
        let on: Settings = serde_json::from_str(r#"{ "sixteenths": true }"#).unwrap();
        assert_eq!(on.grid.get(), 16);
        let off: Settings = serde_json::from_str(r#"{ "sixteenths": false }"#).unwrap();
        assert_eq!(off.grid.get(), 4);
    }

    #[test]
    fn the_grid_opens_on_beats_and_keeps_to_the_grids_it_offers() {
        assert_eq!(Settings::default().grid.get(), 4);
        let odd: Settings = serde_json::from_str(r#"{ "grid": 7 }"#).unwrap();
        assert_eq!(odd.grid.get(), 4);
    }

    #[test]
    fn snap_and_grid_note_values_keep_their_numeric_settings() {
        let settings = Settings {
            snap: NoteValue::new(24).unwrap(),
            grid: NoteValue::new(16).unwrap(),
            ..Settings::default()
        };
        let stored: Value = serde_json::from_str(&settings.to_kept()).unwrap();
        assert_eq!(stored, serde_json::json!({ "snap": 24, "grid": 16 }));
        let restored = Settings::from_kept(r#"{ "snap": 24, "grid": 16 }"#);
        assert_eq!(
            (restored.snap, restored.grid),
            (settings.snap, settings.grid)
        );
    }

    #[test]
    fn stored_settings_come_back_and_missing_ones_take_their_defaults() {
        let settings = Settings {
            auto_jump: false,
            snap: NoteValue::new(24).unwrap(),
            notes_keep_time: false,
            zoom: 150.0.into(),
            scale: 1.25.into(),
            direction: Direction::Right,
            note_ticks: true,
            tick_sound: Sound::Clap,
            tuned_lanes: true,
            metronome: true,
            metronome_sound: Kit::ShakerBurst,
            volumes: Volumes {
                music: Gain::from(0.5),
                ..Volumes::default()
            },
            autosave: false,
            autosave_seconds: 90_u32.into(),
            snapshots_in_folder: true,
            snapshot_folder: Some("/snapshots".into()),
            snapshots_beside_song: true,
            snapshot_minutes: 10_u32.into(),
            snapshots_kept: 5_u32.into(),
            recent: vec!["/songs/a.json".into()],
            song_panel: false,
            waveform: false,
            frequencies: true,
            minimap: false,
            colour_notes_by_layer: true,
            grid: NoteValue::new(24).unwrap(),
            metronome_grid: true,
            rate_b: 0.5.into(),
            snippet_ms: 120_u32.into(),
            keep_pitch: false,
            ffmpeg_path: String::new(),
            ffmpeg_offered: false,
            audio_latency_ms: 12.0.into(),
            input_latency_ms: 20.0.into(),
        };
        let stored = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&stored).unwrap(), settings);
        let together: Settings = serde_json::from_str(r#"{ "latency_ms": 30.0 }"#).unwrap();
        assert!((together.input_latency_ms.ms() - 30.0).abs() < f64::EPSILON);
        assert!(together.audio_latency_ms.ms().abs() < f64::EPSILON);
        let older: Settings = serde_json::from_str(r#"{ "auto_jump": false }"#).unwrap();
        assert_eq!(
            older,
            Settings {
                auto_jump: false,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn latency_newtypes_keep_the_old_settings_json_shape() {
        let kept = r#"{ "audio_latency_ms": 12.5, "input_latency_ms": -8.25 }"#;
        let settings = Settings::from_kept(kept);
        assert_eq!(settings.audio_latency_ms, LatencyMs::from(12.5));
        assert_eq!(settings.input_latency_ms, LatencyMs::from(-8.25));

        let stored: Value = serde_json::from_str(&settings.to_kept()).unwrap();
        assert_eq!(stored["audio_latency_ms"], 12.5);
        assert_eq!(stored["input_latency_ms"], -8.25);
    }

    #[test]
    fn stored_numbers_are_bounded_as_they_are_read() {
        let kept = r#"{
            "audio_latency_ms": 1000000000,
            "input_latency_ms": -1000000000,
            "zoom": 1000000000,
            "scale": -5,
            "rate_b": -5,
            "autosave_seconds": 1000000000,
            "snapshot_minutes": 0,
            "snapshots_kept": 0,
            "snippet_ms": 0
        }"#;
        for settings in [
            Settings::from_kept(kept),
            serde_json::from_str::<Settings>(kept).unwrap(),
        ] {
            let stored = serde_json::to_value(settings).unwrap();
            assert_eq!(stored["audio_latency_ms"], 500.0);
            assert_eq!(stored["input_latency_ms"], -500.0);
            assert_eq!(stored["zoom"], 1250.0);
            assert_eq!(stored["scale"], 0.5);
            assert_eq!(stored["rate_b"], 0.1);
            assert_eq!(stored["autosave_seconds"], 600);
            assert_eq!(stored["snapshot_minutes"], 1);
            assert_eq!(stored["snapshots_kept"], 1);
            assert_eq!(stored["snippet_ms"], 50);
        }

        let opposite = r#"{
            "zoom": -5, "rate_b": 1000000000, "autosave_seconds": -5,
            "snapshot_minutes": 1000000000, "snapshots_kept": -5,
            "snippet_ms": 1000000000
        }"#;
        let settings = Settings::from_kept(opposite);
        let stored = serde_json::to_value(settings).unwrap();
        assert_eq!(stored["zoom"], 12.5);
        assert_eq!(stored["rate_b"], 4.0);
        assert_eq!(stored["autosave_seconds"], 5);
        assert_eq!(stored["snapshot_minutes"], 120);
        assert_eq!(stored["snapshots_kept"], 1);
        assert_eq!(stored["snippet_ms"], 1000);
        let direct = serde_json::from_str::<Settings>(opposite).unwrap();
        assert_eq!(serde_json::to_value(direct).unwrap(), stored);
    }

    #[test]
    fn large_saved_integer_settings_reach_their_upper_bounds() {
        let kept = r#"{
            "autosave_seconds": 18446744073709551615,
            "snapshot_minutes": 18446744073709551615,
            "snapshots_kept": 18446744073709551615,
            "snippet_ms": 18446744073709551615
        }"#;
        let settings = Settings::from_kept(kept);
        let stored = serde_json::to_value(settings).unwrap();
        assert_eq!(stored["autosave_seconds"], 600);
        assert_eq!(stored["snapshot_minutes"], 120);
        assert_eq!(stored["snapshots_kept"], 500);
        assert_eq!(stored["snippet_ms"], 1000);
        let direct = serde_json::from_str::<Settings>(kept).unwrap();
        assert_eq!(serde_json::to_value(direct).unwrap(), stored);
    }

    #[test]
    fn nonfinite_numeric_choices_take_their_defaults() {
        let settings = Settings {
            zoom: f32::NAN.into(),
            scale: f32::INFINITY.into(),
            rate_b: f64::NEG_INFINITY.into(),
            ..Settings::default()
        };
        let stored = serde_json::to_value(settings).unwrap();
        assert_eq!(stored["zoom"], 100.0);
        assert_eq!(stored["scale"], 1.0);
        assert_eq!(stored["rate_b"], 0.6);
    }

    #[test]
    fn only_settings_changed_from_the_defaults_are_kept() {
        assert_eq!(Settings::default().to_kept(), "{}");
        let changed = Settings {
            metronome: true,
            zoom: 150.0.into(),
            ..Settings::default()
        };
        let kept = changed.to_kept();
        let value: Value = serde_json::from_str(&kept).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["metronome", "zoom"]);
        assert_eq!(Settings::from_kept(&kept), changed);
    }

    #[test]
    fn an_ffmpeg_path_is_empty_by_default_and_kept_when_chosen() {
        let defaults = Settings::default().to_kept();
        assert!(
            serde_json::from_str::<Value>(&defaults)
                .unwrap()
                .get("ffmpeg_path")
                .is_none()
        );
        let chosen = Settings::from_kept(r#"{"ffmpeg_path":"/tools/ffmpeg"}"#);
        let stored: Value = serde_json::from_str(&chosen.to_kept()).unwrap();
        assert_eq!(stored["ffmpeg_path"], "/tools/ffmpeg");
    }

    #[test]
    fn having_offered_ffmpeg_help_is_remembered_between_runs() {
        let offered = Settings::from_kept(r#"{"ffmpeg_offered":true}"#);
        let stored: Value = serde_json::from_str(&offered.to_kept()).unwrap();
        assert_eq!(stored["ffmpeg_offered"], true);
    }

    #[test]
    fn a_kept_setting_that_no_longer_reads_takes_its_default_and_leaves_the_rest() {
        let kept = r#"{ "metronome": true, "tick_sound": "trombone", "zoom": "wide" }"#;
        let settings = Settings::from_kept(kept);
        assert!(settings.metronome);
        assert_eq!(settings.tick_sound, Settings::default().tick_sound);
        assert!(
            (settings.zoom.percent() - Settings::default().zoom.percent()).abs() < f32::EPSILON
        );
        assert_eq!(Settings::from_kept("not json"), Settings::default());
    }

    #[test]
    fn ticks_kept_as_a_sound_by_older_versions_read_as_on_or_off() {
        let on = Settings::from_kept(r#"{ "tick_sound": "beep" }"#);
        assert!(on.note_ticks);
        assert_eq!(on.tick_sound, Sound::Beep);
        let off = Settings::from_kept(r#"{ "tick_sound": "off" }"#);
        assert!(!off.note_ticks);
        assert_eq!(off.tick_sound, Sound::Click);
    }

    #[test]
    fn a_tick_sound_chosen_with_the_ticks_off_stays_off() {
        let chosen = Settings {
            tick_sound: Sound::Beep,
            ..Settings::default()
        };
        assert_eq!(Settings::from_kept(&chosen.to_kept()), chosen);
    }

    #[test]
    fn older_grid_and_sound_settings_migrate_without_losing_other_choices() {
        let cases = [
            (
                r#"{"sixteenths":true,"note_ticks":"beep","metronome":"off"}"#,
                16,
                true,
                false,
            ),
            (
                r#"{"sixteenths":false,"note_ticks":"off","metronome":"click"}"#,
                4,
                false,
                true,
            ),
            (
                r#"{"grid":0,"tick_sound":"off","zoom":175}"#,
                4,
                false,
                false,
            ),
            (
                r#"{"grid":7,"tick_sound":"hit","latency_ms":18}"#,
                4,
                true,
                false,
            ),
        ];
        for (kept, grid, ticks, metronome) in cases {
            let settings = Settings::from_kept(kept);
            assert_eq!(settings.grid.get(), grid, "{kept}");
            assert_eq!(settings.note_ticks, ticks, "{kept}");
            assert_eq!(settings.metronome, metronome, "{kept}");
        }
        let mixed = Settings::from_kept(r#"{"sixteenths":true,"zoom":"bad","auto_jump":false}"#);
        assert_eq!(mixed.grid.get(), 16);
        assert!(!mixed.auto_jump);
        assert!((mixed.zoom.percent() - 100.0).abs() < f32::EPSILON);
        assert_eq!(Settings::from_kept("[]"), Settings::default());
    }

    #[test]
    fn invalid_scale_and_autosave_waits_are_bounded_at_both_ends() {
        for (value, expected) in [
            (f32::NAN, 1.0),
            (f32::NEG_INFINITY, 1.0),
            (0.5, 0.5),
            (3.0, 3.0),
        ] {
            let settings = Settings {
                scale: value.into(),
                ..Settings::default()
            };
            assert!((settings.scale.factor() - expected).abs() < f32::EPSILON);
        }
        for (seconds, expected) in [(4_u32, 5.0), (5, 5.0), (600, 600.0), (601, 600.0)] {
            let settings = Settings {
                autosave_seconds: seconds.into(),
                ..Settings::default()
            };
            assert_eq!(settings.autosave_after(), Some(expected));
        }
    }

    #[test]
    fn the_snippet_length_is_kept_between_a_twentieth_and_a_second() {
        let length = |snippet_ms: u32| {
            Settings {
                snippet_ms: SnippetMs::from(snippet_ms),
                ..Settings::default()
            }
            .snippet_seconds()
        };
        assert!((length(200) - 0.2).abs() < f64::EPSILON);
        assert!((length(0) - 0.05).abs() < f64::EPSILON);
        assert!((length(u32::MAX) - 1.0).abs() < f64::EPSILON);
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_kept_settings_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let kept = String::from_utf8_lossy(&bytes);
            let settings = Settings::from_kept(&kept);
            prop_assert_eq!(Settings::from_kept(&settings.to_kept()), settings);
        }
    }
}
